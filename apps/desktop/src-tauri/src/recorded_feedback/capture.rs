use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use cpal::{
    SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use hound::{SampleFormat as WavSampleFormat, WavReader, WavSpec, WavWriter};
use image::{DynamicImage, Rgba, RgbaImage, imageops};
use imageproc::drawing::{draw_filled_circle_mut, draw_hollow_ellipse_mut};
#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl};
#[cfg(target_os = "macos")]
use tauri::{Position, Size};
#[cfg(target_os = "macos")]
use tauri_nspanel::{
    CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask, tauri_panel,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use uuid::Uuid;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
use xcap::Monitor;

const MODEL_NAME: &str = "Whisper base.en";
const MODEL_FILE: &str = "ggml-base.en.bin";
const MODEL_BYTES: u64 = 147_964_211;
const MODEL_SHA256: &str = "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002";
const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
const EVENT_STATUS: &str = "feedback://status";
const EVENT_SNAPSHOT: &str = "feedback://snapshot";
const EVENT_FINISHED: &str = "feedback://finished";
const EVENT_TRANSCRIPT: &str = "feedback://transcript";
const EVENT_ERROR: &str = "feedback://error";
const EVENT_TOOL: &str = "feedback://tool";
const EVENT_REGION: &str = "feedback://region";
const EVENT_ANNOTATIONS: &str = "feedback://annotations";
const EVENT_SHORTCUT: &str = "feedback://shortcut";
#[cfg(target_os = "macos")]
const SCREEN_RECORDING_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
// Windows never gates display capture. Recording still needs the microphone,
// which the account can revoke, so point at that pane instead.
#[cfg(windows)]
const SCREEN_RECORDING_SETTINGS_URL: &str = "ms-settings:privacy-microphone";

const DEFAULT_SHORTCUTS: &[(&str, &str)] = &[
    ("snap", "CommandOrControl+Shift+C"),
    ("snapRegion", "CommandOrControl+Shift+R"),
    ("snapFull", "CommandOrControl+Shift+D"),
    ("toggleAnnotation", "CommandOrControl+Shift+A"),
    ("undo", "CommandOrControl+Shift+Z"),
    ("clear", "CommandOrControl+Shift+Backspace"),
    ("togglePause", "CommandOrControl+Shift+Space"),
    ("toggleMute", "CommandOrControl+Shift+M"),
    ("finish", "CommandOrControl+Shift+Return"),
];

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(FeedbackPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

#[derive(Default)]
pub(crate) struct FeedbackState {
    session: Mutex<Option<FeedbackSession>>,
}

impl FeedbackState {
    pub(crate) fn shutdown(&self, app: &AppHandle) {
        let session = self
            .session
            .lock()
            .ok()
            .and_then(|mut active| active.take());
        let Some(session) = session else { return };
        let duration_ms = session_elapsed_ms(&session);
        unregister_shortcuts(app, &session.registered_shortcuts);
        close_feedback_panels(app);
        drop(session.stream);
        let _ = finalize_writer(&session.writer);
        let _ = append_journal(
            &session.media_dir.join("events.jsonl"),
            &json!({
                "event": "interrupted", "reason": "desktop_exit",
                "samples": session.audio_samples.load(Ordering::Relaxed),
                "sample_rate": session.sample_rate,
                "duration_ms": duration_ms, "at": now_millis()
            }),
        );
    }
}

struct FeedbackSession {
    feedback_id: i64,
    project_id: i64,
    media_dir: PathBuf,
    started: Instant,
    paused_at: Option<Instant>,
    paused_duration: Duration,
    started_at_ms: i64,
    sample_rate: u32,
    audio_samples: Arc<AtomicU64>,
    audio_path: PathBuf,
    writer: SharedWriter,
    stream: Stream,
    audio_controls: Arc<AudioControls>,
    input_device_id: String,
    input_device_name: String,
    snapshot_count: usize,
    annotations: Vec<AnnotationStroke>,
    tool: AnnotationTool,
    color: String,
    width: f32,
    registered_shortcuts: Vec<String>,
    display_ids: Vec<u32>,
    capture_in_progress: bool,
}

#[derive(Default)]
struct AudioControls {
    paused: AtomicBool,
    muted: AtomicBool,
    reset_clock: AtomicBool,
}

struct StartedAudio {
    stream: Stream,
    writer: SharedWriter,
    sample_rate: u32,
    samples: Arc<AtomicU64>,
    controls: Arc<AudioControls>,
    input: AudioInputView,
}

struct AudioInputCandidate {
    device: cpal::Device,
    view: AudioInputView,
}

#[derive(Debug)]
struct SnapshotCapture {
    feedback_id: i64,
    project_id: i64,
    media_dir: PathBuf,
    display_ids: Vec<u32>,
    annotations: Vec<AnnotationStroke>,
    ordinal: i64,
    anchor_ms: i64,
    anchor_samples: i64,
    invoked_at_ms: i64,
}

#[derive(Debug)]
struct CapturedSnapshot {
    capture: SnapshotCapture,
    display_index: usize,
    completed_at_ms: i64,
    image_path: PathBuf,
    sha256: String,
    width: u32,
    height: u32,
}

type SharedWriter = Arc<Mutex<Option<WavWriter<std::io::BufWriter<fs::File>>>>>;

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SessionView {
    feedback_id: i64,
    project_id: i64,
    started_at_ms: i64,
    elapsed_ms: i64,
    audio_samples: u64,
    sample_rate: u32,
    snapshot_count: usize,
    paused: bool,
    muted: bool,
    input_device_id: String,
    input_device_name: String,
    phase: &'static str,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AudioInputView {
    id: String,
    name: String,
    is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AudioInputsView {
    devices: Vec<AudioInputView>,
    selected_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SnapshotView {
    feedback_id: i64,
    project_id: i64,
    display_index: usize,
    ordinal: i64,
    anchor_ms: i64,
    anchor_samples: i64,
    invoked_at_ms: i64,
    completed_at_ms: i64,
    image_path: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FinishedView {
    feedback_id: i64,
    project_id: i64,
    duration_ms: i64,
    audio_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TranscriptView {
    feedback_id: i64,
    project_id: i64,
    segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize)]
struct TranscriptSegment {
    start_ms: i64,
    end_ms: i64,
    text: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnnotationTool {
    Pointer,
    Pen,
    Line,
    Arrow,
    Rectangle,
    Ellipse,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AnnotationPoint {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AnnotationStroke {
    display_index: usize,
    tool: AnnotationTool,
    color: String,
    width: f32,
    points: Vec<AnnotationPoint>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct Region {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[tauri::command]
pub(crate) fn feedback_preflight() -> FeedbackPreflight {
    let microphone_available = cpal::default_host().default_input_device().is_some();
    // Keep the permission result separate from display discovery. A disconnected display or a
    // transient xcap lookup failure must not be presented as a denied permission.
    let screen_capture_authorized = screen_capture_authorized();
    let display_available = Monitor::all().is_ok_and(|monitors| !monitors.is_empty());
    let screen_capture_available = screen_capture_authorized && display_available;
    let model_path = model_path();
    let model_installed = model_path
        .metadata()
        .is_ok_and(|metadata| metadata.len() == MODEL_BYTES)
        && sha256_file(&model_path).is_ok_and(|sha| sha == MODEL_SHA256);
    let message = if !microphone_available {
        Some("No microphone is available. Connect or enable one, then retry.".into())
    } else if !screen_capture_authorized {
        Some(BLOCKED_CAPTURE_MESSAGE.into())
    } else if !display_available {
        Some(NO_DISPLAY_MESSAGE.into())
    } else if !model_installed {
        Some("Install the local transcription model before recording.".into())
    } else {
        None
    };
    FeedbackPreflight {
        supported: true,
        platform: std::env::consts::OS,
        microphone_available,
        screen_capture_authorized,
        display_available,
        screen_capture_available,
        model_installed,
        model_name: MODEL_NAME,
        model_size_bytes: MODEL_BYTES,
        model_path: model_path.to_string_lossy().into_owned(),
        message,
    }
}

#[tauri::command]
pub(crate) fn feedback_request_screen_access() -> Result<FeedbackPreflight, String> {
    request_screen_access_inner()?;
    Ok(feedback_preflight())
}

/// macOS gates display capture behind TCC; Windows does not gate it at all.
#[cfg(target_os = "macos")]
fn screen_capture_authorized() -> bool {
    CGPreflightScreenCaptureAccess()
}

#[cfg(windows)]
fn screen_capture_authorized() -> bool {
    true
}

#[cfg(target_os = "macos")]
const BLOCKED_CAPTURE_MESSAGE: &str = "macOS is blocking this exact Workman app. Remove any older Workman entry from Screen Recording, add the current app again, then fully quit and reopen Workman.";
#[cfg(windows)]
const BLOCKED_CAPTURE_MESSAGE: &str = "Windows is not reporting a capturable display. Check that a display is attached, then try again.";

#[cfg(target_os = "macos")]
const NO_DISPLAY_MESSAGE: &str = "Screen Recording is allowed, but Workman could not find an active display. Connect a display, then check again.";
#[cfg(windows)]
const NO_DISPLAY_MESSAGE: &str =
    "Workman could not find an active display. Connect a display, then check again.";

#[cfg(target_os = "macos")]
fn request_screen_access_inner() -> Result<(), String> {
    if !CGPreflightScreenCaptureAccess() {
        let granted = CGRequestScreenCaptureAccess();
        // macOS only presents the consent prompt once. After the user has made a choice,
        // requesting again is a no-op, so take them to the exact privacy pane instead.
        if !granted {
            open_screen_recording_settings()?;
        }
    }
    Ok(())
}

/// Windows grants display capture without a prompt. The microphone can still be
/// switched off for the account, so offer that pane when no input device exists.
#[cfg(windows)]
fn request_screen_access_inner() -> Result<(), String> {
    if cpal::default_host().default_input_device().is_none() {
        open_screen_recording_settings()?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_screen_recording_settings() -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .arg(SCREEN_RECORDING_SETTINGS_URL)
        .status()
        .map_err(|error| format!("Could not open Screen Recording settings: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Could not open Screen Recording settings (open exited with {status})."
        ))
    }
}

/// `ms-settings:` links are shell protocol handlers, so they need the shell to
/// resolve them; the empty argument is the title `start` would otherwise eat.
#[cfg(windows)]
fn open_screen_recording_settings() -> Result<(), String> {
    let status = Command::new("cmd")
        .args(["/c", "start", "", SCREEN_RECORDING_SETTINGS_URL])
        .status()
        .map_err(|error| format!("Could not open microphone settings: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Could not open microphone settings (start exited with {status})."
        ))
    }
}

#[tauri::command]
pub(crate) async fn feedback_install_model(app: AppHandle) -> Result<FeedbackPreflight, String> {
    let path = model_path();
    tauri::async_runtime::spawn_blocking(move || download_model(&app, &path))
        .await
        .map_err(|error| error.to_string())??;
    Ok(feedback_preflight())
}

#[tauri::command]
pub(crate) fn feedback_start(
    feedback_id: i64,
    project_id: i64,
    media_dir: String,
    shortcuts: Option<HashMap<String, String>>,
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let preflight = feedback_preflight();
    if !preflight.microphone_available
        || !preflight.screen_capture_available
        || !preflight.model_installed
    {
        return Err(preflight
            .message
            .unwrap_or_else(|| "Recorded Feedback is not ready.".into()));
    }
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    if active.is_some() {
        return Err("A feedback recording is already in progress.".into());
    }
    let media_dir = validate_media_dir(feedback_id, &media_dir)?;
    let audio_path = media_dir.join("audio.wav");
    let audio = start_audio(&app, feedback_id, project_id, &audio_path)?;
    let (registered_shortcuts, unavailable_shortcuts) = register_shortcuts(&app, shortcuts);
    let display_ids = match create_feedback_panels(&app) {
        Ok(display_ids) => display_ids,
        Err(error) => {
            unregister_shortcuts(&app, &registered_shortcuts);
            close_feedback_panels(&app);
            drop(audio.stream);
            finalize_writer(&audio.writer)?;
            return Err(error);
        }
    };
    let session = FeedbackSession {
        feedback_id,
        project_id,
        media_dir,
        started: Instant::now(),
        paused_at: None,
        paused_duration: Duration::ZERO,
        started_at_ms: now_millis(),
        sample_rate: audio.sample_rate,
        audio_samples: audio.samples,
        audio_path,
        writer: audio.writer,
        stream: audio.stream,
        audio_controls: audio.controls,
        input_device_id: audio.input.id,
        input_device_name: audio.input.name,
        snapshot_count: 0,
        annotations: Vec::new(),
        tool: AnnotationTool::Pointer,
        color: "#ff4d5e".into(),
        width: 4.0,
        registered_shortcuts,
        display_ids,
        capture_in_progress: false,
    };
    let view = session_view(&session, "recording", None);
    let feedback_id = session.feedback_id;
    let project_id = session.project_id;
    *active = Some(session);
    drop(active);
    let _ = app.emit(EVENT_STATUS, &view);
    if !unavailable_shortcuts.is_empty() {
        let _ = app.emit(
            EVENT_ERROR,
            json!({
                "feedback_id": feedback_id, "project_id": project_id,
                "code": "shortcuts_unavailable",
                "message": format!(
                    "Another app already owns {}, so {} off for this recording. Use the toolbar, or pick different chords in Settings.",
                    unavailable_shortcuts.join(", "),
                    if unavailable_shortcuts.len() == 1 { "it is" } else { "they are" }
                )
            }),
        );
    }
    Ok(view)
}

#[tauri::command]
pub(crate) fn feedback_status(
    state: State<'_, FeedbackState>,
) -> Result<Option<SessionView>, String> {
    let active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    Ok(active
        .as_ref()
        .map(|session| session_view(session, "recording", None)))
}

#[tauri::command]
pub(crate) fn feedback_audio_inputs(
    state: State<'_, FeedbackState>,
) -> Result<AudioInputsView, String> {
    let selected_id = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?
        .as_ref()
        .ok_or("No feedback recording is active.")?
        .input_device_id
        .clone();
    let devices = audio_input_candidates()?
        .into_iter()
        .map(|candidate| candidate.view)
        .collect();
    Ok(AudioInputsView {
        devices,
        selected_id,
    })
}

#[tauri::command]
pub(crate) fn feedback_toggle_pause(
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_mut().ok_or("No feedback recording is active.")?;
    if session.capture_in_progress {
        return Err("Wait for the current snapshot to finish saving.".into());
    }
    let now = Instant::now();
    let paused = if let Some(paused_at) = session.paused_at.take() {
        session.paused_duration += now.saturating_duration_since(paused_at);
        session
            .audio_controls
            .reset_clock
            .store(true, Ordering::Release);
        session
            .audio_controls
            .paused
            .store(false, Ordering::Release);
        false
    } else {
        session.audio_controls.paused.store(true, Ordering::Release);
        session
            .audio_controls
            .reset_clock
            .store(true, Ordering::Release);
        session.paused_at = Some(now);
        true
    };
    let view = session_view(session, "recording", None);
    let _ = append_journal(
        &session.media_dir.join("events.jsonl"),
        &json!({
            "event": if paused { "paused" } else { "resumed" },
            "elapsed_ms": view.elapsed_ms,
            "samples": view.audio_samples,
            "at": now_millis()
        }),
    );
    drop(active);
    let _ = app.emit(EVENT_STATUS, &view);
    Ok(view)
}

#[tauri::command]
pub(crate) fn feedback_toggle_mute(
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_ref().ok_or("No feedback recording is active.")?;
    let muted = !session.audio_controls.muted.load(Ordering::Acquire);
    session.audio_controls.muted.store(muted, Ordering::Release);
    let view = session_view(session, "recording", None);
    let _ = append_journal(
        &session.media_dir.join("events.jsonl"),
        &json!({
            "event": if muted { "microphone_muted" } else { "microphone_unmuted" },
            "elapsed_ms": view.elapsed_ms,
            "samples": view.audio_samples,
            "at": now_millis()
        }),
    );
    drop(active);
    let _ = app.emit(EVENT_STATUS, &view);
    Ok(view)
}

#[tauri::command]
pub(crate) fn feedback_set_input_device(
    device_id: String,
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let input = resolve_audio_input(Some(&device_id), true)?;
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_mut().ok_or("No feedback recording is active.")?;
    if session.input_device_id == input.view.id {
        return Ok(session_view(session, "recording", None));
    }

    let was_paused = session.paused_at.is_some();
    let switch_started = Instant::now();
    session.audio_controls.paused.store(true, Ordering::Release);
    session
        .audio_controls
        .reset_clock
        .store(true, Ordering::Release);
    let stream = match create_audio_stream(
        &app,
        session.feedback_id,
        session.project_id,
        &input.device,
        session.sample_rate,
        session.writer.clone(),
        session.audio_samples.clone(),
        session.audio_controls.clone(),
    ) {
        Ok(stream) => stream,
        Err(error) => {
            session
                .audio_controls
                .reset_clock
                .store(true, Ordering::Release);
            session
                .audio_controls
                .paused
                .store(was_paused, Ordering::Release);
            return Err(error);
        }
    };
    let previous_stream = std::mem::replace(&mut session.stream, stream);
    drop(previous_stream);
    if !was_paused {
        session.paused_duration += switch_started.elapsed();
        session
            .audio_controls
            .reset_clock
            .store(true, Ordering::Release);
        session
            .audio_controls
            .paused
            .store(false, Ordering::Release);
    }
    session.input_device_id = input.view.id;
    session.input_device_name = input.view.name;
    let view = session_view(session, "recording", None);
    let _ = append_journal(
        &session.media_dir.join("events.jsonl"),
        &json!({
            "event": "input_device_changed",
            "input_device_id": session.input_device_id,
            "input_device_name": session.input_device_name,
            "elapsed_ms": view.elapsed_ms,
            "samples": view.audio_samples,
            "at": now_millis()
        }),
    );
    drop(active);
    let _ = app.emit(EVENT_STATUS, &view);
    Ok(view)
}

#[tauri::command]
pub(crate) fn feedback_raise_toolbar(app: AppHandle) -> Result<(), String> {
    raise_toolbar(&app)
}

#[cfg(target_os = "macos")]
fn raise_toolbar(app: &AppHandle) -> Result<(), String> {
    let toolbar = app
        .get_webview_panel("feedback-toolbar")
        .map_err(|error| format!("{error:?}"))?;
    // Reassert after the toolbar webview has mounted. Ordering the panel during construction can
    // be too early for AppKit when Workman is not the active application.
    toolbar.set_level(PanelLevel::Custom(1001).value());
    toolbar.show();
    toolbar.order_front_regardless();
    Ok(())
}

/// Windows drops a window out of the topmost band whenever another app is
/// activated, so reassert it once the toolbar webview has mounted.
#[cfg(windows)]
fn raise_toolbar(app: &AppHandle) -> Result<(), String> {
    let toolbar = app
        .get_webview_window("feedback-toolbar")
        .ok_or("The recorder toolbar is not open.")?;
    toolbar
        .set_always_on_top(true)
        .map_err(|error| error.to_string())?;
    toolbar.show().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn feedback_set_tool(
    tool: AnnotationTool,
    color: String,
    width: f32,
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    if !is_color(&color) || !(1.0..=16.0).contains(&width) {
        return Err("Annotation color or width is invalid.".into());
    }
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_mut().ok_or("No feedback recording is active.")?;
    session.tool = tool;
    session.color = color.clone();
    session.width = width;
    set_overlays_interactive(&app, tool != AnnotationTool::Pointer)?;
    let payload = json!({ "tool": tool, "color": color, "width": width });
    let _ = app.emit(EVENT_TOOL, payload);
    Ok(session_view(session, "recording", None))
}

#[tauri::command]
pub(crate) fn feedback_record_stroke(
    stroke: AnnotationStroke,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    validate_stroke(&stroke)?;
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_mut().ok_or("No feedback recording is active.")?;
    if stroke.tool == AnnotationTool::Pointer {
        return Err("Pointer mode cannot draw.".into());
    }
    session.annotations.push(stroke);
    Ok(session_view(session, "recording", None))
}

#[tauri::command]
pub(crate) fn feedback_undo(
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_mut().ok_or("No feedback recording is active.")?;
    session.annotations.pop();
    let _ = app.emit(EVENT_ANNOTATIONS, &session.annotations);
    Ok(session_view(session, "recording", None))
}

#[tauri::command]
pub(crate) fn feedback_clear(
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_mut().ok_or("No feedback recording is active.")?;
    session.annotations.clear();
    let _ = app.emit(EVENT_ANNOTATIONS, &session.annotations);
    Ok(session_view(session, "recording", None))
}

#[tauri::command]
pub(crate) fn feedback_begin_region(
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_ref().ok_or("No feedback recording is active.")?;
    if session.paused_at.is_some() {
        return Err("Resume feedback before selecting a snapshot region.".into());
    }
    set_overlays_interactive(&app, true)?;
    let _ = app.emit(EVENT_REGION, json!({ "selecting": true }));
    Ok(session_view(session, "recording", None))
}

#[tauri::command]
pub(crate) fn feedback_cancel_region(
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<SessionView, String> {
    let active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_ref().ok_or("No feedback recording is active.")?;
    set_overlays_interactive(&app, session.tool != AnnotationTool::Pointer)?;
    let _ = app.emit(EVENT_REGION, json!({ "selecting": false }));
    Ok(session_view(session, "recording", None))
}

#[tauri::command]
pub(crate) async fn feedback_capture_snapshot(
    display_index: Option<usize>,
    region: Option<Region>,
    app: AppHandle,
) -> Result<SnapshotView, String> {
    let selecting_region = region.is_some();
    let capture_app = app.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        capture_snapshot(display_index, region, &capture_app)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            clear_active_snapshot_reservation(&app);
            Err(format!("Snapshot worker stopped: {error}"))
        }
    };
    if selecting_region {
        let restore_app = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = restore_overlay_interaction(&restore_app);
        });
        let _ = app.emit(EVENT_REGION, json!({ "selecting": false }));
    }
    result
}

fn capture_snapshot(
    display_index: Option<usize>,
    region: Option<Region>,
    app: &AppHandle,
) -> Result<SnapshotView, String> {
    let state = app.state::<FeedbackState>();
    let capture = {
        let mut active = state
            .session
            .lock()
            .map_err(|_| "feedback state is unavailable")?;
        let session = active.as_mut().ok_or("No feedback recording is active.")?;
        if session.paused_at.is_some() {
            return Err("Resume feedback before taking a snapshot.".into());
        }
        if session.capture_in_progress {
            return Err("A snapshot is already being saved.".into());
        }
        session.capture_in_progress = true;
        SnapshotCapture {
            feedback_id: session.feedback_id,
            project_id: session.project_id,
            media_dir: session.media_dir.clone(),
            display_ids: session.display_ids.clone(),
            annotations: session.annotations.clone(),
            ordinal: session.snapshot_count as i64,
            anchor_ms: session_elapsed_ms(session),
            anchor_samples: session.audio_samples.load(Ordering::Relaxed) as i64,
            invoked_at_ms: now_millis(),
        }
    };
    let feedback_id = capture.feedback_id;
    let ordinal = capture.ordinal;
    let result = capture_snapshot_image(display_index, region, app, capture)
        .and_then(|captured| commit_snapshot(app, captured));
    if result.is_err() {
        clear_snapshot_reservation(app, feedback_id, ordinal);
    }
    result
}

fn capture_snapshot_image(
    display_index: Option<usize>,
    region: Option<Region>,
    app: &AppHandle,
    capture: SnapshotCapture,
) -> Result<CapturedSnapshot, String> {
    let monitors =
        Monitor::all().map_err(|error| format!("Could not inspect displays: {error}"))?;
    let current_display_ids = monitors
        .iter()
        .map(|monitor| monitor.id().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected_display_ids = capture.display_ids.clone();
    let mut observed_display_ids = current_display_ids.clone();
    expected_display_ids.sort_unstable();
    observed_display_ids.sort_unstable();
    if expected_display_ids != observed_display_ids {
        return Err(
            "The display layout changed during recording. Finish this recording and start a new one before taking another snapshot."
                .into(),
        );
    }
    let display_index = display_index
        .or_else(|| {
            active_toolbar_display_id(app, &monitors).and_then(|id| {
                capture
                    .display_ids
                    .iter()
                    .position(|candidate| *candidate == id)
            })
        })
        .unwrap_or(0);
    let display_id = *capture
        .display_ids
        .get(display_index)
        .ok_or("The selected display is no longer available.")?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.id().is_ok_and(|id| id == display_id))
        .ok_or("The selected display is no longer available.")?;
    let scale = monitor.scale_factor().unwrap_or(1.0).max(1.0);
    let mut image = monitor
        .capture_image()
        .map_err(|error| format!("Snapshot failed: {error}"))?;
    composite_annotations(&mut image, &capture.annotations, display_index, scale);
    if let Some(region) = region {
        let (x, y, width, height) = pixel_region(region, scale, image.width(), image.height())?;
        image = imageops::crop_imm(&image, x, y, width, height).to_image();
    }
    let path = capture
        .media_dir
        .join(format!("snapshot-{:03}.png", capture.ordinal + 1));
    write_png_atomic(&image, &path)?;
    let sha256 = sha256_file(&path).inspect_err(|_| {
        let _ = fs::remove_file(&path);
    })?;
    Ok(CapturedSnapshot {
        capture,
        display_index,
        completed_at_ms: now_millis(),
        image_path: path,
        sha256,
        width: image.width(),
        height: image.height(),
    })
}

fn commit_snapshot(app: &AppHandle, captured: CapturedSnapshot) -> Result<SnapshotView, String> {
    if let Err(error) = append_journal(
        &captured.capture.media_dir.join("events.jsonl"),
        &json!({
            "event": "snapshot", "ordinal": captured.capture.ordinal,
            "display_index": captured.display_index,
            "anchor_ms": captured.capture.anchor_ms,
            "anchor_samples": captured.capture.anchor_samples,
            "invoked_at_ms": captured.capture.invoked_at_ms,
            "completed_at_ms": captured.completed_at_ms,
            "image_path": captured.image_path, "sha256": captured.sha256,
            "width": captured.width, "height": captured.height
        }),
    ) {
        let _ = fs::remove_file(&captured.image_path);
        return Err(error.to_string());
    }
    let state = app.state::<FeedbackState>();
    let mut active = state.session.lock().map_err(|_| {
        let _ = fs::remove_file(&captured.image_path);
        "feedback state is unavailable"
    })?;
    let Some(session) = active.as_mut() else {
        let _ = fs::remove_file(&captured.image_path);
        return Err("The feedback recording ended before the snapshot was saved.".into());
    };
    if session.feedback_id != captured.capture.feedback_id
        || !session.capture_in_progress
        || session.snapshot_count as i64 != captured.capture.ordinal
    {
        let _ = fs::remove_file(&captured.image_path);
        return Err("The feedback recording changed before the snapshot was saved.".into());
    }
    session.snapshot_count += 1;
    session.capture_in_progress = false;
    let view = SnapshotView {
        feedback_id: captured.capture.feedback_id,
        project_id: captured.capture.project_id,
        display_index: captured.display_index,
        ordinal: captured.capture.ordinal,
        anchor_ms: captured.capture.anchor_ms,
        anchor_samples: captured.capture.anchor_samples,
        invoked_at_ms: captured.capture.invoked_at_ms,
        completed_at_ms: captured.completed_at_ms,
        image_path: captured.image_path.to_string_lossy().into_owned(),
        sha256: captured.sha256,
    };
    let _ = app.emit(EVENT_SNAPSHOT, &view);
    Ok(view)
}

fn clear_snapshot_reservation(app: &AppHandle, feedback_id: i64, ordinal: i64) {
    let state = app.state::<FeedbackState>();
    if let Ok(mut active) = state.session.lock()
        && let Some(session) = active.as_mut()
        && session.feedback_id == feedback_id
        && session.snapshot_count as i64 == ordinal
    {
        session.capture_in_progress = false;
    }
}

fn clear_active_snapshot_reservation(app: &AppHandle) {
    let state = app.state::<FeedbackState>();
    if let Ok(mut active) = state.session.lock()
        && let Some(session) = active.as_mut()
    {
        session.capture_in_progress = false;
    }
}

fn active_toolbar_display_id(app: &AppHandle, monitors: &[Monitor]) -> Option<u32> {
    let monitor = app
        .get_webview_window("feedback-toolbar")?
        .current_monitor()
        .ok()??;
    let scale = monitor.scale_factor();
    let logical_x = (monitor.position().x as f64 / scale).round() as i32;
    let logical_y = (monitor.position().y as f64 / scale).round() as i32;
    monitors
        .iter()
        .find(|candidate| {
            candidate.x().is_ok_and(|x| x == logical_x)
                && candidate.y().is_ok_and(|y| y == logical_y)
        })
        .and_then(|monitor| monitor.id().ok())
}

fn restore_overlay_interaction(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<FeedbackState>();
    let active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_ref().ok_or("No feedback recording is active.")?;
    set_overlays_interactive(app, session.tool != AnnotationTool::Pointer)
}

#[tauri::command]
pub(crate) fn feedback_abort(
    feedback_id: i64,
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<bool, String> {
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let Some(session) = active.as_ref() else {
        return Ok(false);
    };
    if session.feedback_id != feedback_id {
        return Err("A different feedback recording is active.".into());
    }
    let session = active.take().expect("active session checked above");
    drop(active);
    let duration_ms = session_elapsed_ms(&session);
    unregister_shortcuts(&app, &session.registered_shortcuts);
    close_feedback_panels(&app);
    drop(session.stream);
    let finalize_result = finalize_writer(&session.writer);
    let _ = append_journal(
        &session.media_dir.join("events.jsonl"),
        &json!({
            "event": "interrupted", "reason": "native_error",
            "samples": session.audio_samples.load(Ordering::Relaxed),
            "sample_rate": session.sample_rate,
            "duration_ms": duration_ms, "at": now_millis()
        }),
    );
    finalize_result.map(|_| true)
}

#[tauri::command]
pub(crate) fn feedback_finish(
    app: AppHandle,
    state: State<'_, FeedbackState>,
) -> Result<FinishedView, String> {
    let mut active = state
        .session
        .lock()
        .map_err(|_| "feedback state is unavailable")?;
    let session = active.as_ref().ok_or("No feedback recording is active.")?;
    if session.capture_in_progress {
        return Err("Wait for the current snapshot to finish saving.".into());
    }
    let session = active.take().expect("active session checked above");
    drop(active);
    let duration_ms = session_elapsed_ms(&session);
    unregister_shortcuts(&app, &session.registered_shortcuts);
    close_feedback_panels(&app);
    focus_main_window(&app);
    drop(session.stream);
    if let Err(error) = finalize_writer(&session.writer) {
        let _ = app.emit(
            EVENT_ERROR,
            json!({
                "feedback_id": session.feedback_id, "project_id": session.project_id,
                "code": "audio_finalize_failed", "message": error
            }),
        );
        return Err("Could not finish the feedback audio.".into());
    }
    let _ = append_journal(
        &session.media_dir.join("events.jsonl"),
        &json!({
            "event": "audio_finished", "duration_ms": duration_ms, "audio_path": session.audio_path,
            "samples": session.audio_samples.load(Ordering::Relaxed), "sample_rate": session.sample_rate,
            "at": now_millis()
        }),
    );
    let finished = FinishedView {
        feedback_id: session.feedback_id,
        project_id: session.project_id,
        duration_ms,
        audio_path: Some(session.audio_path.to_string_lossy().into_owned()),
    };
    let _ = app.emit(EVENT_FINISHED, &finished);
    let feedback_id = session.feedback_id;
    let project_id = session.project_id;
    let audio_path = session.audio_path;
    let transcription_app = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name(format!("feedback-whisper-{feedback_id}"))
        .spawn(move || match transcribe(&audio_path, session.sample_rate) {
            Ok(segments) => {
                let payload = TranscriptView {
                    feedback_id,
                    project_id,
                    segments,
                };
                let _ = transcription_app.emit(EVENT_TRANSCRIPT, payload);
            }
            Err(error) => {
                let _ = transcription_app.emit(
                    EVENT_ERROR,
                    json!({
                        "feedback_id": feedback_id, "project_id": project_id,
                        "code": "transcription_failed", "message": error
                    }),
                );
            }
        })
    {
        let message = error.to_string();
        let _ = app.emit(
            EVENT_ERROR,
            json!({
                "feedback_id": feedback_id, "project_id": project_id,
                "code": "transcription_failed", "message": message
            }),
        );
        return Err(message);
    }
    Ok(finished)
}

fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn start_audio(
    app: &AppHandle,
    feedback_id: i64,
    project_id: i64,
    path: &Path,
) -> Result<StartedAudio, String> {
    let input = resolve_audio_input(None, false)?;
    let supported = input
        .device
        .default_input_config()
        .map_err(|error| format!("Could not open the microphone: {error}"))?;
    let sample_rate = supported.sample_rate();
    let writer = Arc::new(Mutex::new(Some(
        WavWriter::create(
            path,
            WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: 32,
                sample_format: WavSampleFormat::Float,
            },
        )
        .map_err(|error| error.to_string())?,
    )));
    let samples = Arc::new(AtomicU64::new(0));
    let controls = Arc::new(AudioControls::default());
    let stream = create_audio_stream_with_config(
        app,
        feedback_id,
        project_id,
        &input.device,
        supported,
        writer.clone(),
        samples.clone(),
        controls.clone(),
    )?;
    Ok(StartedAudio {
        stream,
        writer,
        sample_rate,
        samples,
        controls,
        input: input.view,
    })
}

fn create_audio_stream(
    app: &AppHandle,
    feedback_id: i64,
    project_id: i64,
    device: &cpal::Device,
    sample_rate: u32,
    writer: SharedWriter,
    samples: Arc<AtomicU64>,
    controls: Arc<AudioControls>,
) -> Result<Stream, String> {
    let supported = device
        .supported_input_configs()
        .map_err(|error| format!("Could not inspect that microphone: {error}"))?
        .filter_map(|range| range.try_with_sample_rate(sample_rate))
        .max_by_key(|config| {
            let format_rank = match config.sample_format() {
                SampleFormat::F32 => 3,
                SampleFormat::I16 => 2,
                SampleFormat::U16 => 1,
                _ => 0,
            };
            (format_rank, config.channels())
        })
        .ok_or_else(|| {
            let name = device
                .description()
                .map(|description| description.name().to_owned())
                .unwrap_or_else(|_| "That microphone".into());
            format!("{name} does not support this recording's sample rate ({sample_rate} Hz).")
        })?;
    create_audio_stream_with_config(
        app,
        feedback_id,
        project_id,
        device,
        supported,
        writer,
        samples,
        controls,
    )
}

fn create_audio_stream_with_config(
    app: &AppHandle,
    feedback_id: i64,
    project_id: i64,
    device: &cpal::Device,
    supported: cpal::SupportedStreamConfig,
    writer: SharedWriter,
    samples: Arc<AtomicU64>,
    controls: Arc<AudioControls>,
) -> Result<Stream, String> {
    let channels = supported.channels() as usize;
    let config: StreamConfig = supported.into();
    let error_app = app.clone();
    let error_handler = move |error| {
        let _ = error_app.emit(
            EVENT_ERROR,
            json!({
                "feedback_id": feedback_id, "project_id": project_id,
                "code": "microphone_disconnected",
                "message": format!("Microphone disconnected: {error}")
            }),
        );
    };
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config,
            channels,
            writer.clone(),
            samples.clone(),
            controls.clone(),
            error_handler,
            |value| value,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config,
            channels,
            writer.clone(),
            samples.clone(),
            controls.clone(),
            error_handler,
            |value| value as f32 / i16::MAX as f32,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config,
            channels,
            writer.clone(),
            samples.clone(),
            controls.clone(),
            error_handler,
            |value| value as f32 / u16::MAX as f32 * 2.0 - 1.0,
        ),
        format => return Err(format!("Unsupported microphone sample format: {format}")),
    }?;
    stream
        .play()
        .map_err(|error| format!("Could not start the microphone: {error}"))?;
    Ok(stream)
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    writer: SharedWriter,
    samples: Arc<AtomicU64>,
    controls: Arc<AudioControls>,
    error_handler: impl FnMut(cpal::Error) + Send + 'static,
    convert: impl Fn(T) -> f32 + Send + 'static,
) -> Result<Stream, String>
where
    T: cpal::SizedSample + Copy,
{
    let sample_rate = config.sample_rate as f64;
    let mut last_callback: Option<Instant> = None;
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                let now = Instant::now();
                if controls.reset_clock.swap(false, Ordering::AcqRel) {
                    last_callback = None;
                }
                if controls.paused.load(Ordering::Acquire) {
                    last_callback = Some(now);
                    return;
                }
                let frames = data.len() / channels.max(1);
                let muted = controls.muted.load(Ordering::Acquire);
                if let Ok(mut guard) = writer.lock()
                    && let Some(writer) = guard.as_mut()
                {
                    if let Some(last) = last_callback {
                        let elapsed_frames =
                            (now.duration_since(last).as_secs_f64() * sample_rate).round() as usize;
                        if elapsed_frames > frames + (sample_rate as usize / 10) {
                            let gap = (elapsed_frames - frames).min(sample_rate as usize * 5);
                            for _ in 0..gap {
                                let _ = writer.write_sample(0.0_f32);
                            }
                            samples.fetch_add(gap as u64, Ordering::Relaxed);
                        }
                    }
                    for frame in data.chunks(channels.max(1)) {
                        let mono = if muted {
                            0.0
                        } else {
                            frame.iter().copied().map(&convert).sum::<f32>() / frame.len() as f32
                        };
                        let _ = writer.write_sample(mono.clamp(-1.0, 1.0));
                    }
                    samples.fetch_add(frames as u64, Ordering::Relaxed);
                }
                last_callback = Some(now);
            },
            error_handler,
            None,
        )
        .map_err(|error| format!("Could not create the microphone stream: {error}"))
}

fn audio_input_candidates() -> Result<Vec<AudioInputCandidate>, String> {
    let host = cpal::default_host();
    let default_device = host.default_input_device();
    let devices = host
        .input_devices()
        .map_err(|error| format!("Could not list microphones: {error}"))?;
    let mut candidates = Vec::new();
    for (index, device) in devices.enumerate() {
        let name = device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| format!("Microphone {}", index + 1));
        let id = device
            .id()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| format!("unavailable-{index}"));
        let is_default = default_device
            .as_ref()
            .is_some_and(|default| default == &device);
        candidates.push(AudioInputCandidate {
            device,
            view: AudioInputView {
                id,
                name,
                is_default,
            },
        });
    }
    if candidates.is_empty() {
        return Err("No microphone is available.".into());
    }
    Ok(candidates)
}

fn resolve_audio_input(
    requested_id: Option<&str>,
    require_requested: bool,
) -> Result<AudioInputCandidate, String> {
    let mut candidates = audio_input_candidates()?;
    let requested = requested_id.and_then(|id| {
        candidates
            .iter()
            .position(|candidate| candidate.view.id == id)
    });
    if requested_id.is_some() && requested.is_none() && require_requested {
        return Err("That microphone is no longer available.".into());
    }
    let index = requested
        .or_else(|| {
            candidates
                .iter()
                .position(|candidate| candidate.view.is_default)
        })
        .unwrap_or(0);
    Ok(candidates.remove(index))
}

fn finalize_writer(writer: &SharedWriter) -> Result<(), String> {
    let writer = writer
        .lock()
        .map_err(|_| "audio writer is unavailable")?
        .take();
    if let Some(writer) = writer {
        writer.finalize().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn create_feedback_panels(app: &AppHandle) -> Result<Vec<u32>, String> {
    close_feedback_panels(app);
    let monitors = Monitor::all().map_err(|error| error.to_string())?;
    let display_ids = monitors
        .iter()
        .map(|monitor| monitor.id().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    for (index, monitor) in monitors.iter().enumerate() {
        let position = LogicalPosition::new(
            monitor.x().map_err(|error| error.to_string())? as f64,
            monitor.y().map_err(|error| error.to_string())? as f64,
        );
        let size = LogicalSize::new(
            monitor.width().map_err(|error| error.to_string())? as f64,
            monitor.height().map_err(|error| error.to_string())? as f64,
        );
        build_overlay_window(app, &format!("feedback-overlay-{index}"), position, size)?;
    }
    let primary = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or("No display is available.")?;
    let position_x = primary.x().map_err(|error| error.to_string())? as f64;
    let position_y = primary.y().map_err(|error| error.to_string())? as f64;
    let monitor_width = primary.width().map_err(|error| error.to_string())? as f64;
    let width = 960.0_f64.min((monitor_width - 32.0).max(720.0));
    let x = position_x + ((monitor_width - width) / 2.0).max(16.0);
    let y = position_y + 28.0;
    build_toolbar_window(
        app,
        LogicalPosition::new(x, y),
        LogicalSize::new(width, 60.0),
    )?;
    Ok(display_ids)
}

/// The recorder floats above every space as a non-activating NSPanel on macOS.
#[cfg(target_os = "macos")]
fn build_overlay_window(
    app: &AppHandle,
    label: &str,
    position: LogicalPosition<f64>,
    size: LogicalSize<f64>,
) -> Result<(), String> {
    let overlay = PanelBuilder::<_, FeedbackPanel>::new(app, label)
        .url(WebviewUrl::App("index.html".into()))
        .position(Position::Logical(position))
        .size(Size::Logical(size))
        .level(PanelLevel::ScreenSaver)
        .has_shadow(false)
        .opaque(false)
        .transparent(true)
        .hides_on_deactivate(false)
        .ignores_mouse_events(true)
        .works_when_modal(true)
        .no_activate(true)
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .stationary()
                .ignores_cycle(),
        )
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|window| {
            window
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .focusable(false)
                .content_protected(true)
        })
        .build()
        .map_err(|error| error.to_string())?;
    overlay.show();
    Ok(())
}

/// Windows has no panel class, so the overlay is an ordinary borderless window
/// that stays on top and passes every click through to whatever is beneath it.
#[cfg(windows)]
fn build_overlay_window(
    app: &AppHandle,
    label: &str,
    position: LogicalPosition<f64>,
    size: LogicalSize<f64>,
) -> Result<(), String> {
    let app = app.clone();
    let label = label.to_owned();
    // WebView2 finishes creating on the main thread's message loop, and this runs
    // from a command occupying that loop, so building here waits on a message that
    // can never arrive and Windows eventually kills the app as unresponsive. Hand
    // the build to the async runtime and let the loop deliver it; the recorder
    // only needs the window once its own webview mounts and calls back in.
    tauri::async_runtime::spawn(async move {
        let built =
            tauri::WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
                .position(position.x, position.y)
                .inner_size(size.width, size.height)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .resizable(false)
                .shadow(false)
                .content_protected(true)
                .build();
        match built {
            Ok(overlay) => {
                let _ = overlay.set_ignore_cursor_events(true);
                let _ = overlay.show();
            }
            Err(error) => {
                let _ = app.emit(
                    EVENT_ERROR,
                    json!({ "code": "overlay_failed", "message": error.to_string() }),
                );
            }
        }
    });
    Ok(())
}

#[cfg(target_os = "macos")]
fn build_toolbar_window(
    app: &AppHandle,
    position: LogicalPosition<f64>,
    size: LogicalSize<f64>,
) -> Result<(), String> {
    let toolbar = PanelBuilder::<_, FeedbackPanel>::new(app, "feedback-toolbar")
        .url(WebviewUrl::App("index.html".into()))
        .position(Position::Logical(position))
        .size(Size::Logical(size))
        .level(PanelLevel::Custom(1001))
        .has_shadow(true)
        .corner_radius(10.0)
        .opaque(false)
        .transparent(true)
        .hides_on_deactivate(false)
        .works_when_modal(true)
        .movable_by_window_background(true)
        .no_activate(true)
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .stationary()
                .ignores_cycle(),
        )
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|window| {
            window
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .focusable(false)
                .content_protected(true)
        })
        .build()
        .map_err(|error| error.to_string())?;
    toolbar.show();
    toolbar.order_front_regardless();
    Ok(())
}

/// The toolbar is the one recorder window that accepts clicks, so unlike the
/// overlays it keeps cursor events and is dragged by its own background.
#[cfg(windows)]
fn build_toolbar_window(
    app: &AppHandle,
    position: LogicalPosition<f64>,
    size: LogicalSize<f64>,
) -> Result<(), String> {
    let app = app.clone();
    // Built off the command thread for the same reason as the overlays.
    tauri::async_runtime::spawn(async move {
        let built = tauri::WebviewWindowBuilder::new(
            &app,
            "feedback-toolbar",
            WebviewUrl::App("index.html".into()),
        )
        .position(position.x, position.y)
        .inner_size(size.width, size.height)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .resizable(false)
        .content_protected(true)
        .build();
        match built {
            Ok(toolbar) => {
                let _ = toolbar.show();
            }
            Err(error) => {
                let _ = app.emit(
                    EVENT_ERROR,
                    json!({ "code": "toolbar_failed", "message": error.to_string() }),
                );
            }
        }
    });
    Ok(())
}

fn close_feedback_panels(app: &AppHandle) {
    let labels = app
        .webview_windows()
        .into_keys()
        .filter(|label| label.starts_with("feedback-"))
        .collect::<Vec<_>>();
    for label in labels {
        close_feedback_window(app, &label);
    }
}

#[cfg(target_os = "macos")]
fn close_feedback_window(app: &AppHandle, label: &str) {
    // tauri-nspanel changes the native Objective-C class behind Tauri's window. Restore
    // that class and unregister the panel before asking Tauri to close the webview;
    // destroying the converted NSPanel directly can raise an Objective-C exception on the
    // next event-loop turn and abort the entire app.
    if let Ok(panel) = app.get_webview_panel(label) {
        panel.hide();
        if let Some(window) = panel.to_window() {
            let _ = window.close();
        }
    } else if let Some(window) = app.get_webview_window(label) {
        let _ = window.close();
    }
}

#[cfg(windows)]
fn close_feedback_window(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.close();
    }
}

