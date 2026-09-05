//! Microphone-only input using the same capture, model cache, and Whisper pipeline as feedback.
use super::*;

pub(super) struct DictationSession {
    id: String,
    directory: PathBuf,
    audio: Option<StartedAudio>,
}

impl DictationSession {
    fn create(id: String, directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Could not create voice input storage: {error}"))?;
        let session = Self {
            id,
            directory,
            audio: None,
        };
        set_private_permissions(&session.directory)
            .map_err(|error| format!("Could not prepare voice input storage: {error}"))?;
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

#[tauri::command]
pub(crate) fn dictation_preflight() -> DictationPreflight {
    DictationPreflight {
        supported: true,
        microphone_available: cpal::default_host().default_input_device().is_some(),
        model_installed: model_is_installed(),
        model_size_bytes: MODEL_BYTES,
    }
}

#[tauri::command]
pub(crate) async fn dictation_install_model(app: AppHandle) -> Result<DictationPreflight, String> {
    tauri::async_runtime::spawn_blocking(move || download_model(&app, &model_path()))
        .await
        .map_err(|error| error.to_string())??;
    Ok(dictation_preflight())
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
    if !model_is_installed() {
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
        });
        assert!(take_session(&state, "old").is_err());
        assert!(directory.exists());
        drop(take_session(&state, "current").unwrap());
        assert!(!directory.exists());
        assert!(state.dictation.lock().unwrap().is_none());
    }
}
