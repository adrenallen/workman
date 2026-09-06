use super::*;

fn wav(seconds: u32, channels: u16) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for sample in 0..seconds * 8_000 * u32::from(channels) {
        writer.write_sample((sample % 100) as i16).unwrap();
    }
    writer.finalize().unwrap();
    bytes.into_inner()
}

#[test]
fn imports_owned_copy_persists_replaces_and_resets_without_touching_user_files() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("app");
    let sounds = temp.path().join("Library/Sounds");
    let store = SoundStore::new(&data, sounds.clone());
    let source = temp.path().join("My chime.WAV");
    fs::write(&source, wav(1, 2)).unwrap();
    assert_eq!(
        store.import(&source).unwrap().name.as_deref(),
        Some("My chime.WAV")
    );
    let first = store.selected_path().unwrap();
    assert_ne!(first, source);
    assert_eq!(fs::read(&first).unwrap(), fs::read(&source).unwrap());
    fs::remove_file(&source).unwrap();
    let reopened = SoundStore::new(&data, sounds.clone());
    assert_eq!(reopened.selected_path().as_ref(), Some(&first));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    fs::write(&source, wav(2, 1)).unwrap();
    reopened.import(&source).unwrap();
    assert!(!first.exists());
    let second = reopened.selected_path().unwrap();
    let unrelated = sounds.join("SomeoneElses.wav");
    fs::write(&unrelated, b"untouched").unwrap();
    assert!(reopened.reset().unwrap().name.is_none());
    assert!(!second.exists());
    assert!(source.exists());
    assert_eq!(fs::read(unrelated).unwrap(), b"untouched");
    assert!(reopened.reset().is_ok());
}

#[test]
fn rejects_bad_empty_truncated_oversize_and_long_audio_without_losing_selection() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    let source = temp.path().join("sound.wav");
    fs::write(&source, wav(1, 1)).unwrap();
    store.import(&source).unwrap();
    let original = store.selected_path().unwrap();
    let mut truncated = wav(1, 1);
    truncated.truncate(truncated.len() - 10);
    let mut invalid_rate = wav(1, 1);
    invalid_rate[24..28].copy_from_slice(&0u32.to_le_bytes());
    for bytes in [
        b"not audio".to_vec(),
        wav(0, 1),
        wav(30, 2),
        truncated,
        invalid_rate,
        vec![0; MAX_BYTES as usize + 1],
    ] {
        fs::write(&source, bytes).unwrap();
        assert!(store.import(&source).is_err());
        assert_eq!(store.selected_path().as_ref(), Some(&original));
    }
    assert!(validate_wav(&wav(29, 2)).is_ok());
    let mp3 = temp.path().join("sound.mp3");
    fs::write(&mp3, wav(1, 1)).unwrap();
    assert!(store.import(&mp3).is_err());
    assert!(store.import(temp.path()).is_err());
}