#[cfg(target_os = "macos")]
fn set_overlays_interactive(app: &AppHandle, interactive: bool) -> Result<(), String> {
    for (label, _) in app.webview_windows() {
        if label.starts_with("feedback-overlay-") {
            let panel = app
                .get_webview_panel(&label)
                .map_err(|error| format!("{error:?}"))?;
            panel.set_ignores_mouse_events(!interactive);
        }
    }
    let toolbar = app
        .get_webview_panel("feedback-toolbar")
        .map_err(|error| format!("{error:?}"))?;
    toolbar.set_level(PanelLevel::Custom(1001).value());
    toolbar.order_front_regardless();
    Ok(())
}

/// Annotation needs the overlays to accept the cursor; every other moment they
/// must let clicks reach the app being recorded.
#[cfg(windows)]
fn set_overlays_interactive(app: &AppHandle, interactive: bool) -> Result<(), String> {
    for (label, window) in app.webview_windows() {
        if label.starts_with("feedback-overlay-") {
            window
                .set_ignore_cursor_events(!interactive)
                .map_err(|error| error.to_string())?;
        }
    }
    raise_toolbar(app)
}

/// Registers what the platform will give us and reports the rest.
///
/// Global accelerators are first come, first served across the whole machine, so
/// a chord an already running app owns cannot be had. Losing one is not worth
/// abandoning the recording, because every shortcut has a toolbar button: claim
/// what is free and hand back the losers for the session to report.
fn register_shortcuts(
    app: &AppHandle,
    requested: Option<HashMap<String, String>>,
) -> (Vec<String>, Vec<String>) {
    let values = if let Some(requested) = requested {
        DEFAULT_SHORTCUTS
            .iter()
            .filter_map(|(action, _)| {
                requested
                    .get(*action)
                    .map(|value| ((*action).to_owned(), value.clone()))
            })
            .collect::<Vec<_>>()
    } else {
        DEFAULT_SHORTCUTS
            .iter()
            .map(|(action, fallback)| ((*action).to_owned(), (*fallback).to_owned()))
            .collect::<Vec<_>>()
    };
    let manager = app.global_shortcut();
    // Releasing a shortcut is best effort, so a session that ended badly can leave
    // one claimed by this process for the rest of its life. Reclaim ours first.
    for (_, shortcut) in &values {
        if manager.is_registered(shortcut.as_str()) {
            let _ = manager.unregister(shortcut.as_str());
        }
    }
    let mut registered = Vec::new();
    let mut unavailable = Vec::new();
    for (action, shortcut) in values {
        let emitted_action = action.clone();
        match manager.on_shortcut(shortcut.as_str(), move |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                let _ = app.emit(EVENT_SHORTCUT, &emitted_action);
            }
        }) {
            Ok(()) => registered.push(shortcut),
            Err(_) => {
                let _ = manager.unregister(shortcut.as_str());
                unavailable.push(format!("{action} ({shortcut})"));
            }
        }
    }
    (registered, unavailable)
}

