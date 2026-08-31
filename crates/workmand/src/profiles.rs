//! Switchable whole-config profiles and secret-free portable archives.
//!
//! Profiles own project membership/order/selection, the terminal shell override, agent-tool
//! presets, and custom agent icons. Canonical project data (process history, todos, scratchpads,
//! worktree metadata, and project appearance) follows the project and is shared when the same
//! path is attached to multiple profiles. Daemon/MCP credentials, update keys, notification
//! history, UI-local appearance, and repository-local `workman.yml` files are global and are
//! deliberately absent from archives.

use std::{collections::HashSet, fs, io::Write, path::Path};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use workman_core::{
    AgentTool, AgentToolSource, ImportedProjectFolder, ImportedProjectFolderMembership,
    ProcessStatus, ProfileId,
};

use crate::{ProcessRegistry, control::agent_icons};

const ARCHIVE_FORMAT: &str = "workman-profile";
const ARCHIVE_VERSION: u32 = 3;
const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;

type ControlResult = Result<Value, (&'static str, String)>;

#[derive(Debug, Serialize)]
struct SwitchImpact {
    profile_id: ProfileId,
    running_processes: Vec<RunningProcess>,
}

#[derive(Debug, Serialize)]
struct RunningProcess {
    id: i64,
    project_id: i64,
    name: String,
    status: ProcessStatus,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileArchive {
    format: String,
    version: u32,
    name: String,
    terminal_shell: Option<String>,
    #[serde(default)]
    folders: Vec<ArchiveFolder>,
    projects: Vec<ArchiveProject>,
    agent_tools: Vec<ArchiveAgentTool>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArchiveProject {
    path: String,
    selected: bool,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    sort_order: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArchiveFolder {
    name: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    name_color: Option<String>,
    collapsed: bool,
    sort_order: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArchiveAgentTool {
    name: String,
    command: String,
    tool_type: String,
    enabled: bool,
    resume_args: Option<String>,
    continue_args: Option<String>,
    icon_png_base64: Option<String>,
}

pub(crate) fn list(registry: &ProcessRegistry) -> ControlResult {
    registry
        .store()
        .list_profiles()
        .map(|profiles| json!({ "profiles": profiles }))
        .map_err(store_error)
}

pub(crate) fn switch_impact(
    registry: &mut ProcessRegistry,
    profile_id: ProfileId,
) -> ControlResult {
    let profile = registry
        .store()
        .get_profile(profile_id)
        .map_err(store_error)?
        .ok_or((
            "profile_not_found",
            format!("profile {profile_id} was not found"),
        ))?;
    let running_processes = outgoing_running_processes(registry)?;
    Ok(json!({
        "profile": profile,
        "impact": SwitchImpact { profile_id, running_processes },
    }))
}

pub(crate) fn create(
    registry: &ProcessRegistry,
    data_dir: &Path,
    name: &str,
    copy_current: bool,
) -> ControlResult {
    validate_profile_name(name)?;
    let (profile, icon_pairs) = registry
        .store()
        .create_profile(name, copy_current)
        .map_err(profile_store_error)?;
    let mut installed = Vec::new();
    for (source_id, target_id) in icon_pairs {
        if let Err(error) = agent_icons::clone_override(data_dir, source_id, target_id) {
            for id in installed {
                let _ = agent_icons::delete_override(data_dir, id);
            }
            let _ = registry.store().delete_profile(profile.id);
            return Err(("profile_icon_error", error.to_string()));
        }
        installed.push(target_id);
    }
    Ok(json!({ "profile": profile }))
}

pub(crate) fn rename(
    registry: &ProcessRegistry,
    profile_id: ProfileId,
    name: &str,
) -> ControlResult {
    validate_profile_name(name)?;
    registry
        .store()
        .rename_profile(profile_id, name)
        .map(|profile| json!({ "profile": profile }))
        .map_err(profile_store_error)
}

pub(crate) fn delete(
    registry: &ProcessRegistry,
    data_dir: &Path,
    profile_id: ProfileId,
    confirm_delete: bool,
) -> ControlResult {
    if !confirm_delete {
        return Err((
            "confirmation_required",
            "set confirm_delete=true to delete this profile".into(),
        ));
    }
    let tool_ids = registry
        .store()
        .delete_profile(profile_id)
        .map_err(profile_store_error)?;
    for tool_id in tool_ids {
        agent_icons::delete_override(data_dir, tool_id)
            .map_err(|error| ("profile_icon_error", error.to_string()))?;
    }
    Ok(json!({ "profile_id": profile_id, "deleted": true }))
}

pub(crate) fn switch(
    registry: &mut ProcessRegistry,
    profile_id: ProfileId,
    confirm_stop_running: bool,
) -> ControlResult {
    let target = registry
        .store()
        .get_profile(profile_id)
        .map_err(store_error)?
        .ok_or((
            "profile_not_found",
            format!("profile {profile_id} was not found"),
        ))?;
    if target.active {
        return Ok(json!({ "profile": target, "stopped_processes": [] }));
    }
    let running = outgoing_running_processes(registry)?;
    if !running.is_empty() && !confirm_stop_running {
        return Err((
            "profile_switch_requires_confirmation",
            format!(
                "switching profiles will stop these {} running processes: {}",
                running.len(),
                running
                    .iter()
                    .map(|process| format!("{} ({})", process.name, process.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    let mut stopped = Vec::new();
    for process in &running {
        registry.stop(process.id).map_err(|error| {
            (
                "profile_switch_stop_failed",
                format!(
                    "could not stop process {} ({}); profile was not switched: {error}",
                    process.name, process.id
                ),
            )
        })?;
        stopped.push(process.id);
    }

    let outgoing_shell = registry
        .store()
        .active_profile_terminal_shell()
        .map_err(store_error)?;
    let target_shell = registry
        .store()
        .profile_terminal_shell(profile_id)
        .map_err(store_error)?;
    crate::user_config::save_user_shell_from_settings_at(
        registry.user_environment_resolver().config_path(),
        target_shell.as_deref(),
    )
    .map_err(|error| ("profile_config_error", error.to_string()))?;
    let switched = match registry.store().switch_profile(profile_id) {
        Ok(profile) => profile,
        Err(error) => {
            let _ = crate::user_config::save_user_shell_from_settings_at(
                registry.user_environment_resolver().config_path(),
                outgoing_shell.as_deref(),
            );
            return Err(profile_store_error(error));
        }
    };
    Ok(json!({ "profile": switched, "stopped_processes": stopped }))
}

pub(crate) fn export(
    registry: &ProcessRegistry,
    data_dir: &Path,
    profile_id: ProfileId,
    path: &Path,
) -> ControlResult {
    let profile = registry
        .store()
        .get_profile(profile_id)
        .map_err(store_error)?
        .ok_or((
            "profile_not_found",
            format!("profile {profile_id} was not found"),
        ))?;
    let folders = registry
        .store()
        .list_project_folders_for(profile_id)
        .map_err(store_error)?;
    let folder_names = folders
        .iter()
        .map(|folder| (folder.id, folder.name.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let folders = folders
        .into_iter()
        .map(|folder| ArchiveFolder {
            name: folder.name,
            icon: folder.icon,
            name_color: folder.name_color,
            collapsed: folder.collapsed,
            sort_order: folder.sort_order,
        })
        .collect();
    let mut projects = Vec::new();
    for project in registry
        .store()
        .list_profile_projects(profile_id)
        .map_err(store_error)?
    {
        let folder = registry
            .store()
            .project_folder_id_for(profile_id, project.id)
            .map_err(store_error)?
            .and_then(|folder_id| folder_names.get(&folder_id).cloned());
        projects.push(ArchiveProject {
            path: project.path,
            selected: project.selected,
            folder,
            sort_order: Some(project.sort_order),
        });
    }
    let mut agent_tools = Vec::new();
    for tool in registry
        .store()
        .list_profile_agent_tools(profile_id)
        .map_err(store_error)?
    {
        if [
            &tool.command,
            tool.resume_args.as_deref().unwrap_or(""),
            tool.continue_args.as_deref().unwrap_or(""),
        ]
        .into_iter()
        .any(contains_likely_secret)
        {
            return Err((
                "profile_export_contains_secret",
                format!(
                    "agent preset {:?} appears to contain an inline credential; move it to the environment before exporting",
                    tool.name
                ),
            ));
        }
        let icon_png_base64 = agent_icons::export_override(data_dir, tool.id)
            .map_err(|error| ("profile_icon_error", error.to_string()))?
            .map(|bytes| BASE64.encode(bytes));
        agent_tools.push(ArchiveAgentTool {
            name: tool.name,
            command: tool.command,
            tool_type: tool.tool_type,
            enabled: tool.enabled,
            resume_args: tool.resume_args,
            continue_args: tool.continue_args,
            icon_png_base64,
        });
    }
    let archive = ProfileArchive {
        format: ARCHIVE_FORMAT.into(),
        version: ARCHIVE_VERSION,
        name: profile.name,
        terminal_shell: registry
            .store()
            .profile_terminal_shell(profile_id)
            .map_err(store_error)?,
        folders,
        projects,
        agent_tools,
    };
    let bytes = serde_json::to_vec_pretty(&archive)
        .map_err(|error| ("profile_export_error", error.to_string()))?;
    write_atomic(path, &bytes).map_err(|error| ("profile_export_error", error.to_string()))?;
    Ok(json!({ "profile_id": profile_id, "path": path, "exported": true }))
}

pub(crate) fn import(
    registry: &ProcessRegistry,
    data_dir: &Path,
    path: &Path,
    name_override: Option<&str>,
) -> ControlResult {
    let metadata =
        fs::metadata(path).map_err(|error| ("profile_import_error", error.to_string()))?;
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err((
            "profile_import_error",
            "profile archive must be a file no larger than 16 MiB".into(),
        ));
    }
    let bytes = fs::read(path).map_err(|error| ("profile_import_error", error.to_string()))?;
    let archive: ProfileArchive = serde_json::from_slice(&bytes)
        .map_err(|error| ("profile_import_invalid", error.to_string()))?;
    if archive.format != ARCHIVE_FORMAT || !(1..=ARCHIVE_VERSION).contains(&archive.version) {
        return Err((
            "profile_import_invalid",
            format!(
                "unsupported profile archive format/version {:?}/{}",
                archive.format, archive.version
            ),
        ));
    }
    let name = name_override.unwrap_or(&archive.name);
    validate_profile_name(name)?;
    if registry
        .store()
        .list_profiles()
        .map_err(store_error)?
        .iter()
        .any(|profile| profile.name.eq_ignore_ascii_case(name))
    {
        return Err((
            "profile_name_conflict",
            format!("a profile named {name:?} already exists; provide a different name"),
        ));
    }

    let terminal_shell = archive
        .terminal_shell
        .as_deref()
        .map(|shell| {
            crate::user_environment::validate_shell_override(Path::new(shell))
                .map(|path| path.to_string_lossy().into_owned())
        })
        .transpose()
        .map_err(|error| ("profile_import_invalid", error))?;
    let mut folder_names = HashSet::new();
    let mut imported_folders = Vec::with_capacity(archive.folders.len());
    for folder in archive.folders {
        let folded = folder.name.trim().to_lowercase();
        if folded.is_empty()
            || folder.name.chars().count() > 80
            || folder.name.chars().any(char::is_control)
            || folder.sort_order < 0
            || !folder_names.insert(folded)
            || folder.icon.as_deref().is_some_and(|icon| {
                icon.len() > 80
                    || icon.is_empty()
                    || icon.starts_with('-')
                    || icon.ends_with('-')
                    || !icon.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            })
            || folder.name_color.as_deref().is_some_and(|color| {
                !["amber", "blue", "rose", "slate", "teal", "violet"].contains(&color)
            })
        {
            return Err((
                "profile_import_invalid",
                format!("invalid or duplicate project folder name {:?}", folder.name),
            ));
        }
        imported_folders.push(ImportedProjectFolder {
            name: folder.name,
            icon: folder.icon,
            name_color: folder.name_color,
            collapsed: folder.collapsed,
            sort_order: folder.sort_order,
        });
    }
    let mut seen_paths = HashSet::new();
    let mut projects = Vec::with_capacity(archive.projects.len());
    let mut memberships = Vec::with_capacity(archive.projects.len());
    for (position, project) in archive.projects.into_iter().enumerate() {
        let path = workman_core::canonical_path(&project.path).map_err(|error| {
            (
                "profile_import_invalid",
                format!("{}: {error}", project.path),
            )
        })?;
        if !path.is_dir() {
            return Err((
                "profile_import_invalid",
                format!("{} is not a directory", path.display()),
            ));
        }
        let path = path.to_string_lossy().into_owned();
        if !seen_paths.insert(path.clone()) {
            return Err((
                "profile_import_invalid",
                format!("project path {path:?} occurs more than once"),
            ));
        }
        let folder = project.folder.map(|name| name.trim().to_owned());
        if folder
            .as_ref()
            .is_some_and(|name| !folder_names.contains(&name.to_lowercase()))
        {
            return Err((
                "profile_import_invalid",
                format!("project path {path:?} references a missing folder"),
            ));
        }
        let sort_order = project.sort_order.unwrap_or(position as i64);
        if sort_order < 0 {
            return Err((
                "profile_import_invalid",
                format!("project path {path:?} has invalid sort order"),
            ));
        }
        memberships.push(ImportedProjectFolderMembership {
            project_path: path.clone(),
            folder_name: folder,
            sort_order,
        });
        projects.push((path, project.selected));
    }
    if projects.iter().filter(|(_, selected)| *selected).count() > 1 {
        return Err((
            "profile_import_invalid",
            "profile archive selects more than one project".into(),
        ));
    }

    let mut seen_tools = HashSet::new();
    let mut tools = Vec::with_capacity(archive.agent_tools.len());
    let mut icons = Vec::with_capacity(archive.agent_tools.len());
    for tool in archive.agent_tools {
        validate_agent_tool(&tool, &mut seen_tools)?;
        let icon = tool
            .icon_png_base64
            .as_deref()
            .map(|encoded| {
                BASE64
                    .decode(encoded)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| {
                        agent_icons::validate_png_override(&bytes)
                            .map_err(|error| error.to_string())?;
                        Ok(bytes)
                    })
            })
            .transpose()
            .map_err(|error| ("profile_import_invalid", error))?;
        tools.push(AgentTool {
            id: 0,
            name: tool.name,
            command: tool.command,
            tool_type: tool.tool_type,
            enabled: tool.enabled,
            source: AgentToolSource::Config,
            resume_args: tool.resume_args,
            continue_args: tool.continue_args,
        });
        icons.push(icon);
    }

    // All parsing, filesystem canonicalization, duplicate checks, shell checks, credential
    // checks, and PNG decoding finish before this first durable write.
    let (profile, tool_ids) = registry
        .store()
        .import_profile(name, terminal_shell.as_deref(), &projects, &tools)
        .map_err(profile_store_error)?;
    if let Err(error) = registry.store().restore_imported_project_folders(
        profile.id,
        &imported_folders,
        &memberships,
    ) {
        let _ = registry.store().delete_profile(profile.id);
        return Err(("profile_import_invalid", error.to_string()));
    }
    let mut installed = Vec::new();
    for (tool_id, icon) in tool_ids.into_iter().zip(icons) {
        if let Some(icon) = icon
            && let Err(error) = agent_icons::install_png_override(data_dir, tool_id, &icon)
        {
            for id in installed {
                let _ = agent_icons::delete_override(data_dir, id);
            }
            let _ = registry.store().delete_profile(profile.id);
            return Err(("profile_icon_error", error.to_string()));
        }
        installed.push(tool_id);
    }
    Ok(json!({ "profile": profile, "path": path, "imported": true }))
}

fn outgoing_running_processes(
    registry: &mut ProcessRegistry,
) -> Result<Vec<RunningProcess>, (&'static str, String)> {
    let projects = registry.store().list_projects().map_err(store_error)?;
    let mut running = Vec::new();
    for project in projects {
        for process in registry
            .list(Some(project.id))
            .map_err(|error| (error.code(), error.to_string()))?
        {
            if matches!(
                process.status,
                ProcessStatus::Starting | ProcessStatus::Running
            ) {
                running.push(RunningProcess {
                    id: process.id,
                    project_id: process.project_id,
                    name: process.name,
                    status: process.status,
                });
            }
        }
    }
    Ok(running)
}

fn validate_profile_name(name: &str) -> Result<(), (&'static str, String)> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err((
            "invalid_profile_name",
            "profile name must contain 1–80 visible characters".into(),
        ));
    }
    Ok(())
}

fn validate_agent_tool(
    tool: &ArchiveAgentTool,
    seen: &mut HashSet<String>,
) -> Result<(), (&'static str, String)> {
    let folded = tool.name.trim().to_ascii_lowercase();
    if folded.is_empty() || tool.name.chars().count() > 80 || !seen.insert(folded) {
        return Err((
            "profile_import_invalid",
            format!("invalid or duplicate agent preset name {:?}", tool.name),
        ));
    }
    if tool.command.trim().is_empty()
        || tool.command.len() > 65_536
        || tool.command.contains('\0')
        || tool.tool_type.trim().is_empty()
        || tool.tool_type.len() > 128
    {
        return Err((
            "profile_import_invalid",
            format!(
                "agent preset {:?} has invalid command/type fields",
                tool.name
            ),
        ));
    }
    if tool
        .resume_args
        .as_deref()
        .is_some_and(|args| args.contains('\0') || !args.contains("{session_id}"))
        || tool
            .continue_args
            .as_deref()
            .is_some_and(|args| args.contains('\0'))
    {
        return Err((
            "profile_import_invalid",
            format!("agent preset {:?} has invalid resume arguments", tool.name),
        ));
    }
    if [
        &tool.command,
        tool.resume_args.as_deref().unwrap_or(""),
        tool.continue_args.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .any(contains_likely_secret)
    {
        return Err((
            "profile_import_invalid",
            format!(
                "agent preset {:?} appears to contain an inline credential",
                tool.name
            ),
        ));
    }
    Ok(())
}

fn contains_likely_secret(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "token=",
        "api_key=",
        "apikey=",
        "secret=",
        "authorization:",
        "bearer ",
        "--token",
        "--api-key",
        "--secret",
        "private_key",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "archive path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("profile"),
        Uuid::new_v4()
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn store_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("store_error", error.to_string())
}

fn profile_store_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("profile_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_schema_excludes_secret_bearing_global_fields() {
        let archive = ProfileArchive {
            format: ARCHIVE_FORMAT.into(),
            version: ARCHIVE_VERSION,
            name: "Demo".into(),
            terminal_shell: None,
            folders: Vec::new(),
            projects: Vec::new(),
            agent_tools: Vec::new(),
        };
        let json = serde_json::to_string(&archive).unwrap();
        for forbidden in ["token", "update", "signing", "download_key", "environment"] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn likely_inline_credentials_are_rejected() {
        for command in [
            "agent --token abc",
            "API_KEY=abc agent",
            "agent --header 'Authorization: Bearer abc'",
        ] {
            assert!(contains_likely_secret(command));
        }
        assert!(!contains_likely_secret("codex --model gpt-5"));
    }
}
