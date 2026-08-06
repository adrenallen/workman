//! Per-user configuration loaded when the daemon starts.

use std::{
    collections::{HashMap, HashSet},
    env,
    error::Error,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use workman_core::{
    AgentTool, AgentToolSource, DEFAULT_UPDATE_KEY, Store, StoreError, WORKMAN_UPDATE_KEY_ENV,
};

/// Environment variable overriding the platform-specific user config path.
pub const WORKMAN_CONFIG_ENV: &str = "WORKMAN_CONFIG";

/// Filename used beneath the platform-specific `workman` config directory.
pub const USER_CONFIG_FILE: &str = "config.yml";

/// Top-level per-user workman configuration.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserConfig {
    #[serde(default)]
    pub agent_tools: Vec<UserAgentTool>,
    #[serde(default, skip_serializing_if = "UserUpdateConfig::is_empty")]
    pub update: UserUpdateConfig,
}

/// Per-user release download configuration.
#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserUpdateConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl UserUpdateConfig {
    fn is_empty(&self) -> bool {
        self.key.is_none()
    }
}

impl fmt::Debug for UserUpdateConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserUpdateConfig")
            .field("key", &self.key.as_ref().map(|_| "[redacted]"))
            .finish()
    }
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

/// Resolve the user config path, honoring `WORKMAN_CONFIG` before platform defaults.
pub fn user_config_path() -> PathBuf {
    env::var_os(WORKMAN_CONFIG_ENV)
        .map_or_else(|| default_user_config_path("workman"), PathBuf::from)
}

pub(crate) fn default_user_config_path(app_name: &str) -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(app_name)
        .join(USER_CONFIG_FILE)
}

pub fn parse_user_config(yaml: &str) -> Result<UserConfig, UserConfigError> {
    if yaml.trim().is_empty() {
        return Ok(UserConfig::default());
    }
    Ok(serde_yaml::from_str(yaml)?)
}

/// Resolve the update key in command-line, config file, environment, application order.
pub fn resolve_update_key(explicit: Option<&str>) -> Result<String, UserConfigError> {
    if explicit.is_some() {
        return select_update_key(explicit, None, None);
    }
    let path = user_config_path();
    let yaml = match fs::read_to_string(&path) {
        Ok(yaml) => yaml,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let config = parse_user_config(&yaml)?;
    let environment_key = match env::var(WORKMAN_UPDATE_KEY_ENV) {
        Ok(key) => Some(key),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(UserConfigError::Invalid(format!(
                "{WORKMAN_UPDATE_KEY_ENV} must be valid UTF-8"
            )));
        }
    };
    select_update_key(
        explicit,
        config.update.key.as_deref(),
        environment_key.as_deref(),
    )
}

fn select_update_key(
    explicit: Option<&str>,
    configured: Option<&str>,
    environment: Option<&str>,
) -> Result<String, UserConfigError> {
    if let Some(key) = explicit {
        return nonempty_update_key(key, "--key");
    }
    if let Some(key) = configured {
        return nonempty_update_key(key, "config.yml update.key");
    }
    if let Some(key) = environment {
        return nonempty_update_key(key, WORKMAN_UPDATE_KEY_ENV);
    }
    nonempty_update_key(DEFAULT_UPDATE_KEY, "compiled-in update key")
}

fn nonempty_update_key(key: &str, source: &str) -> Result<String, UserConfigError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(UserConfigError::Invalid(format!(
            "update key from {source} must not be empty"
        )));
    }
    Ok(key.to_owned())
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

/// Add or update one registry row at its source-of-truth YAML file.
///
/// Settings mutations promote legacy local rows into config-managed rows. The
/// agent_tools block is rewritten while unrelated YAML text and comments stay
/// byte-for-byte intact.
pub(crate) fn save_agent_tool_from_settings(
    store: &Store,
    agent_tool_id: Option<i64>,
    name: String,
    command: String,
    tool_type: String,
    enabled: bool,
) -> Result<AgentTool, UserConfigError> {
    save_agent_tool_from_settings_at(
        store,
        &user_config_path(),
        agent_tool_id,
        name,
        command,
        tool_type,
        enabled,
    )
}

