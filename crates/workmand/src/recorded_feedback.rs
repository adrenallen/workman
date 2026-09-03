//! Desktop control surface for local recorded-feedback packets.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use image::GenericImageView;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use workman_core::attention::AttentionState;
use workman_core::{
    NewRecordedFeedbackDelivery, NewRecordedFeedbackSnapshot, ProcessKind, ProcessStatus,
    ProjectId, RecordedFeedback, RecordedFeedbackBlock, RecordedFeedbackDocumentUpdate,
    RecordedFeedbackError, RecordedFeedbackId, RecordedFeedbackService, RecordedFeedbackStatus,
    RecordedFeedbackTranscriptSegment, Scratchpad, ScratchpadService, ScratchpadServiceError,
};

use crate::ProcessRegistry;

pub(crate) type ControlResult = Result<Value, (&'static str, String)>;

const FEEDBACK_DIRECTORY: &str = "recorded-feedback";
const PACKET_DIRECTORY: &str = "feedback-packets";
// The desktop renews every five seconds. Three missed renewals distinguish a crashed recorder
// from a brief daemon hiccup without leaving the sidebar stuck for a full minute.
const LEASE_MS: i64 = 15_000;

#[derive(Debug, Deserialize)]
struct CreateParams {
    project_id: ProjectId,
    title: String,
    lease_owner: String,
}

#[derive(Debug, Deserialize)]
struct FeedbackParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
}

#[derive(Debug, Deserialize)]
struct ListParams {
    project_id: ProjectId,
    #[serde(default)]
    archived: bool,
}

#[derive(Debug, Deserialize)]
struct LeaseParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    lease_owner: String,
}

#[derive(Debug, Deserialize)]
struct SnapshotParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    ordinal: i64,
    anchor_ms: i64,
    anchor_samples: i64,
    invoked_at_ms: i64,
    completed_at_ms: i64,
    image_path: String,
    #[serde(default)]
    caption: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct BeginTranscriptionParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    duration_ms: i64,
    audio_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CompleteParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    transcript: Vec<RecordedFeedbackTranscriptSegment>,
    blocks: Vec<RecordedFeedbackBlock>,
}

#[derive(Debug, Deserialize)]
struct UpdateParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    expected_revision: i64,
    title: String,
    blocks: Vec<RecordedFeedbackBlock>,
    #[serde(default)]
    snapshot_captions: Vec<SnapshotCaption>,
}

#[derive(Debug, Deserialize)]
struct SnapshotCaption {
    snapshot_id: i64,
    caption: String,
}

#[derive(Debug, Deserialize)]
struct FailedParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    code: String,
}

#[derive(Debug, Deserialize)]
struct ArchiveParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    #[serde(default = "default_true")]
    archived: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    #[serde(default)]
    confirm_delete: bool,
}

#[derive(Debug, Deserialize)]
struct DeliverAgentParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    process_id: i64,
    #[serde(default)]
    direct_input: bool,
}

