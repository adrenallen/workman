//! Revision-guarded scratchpad editing and filesystem import/export.

use std::{
    collections::HashSet,
    error::Error,
    fmt, fs,
    path::{Component, Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    Project, ProjectId, Scratchpad, ScratchpadComment, ScratchpadCommentId, ScratchpadId, Store,
    StoreError,
};

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 200;
const DEFAULT_FIND_LIMIT: usize = 20;
const MAX_FIND_LIMIT: usize = 100;
const MAX_CONTEXT_LINES: usize = 3;
const MAX_SNIPPET_CHARS: usize = 240;
const MAX_COMMENT_BODY_CHARS: usize = 65_536;
const MAX_ANCHOR_CONTEXT_CHARS: usize = 64;
const MAX_COMMENT_QUOTE_CHARS: usize = 4_096;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScratchpadReadMode {
    #[default]
    Full,
    Content,
    Headings,
    Section,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScratchpadFindScope {
    #[default]
    All,
    Headings,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScratchpadEditTarget {
    Section { heading: String },
    LineRange { offset: usize, limit: usize },
}

#[derive(Debug, Clone, Default)]
pub struct ScratchpadListQuery {
    pub query: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    pub offset: usize,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ScratchpadFindQuery {
    pub query: String,
    pub scope: ScratchpadFindScope,
    pub case_sensitive: bool,
    pub limit: Option<usize>,
    pub context_lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadSummary {
    pub id: ScratchpadId,
    pub project_id: ProjectId,
    pub name: String,
    pub revision: i64,
    pub archived: bool,
    pub sort_order: i64,
    pub tags: Vec<String>,
    pub created_by: String,
    pub updated_by: String,
    pub unresolved_comment_count: usize,
    pub comments_revision: i64,
    pub matched_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_snippet: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScratchpadAnchorState {
    Anchored,
    Orphaned,
    Unanchored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadAnchorResolution {
    pub anchor_state: ScratchpadAnchorState,
    pub current_start: Option<usize>,
    pub current_end: Option<usize>,
    pub current_start_line: Option<usize>,
    pub current_end_line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadCommentView {
    #[serde(flatten)]
    pub comment: ScratchpadComment,
    #[serde(flatten)]
    pub anchor: ScratchpadAnchorResolution,
    pub actor_kind: ScratchpadCommentActorKind,
    pub can_edit: bool,
    pub can_resolve: bool,
    pub can_delete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScratchpadCommentActorKind {
    User,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadCommentPage {
    pub comments: Vec<ScratchpadCommentView>,
    pub total_count: usize,
    pub unresolved_count: usize,
    pub comments_revision: i64,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct NewScratchpadComment {
    pub body: String,
    pub quote: Option<String>,
    pub anchor_start: Option<usize>,
    pub anchor_end: Option<usize>,
    pub anchor_prefix: Option<String>,
    pub anchor_suffix: Option<String>,
    pub allow_unanchored: bool,
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadPage {
    pub scratchpads: Vec<ScratchpadSummary>,
    pub total_count: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadRead {
    pub scratchpad: Scratchpad,
    pub total_lines: usize,
    pub offset: usize,
    pub returned_lines: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadTail {
    pub scratchpad_id: ScratchpadId,
    pub project_id: ProjectId,
    pub name: String,
    pub revision: i64,
    pub created_by: String,
    pub updated_by: String,
    pub content: String,
    pub total_lines: usize,
    pub requested_lines: usize,
    pub returned_lines: usize,
    pub start_offset: usize,
    pub has_more_above: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadHeading {
    pub level: usize,
    pub text: String,
    pub line_number: usize,
    pub read_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadContextLine {
    pub line_number: usize,
    pub read_offset: usize,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadFindMatch {
    pub kind: String,
    pub line_number: usize,
    pub read_offset: usize,
    pub heading: Option<ScratchpadHeading>,
    pub line: String,
    pub snippet: String,
    pub context: Vec<ScratchpadContextLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadFindResult {
    pub scratchpad_id: ScratchpadId,
    pub project_id: ProjectId,
    pub name: String,
    pub revision: i64,
    pub created_by: String,
    pub updated_by: String,
    pub query: String,
    pub scope: ScratchpadFindScope,
    pub case_sensitive: bool,
    pub total_lines: usize,
    pub total_matches: usize,
    pub returned_count: usize,
    pub limit: usize,
    pub context_lines: usize,
    pub has_more: bool,
    pub matches: Vec<ScratchpadFindMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScratchpadServiceError {
    Store(String),
    Io(String),
    ProjectNotFound(ProjectId),
    ScratchpadNotFound(ScratchpadId),
    CommentNotFound(ScratchpadCommentId),
    CommentPermissionDenied {
        comment_id: ScratchpadCommentId,
        action: &'static str,
    },
    RevisionConflict {
        scratchpad_id: ScratchpadId,
        expected: i64,
        current: i64,
    },
    NameConflict {
        project_id: ProjectId,
        name: String,
    },
    HeadingNotFound(String),
    InvalidInput(String),
    PathEscapesProject(PathBuf),
}

impl ScratchpadServiceError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "store_error",
            Self::Io(_) => "scratchpad_io_error",
            Self::ProjectNotFound(_) => "project_not_found",
            Self::ScratchpadNotFound(_) => "scratchpad_not_found",
            Self::CommentNotFound(_) => "scratchpad_comment_not_found",
            Self::CommentPermissionDenied { .. } => "scratchpad_comment_permission_denied",
            Self::RevisionConflict { .. } => "scratchpad_revision_conflict",
            Self::NameConflict { .. } => "scratchpad_name_conflict",
            Self::HeadingNotFound(_) => "scratchpad_heading_not_found",
            Self::InvalidInput(_) => "invalid_scratchpad_input",
            Self::PathEscapesProject(_) => "scratchpad_path_escape",
        }
    }
}

impl fmt::Display for ScratchpadServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(message) | Self::Io(message) | Self::InvalidInput(message) => {
                formatter.write_str(message)
            }
            Self::ProjectNotFound(id) => write!(formatter, "project {id} was not found"),
            Self::ScratchpadNotFound(id) => {
                write!(formatter, "scratchpad {id} was not found in this project")
            }
            Self::CommentNotFound(id) => write!(formatter, "scratchpad comment {id} was not found"),
            Self::CommentPermissionDenied { comment_id, action } => write!(
                formatter,
                "scratchpad comment {comment_id} may only be {action} by its author"
            ),
            Self::RevisionConflict {
                scratchpad_id,
                expected,
                current,
            } => write!(
                formatter,
                "scratchpad {scratchpad_id} revision mismatch: expected {expected}, current {current}"
            ),
            Self::NameConflict { project_id, name } => write!(
                formatter,
                "scratchpad name {name:?} already exists in project {project_id}"
            ),
            Self::HeadingNotFound(heading) => {
                write!(formatter, "markdown heading {heading:?} was not found")
            }
            Self::PathEscapesProject(path) => write!(
                formatter,
                "relative path {} escapes the project directory",
                path.display()
            ),
        }
    }
}

impl Error for ScratchpadServiceError {}

impl From<StoreError> for ScratchpadServiceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

impl From<rusqlite::Error> for ScratchpadServiceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.to_string())
    }
}

impl From<std::io::Error> for ScratchpadServiceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub type ScratchpadServiceResult<T> = Result<T, ScratchpadServiceError>;

/// Internal scratchpad service shared by MCP and future control/UI adapters.
pub struct ScratchpadService<'store> {
    store: &'store Store,
    actor_label: String,
}

impl<'store> ScratchpadService<'store> {
    pub fn new(store: &'store Store) -> Self {
        Self {
            store,
            actor_label: "workman".into(),
        }
    }

    pub fn attributed(store: &'store Store, actor_label: impl Into<String>) -> Self {
        Self {
            store,
            actor_label: actor_label.into(),
        }
    }

    pub fn write(
        &self,
        project_id: ProjectId,
        scratchpad_id: Option<ScratchpadId>,
        name: String,
        content: String,
        tags: Option<Vec<String>>,
        expected_revision: Option<i64>,
    ) -> ScratchpadServiceResult<(Scratchpad, bool)> {
        self.require_project(project_id)?;
        let content = normalize_line_endings(&content);
        let (name, content) = split_leading_h1(name, content)?;
        let tags = tags.map(normalize_tags).transpose()?;
        match scratchpad_id {
            None => {
                if expected_revision.is_some() {
                    return Err(ScratchpadServiceError::InvalidInput(
                        "expected_revision must be omitted when creating a scratchpad".into(),
                    ));
                }
                self.ensure_name_available(project_id, &name, None)?;
                let transaction = self.store.connection().unchecked_transaction()?;
                transaction.execute(
                    "INSERT INTO scratchpads (
                        project_id, name, content, revision, archived, sort_order,
                        created_by, updated_by
                     ) VALUES (
                        ?1, ?2, ?3, 1, 0,
                        (SELECT COALESCE(MAX(sort_order), -1) + 1
                         FROM scratchpads WHERE project_id = ?1),
                        ?4, ?4
                     )",
                    params![project_id, name, content, self.actor_label],
                )?;
                let id = transaction.last_insert_rowid();
                replace_tags(&transaction, id, tags.as_deref().unwrap_or_default())?;
                transaction.commit()?;
                Ok((self.require_scratchpad(project_id, id)?, true))
            }
            Some(id) => {
                let expected = expected_revision.ok_or_else(|| {
                    ScratchpadServiceError::InvalidInput(
                        "expected_revision is required when replacing a scratchpad".into(),
                    )
                })?;
                let current = self.require_revision(project_id, id, Some(expected))?;
                self.ensure_name_available(project_id, &name, Some(id))?;
                let updated = Scratchpad {
                    id,
                    project_id,
                    name,
                    content,
                    revision: current.revision,
                    tags: tags.unwrap_or(current.tags),
                    archived: current.archived,
                    created_by: current.created_by,
                    updated_by: self.actor_label.clone(),
                };
                Ok((self.persist_update(updated, current.revision)?, false))
            }
        }
    }

    pub fn get(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
    ) -> ScratchpadServiceResult<Option<Scratchpad>> {
        let scratchpad = self.store.get_scratchpad(scratchpad_id)?;
        Ok(scratchpad.filter(|scratchpad| scratchpad.project_id == project_id))
    }

    pub fn read(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        mode: ScratchpadReadMode,
        section_heading: Option<&str>,
        offset: usize,
        limit: Option<usize>,
    ) -> ScratchpadServiceResult<ScratchpadRead> {
        let mut scratchpad = self.require_scratchpad(project_id, scratchpad_id)?;
        let selected = match mode {
            ScratchpadReadMode::Full | ScratchpadReadMode::Content => scratchpad.content.clone(),
            ScratchpadReadMode::Headings => headings_outline(&scratchpad),
            ScratchpadReadMode::Section => {
                let heading = section_heading.ok_or_else(|| {
                    ScratchpadServiceError::InvalidInput(
                        "section_heading is required for section reads".into(),
                    )
                })?;
                if heading_matches(heading, &scratchpad.name) {
                    if scratchpad.content.is_empty() {
                        format!("# {}", scratchpad.name)
                    } else {
                        format!("# {}\n\n{}", scratchpad.name, scratchpad.content)
                    }
                } else {
                    let lines = content_lines(&scratchpad.content);
                    let range = section_range(&lines, heading)?;
                    lines[range].join("\n")
                }
            }
        };
        let lines = content_lines(&selected);
        let total_lines = lines.len();
        let offset = offset.min(total_lines);
        let end = limit
            .map(|limit| offset.saturating_add(limit).min(total_lines))
            .unwrap_or(total_lines);
        scratchpad.content = lines[offset..end].join("\n");
        Ok(ScratchpadRead {
            scratchpad,
            total_lines,
            offset,
            returned_lines: end - offset,
            has_more: end < total_lines,
        })
    }

    pub fn comment_create(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        mut input: NewScratchpadComment,
        now_ms: i64,
    ) -> ScratchpadServiceResult<ScratchpadCommentView> {
        validate_comment_body(&input.body)?;
        input.quote = input.quote.map(|quote| normalize_line_endings(&quote));
        input.anchor_prefix = input
            .anchor_prefix
            .map(|prefix| normalize_line_endings(&prefix));
        input.anchor_suffix = input
            .anchor_suffix
            .map(|suffix| normalize_line_endings(&suffix));
        let scratchpad = self.require_scratchpad(project_id, scratchpad_id)?;
        if let Some(expected) = input.expected_revision
            && expected != scratchpad.revision
        {
            return Err(ScratchpadServiceError::RevisionConflict {
                scratchpad_id,
                expected,
                current: scratchpad.revision,
            });
        }
        let (anchor_start, anchor_end, anchor_prefix, anchor_suffix, anchor_revision) = match input
            .quote
            .as_deref()
        {
            None => {
                if input.anchor_start.is_some()
                    || input.anchor_end.is_some()
                    || input.anchor_prefix.is_some()
                    || input.anchor_suffix.is_some()
                {
                    return Err(ScratchpadServiceError::InvalidInput(
                        "anchor fields require a non-empty quote".into(),
                    ));
                }
                (None, None, None, None, None)
            }
            Some("") => {
                return Err(ScratchpadServiceError::InvalidInput(
                    "comment quote must not be empty".into(),
                ));
            }
            Some(quote) => {
                validate_comment_quote(quote)?;
                let range = match (input.anchor_start, input.anchor_end) {
                    (Some(start), Some(end)) => {
                        if utf16_slice(&scratchpad.content, start, end) != Some(quote) {
                            return Err(ScratchpadServiceError::InvalidInput(
                                "anchor offsets do not select the supplied quote".into(),
                            ));
                        }
                        Some((start, end))
                    }
                    (None, None) => {
                        let matches = exact_quote_ranges_utf16(&scratchpad.content, quote);
                        match matches.as_slice() {
                            [range] => Some(*range),
                            [] if input.allow_unanchored => None,
                            [] => {
                                return Err(ScratchpadServiceError::InvalidInput(
                                        "comment quote was not found in the scratchpad; set allow_unanchored=true to keep it as orphaned"
                                            .into(),
                                    ));
                            }
                            _ => {
                                return Err(ScratchpadServiceError::InvalidInput(
                                        "comment quote appears more than once; provide anchor_start and anchor_end"
                                            .into(),
                                    ));
                            }
                        }
                    }
                    _ => {
                        return Err(ScratchpadServiceError::InvalidInput(
                            "anchor_start and anchor_end must be provided together".into(),
                        ));
                    }
                };
                let (prefix, suffix) = match range {
                    Some((start, end)) => (
                        Some(
                            input
                                .anchor_prefix
                                .take()
                                .unwrap_or_else(|| utf16_prefix(&scratchpad.content, start)),
                        ),
                        Some(
                            input
                                .anchor_suffix
                                .take()
                                .unwrap_or_else(|| utf16_suffix(&scratchpad.content, end)),
                        ),
                    ),
                    None => (input.anchor_prefix.take(), input.anchor_suffix.take()),
                };
                if let Some(prefix) = &prefix {
                    validate_anchor_context("anchor_prefix", prefix)?;
                }
                if let Some(suffix) = &suffix {
                    validate_anchor_context("anchor_suffix", suffix)?;
                }
                (
                    range.map(|range| range.0),
                    range.map(|range| range.1),
                    prefix,
                    suffix,
                    Some(scratchpad.revision),
                )
            }
        };

        self.store.connection().execute(
            "INSERT INTO scratchpad_comments (
                scratchpad_id, actor, body, quote, anchor_start, anchor_end,
                anchor_prefix, anchor_suffix, anchor_revision, resolved, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?10)",
            params![
                scratchpad_id,
                self.actor_label,
                input.body,
                input.quote,
                anchor_start,
                anchor_end,
                anchor_prefix,
                anchor_suffix,
                anchor_revision,
                now_ms,
            ],
        )?;
        let id = self.store.connection().last_insert_rowid();
        self.require_comment(project_id, id)
    }

    pub fn comment_list(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        include_resolved: bool,
    ) -> ScratchpadServiceResult<ScratchpadCommentPage> {
        self.comment_list_page(project_id, scratchpad_id, include_resolved, 0, None)
    }

    pub fn comment_list_page(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        include_resolved: bool,
        offset: usize,
        limit: Option<usize>,
    ) -> ScratchpadServiceResult<ScratchpadCommentPage> {
        let scratchpad = self.require_scratchpad(project_id, scratchpad_id)?;
        let (total_count, unresolved_count, comments_revision) =
            self.comment_counts_unchecked(scratchpad_id)?;
        let selected_count = if include_resolved {
            total_count
        } else {
            unresolved_count
        };
        let offset = offset.min(selected_count);
        let limit = limit.map(|value| value.clamp(1, MAX_PAGE_SIZE));
        let mut statement = self.store.connection().prepare(
            "SELECT id, scratchpad_id, actor, body, quote, anchor_start, anchor_end,
                    anchor_prefix, anchor_suffix, anchor_revision, resolved, created_at, updated_at
             FROM scratchpad_comments
             WHERE scratchpad_id = ?1 AND (?2 OR resolved = 0)
             ORDER BY created_at, id
             LIMIT ?3 OFFSET ?4",
        )?;
        let comments = statement
            .query_map(
                params![
                    scratchpad_id,
                    include_resolved,
                    limit.map(|value| value as i64).unwrap_or(-1),
                    offset as i64,
                ],
                comment_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let returned = comments.len();
        let end = offset + returned;
        let has_more = end < selected_count;
        Ok(ScratchpadCommentPage {
            comments: comments
                .into_iter()
                .map(|comment| self.comment_view(comment, &scratchpad))
                .collect(),
            total_count,
            unresolved_count,
            comments_revision,
            offset,
            limit: limit.unwrap_or(returned),
            has_more,
            next_offset: has_more.then_some(end),
        })
    }

    pub fn comment_update(
        &self,
        project_id: ProjectId,
        comment_id: ScratchpadCommentId,
        body: String,
        now_ms: i64,
    ) -> ScratchpadServiceResult<ScratchpadCommentView> {
        validate_comment_body(&body)?;
        let (comment, _) = self.require_comment_record(project_id, comment_id)?;
        self.require_comment_author(&comment, "edited")?;
        self.store.connection().execute(
            "UPDATE scratchpad_comments SET body = ?1, updated_at = ?2 WHERE id = ?3",
            params![body, now_ms, comment_id],
        )?;
        self.require_comment(project_id, comment_id)
    }

    pub fn comment_set_resolved(
        &self,
        project_id: ProjectId,
        comment_id: ScratchpadCommentId,
        resolved: bool,
        now_ms: i64,
    ) -> ScratchpadServiceResult<ScratchpadCommentView> {
        let (comment, _) = self.require_comment_record(project_id, comment_id)?;
        if !self.can_resolve_comment(&comment) {
            return Err(ScratchpadServiceError::CommentPermissionDenied {
                comment_id,
                action: "resolved or reopened",
            });
        }
        self.store.connection().execute(
            "UPDATE scratchpad_comments SET resolved = ?1, updated_at = ?2 WHERE id = ?3",
            params![resolved, now_ms, comment_id],
        )?;
        self.require_comment(project_id, comment_id)
    }

    pub fn comment_delete(
        &self,
        project_id: ProjectId,
        comment_id: ScratchpadCommentId,
    ) -> ScratchpadServiceResult<ScratchpadId> {
        let (comment, _) = self.require_comment_record(project_id, comment_id)?;
        self.require_comment_author(&comment, "deleted")?;
        self.store.connection().execute(
            "DELETE FROM scratchpad_comments WHERE id = ?1",
            [comment_id],
        )?;
        Ok(comment.scratchpad_id)
    }

    pub fn comment_count(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        include_resolved: bool,
    ) -> ScratchpadServiceResult<usize> {
        self.store
            .connection()
            .query_row(
                "SELECT COUNT(comments.id)
                 FROM scratchpads
                 LEFT JOIN scratchpad_comments AS comments
                   ON comments.scratchpad_id = scratchpads.id
                  AND (?3 OR comments.resolved = 0)
                 WHERE scratchpads.id = ?1 AND scratchpads.project_id = ?2
                 GROUP BY scratchpads.id",
                params![scratchpad_id, project_id, include_resolved],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(ScratchpadServiceError::ScratchpadNotFound(scratchpad_id))
    }

    pub fn append(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        content: String,
        expected_revision: Option<i64>,
    ) -> ScratchpadServiceResult<Scratchpad> {
        validate_nonempty("append content", &content)?;
        let mut scratchpad = self.require_revision(project_id, scratchpad_id, expected_revision)?;
        scratchpad.content = append_text(&scratchpad.content, &content);
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn append_section(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        heading: &str,
        content: String,
        expected_revision: Option<i64>,
    ) -> ScratchpadServiceResult<Scratchpad> {
        self.append_section_with_create(
            project_id,
            scratchpad_id,
            heading,
            content,
            false,
            expected_revision,
        )
    }

    pub fn append_section_with_create(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        heading: &str,
        content: String,
        create_heading: bool,
        expected_revision: Option<i64>,
    ) -> ScratchpadServiceResult<Scratchpad> {
        validate_nonempty("append content", &content)?;
        let mut scratchpad = self.require_revision(project_id, scratchpad_id, expected_revision)?;
        if heading_matches(heading, &scratchpad.name) {
            scratchpad.content = append_text(&scratchpad.content, &content);
            let revision = scratchpad.revision;
            return self.persist_update(scratchpad, revision);
        }
        let mut lines = content_lines(&scratchpad.content);
        let range = match section_range(&lines, heading) {
            Ok(range) => range,
            Err(ScratchpadServiceError::HeadingNotFound(_)) if create_heading => {
                scratchpad.content =
                    append_created_section(&scratchpad.content, heading, &content)?;
                let revision = scratchpad.revision;
                return self.persist_update(scratchpad, revision);
            }
            Err(error) => return Err(error),
        };
        let appended = content_lines(&content);
        lines.splice(range.end..range.end, appended);
        scratchpad.content = lines.join("\n");
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn edit(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        target: ScratchpadEditTarget,
        content: String,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<Scratchpad> {
        let mut scratchpad =
            self.require_revision(project_id, scratchpad_id, Some(expected_revision))?;
        if let ScratchpadEditTarget::Section { heading } = &target
            && heading_matches(heading, &scratchpad.name)
        {
            let (name, replacement) = split_leading_h1(scratchpad.name.clone(), content)?;
            self.ensure_name_available(project_id, &name, Some(scratchpad_id))?;
            scratchpad.name = name;
            scratchpad.content = replacement;
            let revision = scratchpad.revision;
            return self.persist_update(scratchpad, revision);
        }
        let mut lines = content_lines(&scratchpad.content);
        match target {
            ScratchpadEditTarget::Section { heading } => {
                let range = section_range(&lines, &heading)?;
                let mut replacement = content_lines(&content);
                let begins_with_heading = replacement
                    .first()
                    .and_then(|line| markdown_heading(line))
                    .is_some();
                if !begins_with_heading {
                    let original_heading = lines[range.start].clone();
                    replacement.insert(0, original_heading);
                }
                lines.splice(range, replacement);
            }
            ScratchpadEditTarget::LineRange { offset, limit } => {
                if offset > lines.len() {
                    return Err(ScratchpadServiceError::InvalidInput(format!(
                        "line offset {offset} exceeds {} content lines",
                        lines.len()
                    )));
                }
                let end = offset.saturating_add(limit).min(lines.len());
                lines.splice(offset..end, content_lines(&content));
            }
        }
        scratchpad.content = lines.join("\n");
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn find(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        query: ScratchpadFindQuery,
    ) -> ScratchpadServiceResult<ScratchpadFindResult> {
        validate_nonempty("search query", &query.query)?;
        let scratchpad = self.require_scratchpad(project_id, scratchpad_id)?;
        let lines = content_lines(&scratchpad.content);
        let headings = parsed_headings(&lines);
        let limit = query
            .limit
            .unwrap_or(DEFAULT_FIND_LIMIT)
            .clamp(1, MAX_FIND_LIMIT);
        let context_lines = query.context_lines.unwrap_or(1).min(MAX_CONTEXT_LINES);
        let needle = (!query.case_sensitive).then(|| query.query.to_lowercase());
        let mut matches = Vec::new();
        let mut total_matches = 0;
        let mut active_heading: Option<ScratchpadHeading> = None;
        for (index, line) in lines.iter().enumerate() {
            let parsed = headings.iter().find(|heading| heading.line == index);
            if let Some(heading) = parsed {
                active_heading = Some(heading.public());
            }
            let is_heading = parsed.is_some();
            if matches!(query.scope, ScratchpadFindScope::Headings) && !is_heading
                || matches!(query.scope, ScratchpadFindScope::Content) && is_heading
            {
                continue;
            }
            let matched = if query.case_sensitive {
                line.contains(&query.query)
            } else {
                line.to_lowercase()
                    .contains(needle.as_deref().unwrap_or_default())
            };
            if !matched {
                continue;
            }
            total_matches += 1;
            if matches.len() == limit {
                continue;
            }
            let context_start = index.saturating_sub(context_lines);
            let context_end = index
                .saturating_add(context_lines)
                .saturating_add(1)
                .min(lines.len());
            let context = (context_start..context_end)
                .filter(|candidate| *candidate != index)
                .map(|candidate| ScratchpadContextLine {
                    line_number: candidate + 1,
                    read_offset: candidate,
                    line: lines[candidate].clone(),
                })
                .collect();
            matches.push(ScratchpadFindMatch {
                kind: if is_heading { "heading" } else { "content" }.into(),
                line_number: index + 1,
                read_offset: index,
                heading: active_heading.clone(),
                line: line.clone(),
                snippet: line.clone(),
                context,
            });
        }
        Ok(ScratchpadFindResult {
            scratchpad_id,
            project_id,
            name: scratchpad.name,
            revision: scratchpad.revision,
            created_by: scratchpad.created_by,
            updated_by: scratchpad.updated_by,
            query: query.query,
            scope: query.scope,
            case_sensitive: query.case_sensitive,
            total_lines: lines.len(),
            total_matches,
            returned_count: matches.len(),
            limit,
            context_lines,
            has_more: total_matches > matches.len(),
            matches,
        })
    }

    pub fn tail(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        requested_lines: Option<usize>,
    ) -> ScratchpadServiceResult<ScratchpadTail> {
        let scratchpad = self.require_scratchpad(project_id, scratchpad_id)?;
        let lines = content_lines(&scratchpad.content);
        let requested_lines = requested_lines.unwrap_or(10);
        let start = lines.len().saturating_sub(requested_lines);
        let selected = if requested_lines == 0 {
            String::new()
        } else {
            lines[start..].join("\n")
        };
        Ok(ScratchpadTail {
            scratchpad_id,
            project_id,
            name: scratchpad.name,
            revision: scratchpad.revision,
            created_by: scratchpad.created_by,
            updated_by: scratchpad.updated_by,
            content: selected,
            total_lines: lines.len(),
            requested_lines,
            returned_lines: lines.len() - start,
            start_offset: start,
            has_more_above: start > 0,
        })
    }

    pub fn list(
        &self,
        project_id: ProjectId,
        query: ScratchpadListQuery,
    ) -> ScratchpadServiceResult<ScratchpadPage> {
        self.require_project(project_id)?;
        let tags = normalize_tags(query.tags)?;
        let needle = query
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_lowercase);
        let mut statement = self.store.connection().prepare(
            "SELECT scratchpads.id, scratchpads.sort_order, scratchpads.comments_revision,
                    COUNT(comments.id)
             FROM scratchpads
             LEFT JOIN scratchpad_comments AS comments
               ON comments.scratchpad_id = scratchpads.id AND comments.resolved = 0
             WHERE scratchpads.project_id = ?1 AND scratchpads.archived = ?2
             GROUP BY scratchpads.id
             ORDER BY scratchpads.sort_order, scratchpads.id",
        )?;
        let rows = statement
            .query_map(params![project_id, query.archived], |row| {
                Ok((
                    row.get::<_, ScratchpadId>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, usize>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut matches = Vec::new();
        for (id, sort_order, comments_revision, unresolved_comment_count) in rows {
            let scratchpad = self.require_scratchpad(project_id, id)?;
            if !tags.is_empty()
                && !scratchpad
                    .tags
                    .iter()
                    .any(|tag| tags.iter().any(|filter| tag.eq_ignore_ascii_case(filter)))
            {
                continue;
            }
            let mut matched_fields = Vec::new();
            let mut match_snippet = None;
            if let Some(needle) = &needle {
                if scratchpad.name.to_lowercase().contains(needle) {
                    matched_fields.push("name".into());
                }
                if scratchpad.content.to_lowercase().contains(needle) {
                    matched_fields.push("content".into());
                    match_snippet = Some(content_snippet(&scratchpad.content, needle));
                }
                if matched_fields.is_empty() {
                    continue;
                }
            }
            matches.push(ScratchpadSummary {
                id: scratchpad.id,
                project_id: scratchpad.project_id,
                name: scratchpad.name,
                revision: scratchpad.revision,
                archived: scratchpad.archived,
                sort_order,
                tags: scratchpad.tags,
                created_by: scratchpad.created_by,
                updated_by: scratchpad.updated_by,
                unresolved_comment_count,
                comments_revision,
                matched_fields,
                match_snippet,
            });
        }
        let total_count = matches.len();
        let limit = query
            .limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        let offset = query.offset.min(total_count);
        let scratchpads = matches
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        let end = offset + scratchpads.len();
        let has_more = end < total_count;
        Ok(ScratchpadPage {
            scratchpads,
            total_count,
            offset,
            limit,
            has_more,
            next_offset: has_more.then_some(end),
        })
    }

    /// Replace the manual order of active scratchpads while preserving archived-row slots.
    pub fn reorder(
        &self,
        project_id: ProjectId,
        ordered_ids: &[ScratchpadId],
    ) -> ScratchpadServiceResult<Vec<ScratchpadSummary>> {
        self.require_project(project_id)?;
        let mut statement = self.store.connection().prepare(
            "SELECT id, sort_order FROM scratchpads
             WHERE project_id = ?1 AND archived = 0
             ORDER BY sort_order, id",
        )?;
        let current = statement
            .query_map([project_id], |row| {
                Ok((row.get::<_, ScratchpadId>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        validate_reorder_ids(
            "scratchpad",
            &current.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            ordered_ids,
        )?;

        let transaction = self.store.connection().unchecked_transaction()?;
        for ((_, sort_order), scratchpad_id) in current.iter().zip(ordered_ids) {
            transaction.execute(
                "UPDATE scratchpads SET sort_order = ?1
                 WHERE id = ?2 AND project_id = ?3 AND archived = 0",
                params![sort_order, scratchpad_id, project_id],
            )?;
        }
        transaction.commit()?;
        Ok(self
            .list(
                project_id,
                ScratchpadListQuery {
                    limit: Some(MAX_PAGE_SIZE),
                    ..ScratchpadListQuery::default()
                },
            )?
            .scratchpads)
    }

    pub fn rename(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        name: String,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<Scratchpad> {
        validate_name(&name)?;
        let mut scratchpad =
            self.require_revision(project_id, scratchpad_id, Some(expected_revision))?;
        let name = normalize_heading_text(&name);
        self.ensure_name_available(project_id, &name, Some(scratchpad_id))?;
        scratchpad.name = name;
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn add_tags(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        tags: Vec<String>,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<Scratchpad> {
        let mut scratchpad =
            self.require_revision(project_id, scratchpad_id, Some(expected_revision))?;
        let additions = normalize_tags(tags)?;
        scratchpad.tags.extend(additions);
        scratchpad.tags = normalize_tags(scratchpad.tags)?;
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn remove_tags(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        tags: Vec<String>,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<Scratchpad> {
        let mut scratchpad =
            self.require_revision(project_id, scratchpad_id, Some(expected_revision))?;
        let removals = normalize_tags(tags)?;
        scratchpad.tags.retain(|tag| !removals.contains(tag));
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn tags_list(&self, project_id: ProjectId) -> ScratchpadServiceResult<Vec<String>> {
        self.require_project(project_id)?;
        let mut statement = self.store.connection().prepare(
            "SELECT DISTINCT lower(tags.tag)
             FROM scratchpad_tags AS tags
             JOIN scratchpads ON scratchpads.id = tags.scratchpad_id
             WHERE scratchpads.project_id = ?1 AND scratchpads.archived = 0
             ORDER BY lower(tags.tag)",
        )?;
        statement
            .query_map([project_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn archive(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        expected_revision: Option<i64>,
    ) -> ScratchpadServiceResult<Scratchpad> {
        let mut scratchpad = self.require_revision(project_id, scratchpad_id, expected_revision)?;
        scratchpad.archived = true;
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn clear(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<Scratchpad> {
        let mut scratchpad =
            self.require_revision(project_id, scratchpad_id, Some(expected_revision))?;
        scratchpad.content.clear();
        let revision = scratchpad.revision;
        self.persist_update(scratchpad, revision)
    }

    pub fn delete(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<()> {
        self.require_revision(project_id, scratchpad_id, Some(expected_revision))?;
        let changed = self.store.connection().execute(
            "DELETE FROM scratchpads WHERE id = ?1 AND project_id = ?2 AND revision = ?3",
            params![scratchpad_id, project_id, expected_revision],
        )?;
        if changed == 0 {
            let current = self.require_scratchpad(project_id, scratchpad_id)?;
            return Err(ScratchpadServiceError::RevisionConflict {
                scratchpad_id,
                expected: expected_revision,
                current: current.revision,
            });
        }
        Ok(())
    }

    pub fn transfer(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        target_project_id: ProjectId,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<Scratchpad> {
        if project_id == target_project_id {
            return Err(ScratchpadServiceError::InvalidInput(
                "target project must differ from the source project".into(),
            ));
        }
        self.require_project(target_project_id)?;
        let scratchpad =
            self.require_revision(project_id, scratchpad_id, Some(expected_revision))?;
        self.ensure_name_available(target_project_id, &scratchpad.name, None)?;
        let changed = self.store.connection().execute(
            "UPDATE scratchpads SET project_id = ?1, revision = revision + 1,
                 updated_by = ?5,
                 sort_order = (SELECT COALESCE(MAX(sort_order), -1) + 1
                               FROM scratchpads WHERE project_id = ?1)
             WHERE id = ?2 AND project_id = ?3 AND revision = ?4",
            params![
                target_project_id,
                scratchpad_id,
                project_id,
                expected_revision,
                self.actor_label,
            ],
        )?;
        if changed == 0 {
            let current = self.require_scratchpad(project_id, scratchpad_id)?;
            return Err(ScratchpadServiceError::RevisionConflict {
                scratchpad_id,
                expected: expected_revision,
                current: current.revision,
            });
        }
        self.require_scratchpad(target_project_id, scratchpad_id)
    }

    pub fn save_to_file(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        path: impl AsRef<Path>,
    ) -> ScratchpadServiceResult<(Scratchpad, PathBuf)> {
        let scratchpad = self.require_scratchpad(project_id, scratchpad_id)?;
        let project = self.require_project(project_id)?;
        let path = resolve_project_file(&project, path.as_ref(), true)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            verify_relative_parent(&project, path.as_path())?;
        }
        let content = if scratchpad.content.is_empty() {
            format!("# {}\n", scratchpad.name)
        } else {
            format!("# {}\n\n{}", scratchpad.name, scratchpad.content)
        };
        fs::write(&path, content)?;
        Ok((scratchpad, path))
    }

    pub fn load_from_file(
        &self,
        project_id: ProjectId,
        scratchpad_id: Option<ScratchpadId>,
        name: String,
        path: impl AsRef<Path>,
        expected_revision: Option<i64>,
    ) -> ScratchpadServiceResult<(Scratchpad, bool, PathBuf)> {
        let project = self.require_project(project_id)?;
        let path = resolve_project_file(&project, path.as_ref(), false)?;
        let content = fs::read_to_string(&path)?;
        let (scratchpad, created) = self.write(
            project_id,
            scratchpad_id,
            name,
            content,
            None,
            expected_revision,
        )?;
        Ok((scratchpad, created, path))
    }

    fn require_project(&self, project_id: ProjectId) -> ScratchpadServiceResult<Project> {
        self.store
            .get_project(project_id)?
            .ok_or(ScratchpadServiceError::ProjectNotFound(project_id))
    }

    fn require_scratchpad(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
    ) -> ScratchpadServiceResult<Scratchpad> {
        self.get(project_id, scratchpad_id)?
            .ok_or(ScratchpadServiceError::ScratchpadNotFound(scratchpad_id))
    }

    fn require_comment_record(
        &self,
        project_id: ProjectId,
        comment_id: ScratchpadCommentId,
    ) -> ScratchpadServiceResult<(ScratchpadComment, Scratchpad)> {
        let comment = self
            .store
            .connection()
            .query_row(
                "SELECT id, scratchpad_id, actor, body, quote, anchor_start, anchor_end,
                        anchor_prefix, anchor_suffix, anchor_revision, resolved, created_at, updated_at
                 FROM scratchpad_comments WHERE id = ?1",
                [comment_id],
                comment_from_row,
            )
            .optional()?
            .ok_or(ScratchpadServiceError::CommentNotFound(comment_id))?;
        let scratchpad = self.require_scratchpad(project_id, comment.scratchpad_id)?;
        Ok((comment, scratchpad))
    }

    fn require_comment(
        &self,
        project_id: ProjectId,
        comment_id: ScratchpadCommentId,
    ) -> ScratchpadServiceResult<ScratchpadCommentView> {
        let (comment, scratchpad) = self.require_comment_record(project_id, comment_id)?;
        Ok(self.comment_view(comment, &scratchpad))
    }

    fn comment_view(
        &self,
        mut comment: ScratchpadComment,
        scratchpad: &Scratchpad,
    ) -> ScratchpadCommentView {
        let is_author = comment.actor == self.actor_label;
        let can_resolve = self.is_user_actor() || is_author;
        let actor_kind = if is_user_actor_id(&comment.actor) {
            ScratchpadCommentActorKind::User
        } else {
            ScratchpadCommentActorKind::Agent
        };
        let anchor = comment_anchor_resolution(&comment, scratchpad);
        comment.actor = self.store.actor_display_label(&comment.actor);
        ScratchpadCommentView {
            comment,
            anchor,
            actor_kind,
            can_edit: is_author,
            can_resolve,
            can_delete: is_author,
        }
    }

    fn is_user_actor(&self) -> bool {
        is_user_actor_id(&self.actor_label)
    }

    fn can_resolve_comment(&self, comment: &ScratchpadComment) -> bool {
        self.is_user_actor() || comment.actor == self.actor_label
    }

    fn require_comment_author(
        &self,
        comment: &ScratchpadComment,
        action: &'static str,
    ) -> ScratchpadServiceResult<()> {
        if comment.actor == self.actor_label {
            return Ok(());
        }
        Err(ScratchpadServiceError::CommentPermissionDenied {
            comment_id: comment.id,
            action,
        })
    }

    fn comment_counts_unchecked(
        &self,
        scratchpad_id: ScratchpadId,
    ) -> ScratchpadServiceResult<(usize, usize, i64)> {
        self.store
            .connection()
            .query_row(
                "SELECT COUNT(comments.id),
                        COALESCE(SUM(CASE WHEN comments.resolved = 0 THEN 1 ELSE 0 END), 0),
                        scratchpads.comments_revision
                 FROM scratchpads
                 LEFT JOIN scratchpad_comments AS comments
                   ON comments.scratchpad_id = scratchpads.id
                 WHERE scratchpads.id = ?1
                 GROUP BY scratchpads.id",
                [scratchpad_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(Into::into)
    }

    fn require_revision(
        &self,
        project_id: ProjectId,
        scratchpad_id: ScratchpadId,
        expected_revision: Option<i64>,
    ) -> ScratchpadServiceResult<Scratchpad> {
        let scratchpad = self.require_scratchpad(project_id, scratchpad_id)?;
        if let Some(expected) = expected_revision
            && expected != scratchpad.revision
        {
            return Err(ScratchpadServiceError::RevisionConflict {
                scratchpad_id,
                expected,
                current: scratchpad.revision,
            });
        }
        Ok(scratchpad)
    }

    fn ensure_name_available(
        &self,
        project_id: ProjectId,
        name: &str,
        excluding_id: Option<ScratchpadId>,
    ) -> ScratchpadServiceResult<()> {
        let conflicting_id = self
            .store
            .connection()
            .query_row(
                "SELECT id FROM scratchpads
                 WHERE project_id = ?1 AND lower(name) = lower(?2)
                   AND (?3 IS NULL OR id != ?3)",
                params![project_id, name, excluding_id],
                |row| row.get::<_, ScratchpadId>(0),
            )
            .optional()?;
        if conflicting_id.is_some() {
            return Err(ScratchpadServiceError::NameConflict {
                project_id,
                name: name.to_owned(),
            });
        }
        Ok(())
    }

    fn persist_update(
        &self,
        mut scratchpad: Scratchpad,
        expected_revision: i64,
    ) -> ScratchpadServiceResult<Scratchpad> {
        scratchpad.content = normalize_line_endings(&scratchpad.content);
        let transaction = self.store.connection().unchecked_transaction()?;
        let changed = transaction.execute(
            "UPDATE scratchpads SET name = ?1, content = ?2, revision = revision + 1,
                 archived = ?3, updated_by = ?7
             WHERE id = ?4 AND project_id = ?5 AND revision = ?6",
            params![
                scratchpad.name,
                scratchpad.content,
                scratchpad.archived,
                scratchpad.id,
                scratchpad.project_id,
                expected_revision,
                self.actor_label,
            ],
        )?;
        if changed == 0 {
            drop(transaction);
            let current = self.require_scratchpad(scratchpad.project_id, scratchpad.id)?;
            return Err(ScratchpadServiceError::RevisionConflict {
                scratchpad_id: scratchpad.id,
                expected: expected_revision,
                current: current.revision,
            });
        }
        replace_tags(&transaction, scratchpad.id, &scratchpad.tags)?;
        transaction.commit()?;
        self.require_scratchpad(scratchpad.project_id, scratchpad.id)
    }
}

/// Resolve a saved scratchpad quote against current content.
///
/// Offsets are UTF-16 code units. The saved range wins while it still selects
/// the quote. Otherwise an exact quote match must be unique, optionally after
/// exact prefix/suffix context filtering; ambiguous and missing quotes orphan.
pub fn resolve_scratchpad_anchor(
    content: &str,
    quote: Option<&str>,
    anchor_start: Option<usize>,
    anchor_end: Option<usize>,
    anchor_prefix: Option<&str>,
    anchor_suffix: Option<&str>,
) -> ScratchpadAnchorResolution {
    let Some(quote) = quote else {
        return ScratchpadAnchorResolution {
            anchor_state: ScratchpadAnchorState::Unanchored,
            current_start: None,
            current_end: None,
            current_start_line: None,
            current_end_line: None,
        };
    };
    let content = normalize_line_endings(content);
    let quote = normalize_line_endings(quote);
    let anchor_prefix = anchor_prefix.map(normalize_line_endings);
    let anchor_suffix = anchor_suffix.map(normalize_line_endings);
    let content = content.as_str();
    let quote = quote.as_str();
    let anchor_prefix = anchor_prefix.as_deref();
    let anchor_suffix = anchor_suffix.as_deref();
    if let (Some(start), Some(end)) = (anchor_start, anchor_end)
        && utf16_slice(content, start, end) == Some(quote)
    {
        return anchored_resolution(content, quote, start, end);
    }

    let candidates = exact_quote_ranges_utf16(content, quote);
    let has_context = anchor_prefix.is_some_and(|value| !value.is_empty())
        || anchor_suffix.is_some_and(|value| !value.is_empty());
    let contextual = candidates
        .iter()
        .copied()
        .filter(|(start, end)| {
            let Some(start_byte) = utf16_to_byte(content, *start) else {
                return false;
            };
            let Some(end_byte) = utf16_to_byte(content, *end) else {
                return false;
            };
            anchor_prefix
                .filter(|prefix| !prefix.is_empty())
                .is_none_or(|prefix| content[..start_byte].ends_with(prefix))
                && anchor_suffix
                    .filter(|suffix| !suffix.is_empty())
                    .is_none_or(|suffix| content[end_byte..].starts_with(suffix))
        })
        .collect::<Vec<_>>();
    let resolved = if has_context {
        match contextual.as_slice() {
            [range] => Some(*range),
            _ => None,
        }
    } else {
        match candidates.as_slice() {
            [range] => Some(*range),
            _ => None,
        }
    };
    match resolved {
        Some((start, end)) => anchored_resolution(content, quote, start, end),
        None => ScratchpadAnchorResolution {
            anchor_state: ScratchpadAnchorState::Orphaned,
            current_start: None,
            current_end: None,
            current_start_line: None,
            current_end_line: None,
        },
    }
}

fn anchored_resolution(
    content: &str,
    quote: &str,
    start: usize,
    end: usize,
) -> ScratchpadAnchorResolution {
    let start_line = utf16_to_byte(content, start).map(|byte| {
        content[..byte]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1
    });
    ScratchpadAnchorResolution {
        anchor_state: ScratchpadAnchorState::Anchored,
        current_start: Some(start),
        current_end: Some(end),
        current_start_line: start_line,
        current_end_line: start_line
            .map(|line| line + quote.bytes().filter(|byte| *byte == b'\n').count()),
    }
}

fn comment_anchor_resolution(
    comment: &ScratchpadComment,
    scratchpad: &Scratchpad,
) -> ScratchpadAnchorResolution {
    if comment.anchor_revision == Some(scratchpad.revision) {
        return match (
            comment.quote.as_deref(),
            comment.anchor_start,
            comment.anchor_end,
        ) {
            (None, _, _) => {
                resolve_scratchpad_anchor(&scratchpad.content, None, None, None, None, None)
            }
            (Some(quote), Some(start), Some(end))
                if utf16_slice(&scratchpad.content, start, end) == Some(quote) =>
            {
                anchored_resolution(&scratchpad.content, quote, start, end)
            }
            _ => ScratchpadAnchorResolution {
                anchor_state: ScratchpadAnchorState::Orphaned,
                current_start: None,
                current_end: None,
                current_start_line: None,
                current_end_line: None,
            },
        };
    }
    resolve_scratchpad_anchor(
        &scratchpad.content,
        comment.quote.as_deref(),
        comment.anchor_start,
        comment.anchor_end,
        comment.anchor_prefix.as_deref(),
        comment.anchor_suffix.as_deref(),
    )
}

fn comment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScratchpadComment> {
    Ok(ScratchpadComment {
        id: row.get(0)?,
        scratchpad_id: row.get(1)?,
        actor: row.get(2)?,
        body: row.get(3)?,
        quote: row.get(4)?,
        anchor_start: row.get(5)?,
        anchor_end: row.get(6)?,
        anchor_prefix: row.get(7)?,
        anchor_suffix: row.get(8)?,
        anchor_revision: row.get(9)?,
        resolved: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn validate_comment_body(body: &str) -> ScratchpadServiceResult<()> {
    validate_nonempty("comment", body)?;
    if body.chars().count() > MAX_COMMENT_BODY_CHARS {
        return Err(ScratchpadServiceError::InvalidInput(format!(
            "comment must not exceed {MAX_COMMENT_BODY_CHARS} characters"
        )));
    }
    Ok(())
}

fn validate_comment_quote(quote: &str) -> ScratchpadServiceResult<()> {
    if quote.chars().count() > MAX_COMMENT_QUOTE_CHARS {
        return Err(ScratchpadServiceError::InvalidInput(format!(
            "comment quote must not exceed {MAX_COMMENT_QUOTE_CHARS} characters"
        )));
    }
    Ok(())
}

fn validate_anchor_context(field: &str, context: &str) -> ScratchpadServiceResult<()> {
    if context.chars().count() > MAX_ANCHOR_CONTEXT_CHARS {
        return Err(ScratchpadServiceError::InvalidInput(format!(
            "{field} must not exceed {MAX_ANCHOR_CONTEXT_CHARS} characters"
        )));
    }
    Ok(())
}

fn exact_quote_ranges_utf16(content: &str, quote: &str) -> Vec<(usize, usize)> {
    content
        .match_indices(quote)
        .map(|(start, matched)| {
            let start_utf16 = content[..start].encode_utf16().count();
            (start_utf16, start_utf16 + matched.encode_utf16().count())
        })
        .collect()
}

fn is_user_actor_id(actor: &str) -> bool {
    matches!(actor, "user" | "desktop-ui" | "workman")
}

fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

fn utf16_to_byte(content: &str, offset: usize) -> Option<usize> {
    let mut utf16 = 0;
    for (byte, character) in content.char_indices() {
        if utf16 == offset {
            return Some(byte);
        }
        utf16 += character.len_utf16();
        if utf16 > offset {
            return None;
        }
    }
    (utf16 == offset).then_some(content.len())
}

fn utf16_slice(content: &str, start: usize, end: usize) -> Option<&str> {
    if end <= start {
        return None;
    }
    let start = utf16_to_byte(content, start)?;
    let end = utf16_to_byte(content, end)?;
    content.get(start..end)
}

fn utf16_prefix(content: &str, start: usize) -> String {
    let Some(start) = utf16_to_byte(content, start) else {
        return String::new();
    };
    let chars = content[..start].chars().collect::<Vec<_>>();
    chars[chars.len().saturating_sub(MAX_ANCHOR_CONTEXT_CHARS)..]
        .iter()
        .collect()
}

fn utf16_suffix(content: &str, end: usize) -> String {
    let Some(end) = utf16_to_byte(content, end) else {
        return String::new();
    };
    content[end..]
        .chars()
        .take(MAX_ANCHOR_CONTEXT_CHARS)
        .collect()
}

#[derive(Debug, Clone)]
struct ParsedHeading {
    line: usize,
    level: usize,
    text: String,
}

impl ParsedHeading {
    fn public(&self) -> ScratchpadHeading {
        ScratchpadHeading {
            level: self.level,
            text: self.text.clone(),
            line_number: self.line + 1,
            read_offset: self.line,
        }
    }
}

fn replace_tags(
    connection: &Connection,
    scratchpad_id: ScratchpadId,
    tags: &[String],
) -> rusqlite::Result<()> {
    connection.execute(
        "DELETE FROM scratchpad_tags WHERE scratchpad_id = ?1",
        [scratchpad_id],
    )?;
    for (position, tag) in tags.iter().enumerate() {
        connection.execute(
            "INSERT INTO scratchpad_tags (scratchpad_id, tag, position) VALUES (?1, ?2, ?3)",
            params![scratchpad_id, tag, position],
        )?;
    }
    Ok(())
}

fn validate_nonempty(field: &str, value: &str) -> ScratchpadServiceResult<()> {
    if value.trim().is_empty() {
        return Err(ScratchpadServiceError::InvalidInput(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn validate_name(name: &str) -> ScratchpadServiceResult<()> {
    validate_nonempty("scratchpad name", name)
}

fn normalize_tags(tags: Vec<String>) -> ScratchpadServiceResult<Vec<String>> {
    let mut normalized = Vec::with_capacity(tags.len());
    let mut seen = HashSet::new();
    for tag in tags {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() {
            return Err(ScratchpadServiceError::InvalidInput(
                "scratchpad tags must not be empty".into(),
            ));
        }
        if seen.insert(tag.clone()) {
            normalized.push(tag);
        }
    }
    normalized.sort();
    Ok(normalized)
}

fn split_leading_h1(
    fallback_name: String,
    content: String,
) -> ScratchpadServiceResult<(String, String)> {
    let mut split = content.splitn(2, '\n');
    let first = split.next().unwrap_or_default();
    let (name, content) = match markdown_heading(first) {
        Some((1, heading)) => {
            let remaining = split.next().unwrap_or_default();
            let remaining = remaining
                .strip_prefix("\r\n")
                .or_else(|| remaining.strip_prefix('\n'))
                .unwrap_or(remaining);
            (heading, remaining.into())
        }
        _ => (normalize_heading_text(&fallback_name), content),
    };
    validate_name(&name)?;
    Ok((name, content))
}

fn append_text(existing: &str, addition: &str) -> String {
    if existing.is_empty() {
        addition.to_owned()
    } else if existing.ends_with('\n') {
        format!("{existing}{addition}")
    } else {
        format!("{existing}\n{addition}")
    }
}

fn append_created_section(
    existing: &str,
    heading: &str,
    content: &str,
) -> ScratchpadServiceResult<String> {
    let normalized = normalize_heading_text(heading);
    validate_nonempty("section heading", &normalized)?;
    let level = markdown_heading(heading)
        .map(|(level, _)| level.max(2))
        .unwrap_or(2);
    let separator = if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!(
        "{existing}{separator}{} {normalized}\n\n{content}",
        "#".repeat(level)
    ))
}

fn content_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        Vec::new()
    } else {
        content.split('\n').map(str::to_owned).collect()
    }
}

fn markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let remainder = &trimmed[level..];
    if !remainder.is_empty() && !remainder.starts_with(char::is_whitespace) {
        return None;
    }
    let text = remainder
        .trim()
        .trim_end_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some((level, text))
}

fn normalize_heading_text(value: &str) -> String {
    let value = value.trim();
    let value = markdown_heading(value)
        .map(|(_, heading)| heading)
        .unwrap_or_else(|| value.to_owned());
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalized_heading_key(value: &str) -> String {
    normalize_heading_text(value).to_lowercase()
}

fn heading_matches(left: &str, right: &str) -> bool {
    normalized_heading_key(left) == normalized_heading_key(right)
}

fn parsed_headings(lines: &[String]) -> Vec<ParsedHeading> {
    let mut headings = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (line, text) in lines.iter().enumerate() {
        let trimmed = text.trim_start();
        if let Some((marker, width)) = fence {
            if trimmed
                .chars()
                .take_while(|candidate| *candidate == marker)
                .count()
                >= width
            {
                fence = None;
            }
            continue;
        }
        let marker = trimmed.chars().next();
        if matches!(marker, Some('`' | '~')) {
            let marker = marker.unwrap_or('`');
            let width = trimmed
                .chars()
                .take_while(|candidate| *candidate == marker)
                .count();
            if width >= 3 {
                fence = Some((marker, width));
                continue;
            }
        }
        if let Some((level, text)) = markdown_heading(text) {
            headings.push(ParsedHeading { line, level, text });
        }
    }
    headings
}

fn section_range(
    lines: &[String],
    requested_heading: &str,
) -> ScratchpadServiceResult<std::ops::Range<usize>> {
    let requested = normalized_heading_key(requested_heading);
    let headings = parsed_headings(lines);
    let Some((position, heading)) = headings
        .iter()
        .enumerate()
        .find(|(_, heading)| normalized_heading_key(&heading.text) == requested)
    else {
        return Err(ScratchpadServiceError::HeadingNotFound(
            requested_heading.to_owned(),
        ));
    };
    let end = headings[position + 1..]
        .iter()
        .find(|candidate| candidate.level <= heading.level)
        .map(|candidate| candidate.line)
        .unwrap_or(lines.len());
    Ok(heading.line..end)
}

fn headings_outline(scratchpad: &Scratchpad) -> String {
    let lines = content_lines(&scratchpad.content);
    let mut outline = vec![format!("# {}", scratchpad.name)];
    outline.extend(
        parsed_headings(&lines)
            .into_iter()
            .map(|heading| format!("{} {}", "#".repeat(heading.level), heading.text)),
    );
    outline.join("\n")
}

fn content_snippet(content: &str, needle: &str) -> String {
    let source = content
        .split('\n')
        .find(|line| line.to_lowercase().contains(needle))
        .unwrap_or(content);
    let compact = source.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut snippet = compact.chars().take(MAX_SNIPPET_CHARS).collect::<String>();
    if compact.chars().count() > MAX_SNIPPET_CHARS {
        snippet.push('…');
    }
    snippet
}

fn validate_reorder_ids(
    label: &str,
    current_ids: &[i64],
    ordered_ids: &[i64],
) -> ScratchpadServiceResult<()> {
    let unique = ordered_ids.iter().copied().collect::<HashSet<_>>();
    let current = current_ids.iter().copied().collect::<HashSet<_>>();
    if ordered_ids.len() != current_ids.len()
        || unique.len() != ordered_ids.len()
        || unique != current
    {
        return Err(StoreError::InvalidReorder(format!(
            "{label} reorder must contain every scoped ID exactly once"
        ))
        .into());
    }
    Ok(())
}

fn resolve_project_file(
    project: &Project,
    path: &Path,
    for_write: bool,
) -> ScratchpadServiceResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(ScratchpadServiceError::InvalidInput(
            "file path must not be empty".into(),
        ));
    }
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ScratchpadServiceError::PathEscapesProject(
            path.to_path_buf(),
        ));
    }
    let root = crate::canonical_path(&project.path)?;
    let candidate = root.join(path);
    if for_write {
        let mut existing = candidate.parent().unwrap_or(&root);
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| ScratchpadServiceError::PathEscapesProject(path.to_path_buf()))?;
        }
        let canonical_existing = crate::canonical_path(existing)?;
        if !canonical_existing.starts_with(&root) {
            return Err(ScratchpadServiceError::PathEscapesProject(
                path.to_path_buf(),
            ));
        }
        if fs::symlink_metadata(&candidate).is_ok() {
            let canonical_candidate = crate::canonical_path(&candidate)
                .map_err(|_| ScratchpadServiceError::PathEscapesProject(path.to_path_buf()))?;
            if !canonical_candidate.starts_with(&root) {
                return Err(ScratchpadServiceError::PathEscapesProject(
                    path.to_path_buf(),
                ));
            }
        }
        Ok(candidate)
    } else {
        let canonical = crate::canonical_path(&candidate)?;
        if !canonical.starts_with(&root) {
            return Err(ScratchpadServiceError::PathEscapesProject(
                path.to_path_buf(),
            ));
        }
        Ok(canonical)
    }
}

fn verify_relative_parent(project: &Project, path: &Path) -> ScratchpadServiceResult<()> {
    let root = crate::canonical_path(&project.path)?;
    if !path.is_absolute() {
        return Ok(());
    }
    let project_path = Path::new(&project.path);
    if !path.starts_with(project_path) && !path.starts_with(&root) {
        return Ok(());
    }
    let parent = path.parent().unwrap_or(&root);
    if !crate::canonical_path(parent)?.starts_with(root) {
        return Err(ScratchpadServiceError::PathEscapesProject(
            path.to_path_buf(),
        ));
    }
    Ok(())
}