fn save_agent_tool_from_settings_at(
    store: &Store,
    path: &Path,
    agent_tool_id: Option<i64>,
    name: String,
    command: String,
    tool_type: String,
    enabled: bool,
) -> Result<AgentTool, UserConfigError> {
    let existing = agent_tool_id
        .map(|id| {
            store
                .get_agent_tool(id)?
                .ok_or_else(|| UserConfigError::Invalid(format!("agent tool {id} was not found")))
        })
        .transpose()?;
    if store
        .list_agent_tools()?
        .iter()
        .any(|tool| Some(tool.id) != agent_tool_id && tool.name == name)
    {
        return Err(UserConfigError::Invalid(format!(
            "agent tool name {name:?} is already registered"
        )));
    }
    let (source, mut root) = read_user_config_document(path)?;
    let entries = agent_tool_entries_mut(&mut root)?;
    let entry_index = existing.as_ref().and_then(|existing| {
        entries
            .iter()
            .position(|entry| entry_name(entry) == Some(existing.name.as_str()))
    });
    let entry = match entry_index {
        Some(index) => &mut entries[index],
        None if agent_tool_id.is_none() => {
            entries.push(serde_yaml::Value::Mapping(Default::default()));
            entries.last_mut().expect("entry was just appended")
        }
        None => {
            return Err(UserConfigError::Invalid(format!(
                "agent tool {:?} was not found in {}",
                existing.as_ref().map(|tool| tool.name.as_str()),
                path.display()
            )));
        }
    };
    set_agent_tool_entry(entry, &name, &command, &tool_type, enabled)?;

    let config = validated_document(&root)?;
    write_user_config_document(path, &source, &root)?;
    let id = existing
        .as_ref()
        .map(|tool| tool.id)
        .unwrap_or(store.next_agent_tool_id()?);
    store.put_agent_tool(&AgentTool {
        id,
        name: name.clone(),
        command,
        tool_type,
        enabled,
        source: AgentToolSource::Config,
    })?;
    sync_user_agent_tools(store, &config.agent_tools)?;
    reorder_store_from_config(store, &config.agent_tools)?;
    store
        .list_agent_tools()?
        .into_iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| UserConfigError::Invalid("saved agent tool was not reloaded".to_owned()))
}

pub(crate) fn delete_agent_tool_from_settings(
    store: &Store,
    agent_tool_id: i64,
) -> Result<bool, UserConfigError> {
    delete_agent_tool_from_settings_at(store, &user_config_path(), agent_tool_id)
}

fn delete_agent_tool_from_settings_at(
    store: &Store,
    path: &Path,
    agent_tool_id: i64,
) -> Result<bool, UserConfigError> {
    let Some(existing) = store.get_agent_tool(agent_tool_id)? else {
        return Ok(false);
    };
    let (source, mut root) = read_user_config_document(path)?;
    let entries = agent_tool_entries_mut(&mut root)?;
    let before = entries.len();
    entries.retain(|entry| entry_name(entry) != Some(existing.name.as_str()));
    let removed_from_config = before != entries.len();
    if existing.source == AgentToolSource::Config && !removed_from_config {
        return Err(UserConfigError::Invalid(format!(
            "agent tool {:?} was not found in {}",
            existing.name,
            path.display()
        )));
    }
    let config = validated_document(&root)?;
    if removed_from_config {
        write_user_config_document(path, &source, &root)?;
    }
    let deleted = store.delete_agent_tool(agent_tool_id)?;
    sync_user_agent_tools(store, &config.agent_tools)?;
    reorder_store_from_config(store, &config.agent_tools)?;
    Ok(deleted)
}

pub(crate) fn reorder_agent_tools_from_settings(
    store: &Store,
    ordered_ids: &[i64],
) -> Result<Vec<AgentTool>, UserConfigError> {
    reorder_agent_tools_from_settings_at(store, &user_config_path(), ordered_ids)
}

fn reorder_agent_tools_from_settings_at(
    store: &Store,
    path: &Path,
    ordered_ids: &[i64],
) -> Result<Vec<AgentTool>, UserConfigError> {
    let tools = store.list_agent_tools()?;
    let by_id = tools
        .iter()
        .map(|tool| (tool.id, tool))
        .collect::<HashMap<_, _>>();
    let requested = ordered_ids.iter().copied().collect::<HashSet<_>>();
    if requested.len() != ordered_ids.len()
        || requested != by_id.keys().copied().collect::<HashSet<_>>()
    {
        return Err(UserConfigError::Invalid(
            "agent tool order must contain every registered tool exactly once".to_owned(),
        ));
    }

    let (source, mut root) = read_user_config_document(path)?;
    let entries = agent_tool_entries_mut(&mut root)?;
    let mut existing_entries = entries
        .drain(..)
        .filter_map(|entry| {
            let name = entry_name(&entry)?.to_owned();
            Some((name, entry))
        })
        .collect::<HashMap<_, _>>();
    for id in ordered_ids {
        let tool = by_id[id];
        let mut entry = existing_entries
            .remove(&tool.name)
            .unwrap_or_else(|| serde_yaml::Value::Mapping(Default::default()));
        set_agent_tool_entry(
            &mut entry,
            &tool.name,
            &tool.command,
            &tool.tool_type,
            tool.enabled,
        )?;
        entries.push(entry);
    }
    if !existing_entries.is_empty() {
        return Err(UserConfigError::Invalid(
            "config.yml changed outside Workman; refresh before reordering".to_owned(),
        ));
    }

    let config = validated_document(&root)?;
    write_user_config_document(path, &source, &root)?;
    for id in ordered_ids {
        let mut tool = by_id[id].clone();
        tool.source = AgentToolSource::Config;
        store.put_agent_tool(&tool)?;
    }
    sync_user_agent_tools(store, &config.agent_tools)?;
    store.reorder_agent_tools(ordered_ids)?;
    store.list_agent_tools().map_err(Into::into)
}

