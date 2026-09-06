//! Microphone-only input using the same capture, model cache, and Whisper pipeline as feedback.
use super::*;

pub(super) struct DictationSession {
    id: String,
    directory: PathBuf,
    audio: Option<StartedAudio>,
    storage_lock: Option<fs::File>,
}

impl DictationSession {
    fn create(id: String, directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(directory.parent().ok_or("Missing voice input folder")?)
            .map_err(|error| format!("Could not create voice input storage: {error}"))?;
        // Never adopt an existing session directory, including one owned by another app instance.
        fs::create_dir(&directory)
            .map_err(|error| format!("Could not create voice input storage: {error}"))?;
        let mut session = Self {
            id,
            directory,
            audio: None,
            storage_lock: None,
        };
        set_private_permissions(&session.directory)
            .map_err(|error| format!("Could not prepare voice input storage: {error}"))?;
        let path = session.directory.join("session.lock");
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        set_private_permissions(&path)?;
        lock.try_lock().map_err(|error| error.to_string())?;
        session.storage_lock = Some(lock);
        Ok(session)
    }

    fn stop_audio(&mut self) -> Result<u32, String> {
        let audio = self
            .audio
            .take()
            .ok_or("Voice input has already stopped.")?;
        drop(audio.stream);
        finalize_writer(&audio.writer)?;
        Ok(audio.sample_rate)
    }

    fn transcribe(self, sample_rate: u32) -> Result<String, String> {
        let segments = transcribe(&self.directory.join("audio.wav"), sample_rate)?;
        Ok(segments
            .into_iter()
            .map(|segment| segment.text)
            .collect::<Vec<_>>()
            .join(" "))
        // Drop removes temporary audio on both success and failure.
    }
}