fn unregister_shortcuts(app: &AppHandle, shortcuts: &[String]) {
    if !shortcuts.is_empty() {
        let values = shortcuts.iter().map(String::as_str).collect::<Vec<_>>();
        let _ = app.global_shortcut().unregister_multiple(values);
    }
}

fn composite_annotations(
    image: &mut RgbaImage,
    strokes: &[AnnotationStroke],
    display_index: usize,
    scale: f32,
) {
    for stroke in strokes
        .iter()
        .filter(|stroke| stroke.display_index == display_index)
    {
        let color = annotation_color(&stroke.color);
        let points = stroke
            .points
            .iter()
            .map(|point| (point.x * scale, point.y * scale))
            .collect::<Vec<_>>();
        let radius = ((stroke.width * scale) / 2.0).round().max(1.0) as i32;
        match stroke.tool {
            AnnotationTool::Pen | AnnotationTool::Line | AnnotationTool::Arrow => {
                for pair in points.windows(2) {
                    draw_thick_line(image, pair[0], pair[1], radius, color);
                }
                if stroke.tool == AnnotationTool::Arrow && points.len() >= 2 {
                    draw_arrow_head(
                        image,
                        points[points.len() - 2],
                        points[points.len() - 1],
                        radius,
                        color,
                    );
                }
            }
            AnnotationTool::Rectangle if points.len() >= 2 => {
                let (a, b) = (points[0], *points.last().unwrap());
                draw_thick_line(image, (a.0, a.1), (b.0, a.1), radius, color);
                draw_thick_line(image, (b.0, a.1), (b.0, b.1), radius, color);
                draw_thick_line(image, (b.0, b.1), (a.0, b.1), radius, color);
                draw_thick_line(image, (a.0, b.1), (a.0, a.1), radius, color);
            }
            AnnotationTool::Ellipse if points.len() >= 2 => {
                let (a, b) = (points[0], *points.last().unwrap());
                let center = (
                    ((a.0 + b.0) / 2.0).round() as i32,
                    ((a.1 + b.1) / 2.0).round() as i32,
                );
                let rx = ((a.0 - b.0).abs() / 2.0).round().max(1.0) as i32;
                let ry = ((a.1 - b.1).abs() / 2.0).round().max(1.0) as i32;
                for inset in 0..radius.max(1) {
                    draw_hollow_ellipse_mut(
                        image,
                        center,
                        (rx - inset).max(1),
                        (ry - inset).max(1),
                        color,
                    );
                }
            }
            _ => {}
        }
    }
}

