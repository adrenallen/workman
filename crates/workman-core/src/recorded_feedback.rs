//! Durable metadata for local recorded-feedback sessions.

use std::{error::Error, fmt};

use rusqlite::{OptionalExtension, params};

use crate::{
    ProjectId, RecordedFeedback, RecordedFeedbackAppendState, RecordedFeedbackBlock,
    RecordedFeedbackDelivery, RecordedFeedbackId, RecordedFeedbackSnapshot, RecordedFeedbackStatus,
    RecordedFeedbackSummary, RecordedFeedbackTranscriptSegment, Store, StoreError,
};

const MAX_TITLE_CHARS: usize = 160;
const MAX_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;
const MAX_BLOCKS: usize = 2_000;

#[derive(Debug)]
pub enum RecordedFeedbackError {
    Store(StoreError),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    NotFound(RecordedFeedbackId),
    InvalidInput(String),
    InvalidState {
        expected: &'static str,
        actual: RecordedFeedbackStatus,
    },
    RevisionConflict {
        expected: i64,
        current: i64,
    },
}

impl fmt::Display for RecordedFeedbackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::NotFound(id) => write!(formatter, "recorded feedback {id} was not found"),
            Self::InvalidInput(message) => formatter.write_str(message),
            Self::InvalidState { expected, actual } => {
                write!(
                    formatter,
                    "recorded feedback must be {expected}; it is {actual}"
                )
            }
            Self::RevisionConflict { expected, current } => write!(
                formatter,
                "recorded feedback changed (expected revision {expected}, current revision {current})"
            ),
        }
    }
}

impl Error for RecordedFeedbackError {}

impl From<StoreError> for RecordedFeedbackError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
impl From<rusqlite::Error> for RecordedFeedbackError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}
impl From<serde_json::Error> for RecordedFeedbackError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub type RecordedFeedbackResult<T> = Result<T, RecordedFeedbackError>;

pub struct RecordedFeedbackService<'store> {
    store: &'store Store,
}

#[derive(Debug, Clone)]
pub struct NewRecordedFeedbackSnapshot {
    pub ordinal: i64,
    pub anchor_ms: i64,
    pub anchor_samples: i64,
    pub invoked_at_ms: i64,
    pub completed_at_ms: i64,
    pub image_path: String,
    pub caption: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct RecordedFeedbackDocumentUpdate {
    pub expected_revision: i64,
    pub title: String,
    pub blocks: Vec<RecordedFeedbackBlock>,
    pub snapshot_captions: Vec<(i64, String)>,
    pub now_ms: i64,
}

#[derive(Debug, Clone)]
pub struct NewRecordedFeedbackDelivery {
    pub target_kind: String,
    pub target_id: Option<i64>,
    pub target_name: Option<String>,
    pub status: String,
    pub packet_path: Option<String>,
    pub error_message: Option<String>,
    pub now_ms: i64,
}

impl<'store> RecordedFeedbackService<'store> {
    pub fn new(store: &'store Store) -> Self {
        Self { store }
    }

    pub fn create(
        &self,
        project_id: ProjectId,
        title: &str,
        lease_owner: &str,
        lease_expires_at: i64,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        self.require_project(project_id)?;
        let title = normalized_title(title)?;
        self.store.connection().execute(
            "INSERT INTO recorded_feedback (
                project_id, title, status, lease_owner, lease_expires_at, created_at, updated_at
             ) VALUES (?1, ?2, 'recording', ?3, ?4, ?5, ?5)",
            params![project_id, title, lease_owner, lease_expires_at, now_ms],
        )?;
        self.require(project_id, self.store.connection().last_insert_rowid())
    }