impl Drop for DictationSession {
    fn drop(&mut self) {
        let _ = self.stop_audio();
        self.storage_lock.take();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[derive(Serialize)]
pub(crate) struct DictationPreflight {
    supported: bool,
    microphone_available: bool,
    model_installed: bool,
    model_size_bytes: u64,
}

fn cleanup_abandoned_sessions(root: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err()
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        // A constructor may have created session.lock but not acquired it yet. Give new
        // directories time to finish initialization before treating an unlocked file as stale.
        let old_enough = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= Duration::from_secs(60));
        if !old_enough {
            continue;
        }
        let path = entry.path();
        let lock_path = path.join("session.lock");
        if !fs::symlink_metadata(&lock_path).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        let Ok(lock) = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
        else {
            continue;
        };
        // OS locks survive the move to the transcription worker and release automatically on a
        // crash. Preserve live recordings/transcriptions, including those in another app instance.
        if lock.try_lock().is_ok() {
            drop(lock);
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn preflight() -> DictationPreflight {
    cleanup_abandoned_sessions(
        &workmand::default_data_dir().join("dictation"),
        SystemTime::now(),
    );
    DictationPreflight {
        supported: true,
        microphone_available: cpal::default_host().default_input_device().is_some(),
        model_installed: model_is_installed(),
        model_size_bytes: MODEL_BYTES,
    }
}

#[tauri::command]
pub(crate) async fn dictation_preflight() -> Result<DictationPreflight, String> {
    tauri::async_runtime::spawn_blocking(preflight)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn dictation_install_model(app: AppHandle) -> Result<DictationPreflight, String> {
    tauri::async_runtime::spawn_blocking(move || {
        download_model(&app, &model_path())?;
        Ok(preflight())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) fn dictation_start(
    session_id: String,
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<(), String> {
    // Lock in the same order as feedback_start, so only one microphone consumer can start.
    let feedback = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let mut active = state
        .dictation
        .lock()
        .map_err(|_| "dictation state is unavailable")?;
    if feedback.is_some() || active.is_some() {
        return Err("Finish the current recording before starting voice input.".into());
    }
    let id = Uuid::parse_str(&session_id)
        .map_err(|_| "Invalid voice input session.")?
        .to_string();
    // Preflight verifies SHA-256 on a worker. Avoid hashing 148 MB again on the UI thread.
    if !model_path()
        .metadata()
        .is_ok_and(|metadata| metadata.len() == MODEL_BYTES)
    {
        return Err("Install the local transcription model before using voice input.".into());
    }
    let directory = workmand::default_data_dir().join("dictation").join(&id);
    let mut session = DictationSession::create(id.clone(), directory)?;
    let path = session.directory.join("audio.wav");
    session.audio = Some(start_audio(
        &app,
        AudioErrorTarget::Dictation { session_id: id },
        &path,
    )?);
    set_private_permissions(&path)?;
    *active = Some(session);
    Ok(())
}

fn take_session(state: &FeedbackState, id: &str) -> Result<DictationSession, String> {
    let mut active = state
        .dictation
        .lock()
        .map_err(|_| "dictation state is unavailable")?;
    if !active.as_ref().is_some_and(|session| session.id == id) {
        return Err("This voice input session is no longer active.".into());
    }
    active
        .take()
        .ok_or_else(|| "No voice input is active.".into())
}

#[tauri::command]
pub(crate) fn dictation_cancel(
    session_id: String,
    state: State<'_, FeedbackState>,
) -> Result<(), String> {
    drop(take_session(&state, &session_id)?);
    Ok(())
}

#[tauri::command]
pub(crate) async fn dictation_finish(
    session_id: String,
    state: State<'_, FeedbackState>,
) -> Result<String, String> {
    let mut session = take_session(&state, &session_id)?;
    let sample_rate = session.stop_audio()?;
    tauri::async_runtime::spawn_blocking(move || session.transcribe(sample_rate))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_cleanup_preserves_live_transcriptions_and_unowned_paths() {
        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4().to_string();
        let directory = root.path().join(&id);
        let session = DictationSession::create(id.clone(), directory.clone()).unwrap();
        fs::write(directory.join("audio.wav"), b"live recording").unwrap();
        assert!(DictationSession::create(id.clone(), directory.clone()).is_err());
        let state = FeedbackState::default();
        *state.dictation.lock().unwrap() = Some(session);
        let transcribing = take_session(&state, &id).unwrap();
        let orphan = root.path().join(Uuid::new_v4().to_string());
        fs::create_dir(&orphan).unwrap();
        fs::write(orphan.join("session.lock"), b"").unwrap();
        fs::write(orphan.join("audio.wav"), b"interrupted recording").unwrap();
        let unmarked = root.path().join(Uuid::new_v4().to_string());
        fs::create_dir(&unmarked).unwrap();
        fs::write(unmarked.join("keep.wav"), b"unowned").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&unmarked, root.path().join(Uuid::new_v4().to_string()))
            .unwrap();
        let now = SystemTime::now();
        cleanup_abandoned_sessions(root.path(), now);
        assert!(
            orphan.exists(),
            "new unlocked directories may still be initializing"
        );
        cleanup_abandoned_sessions(root.path(), now + Duration::from_secs(61));
        assert!(!orphan.exists());
        assert_eq!(
            fs::read(directory.join("audio.wav")).unwrap(),
            b"live recording"
        );
        assert_eq!(fs::read(unmarked.join("keep.wav")).unwrap(), b"unowned");
        drop(transcribing);
        assert!(!directory.exists());
    }

    #[test]
    fn dictation_storage_allows_private_audio_creation_reading_and_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("dictation").join("session");
        let session = DictationSession::create("session".into(), directory.clone()).unwrap();
        let path = directory.join("audio.wav");
        let mut writer = WavWriter::create(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 32,
                sample_format: WavSampleFormat::Float,
            },
        )
        .expect("a private dictation directory must still allow creating the recording");
        set_private_permissions(&path).unwrap();
        writer.write_sample(0.25_f32).unwrap();
        writer.finalize().unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.samples::<f32>().next().unwrap().unwrap(), 0.25);
        drop(reader);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(session);
        assert!(
            !directory.exists(),
            "cancellation must remove the recording and directory"
        );
    }

    #[test]
    fn transcription_removes_temporary_audio_on_silence_and_failure() {
        for valid_audio in [false, true] {
            let directory = tempfile::tempdir().unwrap().keep();
            if valid_audio {
                let mut writer = WavWriter::create(
                    directory.join("audio.wav"),
                    WavSpec {
                        channels: 1,
                        sample_rate: 16_000,
                        bits_per_sample: 32,
                        sample_format: WavSampleFormat::Float,
                    },
                )
                .unwrap();
                writer.write_sample(0.0_f32).unwrap();
                writer.finalize().unwrap();
            }
            let session = DictationSession {
                id: "test".into(),
                directory: directory.clone(),
                audio: None,
                storage_lock: None,
            };
            let result = session.transcribe(16_000);
            if valid_audio {
                assert_eq!(result.unwrap(), "");
            } else {
                assert!(result.is_err());
            }
            assert!(!directory.exists());
        }
    }

    #[test]
    fn cancelling_a_stale_session_cannot_stop_a_new_recording() {
        let directory = tempfile::tempdir().unwrap().keep();
        let state = FeedbackState::default();
        *state.dictation.lock().unwrap() = Some(DictationSession {
            id: "current".into(),
            directory: directory.clone(),
            audio: None,
            storage_lock: None,
        });
        assert!(take_session(&state, "old").is_err());
        assert!(directory.exists());
        drop(take_session(&state, "current").unwrap());
        assert!(!directory.exists());
        assert!(state.dictation.lock().unwrap().is_none());
    }
}