fn draw_thick_line(
    image: &mut RgbaImage,
    from: (f32, f32),
    to: (f32, f32),
    radius: i32,
    color: Rgba<u8>,
) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as usize;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        draw_filled_circle_mut(
            image,
            (
                (from.0 + dx * t).round() as i32,
                (from.1 + dy * t).round() as i32,
            ),
            radius,
            color,
        );
    }
}

fn draw_arrow_head(
    image: &mut RgbaImage,
    from: (f32, f32),
    to: (f32, f32),
    radius: i32,
    color: Rgba<u8>,
) {
    let angle = (to.1 - from.1).atan2(to.0 - from.0);
    let length = (radius as f32 * 5.0).max(12.0);
    for offset in [-0.65_f32, 0.65] {
        let endpoint = (
            to.0 - length * (angle + offset).cos(),
            to.1 - length * (angle + offset).sin(),
        );
        draw_thick_line(image, to, endpoint, radius, color);
    }
}

fn annotation_color(value: &str) -> Rgba<u8> {
    match value.to_ascii_lowercase().as_str() {
        "#ffd84d" => Rgba([255, 216, 77, 255]),
        "#35c9ff" => Rgba([53, 201, 255, 255]),
        "#ffffff" => Rgba([255, 255, 255, 255]),
        _ => Rgba([255, 77, 94, 255]),
    }
}