#[derive(Debug, Deserialize)]
struct ToScratchpadParams {
    project_id: ProjectId,
    feedback_id: RecordedFeedbackId,
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PacketManifest {
    version: u8,
    feedback_id: i64,
    revision: i64,
    source_created_at: i64,
    markdown: String,
    images: Vec<PacketImage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PacketImage {
    snapshot_id: i64,
    path: String,
    sha256: String,
}

pub(crate) fn dispatch(
    method: &str,
    params: Value,
    registry: &mut ProcessRegistry,
    data_dir: &Path,
) -> Option<ControlResult> {
    let result = match method {
        "recorded_feedback.list" => list(params, registry),
        "recorded_feedback.get" => get(params, registry),
        "recorded_feedback.create" => create(params, registry, data_dir),
        "recorded_feedback.renew_lease" => renew_lease(params, registry),
        "recorded_feedback.add_snapshot" => add_snapshot(params, registry, data_dir),
        "recorded_feedback.begin_transcription" => begin_transcription(params, registry, data_dir),
        "recorded_feedback.complete" => complete(params, registry),
        "recorded_feedback.update" => update(params, registry),
        "recorded_feedback.failed" => failed(params, registry),
        "recorded_feedback.archive" => archive(params, registry),
        "recorded_feedback.delete" => delete(params, registry, data_dir),
        "recorded_feedback.prepare_packet" => prepare_packet(params, registry, data_dir),
        "recorded_feedback.deliver_agent" => deliver_agent(params, registry, data_dir),
        "recorded_feedback.to_scratchpad" => to_scratchpad(params, registry, data_dir),
        _ => return None,
    };
    Some(result)
}

fn list(params: Value, registry: &ProcessRegistry) -> ControlResult {
    let params: ListParams = params_as(params)?;
    let service = RecordedFeedbackService::new(registry.store());
    service
        .fail_expired(params.project_id, now_millis())
        .map_err(feedback_error)?;
    service
        .list(params.project_id, params.archived)
        .map(json_value)
        .map_err(feedback_error)
}

fn get(params: Value, registry: &ProcessRegistry) -> ControlResult {
    let params: FeedbackParams = params_as(params)?;
    RecordedFeedbackService::new(registry.store())
        .fail_expired(params.project_id, now_millis())
        .map_err(feedback_error)?;
    require_feedback(registry, params.project_id, params.feedback_id).map(json_value)
}

fn create(params: Value, registry: &ProcessRegistry, data_dir: &Path) -> ControlResult {
    let params: CreateParams = params_as(params)?;
    if params.lease_owner.trim().is_empty() || params.lease_owner.len() > 120 {
        return Err(("invalid_params", "lease_owner is invalid".into()));
    }
    let now = now_millis();
    let service = RecordedFeedbackService::new(registry.store());
    let feedback = service
        .create(
            params.project_id,
            &params.title,
            &params.lease_owner,
            now + LEASE_MS,
            now,
        )
        .map_err(feedback_error)?;
    let directory = feedback_directory(data_dir, feedback.id);
    if let Err(error) = create_private_directory(&directory) {
        let _ = service.delete(params.project_id, feedback.id);
        return Err(("feedback_storage_error", error.to_string()));
    }
    let journal = directory.join("events.jsonl");
    if let Err(error) = append_journal(
        &journal,
        &json!({
            "event": "created", "feedback_id": feedback.id, "at": now,
        }),
    ) {
        let _ = service.delete(params.project_id, feedback.id);
        let _ = fs::remove_dir_all(&directory);
        return Err(("feedback_storage_error", error.to_string()));
    }
    Ok(json!({ "feedback": feedback, "media_dir": directory }))
}

fn renew_lease(params: Value, registry: &ProcessRegistry) -> ControlResult {
    let params: LeaseParams = params_as(params)?;
    let now = now_millis();
    RecordedFeedbackService::new(registry.store())
        .renew_lease(
            params.project_id,
            params.feedback_id,
            &params.lease_owner,
            now + LEASE_MS,
            now,
        )
        .map(json_value)
        .map_err(feedback_error)
}

fn add_snapshot(params: Value, registry: &ProcessRegistry, data_dir: &Path) -> ControlResult {
    let params: SnapshotParams = params_as(params)?;
    let path = require_feedback_file(data_dir, params.feedback_id, Path::new(&params.image_path))?;
    let actual_sha = sha256_file(&path).map_err(storage_error)?;
    if actual_sha != params.sha256 {
        return Err((
            "feedback_integrity_error",
            "snapshot checksum does not match".into(),
        ));
    }
    let image = image::open(&path).map_err(|error| ("feedback_image_error", error.to_string()))?;
    let (width, height) = image.dimensions();
    let snapshot = NewRecordedFeedbackSnapshot {
        ordinal: params.ordinal,
        anchor_ms: params.anchor_ms,
        anchor_samples: params.anchor_samples,
        invoked_at_ms: params.invoked_at_ms,
        completed_at_ms: params.completed_at_ms,
        image_path: path.to_string_lossy().into_owned(),
        caption: params.caption,
        width,
        height,
        sha256: actual_sha,
    };
    RecordedFeedbackService::new(registry.store())
        .add_snapshot(
            params.project_id,
            params.feedback_id,
            snapshot,
            now_millis(),
        )
        .map(json_value)
        .map_err(feedback_error)
}

fn begin_transcription(
    params: Value,
    registry: &ProcessRegistry,
    data_dir: &Path,
) -> ControlResult {
    let params: BeginTranscriptionParams = params_as(params)?;
    let audio_path = params
        .audio_path
        .as_deref()
        .map(|path| {
            require_feedback_file(data_dir, params.feedback_id, Path::new(path))
                .map(|path| path.to_string_lossy().into_owned())
        })
        .transpose()?;
    RecordedFeedbackService::new(registry.store())
        .begin_transcription(
            params.project_id,
            params.feedback_id,
            params.duration_ms,
            audio_path.as_deref(),
            now_millis(),
        )
        .map(json_value)
        .map_err(feedback_error)
}

fn complete(params: Value, registry: &ProcessRegistry) -> ControlResult {
    let params: CompleteParams = params_as(params)?;
    validate_block_snapshots(
        registry,
        params.project_id,
        params.feedback_id,
        &params.blocks,
    )?;
    RecordedFeedbackService::new(registry.store())
        .complete(
            params.project_id,
            params.feedback_id,
            params.transcript,
            params.blocks,
            now_millis(),
        )
        .map(json_value)
        .map_err(feedback_error)
}

fn update(params: Value, registry: &ProcessRegistry) -> ControlResult {
    let params: UpdateParams = params_as(params)?;
    validate_block_snapshots(
        registry,
        params.project_id,
        params.feedback_id,
        &params.blocks,
    )?;
    let captions = params
        .snapshot_captions
        .into_iter()
        .map(|value| (value.snapshot_id, value.caption))
        .collect::<Vec<_>>();
    RecordedFeedbackService::new(registry.store())
        .update_document(
            params.project_id,
            params.feedback_id,
            RecordedFeedbackDocumentUpdate {
                expected_revision: params.expected_revision,
                title: params.title,
                blocks: params.blocks,
                snapshot_captions: captions,
                now_ms: now_millis(),
            },
        )
        .map(json_value)
        .map_err(feedback_error)
}

fn failed(params: Value, registry: &ProcessRegistry) -> ControlResult {
    let params: FailedParams = params_as(params)?;
    RecordedFeedbackService::new(registry.store())
        .mark_failed(
            params.project_id,
            params.feedback_id,
            &params.code,
            now_millis(),
        )
        .map(json_value)
        .map_err(feedback_error)
}

fn archive(params: Value, registry: &ProcessRegistry) -> ControlResult {
    let params: ArchiveParams = params_as(params)?;
    RecordedFeedbackService::new(registry.store())
        .archive(
            params.project_id,
            params.feedback_id,
            params.archived,
            now_millis(),
        )
        .map(json_value)
        .map_err(feedback_error)
}

fn delete(params: Value, registry: &ProcessRegistry, data_dir: &Path) -> ControlResult {
    let params: DeleteParams = params_as(params)?;
    if !params.confirm_delete {
        return Err((
            "confirmation_required",
            "confirm_delete=true is required because audio and screenshots cannot be recovered"
                .into(),
        ));
    }
    require_feedback(registry, params.project_id, params.feedback_id)?;
    let directory = feedback_directory(data_dir, params.feedback_id);
    let packet_root = data_dir
        .join(PACKET_DIRECTORY)
        .join(params.feedback_id.to_string());
    let suffix = Uuid::new_v4();
    let staged_directory = directory.with_extension(format!("deleting-{suffix}"));
    let staged_packets = packet_root.with_extension(format!("deleting-{suffix}"));
    let mut staged = Vec::new();
    for (source, target) in [
        (&directory, &staged_directory),
        (&packet_root, &staged_packets),
    ] {
        if !source.exists() {
            continue;
        }
        if let Err(error) = fs::rename(source, target) {
            for (restore_from, restore_to) in staged.iter().rev() {
                let _ = fs::rename(restore_from, restore_to);
            }
            return Err(storage_error(error));
        }
        staged.push((target.to_path_buf(), source.to_path_buf()));
    }
    let deleted = match RecordedFeedbackService::new(registry.store())
        .delete(params.project_id, params.feedback_id)
    {
        Ok(deleted) => deleted,
        Err(error) => {
            for (restore_from, restore_to) in staged.iter().rev() {
                let _ = fs::rename(restore_from, restore_to);
            }
            return Err(feedback_error(error));
        }
    };
    for (staged_path, _) in staged {
        let _ = fs::remove_dir_all(staged_path);
    }
    Ok(json!({ "feedback_id": params.feedback_id, "deleted": deleted }))
}

fn prepare_packet(params: Value, registry: &ProcessRegistry, data_dir: &Path) -> ControlResult {
    let params: FeedbackParams = params_as(params)?;
    let feedback = require_feedback(registry, params.project_id, params.feedback_id)?;
    let packet = compile_packet(data_dir, &feedback)?;
    let delivery = RecordedFeedbackService::new(registry.store())
        .record_delivery(
            params.project_id,
            params.feedback_id,
            NewRecordedFeedbackDelivery {
                target_kind: "clipboard".into(),
                target_id: None,
                status: "queued".into(),
                packet_path: Some(packet.markdown_path.to_string_lossy().into_owned()),
                error_message: None,
                now_ms: now_millis(),
            },
        )
        .map_err(feedback_error)?;
    Ok(
        json!({ "markdown": packet.markdown, "packet_path": packet.markdown_path, "delivery": delivery }),
    )
}

fn deliver_agent(params: Value, registry: &mut ProcessRegistry, data_dir: &Path) -> ControlResult {
    let params: DeliverAgentParams = params_as(params)?;
    let feedback = require_feedback(registry, params.project_id, params.feedback_id)?;
    let status = registry
        .get_status(params.process_id)
        .map_err(registry_error)?;
    if status.process.project_id != params.project_id || status.process.kind != ProcessKind::Agent {
        return Err((
            "feedback_target_error",
            "target is not an agent in this project".into(),
        ));
    }
    if status.process.status != ProcessStatus::Running {
        return Err(("feedback_target_exited", "that agent is not running".into()));
    }
    if !matches!(
        status.agent_state.state,
        AttentionState::Idle | AttentionState::NeedsInput | AttentionState::Waiting
    ) {
        return Err((
            "feedback_target_busy",
            "that agent is working; wait for it to finish or send to a new agent".into(),
        ));
    }
    let packet = compile_packet(data_dir, &feedback)?;
    let (delivery_status, error_message) = if params.direct_input {
        // The desktop will paste the ordered transcript and image blocks into the validated live
        // agent immediately after this response. Keep the immutable packet as the durable audit
        // copy, while describing PTY image import honestly as unverified.
        ("unverified", None)
    } else {
        let prompt = format!(
            "Review and act on the recorded feedback packet at {}. Read feedback.md in order; its images directory contains the referenced screenshots.",
            packet.markdown_path.display()
        );
        match registry.submit_input(params.process_id, prompt.as_bytes()) {
            Ok(_) => ("queued", None),
            Err(error) => ("failed", Some(error.to_string())),
        }
    };
    let delivery = RecordedFeedbackService::new(registry.store())
        .record_delivery(
            params.project_id,
            params.feedback_id,
            NewRecordedFeedbackDelivery {
                target_kind: "agent".into(),
                target_id: Some(params.process_id),
                status: delivery_status.into(),
                packet_path: Some(packet.markdown_path.to_string_lossy().into_owned()),
                error_message: error_message.clone(),
                now_ms: now_millis(),
            },
        )
        .map_err(feedback_error)?;
    if let Some(error) = error_message {
        return Err(("feedback_delivery_failed", error));
    }
    Ok(json!({
        "delivery": delivery,
        "process": status,
        "packet_path": packet.markdown_path
    }))
}

fn to_scratchpad(params: Value, registry: &ProcessRegistry, data_dir: &Path) -> ControlResult {
    let params: ToScratchpadParams = params_as(params)?;
    let feedback = require_feedback(registry, params.project_id, params.feedback_id)?;
    let packet = compile_packet(data_dir, &feedback)?;
    let name = params.name.unwrap_or_else(|| feedback.title.clone());
    let content = scratchpad_packet_content(&packet);
    let service = ScratchpadService::attributed(registry.store(), "user");
    let scratchpad = create_feedback_scratchpad(&service, params.project_id, &name, &content)
        .map_err(|error| ("scratchpad_error", error.to_string()))?;
    let delivery = RecordedFeedbackService::new(registry.store())
        .record_delivery(
            params.project_id,
            params.feedback_id,
            NewRecordedFeedbackDelivery {
                target_kind: "scratchpad".into(),
                target_id: Some(scratchpad.id),
                status: "queued".into(),
                packet_path: Some(packet.markdown_path.to_string_lossy().into_owned()),
                error_message: None,
                now_ms: now_millis(),
            },
        )
        .map_err(feedback_error)?;
    Ok(json!({ "scratchpad": scratchpad, "delivery": delivery }))
}

fn create_feedback_scratchpad(
    service: &ScratchpadService<'_>,
    project_id: ProjectId,
    name: &str,
    content: &str,
) -> Result<Scratchpad, ScratchpadServiceError> {
    for copy in 1..=1_000 {
        let candidate = if copy == 1 {
            name.to_owned()
        } else {
            format!("{name} ({copy})")
        };
        match service.write(
            project_id,
            None,
            candidate.clone(),
            content.to_owned(),
            Some(vec!["recorded-feedback".into()]),
            None,
        ) {
            Ok((created, _)) => return Ok(created),
            Err(ScratchpadServiceError::NameConflict { name, .. }) if name == candidate => continue,
            Err(error @ ScratchpadServiceError::NameConflict { .. }) => return Err(error),
            Err(error) => return Err(error),
        }
    }
    Err(ScratchpadServiceError::InvalidInput(
        "could not find an available scratchpad name".into(),
    ))
}

#[derive(Debug)]
struct CompiledPacket {
    markdown: String,
    markdown_path: PathBuf,
    images: Vec<PacketImage>,
}

fn compile_packet(data_dir: &Path, feedback: &RecordedFeedback) -> ControlResultAs<CompiledPacket> {
    if feedback.status != RecordedFeedbackStatus::Ready {
        return Err((
            "feedback_not_ready",
            "finish transcription before sending this feedback".into(),
        ));
    }
    let packet_parent = data_dir
        .join(PACKET_DIRECTORY)
        .join(feedback.id.to_string());
    create_private_directory(&packet_parent).map_err(storage_error)?;
    remove_abandoned_packet_builds(&packet_parent).map_err(storage_error)?;
    let root = packet_parent.join(format!("r{}", feedback.revision));
    if root.exists() {
        if let Ok(packet) = load_compiled_packet(&root, feedback) {
            return Ok(packet);
        }
        fs::remove_dir_all(&root).map_err(storage_error)?;
    }

    let staged = packet_parent.join(format!(
        ".r{}-{}.building",
        feedback.revision,
        Uuid::new_v4()
    ));
    let build_result = build_compiled_packet(&staged, data_dir, feedback);
    let (markdown, images) = match build_result {
        Ok(packet) => packet,
        Err(error) => {
            let _ = fs::remove_dir_all(&staged);
            return Err(error);
        }
    };
    if let Err(error) = fs::rename(&staged, &root) {
        let _ = fs::remove_dir_all(&staged);
        if root.exists() {
            return load_compiled_packet(&root, feedback);
        }
        return Err(storage_error(error));
    }
    Ok(CompiledPacket {
        markdown,
        markdown_path: root.join("feedback.md"),
        images,
    })
}

fn build_compiled_packet(
    root: &Path,
    data_dir: &Path,
    feedback: &RecordedFeedback,
) -> ControlResultAs<(String, Vec<PacketImage>)> {
    let images_dir = root.join("images");
    create_private_directory(&images_dir).map_err(storage_error)?;
    let mut markdown = format!("# {}\n\n", feedback.title.trim());
    let mut manifest_images = Vec::new();
    for block in &feedback.blocks {
        match block {
            RecordedFeedbackBlock::Text { text, .. } => {
                let text = text.trim();
                if !text.is_empty() {
                    markdown.push_str(text);
                    markdown.push_str("\n\n");
                }
            }
            RecordedFeedbackBlock::Image { snapshot_id } => {
                let snapshot = feedback
                    .snapshots
                    .iter()
                    .find(|snapshot| snapshot.id == *snapshot_id)
                    .ok_or((
                        "feedback_integrity_error",
                        format!("snapshot {snapshot_id} is missing"),
                    ))?;
                let extension = Path::new(&snapshot.image_path)
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("png");
                let file_name = format!("snapshot-{:02}.{extension}", snapshot.ordinal + 1);
                let target = images_dir.join(&file_name);
                let source =
                    require_feedback_file(data_dir, feedback.id, Path::new(&snapshot.image_path))?;
                fs::copy(source, &target).map_err(storage_error)?;
                set_private_file_permissions(&target).map_err(storage_error)?;
                let copied_sha = sha256_file(&target).map_err(storage_error)?;
                if copied_sha != snapshot.sha256 {
                    return Err((
                        "feedback_integrity_error",
                        format!("snapshot {} changed after capture", snapshot.id),
                    ));
                }
                let caption = if snapshot.caption.trim().is_empty() {
                    format!(
                        "Screenshot {} · {}",
                        snapshot.ordinal + 1,
                        format_duration(snapshot.anchor_ms)
                    )
                } else {
                    snapshot.caption.trim().to_owned()
                };
                markdown.push_str(&format!(
                    "![{}](images/{})\n\n",
                    escape_markdown_alt(&caption),
                    file_name
                ));
                manifest_images.push(PacketImage {
                    snapshot_id: snapshot.id,
                    path: format!("images/{file_name}"),
                    sha256: copied_sha,
                });
            }
        }
    }
    atomic_write(&root.join("feedback.md"), markdown.as_bytes()).map_err(storage_error)?;
    let manifest = PacketManifest {
        version: 1,
        feedback_id: feedback.id,
        revision: feedback.revision,
        source_created_at: feedback.created_at,
        markdown: "feedback.md".into(),
        images: manifest_images.clone(),
    };
    atomic_write(
        &root.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)
            .map_err(|error| ("feedback_packet_error", error.to_string()))?,
    )
    .map_err(storage_error)?;
    Ok((markdown, manifest_images))
}

fn load_compiled_packet(
    root: &Path,
    feedback: &RecordedFeedback,
) -> ControlResultAs<CompiledPacket> {
    let manifest: PacketManifest =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).map_err(storage_error)?)
            .map_err(|error| ("feedback_packet_error", error.to_string()))?;
    if manifest.version != 1
        || manifest.feedback_id != feedback.id
        || manifest.revision != feedback.revision
        || manifest.source_created_at != feedback.created_at
        || manifest.markdown != "feedback.md"
    {
        return Err((
            "feedback_packet_error",
            "cached feedback packet metadata does not match".into(),
        ));
    }
    let canonical_parent = root
        .parent()
        .ok_or(("feedback_packet_error", "packet path has no parent".into()))?
        .canonicalize()
        .map_err(storage_error)?;
    let canonical_root = root.canonicalize().map_err(storage_error)?;
    if canonical_root.parent() != Some(canonical_parent.as_path()) {
        return Err((
            "feedback_packet_error",
            "cached feedback packet is outside private storage".into(),
        ));
    }
    for image in &manifest.images {
        let relative = Path::new(&image.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err((
                "feedback_packet_error",
                "cached feedback packet contains an invalid image path".into(),
            ));
        }
        let path = root.join(relative).canonicalize().map_err(storage_error)?;
        if !path.starts_with(&canonical_root)
            || !path.is_file()
            || sha256_file(&path).map_err(storage_error)? != image.sha256
        {
            return Err((
                "feedback_packet_error",
                "cached feedback packet image failed validation".into(),
            ));
        }
    }
    let markdown_path = root.join("feedback.md");
    let canonical_markdown = markdown_path.canonicalize().map_err(storage_error)?;
    if !canonical_markdown.starts_with(&canonical_root) || !canonical_markdown.is_file() {
        return Err((
            "feedback_packet_error",
            "cached feedback packet markdown is invalid".into(),
        ));
    }
    let markdown = fs::read_to_string(&markdown_path).map_err(storage_error)?;
    if markdown != render_packet_markdown(feedback, &manifest.images)? {
        return Err((
            "feedback_packet_error",
            "cached feedback packet content does not match this revision".into(),
        ));
    }
    Ok(CompiledPacket {
        markdown,
        markdown_path,
        images: manifest.images,
    })
}

