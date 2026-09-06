use std::sync::Mutex;

/// Playback prevents overlapping previews and selection changes, but never blocks alert reads.
#[derive(Default)]
pub struct SoundOperations {
    playback: Mutex<()>,
    settings: Mutex<()>,
}

impl SoundOperations {
    pub fn read<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let _settings = self.settings.lock().map_err(|error| error.to_string())?;
        action()
    }

    pub fn update<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let _playback = self
            .playback
            .try_lock()
            .map_err(|_| "A sound operation is already in progress.".to_owned())?;
        self.read(action)
    }

    pub fn preview<T>(
        &self,
        resolve: impl FnOnce() -> Result<T, String>,
        play: impl FnOnce(T) -> Result<(), String>,
    ) -> Result<(), String> {
        let _playback = self
            .playback
            .try_lock()
            .map_err(|_| "A sound operation is already in progress.".to_owned())?;
        let sound = self.read(resolve)?;
        play(sound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    #[test]
    fn alerts_can_read_during_playback_but_changes_and_second_previews_cannot() {
        let operations = Arc::new(SoundOperations::default());
        let (started, playing) = mpsc::channel();
        let (release, finished) = mpsc::channel();
        let preview = operations.clone();
        let thread = std::thread::spawn(move || {
            preview.preview(
                || Ok("Doom"),
                |_| {
                    started.send(()).unwrap();
                    finished.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(())
                },
            )
        });
        playing.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(operations.update(|| Ok(())).is_err());
        assert!(operations.preview(|| Ok(()), |_| Ok(())).is_err());
        assert_eq!(operations.read(|| Ok("Doom")).unwrap(), "Doom");
        release.send(()).unwrap();
        thread.join().unwrap().unwrap();
        assert!(operations.update(|| Ok(())).is_ok());
        assert!(
            operations
                .preview(|| Ok(()), |_| Err("Player failed".into()))
                .is_err()
        );
        assert!(operations.update(|| Ok(())).is_ok());
    }
}
