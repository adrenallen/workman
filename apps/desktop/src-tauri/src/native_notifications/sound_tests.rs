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
