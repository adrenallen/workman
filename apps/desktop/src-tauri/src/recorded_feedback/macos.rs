use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
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
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Position, Size, State, WebviewUrl,
};
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

const DEFAULT_SHORTCUTS: &[(&str, &str)] = &[
    ("snap", "CommandOrControl+Shift+C"),
    ("snapRegion", "CommandOrControl+Shift+R"),
    ("snapFull", "CommandOrControl+Shift+D"),
    ("toggleAnnotation", "CommandOrControl+Shift+A"),
    ("undo", "CommandOrControl+Shift+Z"),
    ("clear", "CommandOrControl+Shift+Backspace"),
    ("finish", "CommandOrControl+Shift+Return"),
];

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
        unregister_shortcuts(app, &session.registered_shortcuts);
        close_feedback_panels(app);
        drop(session.stream);
        let _ = finalize_writer(&session.writer);
        let _ = append_journal(
            &session.media_dir.join("events.jsonl"),
            &json!({
                "event": "interrupted", "reason": "desktop_exit",
                "samples": session.audio_samples.load(Ordering::Relaxed),
                "sample_rate": session.sample_rate, "at": now_millis()
            }),
        );
    }
}

struct FeedbackSession {
    feedback_id: i64,
    project_id: i64,
    media_dir: PathBuf,
    started: Instant,
    started_at_ms: i64,
    sample_rate: u32,
    audio_samples: Arc<AtomicU64>,
    audio_path: PathBuf,
    writer: SharedWriter,
    stream: Stream,
    snapshot_count: usize,
    annotations: Vec<AnnotationStroke>,
    tool: AnnotationTool,
    color: String,
    width: f32,
    registered_shortcuts: Vec<String>,
    display_ids: Vec<u32>,
    capture_in_progress: bool,
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
    phase: &'static str,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SnapshotView {
    feedback_id: i64,
    project_id: i64,
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
    let screen_capture_available = CGPreflightScreenCaptureAccess()
        && Monitor::all().is_ok_and(|monitors| !monitors.is_empty());
    let model_path = model_path();
    let model_installed = model_path
        .metadata()
        .is_ok_and(|metadata| metadata.len() == MODEL_BYTES)
        && sha256_file(&model_path).is_ok_and(|sha| sha == MODEL_SHA256);
    let message = if !microphone_available {
        Some("No microphone is available. Connect or enable one, then retry.".into())
    } else if !screen_capture_available {
        Some("Screen capture is off. Enable Workman in System Settings → Privacy & Security → Screen Recording, then reopen Workman.".into())
    } else if !model_installed {
        Some("Install the local transcription model before recording.".into())
    } else {
        None
    };
    FeedbackPreflight {
        supported: true,
        platform: "macos",
        microphone_available,
        screen_capture_available,
        model_installed,
        model_name: MODEL_NAME,
        model_size_bytes: MODEL_BYTES,
        model_path: model_path.to_string_lossy().into_owned(),
        message,
    }
}