fn render_packet_markdown(
    feedback: &RecordedFeedback,
    images: &[PacketImage],
) -> ControlResultAs<String> {
    let mut markdown = format!("# {}\n\n", feedback.title.trim());
    let mut image_index = 0;
    for block in &feedback.blocks {
        match block {
            RecordedFeedbackBlock::Text { text, .. } => {
                let text = text.trim();
                if !text.is_empty() {
                    markdown.push_str(text);
                    markdown.push_str("\n\n");
                }
            }
            RecordedFeedbackBlock::Image { snapshot_id } => {
                let snapshot = feedback
                    .snapshots
                    .iter()
                    .find(|snapshot| snapshot.id == *snapshot_id)
                    .ok_or((
                        "feedback_integrity_error",
                        format!("snapshot {snapshot_id} is missing"),
                    ))?;
                let image = images.get(image_index).ok_or((
                    "feedback_packet_error",
                    "cached feedback packet is missing an image".into(),
                ))?;
                if image.snapshot_id != snapshot.id || image.sha256 != snapshot.sha256 {
                    return Err((
                        "feedback_packet_error",
                        "cached feedback packet image does not match this revision".into(),
                    ));
                }
                let caption = if snapshot.caption.trim().is_empty() {
                    format!(
                        "Screenshot {} · {}",
                        snapshot.ordinal + 1,
                        format_duration(snapshot.anchor_ms)
                    )
                } else {
                    snapshot.caption.trim().to_owned()
                };
                markdown.push_str(&format!(
                    "![{}]({})\n\n",
                    escape_markdown_alt(&caption),
                    image.path
                ));
                image_index += 1;
            }
        }
    }
    if image_index != images.len() {
        return Err((
            "feedback_packet_error",
            "cached feedback packet has unexpected images".into(),
        ));
    }
    Ok(markdown)
}

