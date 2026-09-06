//! Imported sounds remain local. Automatic alerts use the notification service and its permission
//! and Do Not Disturb policy; explicit previews play audio directly. Windows toasts cannot use
//! arbitrary user audio files.
use std::{
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 6 * 1024 * 1024;
const FILE_PREFIX: &str = "workman-notification-";
// Embed the only shipped sound in every binary, including dev and packaged builds.
const DOOM_WAV: &[u8] = include_bytes!("../../assets/sounds/dsitempickup.wav");

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundPreset {
    System,
    Doom,
    Custom,
}

fn legacy_preset() -> SoundPreset {
    SoundPreset::Custom
}

#[derive(Clone, Debug, Serialize)]
pub struct SoundInfo {
    pub supported: bool,
    pub preset: SoundPreset,
    pub name: Option<String>,
    pub detail: Option<String>,
    pub volume: u8,
    pub volume_supported: bool,
}

#[derive(Serialize, Deserialize)]
struct Selection {
    #[serde(default = "legacy_preset")]
    preset: SoundPreset,
    file_name: String,
    display_name: String,
}

pub struct SoundStore {
    settings: PathBuf,
    volume_settings: PathBuf,
    sounds: PathBuf,
}

impl SoundStore {
    pub fn new(app_data: &Path, sounds: PathBuf) -> Self {
        Self {
            settings: app_data.join("notification-sound.json"),
            volume_settings: app_data.join("notification-volume.json"),
            sounds,
        }
    }

    pub fn info(&self) -> SoundInfo {
        match self.selection() {
            Ok(Some(selection)) if self.available_path(&selection).is_some() => SoundInfo {
                supported: true,
                preset: selection.preset,
                name: Some(selection.display_name),
                detail: None,
                volume: self.volume(),
                volume_supported: true,
            },
            Ok(None) => SoundInfo { supported: true, preset: SoundPreset::System, name: None, detail: None, volume: self.volume(), volume_supported: false },
            _ => SoundInfo {
                supported: true,
                preset: SoundPreset::System,
                name: None,
                detail: Some("The saved sound is unavailable. Using the system sound; choose a file again to replace it.".into()),
                volume: self.volume(),
                volume_supported: false,
            },
        }
    }

    /// Never fail a banner because its optional sound was removed or its settings are unreadable.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selection()
            .ok()
            .flatten()
            .and_then(|selection| self.available_path(&selection))
    }

    pub fn volume(&self) -> u8 {
        read_bounded(&self.volume_settings, 128)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<u8>(&bytes).ok())
            .filter(|volume| *volume <= 100)
            .unwrap_or(100)
    }

    pub fn set_volume(&self, volume: u8) -> Result<SoundInfo, String> {
        if volume > 100 {
            return Err("Notification volume must be between 0 and 100.".into());
        }
        let source = self.selected_path().ok_or("The operating system controls the default sound volume. Choose Doom or a custom WAV to adjust it in Workman.")?;
        let previous_volume = self.volume();
        // Prepare the exact playback copy before changing the saved preference.
        self.render_volume(&source, volume)?;
        let temporary = self
            .volume_settings
            .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            write_new(
                &temporary,
                &serde_json::to_vec(&volume).map_err(|error| error.to_string())?,
            )?;
            fs::rename(&temporary, &self.volume_settings).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        // Retain the previous level for an alert that was just handed to the OS. Bound the cache
        // to two rendered copies instead of retaining a full WAV for every slider position.
        for cached in 0..100 {
            if cached != volume && cached != previous_volume {
                let _ = fs::remove_file(volume_path(&source, cached));
            }
        }
        Ok(self.info())
    }

    /// Previews and native notifications use the same attenuated WAV, preserving OS sound policy.
    pub fn playback_path(&self) -> Result<Option<PathBuf>, String> {
        self.selected_path()
            .map(|source| self.render_volume(&source, self.volume()))
            .transpose()
    }

    fn render_volume(&self, source: &Path, volume: u8) -> Result<PathBuf, String> {
        if volume == 100 {
            return Ok(source.to_owned());
        }
        let path = volume_path(source, volume);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                if read_bounded(&path, MAX_BYTES)
                    .and_then(|bytes| validate_wav(&bytes))
                    .is_ok()
                {
                    return Ok(path);
                }
                // A crash or damaged cache must not permanently replace a muted/custom alert
                // with the system sound. The owned original remains the source of truth.
                fs::remove_file(&path).map_err(|error| error.to_string())?;
            }
            Ok(_) => return Err("The saved volume preview is not a regular file.".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let bytes = read_bounded(source, MAX_BYTES)?;
        validate_wav(&bytes)?;
        let mut reader =
            hound::WavReader::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
        let mut output = Cursor::new(Vec::new());
        let mut writer =
            hound::WavWriter::new(&mut output, reader.spec()).map_err(|error| error.to_string())?;
        for sample in reader.samples::<i16>() {
            let sample = i32::from(sample.map_err(|error| error.to_string())?);
            writer
                .write_sample((sample * i32::from(volume) / 100) as i16)
                .map_err(|error| error.to_string())?;
        }
        writer.finalize().map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        write_new(&temporary, &output.into_inner())?;
        if let Err(error) = fs::rename(&temporary, &path) {
            let _ = fs::remove_file(temporary);
            return Err(error.to_string());
        }
        Ok(path)
    }

    pub fn import(&self, source: &Path) -> Result<SoundInfo, String> {
        if !source
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
        {
            return Err(
                "Choose a WAV file (16-bit PCM, mono or stereo, less than 30 seconds).".into(),
            );
        }
        let metadata =
            fs::metadata(source).map_err(|error| format!("Could not open the sound: {error}"))?;
        if !metadata.is_file() {
            return Err("Choose an audio file.".into());
        }
        if metadata.len() > MAX_BYTES {
            return Err("Choose a WAV file smaller than 6 MB.".into());
        }
        let bytes = read_bounded(source, MAX_BYTES)?;
        let display_name = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .filter(|character| !character.is_control())
            .take(160)
            .collect();
        self.install(&bytes, display_name, SoundPreset::Custom)
    }

    pub fn select(&self, preset: SoundPreset) -> Result<SoundInfo, String> {
        match preset {
            SoundPreset::System => self.reset(),
            SoundPreset::Doom => self.install(DOOM_WAV, "Doom".into(), SoundPreset::Doom),
            SoundPreset::Custom => {
                Err("Choose a WAV file to set a custom notification sound.".into())
            }
        }
    }

    fn install(
        &self,
        bytes: &[u8],
        display_name: String,
        preset: SoundPreset,
    ) -> Result<SoundInfo, String> {
        validate_wav(bytes)?;
        let selection = Selection {
            preset,
            file_name: format!("{FILE_PREFIX}{}.wav", uuid::Uuid::new_v4()),
            display_name,
        };
        let previous = self.selection().ok().flatten();
        fs::create_dir_all(&self.sounds).map_err(|error| error.to_string())?;
        fs::create_dir_all(self.settings.parent().ok_or("Missing settings folder")?)
            .map_err(|error| error.to_string())?;
        let destination = self.sounds.join(&selection.file_name);
        write_new(&destination, bytes)?;
        let temporary = self
            .settings
            .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let save = (|| {
            write_new(
                &temporary,
                &serde_json::to_vec(&selection).map_err(|error| error.to_string())?,
            )?;
            fs::rename(&temporary, &self.settings).map_err(|error| error.to_string())
        })();
        if let Err(error) = save {
            let _ = fs::remove_file(&temporary);
            let _ = fs::remove_file(&destination);
            return Err(format!("Could not save the notification sound: {error}"));
        }
        if let Some(previous) = previous {
            self.remove_owned(&previous);
        }
        Ok(self.info())
    }

    pub fn reset(&self) -> Result<SoundInfo, String> {
        let previous = self.selection().ok().flatten();
        match fs::remove_file(&self.settings) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not reset the notification sound: {error}")),
        }
        if let Some(previous) = previous {
            self.remove_owned(&previous);
        }
        Ok(self.info())
    }

    fn selection(&self) -> Result<Option<Selection>, String> {
        match fs::symlink_metadata(&self.settings) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
            Ok(metadata) if !metadata.is_file() => return Err("Invalid sound settings file".into()),
            Ok(_) => {}
        }
        let selection: Selection = serde_json::from_slice(&read_bounded(&self.settings, 4096)?)
            .map_err(|error| error.to_string())?;
        if selection.preset == SoundPreset::System || !owned_name(&selection.file_name) {
            return Err("Invalid saved sound name".into());
        }
        Ok(Some(selection))
    }

    fn available_path(&self, selection: &Selection) -> Option<PathBuf> {
        let path = self.sounds.join(&selection.file_name);
        fs::symlink_metadata(&path)
            .ok()
            .filter(|metadata| {
                metadata.is_file() && metadata.len() > 44 && metadata.len() <= MAX_BYTES
            })
            .map(|_| path)
    }

    fn remove_owned(&self, selection: &Selection) {
        if owned_name(&selection.file_name) {
            let source = self.sounds.join(&selection.file_name);
            let _ = fs::remove_file(&source);
            for volume in 0..100 {
                let _ = fs::remove_file(volume_path(&source, volume));
            }
        }
    }
}

fn volume_path(source: &Path, volume: u8) -> PathBuf {
    source.with_file_name(format!(
        "{}-volume-{volume}.wav",
        source.file_stem().unwrap_or_default().to_string_lossy()
    ))
}

fn owned_name(name: &str) -> bool {
    name.strip_prefix(FILE_PREFIX)
        .and_then(|name| name.strip_suffix(".wav"))
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("The sound file is too large.".into());
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

fn validate_wav(bytes: &[u8]) -> Result<(), String> {
    let invalid = || "Choose a valid WAV file (16-bit PCM, mono or stereo, 8–48 kHz).".to_owned();
    let mut reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int
        || spec.bits_per_sample != 16
        || !(1..=2).contains(&spec.channels)
        || !(8_000..=48_000).contains(&spec.sample_rate)
    {
        return Err(invalid());
    }
    if reader.duration() == 0 || u64::from(reader.duration()) >= u64::from(spec.sample_rate) * 30 {
        return Err("Choose a non-empty sound shorter than 30 seconds.".into());
    }
    for sample in reader.samples::<i16>() {
        sample.map_err(|_| invalid())?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "sound_tests.rs"]
mod tests;