#[test]
fn unavailable_or_invalid_settings_fall_back_and_never_remove_external_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    let source = temp.path().join("keep.wav");
    fs::write(&source, wav(1, 1)).unwrap();
    store.import(&source).unwrap();
    fs::remove_file(store.selected_path().unwrap()).unwrap();
    assert!(store.selected_path().is_none());
    assert!(store.info().detail.unwrap().contains("system sound"));
    for name in [
        "../keep.wav",
        source.to_str().unwrap(),
        "workman-notification-../../keep.wav",
    ] {
        fs::write(
            &store.settings,
            serde_json::to_vec(&Selection {
                preset: SoundPreset::Custom,
                file_name: name.into(),
                display_name: "Bad".into(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(store.selected_path().is_none());
        store.reset().unwrap();
        assert!(source.exists());
    }
    fs::write(&store.settings, b"broken json").unwrap();
    assert!(store.selected_path().is_none());
    store.import(&source).unwrap();
    assert!(store.selected_path().is_some());
}

#[test]
fn bundled_doom_is_valid_and_preserves_the_supplied_pickup_audio_format_and_timing() {
    validate_wav(DOOM_WAV).unwrap();
    let mut reader = hound::WavReader::new(Cursor::new(DOOM_WAV)).unwrap();
    assert_eq!(reader.spec().sample_rate, 11_025);
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.spec().bits_per_sample, 16);
    assert_eq!(reader.duration(), 2_263);
    let samples: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
    assert!(samples.iter().all(|sample| i32::from(*sample) % 256 == 0));
    assert!(samples.iter().any(|sample| *sample != 0));
}

#[test]
fn selecting_doom_persists_an_embedded_copy_and_system_restores_the_default() {
    let temp = tempfile::tempdir().unwrap();
    let sounds = temp.path().join("Library/Sounds");
    let store = SoundStore::new(temp.path(), sounds.clone());
    assert_eq!(store.info().preset, SoundPreset::System);
    assert!(store.selected_path().is_none());
    let info = store.select(SoundPreset::Doom).unwrap();
    assert_eq!(info.preset, SoundPreset::Doom);
    assert_eq!(info.name.as_deref(), Some("Doom"));
    let copy = store.selected_path().unwrap();
    assert_eq!(fs::read(&copy).unwrap(), DOOM_WAV);
    let reopened = SoundStore::new(temp.path(), sounds);
    assert_eq!(reopened.info().preset, SoundPreset::Doom);
    assert_eq!(reopened.selected_path().as_ref(), Some(&copy));
    assert_eq!(
        reopened.select(SoundPreset::System).unwrap().preset,
        SoundPreset::System
    );
    assert!(!copy.exists());
    assert!(reopened.selected_path().is_none());
}

#[test]
fn legacy_custom_sound_migrates_and_switching_presets_cleans_only_owned_copies() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    let source = temp.path().join("My sound.wav");
    fs::write(&source, wav(1, 1)).unwrap();
    store.import(&source).unwrap();
    let custom = store.selected_path().unwrap();
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&fs::read(&store.settings).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("preset");
    fs::write(&store.settings, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(store.info().preset, SoundPreset::Custom);
    assert_eq!(store.selected_path().as_ref(), Some(&custom));
    store.select(SoundPreset::Doom).unwrap();
    assert!(!custom.exists());
    let doom = store.selected_path().unwrap();
    assert!(
        store.select(SoundPreset::Custom).is_err(),
        "custom requires a selected file"
    );
    assert_eq!(store.selected_path().as_ref(), Some(&doom));
    assert_eq!(store.import(&source).unwrap().preset, SoundPreset::Custom);
    assert!(!doom.exists());
    assert!(source.exists());
    store.select(SoundPreset::System).unwrap();
    assert!(source.exists());
}

#[cfg(unix)]
#[test]
fn linked_audio_falls_back_and_reset_only_removes_the_link() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    let source = temp.path().join("keep.wav");
    fs::write(&source, wav(1, 1)).unwrap();
    store.import(&source).unwrap();
    let copy = store.selected_path().unwrap();
    fs::remove_file(&copy).unwrap();
    std::os::unix::fs::symlink(&source, &copy).unwrap();
    assert!(store.selected_path().is_none());
    store.reset().unwrap();
    assert!(source.exists());
    assert!(!copy.exists());
}

#[test]
fn failed_settings_write_removes_new_copy_and_keeps_previous_sound() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    let source = temp.path().join("sound.wav");
    fs::write(&source, wav(1, 1)).unwrap();
    store.import(&source).unwrap();
    let original = store.selected_path().unwrap();
    // A directory at the metadata destination forces a rename failure without depending on
    // the effective user's permissions (the suite also runs as root in Linux containers).
    let saved = fs::read(&store.settings).unwrap();
    fs::remove_file(&store.settings).unwrap();
    fs::create_dir(&store.settings).unwrap();
    assert!(store.import(&source).is_err());
    assert_eq!(fs::read_dir(&store.sounds).unwrap().count(), 1);
    assert!(original.exists());
    fs::remove_dir(&store.settings).unwrap();
    fs::write(&store.settings, saved).unwrap();
    assert_eq!(store.selected_path().as_ref(), Some(&original));
}