fn read_user_config_document(
    path: &Path,
) -> Result<(String, serde_yaml::Mapping), UserConfigError> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let root = if source.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str::<serde_yaml::Value>(&source)?
            .as_mapping()
            .cloned()
            .ok_or_else(|| {
                UserConfigError::Invalid("per-user config must contain a YAML mapping".to_owned())
            })?
    };
    Ok((source, root))
}

fn agent_tool_entries_mut(
    root: &mut serde_yaml::Mapping,
) -> Result<&mut Vec<serde_yaml::Value>, UserConfigError> {
    let value = root
        .entry(serde_yaml::Value::String("agent_tools".to_owned()))
        .or_insert_with(|| serde_yaml::Value::Sequence(Vec::new()));
    value.as_sequence_mut().ok_or_else(|| {
        UserConfigError::Invalid("per-user config agent_tools must be a list".to_owned())
    })
}

fn entry_name(entry: &serde_yaml::Value) -> Option<&str> {
    entry
        .as_mapping()?
        .get(serde_yaml::Value::String("name".to_owned()))?
        .as_str()
}

fn set_agent_tool_entry(
    entry: &mut serde_yaml::Value,
    name: &str,
    command: &str,
    tool_type: &str,
    enabled: bool,
) -> Result<(), UserConfigError> {
    let entry = entry.as_mapping_mut().ok_or_else(|| {
        UserConfigError::Invalid("each agent_tools entry must be a mapping".to_owned())
    })?;
    for (key, value) in [
        ("name", serde_yaml::Value::String(name.to_owned())),
        ("command", serde_yaml::Value::String(command.to_owned())),
        ("tool_type", serde_yaml::Value::String(tool_type.to_owned())),
        ("enabled", serde_yaml::Value::Bool(enabled)),
    ] {
        entry.insert(serde_yaml::Value::String(key.to_owned()), value);
    }
    Ok(())
}

fn validated_document(root: &serde_yaml::Mapping) -> Result<UserConfig, UserConfigError> {
    let config: UserConfig = serde_yaml::from_value(serde_yaml::Value::Mapping(root.clone()))?;
    validate_user_agent_tools(&config.agent_tools)?;
    Ok(config)
}

fn write_user_config_document(
    path: &Path,
    source: &str,
    root: &serde_yaml::Mapping,
) -> Result<(), UserConfigError> {
    let entries = root
        .get(serde_yaml::Value::String("agent_tools".to_owned()))
        .cloned()
        .unwrap_or_else(|| serde_yaml::Value::Sequence(Vec::new()));
    let mut block = serde_yaml::Mapping::new();
    block.insert(serde_yaml::Value::String("agent_tools".to_owned()), entries);
    let rendered = serde_yaml::to_string(&serde_yaml::Value::Mapping(block))?;
    let updated = replace_top_level_yaml_block(source, "agent_tools", &rendered);
    write_private_atomic(path, updated.as_bytes())?;
    Ok(())
}

fn replace_top_level_yaml_block(source: &str, key: &str, replacement: &str) -> String {
    let mut offset = 0;
    let mut start = None;
    let mut end = source.len();
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let top_level = !trimmed.starts_with([' ', '\t']);
        if start.is_none() && top_level && trimmed.starts_with(&format!("{key}:")) {
            start = Some(offset);
        } else if start.is_some()
            && top_level
            && !trimmed.is_empty()
            && !trimmed.starts_with("- ")
            && (trimmed.starts_with('#') || trimmed.contains(':'))
        {
            end = offset;
            break;
        }
        offset += line.len();
    }
    match start {
        Some(start) => format!("{}{}{}", &source[..start], replacement, &source[end..]),
        None if source.trim().is_empty() => replacement.to_owned(),
        None => format!(
            "{}{}{}",
            source,
            if source.ends_with('\n') { "" } else { "\n" },
            replacement
        ),
    }
}

