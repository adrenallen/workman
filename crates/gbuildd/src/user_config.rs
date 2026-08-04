//! Per-user configuration loaded when the daemon starts.

use std::{
    collections::{HashMap, HashSet},
    env,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use gbuild_core::{AgentTool, AgentToolSource, Store, StoreError};
use serde::{Deserialize, Serialize};

/// Environment variable overriding the platform-specific user config path.
pub const GBUILD_CONFIG_ENV: &str = "GBUILD_CONFIG";

/// Filename used beneath the platform-specific `gbuild` config directory.
pub const USER_CONFIG_FILE: &str = "config.yml";

/// Top-level per-user gbuild configuration.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserConfig {
    #[serde(default)]
    pub agent_tools: Vec<UserAgentTool>,
}

/// One agent command managed by the per-user config file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserAgentTool {
    pub name: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<String>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

/// Counts from reconciling config-managed tools into the durable registry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AgentToolSyncReport {
    pub created: usize,
    pub updated: usize,
    pub removed: usize,
}

#[derive(Debug)]
pub enum UserConfigError {
    Io(io::Error),
    Yaml(serde_yaml::Error),
    Store(StoreError),
    Invalid(String),
}

impl fmt::Display for UserConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "read user config: {error}"),
            Self::Yaml(error) => write!(formatter, "parse user config: {error}"),
            Self::Store(error) => write!(formatter, "sync user config: {error}"),
            Self::Invalid(error) => formatter.write_str(error),
        }
    }
}