fn samples(path: &Path) -> Vec<i16> {
    hound::WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn volume_scales_exact_samples_persists_and_never_changes_the_original() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    assert_eq!(store.info().volume, 100);
    assert!(!store.info().volume_supported);
    assert!(store.set_volume(50).is_err());
    store.select(SoundPreset::Doom).unwrap();
    let source = store.selected_path().unwrap();
    let original = fs::read(&source).unwrap();
    let full = samples(&source);
    assert!(full.iter().any(|sample| *sample < 0));
    assert!(full.iter().any(|sample| *sample > 0));
    assert_eq!(store.set_volume(50).unwrap().volume, 50);
    let half = store.playback_path().unwrap().unwrap();
    assert_eq!(
        samples(&half),
        full.iter()
            .map(|sample| (i32::from(*sample) / 2) as i16)
            .collect::<Vec<_>>()
    );
    assert_eq!(store.playback_path().unwrap().unwrap(), half);
    let reopened = SoundStore::new(temp.path(), temp.path().join("sounds"));
    assert_eq!(reopened.volume(), 50);
    assert_eq!(reopened.playback_path().unwrap().unwrap(), half);
    reopened.set_volume(0).unwrap();
    let muted = reopened.playback_path().unwrap().unwrap();
    assert!(samples(&muted).iter().all(|sample| *sample == 0));
    reopened.set_volume(100).unwrap();
    assert_eq!(reopened.playback_path().unwrap().unwrap(), source);
    assert_eq!(fs::read(&source).unwrap(), original);
    reopened.set_volume(35).unwrap();
    reopened.reset().unwrap();
    assert!(!half.exists());
    assert!(!muted.exists());
    assert_eq!(reopened.volume(), 35);
    reopened.select(SoundPreset::Doom).unwrap();
    assert_eq!(reopened.info().volume, 35);
    assert_ne!(
        reopened.playback_path().unwrap().unwrap(),
        reopened.selected_path().unwrap()
    );
}

#[test]
fn invalid_volume_and_failed_save_preserve_the_previous_level() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    store.select(SoundPreset::Doom).unwrap();
    store.set_volume(40).unwrap();
    assert!(store.set_volume(101).is_err());
    assert_eq!(store.volume(), 40);
    fs::remove_file(&store.volume_settings).unwrap();
    fs::create_dir(&store.volume_settings).unwrap();
    assert!(store.set_volume(80).is_err());
    assert_eq!(store.volume(), 100);
    fs::remove_dir(&store.volume_settings).unwrap();
    for invalid in ["-1", "101", "100000", "\"quiet\"", "null"] {
        fs::write(&store.volume_settings, invalid).unwrap();
        assert_eq!(store.volume(), 100);
    }
}

#[cfg(unix)]
#[test]
fn volume_copy_cannot_overwrite_a_symlink_target() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    store.select(SoundPreset::Doom).unwrap();
    let external = temp.path().join("external.wav");
    fs::write(&external, b"untouched").unwrap();
    let scaled = volume_path(&store.selected_path().unwrap(), 50);
    std::os::unix::fs::symlink(&external, &scaled).unwrap();
    assert!(store.set_volume(50).is_err());
    assert_eq!(store.volume(), 100);
    assert_eq!(fs::read(&external).unwrap(), b"untouched");
    store.reset().unwrap();
    assert!(!scaled.exists());
    assert_eq!(fs::read(&external).unwrap(), b"untouched");
}

#[test]
fn damaged_volume_copies_are_rebuilt_and_the_cache_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let store = SoundStore::new(temp.path(), temp.path().join("sounds"));
    store.select(SoundPreset::Doom).unwrap();
    store.set_volume(0).unwrap();
    let muted = store.playback_path().unwrap().unwrap();
    fs::write(&muted, b"interrupted write").unwrap();
    assert_eq!(store.playback_path().unwrap().unwrap(), muted);
    assert!(samples(&muted).iter().all(|sample| *sample == 0));
    for level in [10, 20, 30, 40, 50, 60] {
        store.set_volume(level).unwrap();
        assert_eq!(
            fs::read_dir(&store.sounds).unwrap().count(),
            3,
            "original plus current and previous levels"
        );
    }
    let original = fs::read(store.selected_path().unwrap()).unwrap();
    assert_eq!(original, DOOM_WAV);
}