fn reorder_store_from_config(
    store: &Store,
    configured: &[UserAgentTool],
) -> Result<(), UserConfigError> {
    let tools = store.list_agent_tools()?;
    let by_name = tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool.id))
        .collect::<HashMap<_, _>>();
    let mut ordered = configured
        .iter()
        .filter_map(|entry| by_name.get(entry.name.trim()).copied())
        .collect::<Vec<_>>();
    let configured_ids = ordered.iter().copied().collect::<HashSet<_>>();
    ordered.extend(
        tools
            .iter()
            .filter(|tool| !configured_ids.contains(&tool.id))
            .map(|tool| tool.id),
    );
    store.reorder_agent_tools(&ordered)?;
    Ok(())
}

fn validate_user_agent_tools(configured: &[UserAgentTool]) -> Result<(), UserConfigError> {
    let scratch = Store::open_in_memory()?;
    sync_user_agent_tools(&scratch, configured).map(|_| ())
}

fn write_private_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "config path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.workman-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
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
    use workman_core::{AgentTool, AgentToolSource, Store};

    use super::{
        UserAgentTool, delete_agent_tool_from_settings_at, parse_user_config,
        reorder_agent_tools_from_settings_at, save_agent_tool_from_settings_at, select_update_key,
        sync_user_agent_tools,
    };

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

    #[test]
    fn update_key_prefers_config_then_environment_then_application_default() {
        let config = parse_user_config("update:\n  key: config-key\n").unwrap();
        assert_eq!(config.update.key.as_deref(), Some("config-key"));
        assert_eq!(
            select_update_key(
                Some("cli-key"),
                config.update.key.as_deref(),
                Some("environment-key")
            )
            .unwrap(),
            "cli-key"
        );
        assert_eq!(
            select_update_key(None, config.update.key.as_deref(), Some("environment-key")).unwrap(),
            "config-key"
        );
        assert_eq!(
            select_update_key(None, None, Some("environment-key")).unwrap(),
            "environment-key"
        );
        assert_eq!(
            select_update_key(None, None, None).unwrap(),
            workman_core::DEFAULT_UPDATE_KEY
        );
        assert!(select_update_key(None, Some("  "), Some("fallback")).is_err());
    }

    #[test]
    fn explicit_settings_edit_updates_config_source_and_preserves_unknown_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.yml");
        std::fs::write(
            &path,
            "# keep this heading\ntheme: night # keep this note\nagent_tools:\n  - name: Fixture OpenCode\n    command: opencode\n    tool_type: opencode\n    enabled: true\n    channel: nightly\n# keep this footer\ntelemetry: false\n",
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        store
            .put_agent_tool(&AgentTool {
                id: 17,
                name: "Fixture OpenCode".to_owned(),
                command: "opencode".to_owned(),
                tool_type: "opencode".to_owned(),
                enabled: true,
                source: AgentToolSource::Config,
            })
            .unwrap();

        let updated = save_agent_tool_from_settings_at(
            &store,
            &path,
            Some(17),
            "Fixture OpenCode nightly".to_owned(),
            "opencode --model nightly".to_owned(),
            "opencode".to_owned(),
            false,
        )
        .unwrap();
        assert_eq!(updated.id, 17);
        assert!(!updated.enabled);
        let yaml = std::fs::read_to_string(&path).unwrap();
        assert!(yaml.contains("# keep this heading"));
        assert!(yaml.contains("theme: night # keep this note"));
        assert!(yaml.contains("# keep this footer\ntelemetry: false"));
        assert!(yaml.contains("channel: nightly"));
        let parsed = parse_user_config(&yaml).unwrap();
        assert_eq!(parsed.agent_tools[0].name, "Fixture OpenCode nightly");
        assert!(!parsed.agent_tools[0].enabled);
        assert_eq!(
            store.get_agent_tool(17).unwrap().unwrap().command,
            "opencode --model nightly"
        );

        let added = save_agent_tool_from_settings_at(
            &store,
            &path,
            None,
            "Added agent".to_owned(),
            "added-agent --safe".to_owned(),
            "custom".to_owned(),
            true,
        )
        .unwrap();
        assert_eq!(added.source, AgentToolSource::Config);
        let mut ids = store
            .list_agent_tools()
            .unwrap()
            .into_iter()
            .map(|tool| tool.id)
            .collect::<Vec<_>>();
        ids.reverse();
        let reordered = reorder_agent_tools_from_settings_at(&store, &path, &ids).unwrap();
        assert_eq!(
            reordered.iter().map(|tool| tool.id).collect::<Vec<_>>(),
            ids
        );
        assert!(delete_agent_tool_from_settings_at(&store, &path, added.id).unwrap());
        let yaml = std::fs::read_to_string(path).unwrap();
        assert!(!yaml.contains("Added agent"));
        assert!(yaml.contains("# keep this heading"));
    }
}
