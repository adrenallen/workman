//! Imported sounds remain local and are played by the notification service, respecting its
//! permission and Do Not Disturb policy. Windows toasts cannot use arbitrary user audio files.
use std::{
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 6 * 1024 * 1024;
const FILE_PREFIX: &str = "workman-notification-";

#[derive(Clone, Debug, Serialize)]
pub struct SoundInfo {
    pub supported: bool,
    pub name: Option<String>,
    pub detail: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Selection {
    file_name: String,
    display_name: String,
}

pub struct SoundStore {
    settings: PathBuf,
    sounds: PathBuf,
}

impl SoundStore {
    pub fn new(app_data: &Path, sounds: PathBuf) -> Self {
        Self {
            settings: app_data.join("notification-sound.json"),
            sounds,
        }
    }

    pub fn info(&self) -> SoundInfo {
        match self.selection() {
            Ok(Some(selection)) if self.available_path(&selection).is_some() => SoundInfo {
                supported: true,
                name: Some(selection.display_name),
                detail: None,
            },
            Ok(None) => SoundInfo { supported: true, name: None, detail: None },
            _ => SoundInfo {
                supported: true,
                name: None,
                detail: Some("The saved sound is unavailable. Using the system sound; choose a file again to replace it.".into()),
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
        validate_wav(&bytes)?;
        let selection = Selection {
            file_name: format!("{FILE_PREFIX}{}.wav", uuid::Uuid::new_v4()),
            display_name: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .filter(|character| !character.is_control())
                .take(160)
                .collect(),
        };
        let previous = self.selection().ok().flatten();
        fs::create_dir_all(&self.sounds).map_err(|error| error.to_string())?;
        fs::create_dir_all(self.settings.parent().ok_or("Missing settings folder")?)
            .map_err(|error| error.to_string())?;
        let destination = self.sounds.join(&selection.file_name);
        write_new(&destination, &bytes)?;
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
        if !owned_name(&selection.file_name) {
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
            let _ = fs::remove_file(self.sounds.join(&selection.file_name));
        }
    }
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
