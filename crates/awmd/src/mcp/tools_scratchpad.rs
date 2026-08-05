//! MCP scratchpad tools backed by [`awm_core::ScratchpadService`].

use awm_core::{
    ProjectId, ScratchpadEditTarget, ScratchpadFindQuery, ScratchpadFindScope, ScratchpadId,
    ScratchpadListQuery, ScratchpadReadMode, ScratchpadService, ScratchpadServiceError,
};
use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;

use super::{AwmMcp, failure, scoped_project, success};

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
}

impl From<ReadMode> for ScratchpadReadMode {
    fn from(mode: ReadMode) -> Self {
        match mode {
            ReadMode::Full => Self::Full,
            ReadMode::Content => Self::Content,
            ReadMode::Headings => Self::Headings,
            ReadMode::Section => Self::Section,
            ReadMode::LineSlice => Self::Content,
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadAppendArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    content: String,
    #[serde(default)]
    expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadAppendSectionArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    heading: String,
    content: String,
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadTailArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    #[serde(default)]
    lines: Option<usize>,
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadRenameArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    name: String,
    expected_revision: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadTagsArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    tags: Vec<String>,
    expected_revision: i64,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProjectScopeArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadArchiveArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    /// Optional guard for compatibility with safe, non-clobbering archive calls.
    #[serde(default)]
    expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadRevisionArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    expected_revision: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScratchpadTransferArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    scratchpad_id: ScratchpadId,
    target_project_id: ProjectId,
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
impl AwmMcp {
    #[tool(
        description = "Create or replace full scratchpad content and tags at an expected revision. A leading Markdown H1 becomes the canonical scratchpad name and is removed from stored body content; title-section reads, heading outlines, and file export reconstruct it"
    )]
    async fn scratchpad_write(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadWriteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).write(
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

    #[tool(description = "Read full content, a heading outline, one section, or a line slice")]
    async fn scratchpad_read(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadReadArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).read(
            project.id,
            args.scratchpad_id,
            args.mode.into(),
            args.section_heading.as_deref(),
            args.offset.unwrap_or(0),
            args.limit,
        ) {
            Ok(read) => success(json!({
                "found": true,
                "scratchpad": read.scratchpad,
                "total_lines": read.total_lines,
                "offset": read.offset,
                "returned_lines": read.returned_lines,
                "has_more": read.has_more,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Append content without replacing existing scratchpad text")]
    async fn scratchpad_append(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadAppendArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).append(
            project.id,
            args.scratchpad_id,
            args.content,
            args.expected_revision,
        ) {
            Ok(scratchpad) => revision_receipt(&scratchpad),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(
        description = "Append under a normalized, case-insensitive markdown heading. Missing headings stay an error unless create_heading=true, which creates the section at the document end; revision-guarded"
    )]
    async fn scratchpad_append_section(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadAppendSectionArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).append_section_with_create(
            project.id,
            args.scratchpad_id,
            &args.heading,
            args.content,
            args.create_heading,
            args.expected_revision,
        ) {
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
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).edit(
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

    #[tool(description = "Return the last N scratchpad lines with revision metadata")]
    async fn scratchpad_tail(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadTailArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).tail(
            project.id,
            args.scratchpad_id,
            args.lines,
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
        match ScratchpadService::new(registry.store()).list(
            project.id,
            ScratchpadListQuery {
                query: args.query,
                tags: args.tags.unwrap_or_default(),
                offset: args.offset.unwrap_or(0),
                limit: args.limit,
            },
        ) {
            Ok(page) => success(page),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Rename a scratchpad at an expected revision")]
    async fn scratchpad_rename(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadRenameArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).rename(
            project.id,
            args.scratchpad_id,
            args.name,
            args.expected_revision,
        ) {
            Ok(scratchpad) => success(json!({
                "project_id": scratchpad.project_id,
                "scratchpad_id": scratchpad.id,
                "revision": scratchpad.revision,
                "name": scratchpad.name,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Add multiple normalized tags in one revision bump")]
    async fn scratchpad_add_tags(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadTagsArgs>,
    ) -> CallToolResult {
        self.scratchpad_tag_change(parts, args, true).await
    }

    #[tool(description = "Remove multiple normalized tags in one revision bump")]
    async fn scratchpad_remove_tags(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadTagsArgs>,
    ) -> CallToolResult {
        self.scratchpad_tag_change(parts, args, false).await
    }

    #[tool(description = "List distinct tags from active scratchpads in a project")]
    async fn scratchpad_tags_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).tags_list(project.id) {
            Ok(tags) => success(json!({ "project_id": project.id, "tags": tags })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Archive a scratchpad so normal lists hide it")]
    async fn scratchpad_archive(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadArchiveArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).archive(
            project.id,
            args.scratchpad_id,
            args.expected_revision,
        ) {
            Ok(scratchpad) => success(json!({
                "project_id": scratchpad.project_id,
                "scratchpad_id": scratchpad.id,
                "revision": scratchpad.revision,
                "archived": scratchpad.archived,
            })),
            Err(error) => scratchpad_failure(error),
        }
    }

    #[tool(description = "Clear scratchpad content at an expected revision")]
    async fn scratchpad_clear(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadRevisionArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).clear(
            project.id,
            args.scratchpad_id,
            args.expected_revision,
        ) {
            Ok(scratchpad) => revision_receipt(&scratchpad),
            Err(error) => scratchpad_failure(error),
        }
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

    #[tool(description = "Move a scratchpad to another project at an expected revision")]
    async fn scratchpad_transfer(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadTransferArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).transfer(
            project.id,
            args.scratchpad_id,
            args.target_project_id,
            args.expected_revision,
        ) {
            Ok(scratchpad) => success(json!({
                "project_id": project.id,
                "target_project_id": scratchpad.project_id,
                "scratchpad_id": scratchpad.id,
                "revision": scratchpad.revision,
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

    #[tool(
        description = "Load UTF-8 text from a project-relative path. A leading Markdown H1 becomes the canonical scratchpad name and is removed from stored body content; title-section reads, heading outlines, and file export reconstruct it"
    )]
    async fn scratchpad_load_from_file(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ScratchpadLoadArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match ScratchpadService::new(registry.store()).load_from_file(
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

    async fn scratchpad_tag_change(
        &self,
        parts: Parts,
        args: ScratchpadTagsArgs,
        add: bool,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = ScratchpadService::new(registry.store());
        let result = if add {
            service.add_tags(
                project.id,
                args.scratchpad_id,
                args.tags,
                args.expected_revision,
            )
        } else {
            service.remove_tags(
                project.id,
                args.scratchpad_id,
                args.tags,
                args.expected_revision,
            )
        };
        match result {
            Ok(scratchpad) => revision_receipt(&scratchpad),
            Err(error) => scratchpad_failure(error),
        }
    }
}

fn revision_receipt(scratchpad: &awm_core::Scratchpad) -> CallToolResult {
    success(json!({
        "project_id": scratchpad.project_id,
        "scratchpad_id": scratchpad.id,
        "revision": scratchpad.revision,
    }))
}

fn scratchpad_failure(error: ScratchpadServiceError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