    pub fn list(
        &self,
        project_id: ProjectId,
        archived: bool,
    ) -> RecordedFeedbackResult<Vec<RecordedFeedbackSummary>> {
        self.require_project(project_id)?;
        let mut statement = self.store.connection().prepare(
            "SELECT feedback.id, feedback.project_id, feedback.title, feedback.status,
                    feedback.revision, feedback.duration_ms, COUNT(snapshots.id),
                    feedback.archived, feedback.error_code, feedback.created_at, feedback.updated_at
             FROM recorded_feedback AS feedback
             LEFT JOIN recorded_feedback_snapshots AS snapshots ON snapshots.feedback_id = feedback.id
             WHERE feedback.project_id = ?1 AND feedback.archived = ?2
             GROUP BY feedback.id
             ORDER BY feedback.updated_at DESC, feedback.id DESC",
        )?;
        Ok(statement
            .query_map(params![project_id, archived], |row| {
                Ok(RecordedFeedbackSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    title: row.get(2)?,
                    status: row.get(3)?,
                    revision: row.get(4)?,
                    duration_ms: row.get(5)?,
                    snapshot_count: row.get(6)?,
                    archived: row.get(7)?,
                    error_code: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn fail_expired(
        &self,
        project_id: ProjectId,
        now_ms: i64,
    ) -> RecordedFeedbackResult<usize> {
        self.require_project(project_id)?;
        Ok(self.store.connection().execute(
            "UPDATE recorded_feedback SET status = 'failed', error_code = 'recording_interrupted',
                    lease_owner = NULL, lease_expires_at = NULL, revision = revision + 1,
                    updated_at = ?1
             WHERE project_id = ?2 AND status IN ('recording', 'transcribing')
               AND lease_expires_at IS NOT NULL AND lease_expires_at < ?1",
            params![now_ms, project_id],
        )?)
    }

    pub fn get(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
    ) -> RecordedFeedbackResult<Option<RecordedFeedback>> {
        let feedback = self
            .store
            .connection()
            .query_row(
                "SELECT id, project_id, title, status, revision, duration_ms, audio_path,
                    transcript_json, blocks_json, error_code, archived, lease_owner,
                    lease_expires_at, created_at, updated_at, append_state_json
             FROM recorded_feedback WHERE id = ?1 AND project_id = ?2",
                params![feedback_id, project_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                        row.get(14)?,
                        row.get::<_, Option<String>>(15)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            id,
            project_id,
            title,
            status,
            revision,
            duration_ms,
            audio_path,
            transcript_json,
            blocks_json,
            error_code,
            archived,
            lease_owner,
            lease_expires_at,
            created_at,
            updated_at,
            append_state_json,
        )) = feedback
        else {
            return Ok(None);
        };
        Ok(Some(RecordedFeedback {
            id,
            project_id,
            title,
            status,
            revision,
            duration_ms,
            audio_path,
            transcript: serde_json::from_str::<Vec<RecordedFeedbackTranscriptSegment>>(
                &transcript_json,
            )?,
            blocks: serde_json::from_str::<Vec<RecordedFeedbackBlock>>(&blocks_json)?,
            snapshots: self.snapshots(id)?,
            deliveries: self.deliveries(id)?,
            append_state: append_state_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?,
            error_code,
            archived,
            lease_owner,
            lease_expires_at,
            created_at,
            updated_at,
        }))
    }

    pub fn begin_append(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        expected_revision: i64,
        lease_owner: &str,
        now_ms: i64,
        lease_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        if current.status != RecordedFeedbackStatus::Ready {
            return Err(RecordedFeedbackError::InvalidState {
                expected: "ready",
                actual: current.status,
            });
        }
        if current.revision != expected_revision {
            return Err(RecordedFeedbackError::RevisionConflict {
                expected: expected_revision,
                current: current.revision,
            });
        }
        let checkpoint = RecordedFeedbackAppendState {
            duration_ms: current.duration_ms,
            audio_path: current.audio_path,
            next_ordinal: current
                .snapshots
                .iter()
                .map(|snapshot| snapshot.ordinal)
                .max()
                .unwrap_or(-1)
                .checked_add(1)
                .ok_or_else(|| {
                    RecordedFeedbackError::InvalidInput("snapshot ordinal is too large".into())
                })?,
        };
        self.store.connection().execute(
            "UPDATE recorded_feedback SET append_state_json = ?1, status = 'recording',
                audio_path = NULL, lease_owner = ?2, lease_expires_at = ?3,
                error_code = NULL, revision = revision + 1, updated_at = ?4 WHERE id = ?5",
            params![
                serde_json::to_string(&checkpoint)?,
                lease_owner,
                now_ms + lease_ms,
                now_ms,
                feedback_id
            ],
        )?;
        self.require(project_id, feedback_id)
    }

    /// Restore the original after a failed addition. Captured files remain local for recovery.
    pub fn discard_append(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        if current.status != RecordedFeedbackStatus::Failed {
            return Err(RecordedFeedbackError::InvalidState {
                expected: "failed",
                actual: current.status,
            });
        }
        let checkpoint = current.append_state.ok_or_else(|| {
            RecordedFeedbackError::InvalidInput("there is no added recording to discard".into())
        })?;
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM recorded_feedback_snapshots WHERE feedback_id = ?1 AND ordinal >= ?2",
            params![feedback_id, checkpoint.next_ordinal],
        )?;
        transaction.execute(
            "UPDATE recorded_feedback SET status = 'ready', append_state_json = NULL,
                duration_ms = ?1, audio_path = ?2, error_code = NULL,
                lease_owner = NULL, lease_expires_at = NULL, revision = revision + 1,
                updated_at = ?3 WHERE id = ?4",
            params![
                checkpoint.duration_ms,
                checkpoint.audio_path,
                now_ms,
                feedback_id
            ],
        )?;
        transaction.commit()?;
        self.require(project_id, feedback_id)
    }

    pub fn renew_lease(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        lease_owner: &str,
        expires_at: i64,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        let status = match current.status {
            RecordedFeedbackStatus::Recording => "recording",
            RecordedFeedbackStatus::Transcribing => "transcribing",
            RecordedFeedbackStatus::Failed if is_interrupted(&current) => {
                if current.audio_path.is_some() {
                    "transcribing"
                } else {
                    "recording"
                }
            }
            _ => {
                return Err(RecordedFeedbackError::InvalidState {
                    expected: "recording, transcribing, or interrupted",
                    actual: current.status,
                });
            }
        };
        let changed = self.store.connection().execute(
            "UPDATE recorded_feedback SET status = ?1, error_code = NULL, lease_owner = ?2,
                    lease_expires_at = ?3, updated_at = ?4
             WHERE id = ?5 AND project_id = ?6
               AND (status IN ('recording', 'transcribing')
                    OR (status = 'failed' AND error_code = 'recording_interrupted'))
               AND (lease_owner IS NULL OR lease_owner = ?2)",
            params![
                status,
                lease_owner,
                expires_at,
                now_ms,
                feedback_id,
                project_id
            ],
        )?;
        if changed == 0 {
            return Err(RecordedFeedbackError::InvalidInput(
                "recorded feedback is owned by another live desktop session".into(),
            ));
        }
        self.require(project_id, feedback_id)
    }

    pub fn add_snapshot(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        mut snapshot: NewRecordedFeedbackSnapshot,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        if current.status != RecordedFeedbackStatus::Recording && !is_interrupted(&current) {
            return Err(RecordedFeedbackError::InvalidState {
                expected: "recording or an interrupted recording",
                actual: current.status,
            });
        }
        validate_snapshot(&snapshot)?;
        if let Some(checkpoint) = &current.append_state {
            snapshot.ordinal = snapshot
                .ordinal
                .checked_add(checkpoint.next_ordinal)
                .ok_or_else(|| {
                    RecordedFeedbackError::InvalidInput("snapshot ordinal is too large".into())
                })?;
            snapshot.anchor_ms = snapshot
                .anchor_ms
                .checked_add(checkpoint.duration_ms)
                .ok_or_else(|| {
                    RecordedFeedbackError::InvalidInput("snapshot time is too large".into())
                })?;
            // Sample offsets are local to the audio file, whose sample rate can vary by segment.
        }
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO recorded_feedback_snapshots (
                feedback_id, ordinal, anchor_ms, anchor_samples, invoked_at_ms, completed_at_ms,
                image_path, caption, width, height, sha256
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                feedback_id,
                snapshot.ordinal,
                snapshot.anchor_ms,
                snapshot.anchor_samples,
                snapshot.invoked_at_ms,
                snapshot.completed_at_ms,
                snapshot.image_path,
                snapshot.caption,
                snapshot.width,
                snapshot.height,
                snapshot.sha256
            ],
        )?;
        transaction.execute(
            "UPDATE recorded_feedback SET status = 'recording', error_code = NULL,
                    revision = revision + 1, updated_at = ?1 WHERE id = ?2",
            params![now_ms, feedback_id],
        )?;
        transaction.commit()?;
        self.require(project_id, feedback_id)
    }

    pub fn begin_transcription(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        duration_ms: i64,
        audio_path: Option<&str>,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        if current.status != RecordedFeedbackStatus::Recording && !is_interrupted(&current) {
            return Err(RecordedFeedbackError::InvalidState {
                expected: "recording or an interrupted recording",
                actual: current.status,
            });
        }
        let duration_ms = duration_ms
            .max(0)
            .checked_add(
                current
                    .append_state
                    .as_ref()
                    .map_or(0, |state| state.duration_ms),
            )
            .ok_or_else(|| {
                RecordedFeedbackError::InvalidInput("recording duration is too large".into())
            })?;
        self.store.connection().execute(
            "UPDATE recorded_feedback SET status = 'transcribing', duration_ms = ?1,
                    audio_path = ?2, error_code = NULL, revision = revision + 1,
                    updated_at = ?3 WHERE id = ?4",
            params![duration_ms.max(0), audio_path, now_ms, feedback_id],
        )?;
        self.require(project_id, feedback_id)
    }

    pub fn complete(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        mut transcript: Vec<RecordedFeedbackTranscriptSegment>,
        mut blocks: Vec<RecordedFeedbackBlock>,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        if current.status != RecordedFeedbackStatus::Transcribing && !is_interrupted(&current) {
            return Err(RecordedFeedbackError::InvalidState {
                expected: "transcribing or interrupted transcription",
                actual: current.status,
            });
        }
        validate_document(&transcript, &blocks)?;
        if let Some(checkpoint) = &current.append_state {
            let offset = |value: i64| {
                value.checked_add(checkpoint.duration_ms).ok_or_else(|| {
                    RecordedFeedbackError::InvalidInput("transcript time is too large".into())
                })
            };
            for segment in &mut transcript {
                segment.start_ms = offset(segment.start_ms)?;
                segment.end_ms = offset(segment.end_ms)?;
            }
            for block in &mut blocks {
                match block {
                    RecordedFeedbackBlock::Text {
                        start_ms, end_ms, ..
                    } => {
                        *start_ms = offset(*start_ms)?;
                        *end_ms = offset(*end_ms)?;
                    }
                    RecordedFeedbackBlock::Image { snapshot_id } => {
                        if !current.snapshots.iter().any(|snapshot| {
                            snapshot.id == *snapshot_id
                                && snapshot.ordinal >= checkpoint.next_ordinal
                        }) {
                            return Err(RecordedFeedbackError::InvalidInput(
                                "added recording references an original snapshot".into(),
                            ));
                        }
                    }
                }
            }
            transcript.splice(0..0, current.transcript);
            blocks.splice(0..0, current.blocks);
            validate_document(&transcript, &blocks)?;
        }
        self.store.connection().execute(
            "UPDATE recorded_feedback SET status = 'ready', transcript_json = ?1, blocks_json = ?2,
                    append_state_json = NULL, error_code = NULL, lease_owner = NULL, lease_expires_at = NULL,
                    revision = revision + 1, updated_at = ?3 WHERE id = ?4",
            params![
                serde_json::to_string(&transcript)?,
                serde_json::to_string(&blocks)?,
                now_ms,
                feedback_id
            ],
        )?;
        self.require(project_id, feedback_id)
    }

    pub fn update_document(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        update: RecordedFeedbackDocumentUpdate,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        if current.status != RecordedFeedbackStatus::Ready {
            return Err(RecordedFeedbackError::InvalidState {
                expected: "ready",
                actual: current.status,
            });
        }
        if current.revision != update.expected_revision {
            return Err(RecordedFeedbackError::RevisionConflict {
                expected: update.expected_revision,
                current: current.revision,
            });
        }
        validate_document(&current.transcript, &update.blocks)?;
        for (snapshot_id, caption) in &update.snapshot_captions {
            if !current
                .snapshots
                .iter()
                .any(|snapshot| snapshot.id == *snapshot_id)
                || caption.chars().count() > 500
                || caption.contains(['\0', '\n', '\r'])
            {
                return Err(RecordedFeedbackError::InvalidInput(
                    "snapshot caption is invalid".into(),
                ));
            }
        }
        let title = normalized_title(&update.title)?;
        let transaction = self.store.connection().unchecked_transaction()?;
        let changed = transaction.execute(
            "UPDATE recorded_feedback SET title = ?1, blocks_json = ?2, revision = revision + 1,
                    updated_at = ?3 WHERE id = ?4 AND project_id = ?5 AND revision = ?6",
            params![
                title,
                serde_json::to_string(&update.blocks)?,
                update.now_ms,
                feedback_id,
                project_id,
                update.expected_revision
            ],
        )?;
        if changed == 0 {
            return Err(RecordedFeedbackError::RevisionConflict {
                expected: update.expected_revision,
                current: self.require(project_id, feedback_id)?.revision,
            });
        }
        for (snapshot_id, caption) in &update.snapshot_captions {
            transaction.execute(
                "UPDATE recorded_feedback_snapshots SET caption = ?1 WHERE id = ?2 AND feedback_id = ?3",
                params![caption.trim(), snapshot_id, feedback_id],
            )?;
        }
        transaction.commit()?;
        self.require(project_id, feedback_id)
    }

    pub fn mark_failed(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        code: &str,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        let current = self.require(project_id, feedback_id)?;
        if !matches!(
            current.status,
            RecordedFeedbackStatus::Recording
                | RecordedFeedbackStatus::Transcribing
                | RecordedFeedbackStatus::Failed
        ) {
            return Err(RecordedFeedbackError::InvalidState {
                expected: "recording, transcribing, or failed",
                actual: current.status,
            });
        }
        if code.is_empty()
            || code.len() > 120
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(RecordedFeedbackError::InvalidInput(
                "failure code is invalid".into(),
            ));
        }
        if current.status == RecordedFeedbackStatus::Failed
            && current.error_code.as_deref() == Some(code)
        {
            return Ok(current);
        }
        self.store.connection().execute(
            "UPDATE recorded_feedback SET status = 'failed', error_code = ?1,
                    lease_owner = NULL, lease_expires_at = NULL, revision = revision + 1,
                    updated_at = ?2 WHERE id = ?3",
            params![code, now_ms, feedback_id],
        )?;
        self.require(project_id, feedback_id)
    }

    pub fn archive(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        archived: bool,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        self.require(project_id, feedback_id)?;
        self.store.connection().execute(
            "UPDATE recorded_feedback SET archived = ?1, revision = revision + 1, updated_at = ?2 WHERE id = ?3",
            params![archived, now_ms, feedback_id],
        )?;
        self.require(project_id, feedback_id)
    }

    pub fn delete(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
    ) -> RecordedFeedbackResult<bool> {
        Ok(self.store.connection().execute(
            "DELETE FROM recorded_feedback WHERE id = ?1 AND project_id = ?2",
            params![feedback_id, project_id],
        )? > 0)
    }

    pub fn record_delivery(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        delivery: NewRecordedFeedbackDelivery,
    ) -> RecordedFeedbackResult<RecordedFeedbackDelivery> {
        let feedback = self.require(project_id, feedback_id)?;
        if !matches!(
            delivery.target_kind.as_str(),
            "agent" | "scratchpad" | "clipboard"
        ) || !matches!(
            delivery.status.as_str(),
            "pending" | "sent" | "queued" | "unverified" | "failed"
        ) {
            return Err(RecordedFeedbackError::InvalidInput(
                "delivery metadata is invalid".into(),
            ));
        }
        self.store.connection().execute(
            "INSERT INTO recorded_feedback_deliveries (
                feedback_id, target_kind, target_id, status, packet_path, error_message,
                created_at, updated_at, target_name, feedback_revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9)",
            params![
                feedback_id,
                delivery.target_kind,
                delivery.target_id,
                delivery.status,
                delivery.packet_path,
                delivery.error_message,
                delivery.now_ms,
                delivery.target_name,
                feedback.revision
            ],
        )?;
        let id = self.store.connection().last_insert_rowid();
        self.store
            .connection()
            .query_row(
                "SELECT id, feedback_id, target_kind, target_id, status, packet_path,
                    error_message, created_at, updated_at, target_name, feedback_revision
             FROM recorded_feedback_deliveries WHERE id = ?1",
                [id],
                |row| {
                    Ok(RecordedFeedbackDelivery {
                        id: row.get(0)?,
                        feedback_id: row.get(1)?,
                        target_kind: row.get(2)?,
                        target_id: row.get(3)?,
                        status: row.get(4)?,
                        packet_path: row.get(5)?,
                        error_message: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        target_name: row.get(9)?,
                        feedback_revision: row.get(10)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn finish_delivery(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
        delivery_id: i64,
        error: Option<&str>,
        now_ms: i64,
    ) -> RecordedFeedbackResult<RecordedFeedbackDelivery> {
        self.require(project_id, feedback_id)?;
        let status = if error.is_some() { "failed" } else { "sent" };
        self.store.connection().execute(
            "UPDATE recorded_feedback_deliveries SET status = ?1, error_message = ?2, updated_at = ?3
             WHERE id = ?4 AND feedback_id = ?5 AND status = 'pending'",
            params![status, error.map(|message| message.chars().take(1000).collect::<String>()), now_ms, delivery_id, feedback_id],
        )?;
        self.deliveries(feedback_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                RecordedFeedbackError::InvalidInput(
                    "delivery was not found in this feedback".into(),
                )
            })
    }

    fn snapshots(
        &self,
        feedback_id: RecordedFeedbackId,
    ) -> RecordedFeedbackResult<Vec<RecordedFeedbackSnapshot>> {
        let mut statement = self.store.connection().prepare(
            "SELECT id, feedback_id, ordinal, anchor_ms, anchor_samples, invoked_at_ms,
                    completed_at_ms, image_path, caption, width, height, sha256
             FROM recorded_feedback_snapshots WHERE feedback_id = ?1 ORDER BY ordinal, id",
        )?;
        Ok(statement
            .query_map([feedback_id], |row| {
                Ok(RecordedFeedbackSnapshot {
                    id: row.get(0)?,
                    feedback_id: row.get(1)?,
                    ordinal: row.get(2)?,
                    anchor_ms: row.get(3)?,
                    anchor_samples: row.get(4)?,
                    invoked_at_ms: row.get(5)?,
                    completed_at_ms: row.get(6)?,
                    image_path: row.get(7)?,
                    caption: row.get(8)?,
                    width: row.get(9)?,
                    height: row.get(10)?,
                    sha256: row.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn deliveries(
        &self,
        feedback_id: RecordedFeedbackId,
    ) -> RecordedFeedbackResult<Vec<RecordedFeedbackDelivery>> {
        let mut statement = self.store.connection().prepare(
            "SELECT id, feedback_id, target_kind, target_id, status, packet_path,
                    error_message, created_at, updated_at, target_name, feedback_revision
             FROM recorded_feedback_deliveries WHERE feedback_id = ?1 ORDER BY created_at DESC, id DESC",
        )?;
        Ok(statement
            .query_map([feedback_id], |row| {
                Ok(RecordedFeedbackDelivery {
                    id: row.get(0)?,
                    feedback_id: row.get(1)?,
                    target_kind: row.get(2)?,
                    target_id: row.get(3)?,
                    status: row.get(4)?,
                    packet_path: row.get(5)?,
                    error_message: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    target_name: row.get(9)?,
                    feedback_revision: row.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn require(
        &self,
        project_id: ProjectId,
        feedback_id: RecordedFeedbackId,
    ) -> RecordedFeedbackResult<RecordedFeedback> {
        self.get(project_id, feedback_id)?
            .ok_or(RecordedFeedbackError::NotFound(feedback_id))
    }

    fn require_project(&self, project_id: ProjectId) -> RecordedFeedbackResult<()> {
        if self.store.get_project(project_id)?.is_none() {
            return Err(RecordedFeedbackError::InvalidInput(format!(
                "project {project_id} was not found"
            )));
        }
        Ok(())
    }
}

fn normalized_title(value: &str) -> RecordedFeedbackResult<String> {
    let title = value.trim();
    if title.is_empty()
        || title.chars().count() > MAX_TITLE_CHARS
        || title.contains(['\0', '\n', '\r'])
    {
        return Err(RecordedFeedbackError::InvalidInput(format!(
            "title must be between 1 and {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(title.to_owned())
}

fn validate_snapshot(snapshot: &NewRecordedFeedbackSnapshot) -> RecordedFeedbackResult<()> {
    if snapshot.ordinal < 0
        || snapshot.anchor_ms < 0
        || snapshot.anchor_samples < 0
        || snapshot.invoked_at_ms < 0
        || snapshot.completed_at_ms < snapshot.invoked_at_ms
        || snapshot.width == 0
        || snapshot.height == 0
        || snapshot.image_path.is_empty()
        || snapshot.sha256.len() != 64
        || !snapshot.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RecordedFeedbackError::InvalidInput(
            "snapshot metadata is invalid".into(),
        ));
    }
    Ok(())
}

fn is_interrupted(feedback: &RecordedFeedback) -> bool {
    feedback.status == RecordedFeedbackStatus::Failed
        && feedback.error_code.as_deref() == Some("recording_interrupted")
}

fn validate_document(
    transcript: &[RecordedFeedbackTranscriptSegment],
    blocks: &[RecordedFeedbackBlock],
) -> RecordedFeedbackResult<()> {
    if blocks.len() > MAX_BLOCKS {
        return Err(RecordedFeedbackError::InvalidInput(format!(
            "feedback may contain at most {MAX_BLOCKS} blocks"
        )));
    }
    if serde_json::to_vec(transcript)?.len() > MAX_TRANSCRIPT_BYTES
        || serde_json::to_vec(blocks)?.len() > MAX_TRANSCRIPT_BYTES
    {
        return Err(RecordedFeedbackError::InvalidInput(
            "transcript is too large".into(),
        ));
    }
    for segment in transcript {
        if segment.start_ms < 0 || segment.end_ms < segment.start_ms || segment.text.contains('\0')
        {
            return Err(RecordedFeedbackError::InvalidInput(
                "transcript timing is invalid".into(),
            ));
        }
    }
    for block in blocks {
        match block {
            RecordedFeedbackBlock::Text {
                text,
                start_ms,
                end_ms,
            } if *start_ms < 0 || *end_ms < *start_ms || text.contains('\0') => {
                return Err(RecordedFeedbackError::InvalidInput(
                    "feedback block timing is invalid".into(),
                ));
            }
            RecordedFeedbackBlock::Image { snapshot_id } if *snapshot_id <= 0 => {
                return Err(RecordedFeedbackError::InvalidInput(
                    "feedback image reference is invalid".into(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Project, Store};

    fn store_with_project() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 1,
                path: "/tmp/rf-project".into(),
                name: "rf".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        store
    }

    fn snapshot(ordinal: i64) -> NewRecordedFeedbackSnapshot {
        NewRecordedFeedbackSnapshot {
            ordinal,
            anchor_ms: 500,
            anchor_samples: 8_000,
            invoked_at_ms: 1_500,
            completed_at_ms: 1_550,
            image_path: format!("/managed/snapshot-{ordinal}.png"),
            caption: "Keep this caption".into(),
            width: 100,
            height: 80,
            sha256: "a".repeat(64),
        }
    }

    fn ready_document(service: &RecordedFeedbackService<'_>) -> RecordedFeedback {
        let created = service
            .create(1, "Checkout feedback", "desktop", 12_000, 1_000)
            .unwrap();
        let captured = service
            .add_snapshot(1, created.id, snapshot(0), 1_600)
            .unwrap();
        service
            .begin_transcription(1, created.id, 2_000, Some("/managed/original.wav"), 3_000)
            .unwrap();
        service
            .complete(
                1,
                created.id,
                vec![RecordedFeedbackTranscriptSegment {
                    text: "Original speech".into(),
                    start_ms: 0,
                    end_ms: 1_000,
                }],
                vec![
                    RecordedFeedbackBlock::Text {
                        text: "User-edited text".into(),
                        start_ms: 0,
                        end_ms: 1_000,
                    },
                    RecordedFeedbackBlock::Image {
                        snapshot_id: captured.snapshots[0].id,
                    },
                ],
                3_100,
            )
            .unwrap()
    }

    #[test]
    fn repeated_appends_preserve_edits_media_and_delivery_history() {
        let store = store_with_project();
        let service = RecordedFeedbackService::new(&store);
        let original = ready_document(&service);
        let receipt = service
            .record_delivery(
                1,
                original.id,
                NewRecordedFeedbackDelivery {
                    target_kind: "agent".into(),
                    target_id: Some(42),
                    target_name: Some("Layout agent".into()),
                    status: "pending".into(),
                    packet_path: Some("/packet/r4/feedback.md".into()),
                    error_message: None,
                    now_ms: 3_200,
                },
            )
            .unwrap();
        service
            .finish_delivery(1, original.id, receipt.id, None, 3_300)
            .unwrap();
        let mut previous = original.clone();
        for addition in 0..2 {
            let now = 4_000 + addition * 4_000;
            assert!(matches!(
                service.begin_append(
                    1,
                    original.id,
                    previous.revision - 1,
                    "desktop",
                    now,
                    15_000
                ),
                Err(RecordedFeedbackError::RevisionConflict { .. })
            ));
            service
                .begin_append(1, original.id, previous.revision, "desktop", now, 15_000)
                .unwrap();
            assert!(
                service
                    .begin_append(1, original.id, previous.revision, "desktop", now, 15_000)
                    .is_err()
            );
            let captured = service
                .add_snapshot(1, original.id, snapshot(0), now + 500)
                .unwrap();
            let latest = captured.snapshots.last().unwrap();
            assert_eq!(latest.ordinal, addition + 1);
            assert_eq!(latest.anchor_ms, previous.duration_ms + 500);
            let transcribing = service
                .begin_transcription(
                    1,
                    original.id,
                    1_000,
                    Some("/managed/segment/audio.wav"),
                    now + 1_000,
                )
                .unwrap();
            assert_eq!(transcribing.duration_ms, previous.duration_ms + 1_000);
            let ready = service
                .complete(
                    1,
                    original.id,
                    vec![RecordedFeedbackTranscriptSegment {
                        text: "More feedback".into(),
                        start_ms: 100,
                        end_ms: 900,
                    }],
                    vec![
                        RecordedFeedbackBlock::Text {
                            text: "More feedback".into(),
                            start_ms: 100,
                            end_ms: 900,
                        },
                        RecordedFeedbackBlock::Image {
                            snapshot_id: latest.id,
                        },
                    ],
                    now + 1_100,
                )
                .unwrap();
            assert_eq!(&ready.blocks[..previous.blocks.len()], &previous.blocks);
            assert_eq!(
                ready.transcript.last().unwrap().start_ms,
                previous.duration_ms + 100
            );
            assert_eq!(ready.snapshots[0], original.snapshots[0]);
            assert_eq!(ready.deliveries[0].status, "sent");
            assert_eq!(
                ready.deliveries[0].feedback_revision,
                Some(original.revision)
            );
            assert_eq!(
                ready.deliveries[0].target_name.as_deref(),
                Some("Layout agent")
            );
            assert!(ready.append_state.is_none());
            previous = ready;
        }
        assert_eq!(service.list(1, false).unwrap().len(), 1);
    }

    #[test]
    fn interrupted_append_checkpoint_survives_reopen_and_can_keep_original() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("feedback.sqlite");
        let original;
        {
            let store = Store::open(&path).unwrap();
            store
                .put_project(&store_with_project().get_project(1).unwrap().unwrap())
                .unwrap();
            let service = RecordedFeedbackService::new(&store);
            original = ready_document(&service);
            service
                .begin_append(1, original.id, original.revision, "desktop", 4_000, 1_000)
                .unwrap();
            service
                .add_snapshot(1, original.id, snapshot(0), 4_500)
                .unwrap();
            service
                .begin_transcription(1, original.id, 1_000, Some("/managed/added.wav"), 4_900)
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let service = RecordedFeedbackService::new(&store);
        assert_eq!(service.fail_expired(1, 5_001).unwrap(), 1);
        let failed = service.get(1, original.id).unwrap().unwrap();
        assert_eq!(failed.blocks, original.blocks);
        assert!(failed.append_state.is_some());
        assert!(service.discard_append(2, original.id, 5_100).is_err());
        let restored = service.discard_append(1, original.id, 5_100).unwrap();
        assert_eq!(restored.status, RecordedFeedbackStatus::Ready);
        assert_eq!(restored.blocks, original.blocks);
        assert_eq!(restored.transcript, original.transcript);
        assert_eq!(restored.snapshots, original.snapshots);
        assert_eq!(restored.duration_ms, original.duration_ms);
        assert_eq!(restored.audio_path, original.audio_path);
        assert!(restored.append_state.is_none());
    }

    #[test]
    fn delivery_receipts_are_scoped_and_terminal_results_are_idempotent() {
        let store = store_with_project();
        let service = RecordedFeedbackService::new(&store);
        let feedback = ready_document(&service);
        let other = ready_document(&service);
        for error in [None, Some("Clipboard image import failed")] {
            let receipt = service
                .record_delivery(
                    1,
                    feedback.id,
                    NewRecordedFeedbackDelivery {
                        target_kind: "agent".into(),
                        target_id: Some(42),
                        target_name: Some("Original name".into()),
                        status: "pending".into(),
                        packet_path: None,
                        error_message: None,
                        now_ms: 4_000,
                    },
                )
                .unwrap();
            assert!(
                service
                    .finish_delivery(2, feedback.id, receipt.id, None, 4_100)
                    .is_err()
            );
            assert!(
                service
                    .finish_delivery(1, other.id, receipt.id, None, 4_100)
                    .is_err()
            );
            let finished = service
                .finish_delivery(1, feedback.id, receipt.id, error, 4_200)
                .unwrap();
            assert_eq!(
                finished.status,
                if error.is_some() { "failed" } else { "sent" }
            );
            assert_eq!(finished.error_message.as_deref(), error);
            assert_eq!(finished.updated_at, 4_200);
            assert_eq!(
                service
                    .finish_delivery(1, feedback.id, receipt.id, Some("late duplicate"), 4_300)
                    .unwrap(),
                finished
            );
        }
        assert_eq!(
            service
                .get(1, feedback.id)
                .unwrap()
                .unwrap()
                .deliveries
                .len(),
            2
        );
    }

    #[test]
    fn lifecycle_is_revision_guarded_and_project_scoped() {
        let store = store_with_project();
        let service = RecordedFeedbackService::new(&store);
        let created = service
            .create(1, "Checkout feedback", "desktop-1", 12_000, 1_000)
            .unwrap();
        assert_eq!(created.status, RecordedFeedbackStatus::Recording);
        let with_snapshot = service
            .add_snapshot(
                1,
                created.id,
                NewRecordedFeedbackSnapshot {
                    ordinal: 0,
                    anchor_ms: 500,
                    anchor_samples: 8_000,
                    invoked_at_ms: 1_500,
                    completed_at_ms: 1_550,
                    image_path: "/managed/snapshot-1.png".into(),
                    caption: String::new(),
                    width: 100,
                    height: 80,
                    sha256: "a".repeat(64),
                },
                1_600,
            )
            .unwrap();
        assert_eq!(with_snapshot.snapshots.len(), 1);
        let transcribing = service
            .begin_transcription(1, created.id, 2_000, Some("/managed/audio.wav"), 3_000)
            .unwrap();
        let ready = service
            .complete(
                1,
                created.id,
                vec![],
                vec![RecordedFeedbackBlock::Image {
                    snapshot_id: transcribing.snapshots[0].id,
                }],
                3_100,
            )
            .unwrap();
        assert_eq!(ready.status, RecordedFeedbackStatus::Ready);
        assert!(matches!(
            service.update_document(
                1,
                created.id,
                RecordedFeedbackDocumentUpdate {
                    expected_revision: ready.revision - 1,
                    title: "Edited".into(),
                    blocks: ready.blocks.clone(),
                    snapshot_captions: vec![],
                    now_ms: 3_200,
                }
            ),
            Err(RecordedFeedbackError::RevisionConflict { .. })
        ));
    }

    #[test]
    fn interrupted_sessions_resume_from_native_progress() {
        let store = store_with_project();
        let service = RecordedFeedbackService::new(&store);
        let created = service
            .create(1, "Interrupted feedback", "desktop-1", 2_000, 1_000)
            .unwrap();
        assert_eq!(service.fail_expired(1, 2_000).unwrap(), 0);
        assert_eq!(service.fail_expired(1, 2_001).unwrap(), 1);
        let failed = service.get(1, created.id).unwrap().unwrap();
        assert_eq!(failed.status, RecordedFeedbackStatus::Failed);
        assert_eq!(failed.error_code.as_deref(), Some("recording_interrupted"));

        let transcribing = service
            .begin_transcription(1, created.id, 2_500, Some("/managed/audio.wav"), 2_100)
            .unwrap();
        assert_eq!(transcribing.status, RecordedFeedbackStatus::Transcribing);
        assert_eq!(transcribing.error_code, None);

        let second = service
            .create(1, "Interrupted snapshot", "desktop-1", 4_000, 3_000)
            .unwrap();
        assert_eq!(service.fail_expired(1, 4_001).unwrap(), 1);
        let recovered = service
            .add_snapshot(
                1,
                second.id,
                NewRecordedFeedbackSnapshot {
                    ordinal: 0,
                    anchor_ms: 1_000,
                    anchor_samples: 16_000,
                    invoked_at_ms: 4_100,
                    completed_at_ms: 4_150,
                    image_path: "/managed/recovered.png".into(),
                    caption: String::new(),
                    width: 100,
                    height: 80,
                    sha256: "b".repeat(64),
                },
                4_200,
            )
            .unwrap();
        assert_eq!(recovered.status, RecordedFeedbackStatus::Recording);
        assert_eq!(recovered.error_code, None);

        let third = service
            .create(1, "Interrupted transcription", "desktop-1", 6_000, 5_000)
            .unwrap();
        service
            .begin_transcription(1, third.id, 1_000, Some("/managed/third.wav"), 5_500)
            .unwrap();
        assert_eq!(service.fail_expired(1, 6_001).unwrap(), 1);
        let completed = service
            .complete(1, third.id, vec![], vec![], 6_100)
            .unwrap();
        assert_eq!(completed.status, RecordedFeedbackStatus::Ready);
        assert_eq!(completed.error_code, None);

        let fourth = service
            .create(1, "Interrupted renewal", "desktop-1", 8_000, 7_000)
            .unwrap();
        assert_eq!(service.fail_expired(1, 8_001).unwrap(), 1);
        let renewed = service
            .renew_lease(1, fourth.id, "desktop-1", 10_000, 8_100)
            .unwrap();
        assert_eq!(renewed.status, RecordedFeedbackStatus::Recording);
        assert_eq!(renewed.error_code, None);
        assert_eq!(renewed.lease_expires_at, Some(10_000));
    }
}