impl Error for UserConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Yaml(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for UserConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_yaml::Error> for UserConfigError {
    fn from(error: serde_yaml::Error) -> Self {
        Self::Yaml(error)
    }
}

impl From<StoreError> for UserConfigError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

fn enabled_by_default() -> bool {
    true
}

/// Resolve the user config path, honoring `GBUILD_CONFIG` before platform defaults.
pub fn user_config_path() -> PathBuf {
    env::var_os(GBUILD_CONFIG_ENV).map_or_else(
        || {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("gbuild")
                .join(USER_CONFIG_FILE)
        },
        PathBuf::from,
    )
}

pub fn parse_user_config(yaml: &str) -> Result<UserConfig, UserConfigError> {
    if yaml.trim().is_empty() {
        return Ok(UserConfig::default());
    }
    Ok(serde_yaml::from_str(yaml)?)
}

/// Reconcile one config file into the registry. A missing file means no managed tools.
pub fn sync_user_config_file(
    store: &Store,
    path: impl AsRef<Path>,
) -> Result<AgentToolSyncReport, UserConfigError> {
    let yaml = match fs::read_to_string(path) {
        Ok(yaml) => yaml,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let config = parse_user_config(&yaml)?;
    sync_user_agent_tools(store, &config.agent_tools)
}

/// Reconcile config-managed rows by stable name while preserving local database rows.
pub fn sync_user_agent_tools(
    store: &Store,
    configured: &[UserAgentTool],
) -> Result<AgentToolSyncReport, UserConfigError> {
    let mut configured_names = HashSet::new();
    let mut normalized = Vec::with_capacity(configured.len());

    for entry in configured {
        let name = entry.name.trim();
        let command = entry.command.trim();
        if name.is_empty() {
            return Err(UserConfigError::Invalid(
                "agent tool name cannot be empty".to_owned(),
            ));
        }
        if command.is_empty() {
            return Err(UserConfigError::Invalid(format!(
                "agent tool {name:?} command cannot be empty"
            )));
        }
        if command.contains('\0') {
            return Err(UserConfigError::Invalid(format!(
                "agent tool {name:?} command may not contain NUL bytes"
            )));
        }
        if !configured_names.insert(name.to_owned()) {
            return Err(UserConfigError::Invalid(format!(
                "agent tool name {name:?} is configured more than once"
            )));
        }

        let tool_type = entry
            .tool_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| infer_tool_type(command));
        normalized.push((
            name.to_owned(),
            command.to_owned(),
            tool_type,
            entry.enabled,
        ));
    }

    let mut report = AgentToolSyncReport::default();
    let existing = store.list_agent_tools()?;
    let mut existing_by_name = existing
        .iter()
        .map(|tool| (tool.name.clone(), tool.clone()))
        .collect::<HashMap<_, _>>();
    let mut next_id = store.next_agent_tool_id()?;

    for (name, command, tool_type, enabled) in normalized {
        let (id, is_new) = existing_by_name
            .remove(&name)
            .map_or_else(|| (next_id, true), |tool| (tool.id, false));
        if is_new {
            next_id += 1;
        }
        let tool = AgentTool {
            id,
            name,
            command,
            tool_type,
            enabled,
            source: AgentToolSource::Config,
        };
        let changed = existing.iter().find(|old| old.id == id) != Some(&tool);
        if changed {
            store.put_agent_tool(&tool)?;
            if is_new {
                report.created += 1;
            } else {
                report.updated += 1;
            }
        }
    }

    for tool in existing {
        if tool.source == AgentToolSource::Config && !configured_names.contains(&tool.name) {
            if store.delete_agent_tool(tool.id)? {
                report.removed += 1;
            }
        }
    }

    Ok(report)
}

fn infer_tool_type(command: &str) -> String {
    let executable = command.split_whitespace().next().unwrap_or("agent");
    Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(executable)
        .trim_end_matches(".exe")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use gbuild_core::{AgentTool, AgentToolSource, Store};

    use super::{UserAgentTool, parse_user_config, sync_user_agent_tools};

    fn configured(name: &str, command: &str, tool_type: Option<&str>) -> UserAgentTool {
        UserAgentTool {
            name: name.to_owned(),
            command: command.to_owned(),
            tool_type: tool_type.map(str::to_owned),
            enabled: true,
        }
    }

    #[test]
    fn sync_uses_name_identity_removes_managed_rows_and_preserves_local_rows() {
        let store = Store::open_in_memory().unwrap();
        store
            .put_agent_tool(&AgentTool {
                id: 90,
                name: "Local script".to_owned(),
                command: "script-agent".to_owned(),
                tool_type: "bespoke".to_owned(),
                enabled: true,
                source: AgentToolSource::Local,
            })
            .unwrap();

        let first = sync_user_agent_tools(
            &store,
            &[
                configured("Codex", "codex --full-auto", Some("codex")),
                configured("Mystery", "/opt/tools/mystery --go", None),
            ],
        )
        .unwrap();
        assert_eq!(first.created, 1);
        assert_eq!(first.updated, 1);
        let tools = store.list_agent_tools().unwrap();
        let codex_id = tools.iter().find(|tool| tool.name == "Codex").unwrap().id;
        assert_eq!(
            tools
                .iter()
                .find(|tool| tool.name == "Mystery")
                .unwrap()
                .tool_type,
            "mystery"
        );

        let second = sync_user_agent_tools(
            &store,
            &[configured(
                "Codex",
                "codex --dangerously-bypass-approvals-and-sandbox",
                Some("unrecognized-codex-compatible"),
            )],
        )
        .unwrap();
        assert_eq!(second.removed, 1);
        let tools = store.list_agent_tools().unwrap();
        assert_eq!(
            tools.iter().find(|tool| tool.name == "Codex").unwrap().id,
            codex_id
        );
        assert!(tools.iter().any(|tool| tool.name == "Local script"));
        assert!(!tools.iter().any(|tool| tool.name == "Mystery"));
    }

    #[test]
    fn yaml_defaults_enabled_and_allows_unknown_tool_types() {
        let config = parse_user_config(
            "agent_tools:\n  - name: Future Agent\n    command: future-agent --yes\n    tool_type: future_v9\n",
        )
        .unwrap();
        assert!(config.agent_tools[0].enabled);
        assert_eq!(
            config.agent_tools[0].tool_type.as_deref(),
            Some("future_v9")
        );
    }
}