#[tauri::command]
pub(crate) fn feedback_request_screen_access() -> FeedbackPreflight {
    if !CGPreflightScreenCaptureAccess() {
        let _ = CGRequestScreenCaptureAccess();
    }
    feedback_preflight()
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
    let (stream, writer, sample_rate, audio_samples) =
        start_audio(&app, feedback_id, project_id, &audio_path)?;
    let registered_shortcuts = register_shortcuts(&app, shortcuts)?;
    let display_ids = match create_feedback_panels(&app) {
        Ok(display_ids) => display_ids,
        Err(error) => {
            unregister_shortcuts(&app, &registered_shortcuts);
            close_feedback_panels(&app);
            drop(stream);
            finalize_writer(&writer)?;
            return Err(error);
        }
    };
    let session = FeedbackSession {
        feedback_id,
        project_id,
        media_dir,
        started: Instant::now(),
        started_at_ms: now_millis(),
        sample_rate,
        audio_samples,
        audio_path,
        writer,
        stream,
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
    *active = Some(session);
    drop(active);
    let _ = app.emit(EVENT_STATUS, &view);
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
            anchor_ms: session.started.elapsed().as_millis().min(i64::MAX as u128) as i64,
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
    unregister_shortcuts(&app, &session.registered_shortcuts);
    close_feedback_panels(&app);
    drop(session.stream);
    let finalize_result = finalize_writer(&session.writer);
    let _ = append_journal(
        &session.media_dir.join("events.jsonl"),
        &json!({
            "event": "interrupted", "reason": "native_error",
            "samples": session.audio_samples.load(Ordering::Relaxed),
            "sample_rate": session.sample_rate, "at": now_millis()
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
    let duration_ms = session.started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    unregister_shortcuts(&app, &session.registered_shortcuts);
    close_feedback_panels(&app);
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

fn start_audio(
    app: &AppHandle,
    feedback_id: i64,
    project_id: i64,
    path: &Path,
) -> Result<(Stream, SharedWriter, u32, Arc<AtomicU64>), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("No microphone is available.")?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("Could not open the microphone: {error}"))?;
    let channels = supported.channels() as usize;
    let sample_rate = supported.sample_rate();
    let config: StreamConfig = supported.into();
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
            error_handler,
            |value| value,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config,
            channels,
            writer.clone(),
            samples.clone(),
            error_handler,
            |value| value as f32 / i16::MAX as f32,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config,
            channels,
            writer.clone(),
            samples.clone(),
            error_handler,
            |value| value as f32 / u16::MAX as f32 * 2.0 - 1.0,
        ),
        format => return Err(format!("Unsupported microphone sample format: {format}")),
    }?;
    stream
        .play()
        .map_err(|error| format!("Could not start the microphone: {error}"))?;
    Ok((stream, writer, sample_rate, samples))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    writer: SharedWriter,
    samples: Arc<AtomicU64>,
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
                let frames = data.len() / channels.max(1);
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
                        let mono =
                            frame.iter().copied().map(&convert).sum::<f32>() / frame.len() as f32;
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
        PanelBuilder::<_, FeedbackPanel>::new(app, format!("feedback-overlay-{index}"))
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
            .map_err(|error| error.to_string())?
            .show();
    }
    let primary = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or("No display is available.")?;
    let position_x = primary.x().map_err(|error| error.to_string())? as f64;
    let position_y = primary.y().map_err(|error| error.to_string())? as f64;
    let monitor_width = primary.width().map_err(|error| error.to_string())? as f64;
    let width = 780.0;
    let x = position_x + ((monitor_width - width) / 2.0).max(16.0);
    let y = position_y + 28.0;
    PanelBuilder::<_, FeedbackPanel>::new(app, "feedback-toolbar")
        .url(WebviewUrl::App("index.html".into()))
        .position(Position::Logical(LogicalPosition::new(x, y)))
        .size(Size::Logical(LogicalSize::new(width, 60.0)))
        .level(PanelLevel::ScreenSaver)
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
        .map_err(|error| error.to_string())?
        .show();
    Ok(display_ids)
}

fn close_feedback_panels(app: &AppHandle) {
    for (_, window) in app.webview_windows() {
        if window.label().starts_with("feedback-") {
            let _ = window.destroy();
        }
    }
}

fn set_overlays_interactive(app: &AppHandle, interactive: bool) -> Result<(), String> {
    for (label, _) in app.webview_windows() {
        if label.starts_with("feedback-overlay-") {
            let panel = app
                .get_webview_panel(&label)
                .map_err(|error| format!("{error:?}"))?;
            panel.set_ignores_mouse_events(!interactive);
        }
    }
    Ok(())
}

fn register_shortcuts(
    app: &AppHandle,
    requested: Option<HashMap<String, String>>,
) -> Result<Vec<String>, String> {
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
    let mut registered = Vec::new();
    for (action, shortcut) in values {
        let emitted_action = action.clone();
        manager
            .on_shortcut(shortcut.as_str(), move |app, _, event| {
                if event.state() == ShortcutState::Pressed {
                    let _ = app.emit(EVENT_SHORTCUT, &emitted_action);
                }
            })
            .map_err(|error| {
                unregister_shortcuts(app, &registered);
                format!("Could not register {action} ({shortcut}): {error}")
            })?;
        registered.push(shortcut);
    }
    Ok(registered)
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

fn set_private_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
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
        elapsed_ms: session.started.elapsed().as_millis().min(i64::MAX as u128) as i64,
        audio_samples: session.audio_samples.load(Ordering::Relaxed),
        sample_rate: session.sample_rate,
        snapshot_count: session.snapshot_count,
        phase,
        error,
    }
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
