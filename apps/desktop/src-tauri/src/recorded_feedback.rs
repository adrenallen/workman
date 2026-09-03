//! Local-only microphone, screenshot, annotation, and Whisper runtime.

use std::{fs, path::Path};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct FeedbackCapability {
    supported: bool,
    platform: &'static str,
}

#[tauri::command]
pub(crate) fn feedback_capability() -> FeedbackCapability {
    FeedbackCapability {
        supported: cfg!(target_os = "macos"),
        platform: std::env::consts::OS,
    }
}

#[tauri::command]
pub(crate) fn feedback_read_image(feedback_id: i64, path: String) -> Result<String, String> {
    let root = workmand::default_data_dir()
        .join("recorded-feedback")
        .join(feedback_id.to_string())
        .canonicalize()
        .map_err(|error| format!("Feedback storage is unavailable: {error}"))?;
    let path = Path::new(&path)
        .canonicalize()
        .map_err(|error| format!("Screenshot is unavailable: {error}"))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err("Screenshot is outside this feedback session.".into());
    }
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    if metadata.len() > 24 * 1024 * 1024 {
        return Err("Screenshot is too large to preview.".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::*;

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use serde::Serialize;
    use tauri::{AppHandle, State};

    #[derive(Default)]
    pub(crate) struct FeedbackState;

    impl FeedbackState {
        pub(crate) fn shutdown(&self, _app: &AppHandle) {}
    }

    #[derive(Serialize)]
    pub(crate) struct FeedbackPreflight {
        supported: bool,
        platform: &'static str,
        microphone_available: bool,
        screen_capture_authorized: bool,
        display_available: bool,
        screen_capture_available: bool,
        model_installed: bool,
        model_name: &'static str,
        model_size_bytes: u64,
        model_path: String,
        message: Option<String>,
    }

    #[tauri::command]
    pub(crate) fn feedback_preflight() -> FeedbackPreflight {
        FeedbackPreflight {
            supported: false,
            platform: std::env::consts::OS,
            microphone_available: false,
            screen_capture_authorized: false,
            display_available: false,
            screen_capture_available: false,
            model_installed: false,
            model_name: "Whisper base.en",
            model_size_bytes: 0,
            model_path: String::new(),
            message: Some("Recorded Feedback is currently available on macOS 14 or newer.".into()),
        }
    }

    #[tauri::command]
    pub(crate) fn feedback_request_screen_access() -> FeedbackPreflight {
        feedback_preflight()
    }

    #[tauri::command]
    pub(crate) async fn feedback_install_model(
        _app: AppHandle,
    ) -> Result<FeedbackPreflight, String> {
        Err("Recorded Feedback is currently available on macOS 14 or newer.".into())
    }

    macro_rules! unsupported_command {
        ($name:ident($($arg:ident: $ty:ty),*) -> $return:ty) => {
            #[tauri::command]
            pub(crate) fn $name($($arg: $ty,)* _state: State<'_, FeedbackState>) -> Result<$return, String> {
                $(let _ = $arg;)*
                Err("Recorded Feedback is currently available on macOS 14 or newer.".into())
            }
        };
    }

    unsupported_command!(feedback_start(feedback_id: i64, project_id: i64, media_dir: String, shortcuts: Option<std::collections::HashMap<String, String>>) -> serde_json::Value);
    unsupported_command!(feedback_status() -> serde_json::Value);
    unsupported_command!(feedback_raise_toolbar(app: AppHandle) -> bool);
    unsupported_command!(feedback_set_tool(tool: String, color: String, width: f32) -> serde_json::Value);
    unsupported_command!(feedback_record_stroke(stroke: serde_json::Value) -> serde_json::Value);
    unsupported_command!(feedback_undo() -> serde_json::Value);
    unsupported_command!(feedback_clear() -> serde_json::Value);
    unsupported_command!(feedback_begin_region() -> serde_json::Value);
    unsupported_command!(feedback_cancel_region() -> serde_json::Value);
    unsupported_command!(feedback_capture_snapshot(display_index: Option<usize>, region: Option<serde_json::Value>) -> serde_json::Value);
    unsupported_command!(feedback_abort(feedback_id: i64, app: AppHandle) -> bool);
    unsupported_command!(feedback_finish(app: AppHandle) -> serde_json::Value);
}

#[cfg(not(target_os = "macos"))]
pub(crate) use unsupported::*;