fn scratchpad_packet_content(packet: &CompiledPacket) -> String {
    let body = packet
        .markdown
        .split_once('\n')
        .map(|(_, body)| body.trim_start_matches('\n').to_owned())
        .unwrap_or_else(|| packet.markdown.clone());
    let mut content = format!("<!-- Workman recorded feedback -->\n\n{body}");
    let root = packet
        .markdown_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    for image in &packet.images {
        let relative = format!("]({})", image.path);
        let absolute = root
            .join(&image.path)
            .to_string_lossy()
            .replace('%', "%25")
            .replace('>', "%3E");
        content = content.replace(&relative, &format!("](<{absolute}>)"));
    }
    content
}

fn remove_abandoned_packet_builds(packet_parent: &Path) -> io::Result<()> {
    for entry in fs::read_dir(packet_parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".r") && name.ends_with(".building") && entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

type ControlResultAs<T> = Result<T, (&'static str, String)>;

fn validate_block_snapshots(
    registry: &ProcessRegistry,
    project_id: i64,
    feedback_id: i64,
    blocks: &[RecordedFeedbackBlock],
) -> ControlResultAs<()> {
    let feedback = require_feedback(registry, project_id, feedback_id)?;
    for block in blocks {
        if let RecordedFeedbackBlock::Image { snapshot_id } = block
            && !feedback
                .snapshots
                .iter()
                .any(|snapshot| snapshot.id == *snapshot_id)
        {
            return Err((
                "feedback_integrity_error",
                format!("snapshot {snapshot_id} does not belong to this feedback"),
            ));
        }
    }
    Ok(())
}

fn require_feedback(
    registry: &ProcessRegistry,
    project_id: i64,
    feedback_id: i64,
) -> ControlResultAs<RecordedFeedback> {
    RecordedFeedbackService::new(registry.store())
        .get(project_id, feedback_id)
        .map_err(feedback_error)?
        .ok_or((
            "feedback_not_found",
            "recorded feedback was not found in this project".into(),
        ))
}

fn feedback_directory(data_dir: &Path, feedback_id: i64) -> PathBuf {
    data_dir
        .join(FEEDBACK_DIRECTORY)
        .join(feedback_id.to_string())
}

fn require_feedback_file(
    data_dir: &Path,
    feedback_id: i64,
    path: &Path,
) -> ControlResultAs<PathBuf> {
    let root = feedback_directory(data_dir, feedback_id);
    let canonical_root = root.canonicalize().map_err(storage_error)?;
    let canonical_path = path.canonicalize().map_err(storage_error)?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return Err((
            "feedback_path_error",
            "media path is outside this feedback session".into(),
        ));
    }
    Ok(canonical_path)
}

fn create_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn append_journal(path: &Path, value: &Value) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    set_private_file_permissions(path)
}

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_directory(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    set_private_file_permissions(&temporary)?;
    fs::rename(temporary, path)
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    io::copy(&mut file, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

fn escape_markdown_alt(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace(['\r', '\n'], " ")
}

fn format_duration(ms: i64) -> String {
    let seconds = ms.max(0) / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn default_true() -> bool {
    true
}
fn params_as<T: for<'de> Deserialize<'de>>(params: Value) -> ControlResultAs<T> {
    serde_json::from_value(params).map_err(|error| ("invalid_params", error.to_string()))
}
fn json_value<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).expect("serializable control response")
}
fn feedback_error(error: RecordedFeedbackError) -> (&'static str, String) {
    let code = match error {
        RecordedFeedbackError::NotFound(_) => "feedback_not_found",
        RecordedFeedbackError::InvalidInput(_) => "invalid_params",
        RecordedFeedbackError::InvalidState { .. } => "feedback_invalid_state",
        RecordedFeedbackError::RevisionConflict { .. } => "feedback_revision_conflict",
        _ => "feedback_store_error",
    };
    (code, error.to_string())
}
fn storage_error(error: io::Error) -> (&'static str, String) {
    ("feedback_storage_error", error.to_string())
}
fn registry_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("feedback_target_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use tempfile::tempdir;
    use workman_core::{Project, RecordedFeedbackSnapshot, Store};

    fn ready_feedback(image_path: &Path, sha256: String) -> RecordedFeedback {
        RecordedFeedback {
            id: 7,
            project_id: 3,
            title: "Navigation feedback".into(),
            status: RecordedFeedbackStatus::Ready,
            revision: 4,
            duration_ms: 2_000,
            audio_path: None,
            transcript: vec![],
            blocks: vec![
                RecordedFeedbackBlock::Text {
                    text: "Move this control closer.".into(),
                    start_ms: 0,
                    end_ms: 1_000,
                },
                RecordedFeedbackBlock::Image { snapshot_id: 11 },
            ],
            snapshots: vec![RecordedFeedbackSnapshot {
                id: 11,
                feedback_id: 7,
                ordinal: 0,
                anchor_ms: 1_000,
                anchor_samples: 16_000,
                invoked_at_ms: 1_000,
                completed_at_ms: 1_050,
                image_path: image_path.to_string_lossy().into_owned(),
                caption: "Menu [open]".into(),
                width: 2,
                height: 2,
                sha256,
            }],
            deliveries: vec![],
            error_code: None,
            archived: false,
            lease_owner: None,
            lease_expires_at: None,
            created_at: 500,
            updated_at: 2_000,
        }
    }

    fn test_png(path: &Path) -> String {
        RgbaImage::from_pixel(2, 2, Rgba([20, 40, 60, 255]))
            .save(path)
            .unwrap();
        sha256_file(path).unwrap()
    }

    #[test]
    fn markdown_alt_text_is_escaped() {
        assert_eq!(escape_markdown_alt(r"one [two] \\"), r"one \[two\] \\\\");
    }

    #[test]
    fn duration_is_compact() {
        assert_eq!(format_duration(65_900), "1:05");
    }

    #[test]
    fn repeated_scratchpad_delivery_uses_an_available_name() {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 3,
                path: "/tmp/feedback-project".into(),
                name: "feedback-project".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        let service = ScratchpadService::attributed(&store, "user");
        let first =
            create_feedback_scratchpad(&service, 3, "Navigation feedback", "First").unwrap();
        let second = create_feedback_scratchpad(
            &service,
            3,
            "Navigation feedback",
            "<!-- Workman recorded feedback -->\n\n# User-authored heading\n\nSecond",
        )
        .unwrap();
        assert_eq!(first.name, "Navigation feedback");
        assert_eq!(second.name, "Navigation feedback (2)");
        assert!(second.content.contains("# User-authored heading"));

        service
            .write(
                3,
                None,
                "Conflicting heading".into(),
                "Existing".into(),
                None,
                None,
            )
            .unwrap();
        let error = create_feedback_scratchpad(
            &service,
            3,
            "Fallback name",
            "# Conflicting heading\n\nContent",
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ScratchpadServiceError::NameConflict { name, .. } if name == "Conflicting heading"
        ));
    }

    #[test]
    fn packet_is_immutable_private_and_uses_relative_images() {
        let temp = tempdir().unwrap();
        let media = feedback_directory(temp.path(), 7);
        create_private_directory(&media).unwrap();
        let image = media.join("snapshot.png");
        let feedback = ready_feedback(&image, test_png(&image));
        let abandoned = temp
            .path()
            .join(PACKET_DIRECTORY)
            .join("7")
            .join(".r1-crashed.building");
        create_private_directory(&abandoned).unwrap();

        let packet = compile_packet(temp.path(), &feedback).unwrap();
        assert!(!abandoned.exists());
        assert!(
            packet
                .markdown_path
                .starts_with(temp.path().join(PACKET_DIRECTORY))
        );
        assert!(packet.markdown.contains("Move this control closer."));
        assert!(
            packet
                .markdown
                .contains("![Menu \\[open\\]](images/snapshot-01.png)")
        );
        assert!(
            packet
                .markdown_path
                .with_file_name("manifest.json")
                .is_file()
        );
        assert!(
            packet
                .markdown_path
                .parent()
                .unwrap()
                .join("images/snapshot-01.png")
                .is_file()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&packet.markdown_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        let scratchpad = scratchpad_packet_content(&packet);
        assert!(scratchpad.starts_with("<!-- Workman recorded feedback -->"));
        assert!(scratchpad.contains(&format!(
            "](<{}>)",
            packet
                .markdown_path
                .parent()
                .unwrap()
                .join("images/snapshot-01.png")
                .display()
        )));

        let reused = compile_packet(temp.path(), &feedback).unwrap();
        assert_eq!(reused.markdown_path, packet.markdown_path);
        assert_eq!(
            fs::read_dir(packet.markdown_path.parent().unwrap().parent().unwrap())
                .unwrap()
                .count(),
            1
        );

        fs::write(&packet.markdown_path, "tampered").unwrap();
        let repaired = compile_packet(temp.path(), &feedback).unwrap();
        assert_eq!(repaired.markdown_path, packet.markdown_path);
        assert_eq!(repaired.markdown, packet.markdown);
    }

    #[test]
    fn packet_rejects_a_tampered_snapshot_path() {
        let temp = tempdir().unwrap();
        create_private_directory(&feedback_directory(temp.path(), 7)).unwrap();
        let outside = temp.path().join("outside.png");
        let feedback = ready_feedback(&outside, test_png(&outside));

        let error = compile_packet(temp.path(), &feedback).unwrap_err();
        assert_eq!(error.0, "feedback_path_error");
    }
}
