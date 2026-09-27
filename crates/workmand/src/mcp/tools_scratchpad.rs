//! MCP scratchpad tools backed by [`workman_core::ScratchpadService`].

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use workman_core::{
    NewScratchpadComment, ProjectId, ScratchpadCommentId, ScratchpadEditTarget,
    ScratchpadFindQuery, ScratchpadFindScope, ScratchpadId, ScratchpadListQuery,
    ScratchpadReadMode, ScratchpadService, ScratchpadServiceError,
};

use super::{WorkmanMcp, failure, now_millis, scoped_project, success};

#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ReadMode {
    #[default]
    Full,
    Content,
    Headings,
    Section,
    #[serde(alias = "lines")]
    LineSlice,
    Tail,
}

impl From<ReadMode> for ScratchpadReadMode {
    fn from(mode: ReadMode) -> Self {
        match mode {
            ReadMode::Full => Self::Full,
            ReadMode::Content => Self::Content,
            ReadMode::Headings => Self::Headings,
            ReadMode::Section => Self::Section,
            ReadMode::LineSlice => Self::Content,
            ReadMode::Tail => Self::Content,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum FindScope {
    #[default]
    All,
    Headings,
    Content,
}

impl From<FindScope> for ScratchpadFindScope {
    fn from(scope: FindScope) -> Self {
        match scope {
            FindScope::All => Self::All,
            FindScope::Headings => Self::Headings,
            FindScope::Content => Self::Content,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EditTarget {
    Section { section_heading: String },
    LineRange { offset: usize, limit: usize },
}

impl From<EditTarget> for ScratchpadEditTarget {
    fn from(target: EditTarget) -> Self {
        match target {
            EditTarget::Section { section_heading } => Self::Section {
                heading: section_heading,
            },
            EditTarget::LineRange { offset, limit } => Self::LineRange { offset, limit },
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadWriteArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    scratchpad_id: Option<ScratchpadId>,
    name: String,
    content: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadReadArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    #[serde(default)]
    mode: ReadMode,
    #[serde(default)]
    section_heading: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    /// Lines to return when mode=tail. Defaults to 10.
    #[serde(default)]
    lines: Option<usize>,
    /// Include unresolved anchored, orphaned, and whole-document comments.
    #[serde(default)]
    include_comments: bool,
    /// Include resolved comments when include_comments=true.
    #[serde(default)]
    include_resolved: bool,
    #[serde(default)]
    comments_offset: Option<usize>,
    #[serde(default)]
    comments_limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadCommentCreateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    body: String,
    #[serde(default)]
    quote: Option<String>,
    /// UTF-16 code-unit offset into the current scratchpad content.
    #[serde(default)]
    anchor_start: Option<usize>,
    /// Exclusive UTF-16 code-unit offset into the current scratchpad content.
    #[serde(default)]
    anchor_end: Option<usize>,
    /// Up to 64 characters immediately before the quote, used to re-anchor it.
    #[serde(default)]
    anchor_prefix: Option<String>,
    /// Up to 64 characters immediately after the quote, used to re-anchor it.
    #[serde(default)]
    anchor_suffix: Option<String>,
    /// Keep a missing quote as an orphaned comment instead of returning an error.
    #[serde(default)]
    allow_unanchored: bool,
    /// Scratchpad revision the anchor was selected from; stale revisions are rejected.
    #[serde(default)]
    expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadCommentUpdateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    comment_id: ScratchpadCommentId,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    resolved: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadCommentDeleteArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    comment_id: ScratchpadCommentId,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadAppendArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    content: String,
    /// Append beneath this heading instead of at the document end.
    #[serde(default)]
    heading: Option<String>,
    /// Create a missing heading when heading is set.
    #[serde(default)]
    create_heading: bool,
    #[serde(default)]
    expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadEditArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    expected_revision: i64,
    target: EditTarget,
    content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadFindArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    query: String,
    #[serde(default)]
    scope: FindScope,
    #[serde(default)]
    case_sensitive: bool,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    context_lines: Option<usize>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ScratchpadListArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    /// Include distinct active scratchpad tags beside the page.
    #[serde(default)]
    include_tags: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadUpdateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    expected_revision: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    add_tags: Option<Vec<String>>,
    #[serde(default)]
    remove_tags: Option<Vec<String>>,
    /// Set true to archive the scratchpad.
    #[serde(default)]
    archived: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadRevisionArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    expected_revision: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadFileArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadLoadArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    scratchpad_id: Option<ScratchpadId>,
    name: String,
    path: String,
    #[serde(default)]
    expected_revision: Option<i64>,
}

#[tool_router(router = scratchpad_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(description = "Create or replace scratchpad content and tags at an expected revision")]
    async fn scratchpad_write(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadWriteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = registry.store().actor_display_label(&actor.id);
        match ScratchpadService::attributed(registry.store(), actor_label).write(
            project.id,
            args.scratchpad_id,
            args.name,
            args.content,
            args.tags,
            args.expected_revision,
        ) {
            Ok((scratchpad, created)) => success(json!({
                "created": created,
                "project_id": scratchpad.project_id,
                "scratchpad_id": scratchpad.id,
                "revision": scratchpad.revision,
                "name": scratchpad.name,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(
        description = "Read scratchpad content, outline, section, line slice, tail, or comments"
    )]
    async fn scratchpad_read(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadReadArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = ScratchpadService::attributed(registry.store(), actor.id);
        let mut response = match args.mode {
            ReadMode::Tail => match service.tail(project.id, args.scratchpad_id, args.lines) {
                Ok(tail) => serde_json::to_value(tail).expect("scratchpad tail serializes"),
                Err(error) => return scratchpad_failure(error),
            },
            mode => match service.read(
                project.id,
                args.scratchpad_id,
                mode.into(),
                args.section_heading.as_deref(),
                args.offset.unwrap_or(0),
                args.limit,
            ) {
                Ok(read) => json!({
                    "found": true,
                    "scratchpad": read.scratchpad,
                    "total_lines": read.total_lines,
                    "offset": read.offset,
                    "returned_lines": read.returned_lines,
                    "has_more": read.has_more,
                }),
                Err(error) => return scratchpad_failure(error),
            },
        };
        if args.include_comments {
            let comments = match service.comment_list_page(
                project.id,
                args.scratchpad_id,
                args.include_resolved,
                args.comments_offset.unwrap_or(0),
                args.comments_limit,
            ) {
                Ok(comments) => comments,
                Err(error) => return scratchpad_failure(error),
            };
            response["comments"] = json!(comments.comments);
            response["comment_total_count"] = json!(comments.total_count);
            response["unresolved_comment_count"] = json!(comments.unresolved_count);
            response["comments_revision"] = json!(comments.comments_revision);
            response["comments_offset"] = json!(comments.offset);
            response["comments_limit"] = json!(comments.limit);
            response["comments_has_more"] = json!(comments.has_more);
            response["comments_next_offset"] = json!(comments.next_offset);
        }
        success(response)
    }

    #[tool(description = "Create an owned scratchpad comment with an optional text anchor")]
    async fn scratchpad_comment_create(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadCommentCreateArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::attributed(registry.store(), actor.id).comment_create(
            project.id,
            args.scratchpad_id,
            NewScratchpadComment {
                body: args.body,
                quote: args.quote,
                anchor_start: args.anchor_start,
                anchor_end: args.anchor_end,
                anchor_prefix: args.anchor_prefix,
                anchor_suffix: args.anchor_suffix,
                allow_unanchored: args.allow_unanchored,
                expected_revision: args.expected_revision,
            },
            now_millis(),
        ) {
            Ok(comment) => success(comment),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Update or resolve a scratchpad comment authored by the calling agent")]
    async fn scratchpad_comment_update(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadCommentUpdateArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        if args.body.is_none() && args.resolved.is_none() {
            return failure("invalid_params", "set body, resolved, or both");
        }
        let service = ScratchpadService::attributed(registry.store(), actor.id);
        let mut comment = if let Some(body) = args.body {
            match service.comment_update(project.id, args.comment_id, body, now_millis()) {
                Ok(comment) => Some(comment),
                Err(error) => return scratchpad_failure(error),
            }
        } else {
            None
        };
        if let Some(resolved) = args.resolved {
            comment = match service.comment_set_resolved(
                project.id,
                args.comment_id,
                resolved,
                now_millis(),
            ) {
                Ok(comment) => Some(comment),
                Err(error) => return scratchpad_failure(error),
            };
        }
        success(comment.expect("one comment mutation ran"))
    }

    #[tool(description = "Delete a scratchpad comment authored by the calling agent")]
    async fn scratchpad_comment_delete(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadCommentDeleteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::attributed(registry.store(), actor.id)
            .comment_delete(project.id, args.comment_id)
        {
            Ok(scratchpad_id) => success(json!({
                "project_id": project.id,
                "scratchpad_id": scratchpad_id,
                "comment_id": args.comment_id,
                "deleted": true,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Append content at the end or beneath a markdown heading")]
    async fn scratchpad_append(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadAppendArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = registry.store().actor_display_label(&actor.id);
        let service = ScratchpadService::attributed(registry.store(), actor_label);
        let result = if let Some(heading) = args.heading {
            service.append_section_with_create(
                project.id,
                args.scratchpad_id,
                &heading,
                args.content,
                args.create_heading,
                args.expected_revision,
            )
        } else {
            service.append(
                project.id,
                args.scratchpad_id,
                args.content,
                args.expected_revision,
            )
        };
        match result {
            Ok(scratchpad) => revision_receipt(&scratchpad),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Replace one markdown section or zero-based line range")]
    async fn scratchpad_edit(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadEditArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = registry.store().actor_display_label(&actor.id);
        match ScratchpadService::attributed(registry.store(), actor_label).edit(
            project.id,
            args.scratchpad_id,
            args.target.into(),
            args.content,
            args.expected_revision,
        ) {
            Ok(scratchpad) => revision_receipt(&scratchpad),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Search one scratchpad for bounded literal matches with context lines")]
    async fn scratchpad_find(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadFindArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).find(
            project.id,
            args.scratchpad_id,
            ScratchpadFindQuery {
                query: args.query,
                scope: args.scope.into(),
                case_sensitive: args.case_sensitive,
                limit: args.limit,
                context_lines: args.context_lines,
            },
        ) {
            Ok(result) => success(result),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(
        description = "List scratchpad metadata with query/tag filters, matched fields, and snippets"
    )]
    async fn scratchpad_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadListArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = ScratchpadService::new(registry.store());
        let page = match service.list(
            project.id,
            ScratchpadListQuery {
                query: args.query,
                tags: args.tags.unwrap_or_default(),
                archived: false,
                offset: args.offset.unwrap_or(0),
                limit: args.limit,
            },
        ) {
            Ok(page) => page,
            Err(error) => return scratchpad_failure(error),
        };
        if args.include_tags {
            match service.tags_list(project.id) {
                Ok(tags) => {
                    let mut result =
                        serde_json::to_value(page).expect("scratchpad page serializes");
                    result
                        .as_object_mut()
                        .expect("scratchpad page is an object")
                        .insert("tags".into(), json!(tags));
                    success(result)
                }
                Err(error) => scratchpad_failure(error),
            }
        } else {
            success(page)
        }
    }

    #[tool(description = "Update scratchpad name, tags, or archive state at an expected revision")]
    async fn scratchpad_update(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadUpdateArgs>,
    ) -> CallToolResult {
        if args.name.is_none()
            && args.add_tags.is_none()
            && args.remove_tags.is_none()
            && args.archived.is_none()
        {
            return failure(
                "invalid_params",
                "set name, add_tags, remove_tags, or archived",
            );
        }
        if args.archived == Some(false) {
            return failure("invalid_params", "archived currently accepts only true");
        }
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = registry.store().actor_display_label(&actor.id);
        let service = ScratchpadService::attributed(registry.store(), actor_label);
        let mut revision = args.expected_revision;
        let mut scratchpad = None;
        if let Some(name) = args.name {
            scratchpad = match service.rename(project.id, args.scratchpad_id, name, revision) {
                Ok(value) => {
                    revision = value.revision;
                    Some(value)
                }
                Err(error) => return scratchpad_failure(error),
            };
        }
        if let Some(tags) = args.add_tags {
            scratchpad = match service.add_tags(project.id, args.scratchpad_id, tags, revision) {
                Ok(value) => {
                    revision = value.revision;
                    Some(value)
                }
                Err(error) => return scratchpad_failure(error),
            };
        }
        if let Some(tags) = args.remove_tags {
            scratchpad = match service.remove_tags(project.id, args.scratchpad_id, tags, revision) {
                Ok(value) => {
                    revision = value.revision;
                    Some(value)
                }
                Err(error) => return scratchpad_failure(error),
            };
        }
        if args.archived == Some(true) {
            scratchpad = match service.archive(project.id, args.scratchpad_id, Some(revision)) {
                Ok(value) => Some(value),
                Err(error) => return scratchpad_failure(error),
            };
        }
        success(scratchpad.expect("one scratchpad mutation ran"))
    }

    #[tool(description = "Delete a scratchpad at an expected revision")]
    async fn scratchpad_delete(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadRevisionArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).delete(
            project.id,
            args.scratchpad_id,
            args.expected_revision,
        ) {
            Ok(()) => success(json!({
                "project_id": project.id,
                "scratchpad_id": args.scratchpad_id,
                "deleted": true,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Save a scratchpad as UTF-8 markdown with a leading H1")]
    async fn scratchpad_save_to_file(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadFileArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).save_to_file(
            project.id,
            args.scratchpad_id,
            &args.path,
        ) {
            Ok((scratchpad, path)) => success(json!({
                "project_id": scratchpad.project_id,
                "scratchpad_id": scratchpad.id,
                "revision": scratchpad.revision,
                "path": path,
                "saved": true,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Load a scratchpad from a project-relative UTF-8 file")]
    async fn scratchpad_load_from_file(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadLoadArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = registry.store().actor_display_label(&actor.id);
        match ScratchpadService::attributed(registry.store(), actor_label).load_from_file(
            project.id,
            args.scratchpad_id,
            args.name,
            &args.path,
            args.expected_revision,
        ) {
            Ok((scratchpad, created, path)) => success(json!({
                "created": created,
                "project_id": scratchpad.project_id,
                "scratchpad_id": scratchpad.id,
                "revision": scratchpad.revision,
                "name": scratchpad.name,
                "path": path,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }
}

fn revision_receipt(scratchpad: &workman_core::Scratchpad) -> CallToolResult {
    success(json!({
        "project_id": scratchpad.project_id,
        "scratchpad_id": scratchpad.id,
        "revision": scratchpad.revision,
    }))
}

fn scratchpad_failure(error: ScratchpadServiceError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