fn pixel_region(
    region: Region,
    scale: f32,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32, u32, u32), String> {
    let x = (region.x.min(region.x + region.width) * scale)
        .round()
        .max(0.0) as u32;
    let y = (region.y.min(region.y + region.height) * scale)
        .round()
        .max(0.0) as u32;
    let width = (region.width.abs() * scale).round().max(1.0) as u32;
    let height = (region.height.abs() * scale).round().max(1.0) as u32;
    let x = x.min(max_width.saturating_sub(1));
    let y = y.min(max_height.saturating_sub(1));
    Ok((x, y, width.min(max_width - x), height.min(max_height - y)))
}

fn validate_stroke(stroke: &AnnotationStroke) -> Result<(), String> {
    if stroke.points.len() < 2
        || stroke.points.len() > 20_000
        || !is_color(&stroke.color)
        || !(1.0..=16.0).contains(&stroke.width)
        || stroke
            .points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
    {
        return Err("Annotation stroke is invalid.".into());
    }
    Ok(())
}

fn is_color(color: &str) -> bool {
    matches!(
        color.to_ascii_lowercase().as_str(),
        "#ff4d5e" | "#ffd84d" | "#35c9ff" | "#ffffff"
    )
}

fn transcribe(path: &Path, source_rate: u32) -> Result<Vec<TranscriptSegment>, String> {
    let mut reader = WavReader::open(path).map_err(|error| error.to_string())?;
    let input = match reader.spec().sample_format {
        WavSampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>(),
        WavSampleFormat::Int => reader
            .samples::<i32>()
            .map(|sample| sample.map(|value| value as f32 / i32::MAX as f32))
            .collect::<Result<Vec<_>, _>>(),
    }
    .map_err(|error| error.to_string())?;
    if input.is_empty() || root_mean_square(&input) < 0.0035 {
        return Ok(Vec::new());
    }
    let audio = resample_linear(&input, source_rate, 16_000);
    let context =
        WhisperContext::new_with_params(model_path(), WhisperContextParameters::default())
            .map_err(|error| format!("Could not load the local model: {error}"))?;
    let mut state = context.create_state().map_err(|error| error.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 2 });
    params.set_language(Some("en"));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    state
        .full(params, &audio)
        .map_err(|error| error.to_string())?;
    Ok(state
        .as_iter()
        .filter_map(|segment| {
            let text = segment
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (!text.is_empty()).then(|| TranscriptSegment {
                start_ms: segment.start_timestamp() * 10,
                end_ms: segment.end_timestamp() * 10,
                text,
            })
        })
        .collect())
}

fn resample_linear(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate {
        return input.to_vec();
    }
    let output_len =
        ((input.len() as u64 * target_rate as u64) / source_rate.max(1) as u64) as usize;
    (0..output_len)
        .map(|index| {
            let source = index as f64 * source_rate as f64 / target_rate as f64;
            let lower = source.floor() as usize;
            let upper = (lower + 1).min(input.len().saturating_sub(1));
            let fraction = (source - lower as f64) as f32;
            input[lower] * (1.0 - fraction) + input[upper] * fraction
        })
        .collect()
}

fn root_mean_square(samples: &[f32]) -> f32 {
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn download_model(app: &AppHandle, path: &Path) -> Result<(), String> {
    if path.parent().is_some_and(|parent| !parent.exists()) {
        fs::create_dir_all(path.parent().unwrap()).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension(format!("{}.part", Uuid::new_v4()));
    let result = (|| {
        let mut response = reqwest::blocking::Client::new()
            .get(MODEL_URL)
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        if response
            .content_length()
            .is_some_and(|length| length != MODEL_BYTES)
        {
            return Err("The transcription model download has an unexpected size.".into());
        }
        let mut file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        let mut downloaded = 0_u64;
        let mut buffer = vec![0_u8; 128 * 1024];
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            if downloaded.saturating_add(count as u64) > MODEL_BYTES {
                return Err("The transcription model download exceeded its expected size.".into());
            }
            file.write_all(&buffer[..count])
                .map_err(|error| error.to_string())?;
            digest.update(&buffer[..count]);
            downloaded += count as u64;
            let _ = app.emit(
                "feedback://model-progress",
                json!({ "downloaded": downloaded, "total": MODEL_BYTES }),
            );
        }
        file.sync_all().map_err(|error| error.to_string())?;
        if downloaded != MODEL_BYTES || format!("{:x}", digest.finalize()) != MODEL_SHA256 {
            return Err("The downloaded transcription model failed checksum verification.".into());
        }
        set_private_permissions(&temporary)?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn model_path() -> PathBuf {
    std::env::var_os("WORKMAN_WHISPER_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workmand::default_data_dir().join("whisper-models"))
        .join(MODEL_FILE)
}

fn validate_media_dir(feedback_id: i64, value: &str) -> Result<PathBuf, String> {
    let expected = workmand::default_data_dir()
        .join("recorded-feedback")
        .join(feedback_id.to_string());
    let expected = expected
        .canonicalize()
        .map_err(|error| format!("Feedback storage is unavailable: {error}"))?;
    let supplied = Path::new(value)
        .canonicalize()
        .map_err(|error| format!("Feedback storage is unavailable: {error}"))?;
    if supplied != expected {
        return Err("Feedback media directory is outside Workman's private storage.".into());
    }
    Ok(supplied)
}

fn write_png_atomic(image: &RgbaImage, path: &Path) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.part", Uuid::new_v4()));
    DynamicImage::ImageRgba8(image.clone())
        .save_with_format(&temporary, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    set_private_permissions(&temporary)?;
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn append_journal(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let mut line = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    line.push(b'\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(&line).map_err(|error| error.to_string())?;
    file.sync_data().map_err(|error| error.to_string())?;
    set_private_permissions(path)
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}

/// Windows has no mode bits. Recordings live under the per-user data directory,
/// whose inherited ACL already limits the files to this account, so there is
/// nothing to tighten without hand-writing a DACL.
#[cfg(windows)]
fn set_private_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", digest.finalize()))
}

fn session_view(
    session: &FeedbackSession,
    phase: &'static str,
    error: Option<String>,
) -> SessionView {
    SessionView {
        feedback_id: session.feedback_id,
        project_id: session.project_id,
        started_at_ms: session.started_at_ms,
        elapsed_ms: session_elapsed_ms(session),
        audio_samples: session.audio_samples.load(Ordering::Relaxed),
        sample_rate: session.sample_rate,
        snapshot_count: session.snapshot_count,
        paused: session.paused_at.is_some(),
        muted: session.audio_controls.muted.load(Ordering::Acquire),
        input_device_id: session.input_device_id.clone(),
        input_device_name: session.input_device_name.clone(),
        phase,
        error,
    }
}

fn session_elapsed_ms(session: &FeedbackSession) -> i64 {
    let current_pause = session
        .paused_at
        .map(|paused_at| paused_at.elapsed())
        .unwrap_or(Duration::ZERO);
    elapsed_without_pauses(
        session.started.elapsed(),
        session.paused_duration + current_pause,
    )
    .as_millis()
    .min(i64::MAX as u128) as i64
}

fn elapsed_without_pauses(wall_time: Duration, paused_time: Duration) -> Duration {
    wall_time.saturating_sub(paused_time)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_time_is_removed_from_the_feedback_timeline() {
        assert_eq!(
            elapsed_without_pauses(Duration::from_secs(14), Duration::from_secs(5)),
            Duration::from_secs(9)
        );
        assert_eq!(
            elapsed_without_pauses(Duration::from_secs(3), Duration::from_secs(5)),
            Duration::ZERO
        );
    }

    #[test]
    fn retina_region_maps_to_physical_pixels() {
        assert_eq!(
            pixel_region(
                Region {
                    x: 10.0,
                    y: 20.0,
                    width: 16.0,
                    height: 16.0
                },
                2.0,
                200,
                200
            )
            .unwrap(),
            (20, 40, 32, 32)
        );
    }

    #[test]
    fn silence_gate_and_resampler_are_deterministic() {
        assert_eq!(root_mean_square(&vec![0.0; 100]), 0.0);
        assert_eq!(
            resample_linear(&[0.0, 1.0, 0.0, -1.0], 4, 2),
            vec![0.0, 0.0]
        );
    }
}
