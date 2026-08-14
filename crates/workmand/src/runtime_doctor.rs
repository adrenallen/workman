//! Cheap runtime diagnostics and consent-gated MCP self-configuration for agent tools.

use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::{process::Command, task::JoinSet, time::timeout};
use workman_core::{AgentTool, AgentToolId, AgentToolSource};

const VERSION_TIMEOUT: Duration = Duration::from_millis(1_500);
const TOKEN_HEADER: &str = "x-workman-mcp-token";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentToolHealth {
    pub id: AgentToolId,
    pub name: String,
    pub command: String,
    pub tool_type: String,
    pub enabled: bool,
    pub source: AgentToolSource,
    pub found_on_path: bool,
    pub resolved_binary: Option<String>,
    pub version: Option<String>,
    pub version_error: Option<String>,
    pub config_path: String,
    pub config_exists: bool,
    pub launch_ready: bool,
    pub install_url: Option<String>,
    pub mcp_launch_supported: bool,
    pub mcp_launch_mechanism: String,
    pub mcp_launch_note: String,
    pub configuration_mode: String,
    pub configuration_note: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentToolsHealth {
    pub checked_at: u64,
    pub ready_count: usize,
    pub total_count: usize,
    pub enabled_ready_count: usize,
    pub enabled_count: usize,
    pub all_enabled_ready: bool,
    pub summary: String,
    pub tools: Vec<AgentToolHealth>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentToolConfigPreview {
    pub agent_tool_id: AgentToolId,
    pub tool_type: String,
    pub automatic_wiring: bool,
    pub can_write: bool,
    pub requires_consent: bool,
    pub path: String,
    pub preview: Option<String>,
    pub preview_sha256: Option<String>,
    pub already_configured: bool,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentToolConfigWrite {
    pub agent_tool_id: AgentToolId,
    pub path: String,
    pub written: bool,
    pub preview_sha256: String,
}

#[derive(Clone, Debug)]
struct DoctorEnvironment {
    home: PathBuf,
    path: OsString,
    variables: BTreeMap<OsString, OsString>,
    version_timeout: Duration,
}

impl DoctorEnvironment {
    fn current() -> Self {
        let variables = env::vars_os().collect::<BTreeMap<_, _>>();
        Self {
            home: env::var_os("HOME")
                .map(PathBuf::from)
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from(".")),
            path: env::var_os("PATH").unwrap_or_default(),
            variables,
            version_timeout: VERSION_TIMEOUT,
        }
    }
}

pub async fn check_agent_tools_with_user_environment(
    tools: Vec<AgentTool>,
    resolved: &crate::user_environment::ResolvedUserEnvironment,
) -> AgentToolsHealth {
    let variables = resolved.command_environment();
    let home = variables
        .get(OsStr::new("HOME"))
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let path = variables
        .get(OsStr::new("PATH"))
        .cloned()
        .unwrap_or_default();
    let environment = DoctorEnvironment {
        home,
        path,
        variables,
        version_timeout: VERSION_TIMEOUT,
    };
    check_agent_tools_in(tools, environment).await
}

async fn check_agent_tools_in(
    tools: Vec<AgentTool>,
    environment: DoctorEnvironment,
) -> AgentToolsHealth {
    let mut checks = JoinSet::new();
    for tool in tools {
        let environment = environment.clone();
        checks.spawn(async move { check_agent_tool(tool, &environment).await });
    }

    let mut health = Vec::new();
    while let Some(result) = checks.join_next().await {
        if let Ok(result) = result {
            health.push(result);
        }
    }
    health.sort_by_key(|tool| tool.id);

    let ready_count = health.iter().filter(|tool| tool.launch_ready).count();
    let enabled_count = health.iter().filter(|tool| tool.enabled).count();
    let enabled_ready_count = health
        .iter()
        .filter(|tool| tool.enabled && tool.launch_ready)
        .count();
    let total_count = health.len();
    AgentToolsHealth {
        checked_at: now_millis(),
        ready_count,
        total_count,
        enabled_ready_count,
        enabled_count,
        all_enabled_ready: enabled_ready_count == enabled_count,
        summary: format!("{ready_count} of {total_count} agent tools are MCP-ready"),
        tools: health,
    }
}

async fn check_agent_tool(tool: AgentTool, environment: &DoctorEnvironment) -> AgentToolHealth {
    let executable = command_executable(&tool.command);
    let resolved = executable
        .as_deref()
        .and_then(|executable| resolve_executable(executable, &environment.path));
    let (version, version_error) = match resolved.as_deref() {
        Some(executable) => {
            capture_version(
                executable,
                &environment.variables,
                environment.version_timeout,
            )
            .await
        }
        None => (None, None),
    };
    let target = config_target(&tool, &environment.home);
    let capability = crate::mcp::agent_spawning::mcp_launch_capability(&tool.tool_type);
    let found_on_path = resolved.is_some();
    let launch_ready = tool.enabled && found_on_path && capability.supported;

    AgentToolHealth {
        id: tool.id,
        name: tool.name,
        command: tool.command,
        tool_type: tool.tool_type,
        enabled: tool.enabled,
        source: tool.source,
        found_on_path,
        resolved_binary: resolved.map(|path| path.to_string_lossy().into_owned()),
        version,
        version_error,
        config_path: target.path.to_string_lossy().into_owned(),
        config_exists: target.exists(),
        launch_ready,
        install_url: install_url(&target.normalized_type).map(str::to_owned),
        mcp_launch_supported: capability.supported,
        mcp_launch_mechanism: capability.mechanism.to_owned(),
        mcp_launch_note: capability.note.to_owned(),
        configuration_mode: if target.automatic_wiring {
            "per_launch".to_owned()
        } else if !target.can_write {
            "unsupported".to_owned()
        } else {
            "self_config".to_owned()
        },
        configuration_note: if target.automatic_wiring || !target.can_write {
            capability.note.to_owned()
        } else {
            "Preview and approve a Workman MCP entry for this runtime.".to_owned()
        },
    }
}

pub fn config_preview(tool: &AgentTool, mcp_url: &str) -> Result<AgentToolConfigPreview, String> {
    config_preview_in(tool, mcp_url, &DoctorEnvironment::current().home)
}

fn config_preview_in(
    tool: &AgentTool,
    mcp_url: &str,
    home: &Path,
) -> Result<AgentToolConfigPreview, String> {
    let target = config_target(tool, home);
    if target.automatic_wiring {
        return Ok(AgentToolConfigPreview {
            agent_tool_id: tool.id,
            tool_type: tool.tool_type.clone(),
            automatic_wiring: true,
            can_write: false,
            requires_consent: false,
            path: target.path.to_string_lossy().into_owned(),
            preview: None,
            preview_sha256: None,
            already_configured: true,
            message: "This runtime receives an isolated workman MCP connector on every launch; no user config change is needed.".to_owned(),
        });
    }
    if !target.can_write {
        return Ok(AgentToolConfigPreview {
            agent_tool_id: tool.id,
            tool_type: tool.tool_type.clone(),
            automatic_wiring: false,
            can_write: false,
            requires_consent: false,
            path: target.path.to_string_lossy().into_owned(),
            preview: None,
            preview_sha256: None,
            already_configured: false,
            message: crate::mcp::agent_spawning::mcp_launch_capability(&tool.tool_type)
                .note
                .to_owned(),
        });
    }

    let mut root = read_json_object(&target.path)?;
    let desired = desired_server(&target.normalized_type, mcp_url);
    let already_configured = current_server(&root, &target.normalized_type) == Some(&desired);
    put_server(&mut root, &target.normalized_type, desired)?;
    let mut preview = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|error| format!("serialize {}: {error}", target.path.display()))?;
    preview.push('\n');
    let preview_sha256 = sha256(&preview);
    Ok(AgentToolConfigPreview {
        agent_tool_id: tool.id,
        tool_type: tool.tool_type.clone(),
        automatic_wiring: false,
        can_write: true,
        requires_consent: true,
        path: target.path.to_string_lossy().into_owned(),
        preview: Some(preview),
        preview_sha256: Some(preview_sha256),
        already_configured,
        message: "Review the complete resulting config, then explicitly approve this write."
            .to_owned(),
    })
}

pub fn apply_config(
    tool: &AgentTool,
    mcp_url: &str,
    confirm_write: bool,
    expected_preview_sha256: &str,
) -> Result<AgentToolConfigWrite, String> {
    apply_config_in(
        tool,
        mcp_url,
        confirm_write,
        expected_preview_sha256,
        &DoctorEnvironment::current().home,
    )
}

fn apply_config_in(
    tool: &AgentTool,
    mcp_url: &str,
    confirm_write: bool,
    expected_preview_sha256: &str,
    home: &Path,
) -> Result<AgentToolConfigWrite, String> {
    if !confirm_write {
        return Err("explicit consent is required before writing an agent config".to_owned());
    }
    let preview = config_preview_in(tool, mcp_url, home)?;
    if !preview.can_write {
        return Err(preview.message);
    }
    let actual_hash = preview.preview_sha256.as_deref().unwrap_or_default();
    if actual_hash != expected_preview_sha256 {
        return Err(
            "the agent config changed after preview; refresh the preview and review it again"
                .to_owned(),
        );
    }
    let path = PathBuf::from(&preview.path);
    let contents = preview.preview.as_deref().unwrap_or_default();
    write_private_atomic(&path, contents.as_bytes())
        .map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(AgentToolConfigWrite {
        agent_tool_id: tool.id,
        path: preview.path,
        written: true,
        preview_sha256: actual_hash.to_owned(),
    })
}

async fn capture_version(
    executable: &Path,
    environment: &BTreeMap<OsString, OsString>,
    version_timeout: Duration,
) -> (Option<String>, Option<String>) {
    let mut command = Command::new(executable);
    command
        .arg("--version")
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match timeout(version_timeout, command.output()).await {
        Ok(Ok(output)) => {
            let text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr).into_owned()
            } else {
                String::from_utf8_lossy(&output.stdout).into_owned()
            };
            let version = text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.chars().take(200).collect::<String>());
            let error = (!output.status.success()).then(|| match version.as_deref() {
                Some(version) => format!("--version exited with {}: {version}", output.status),
                None => format!("--version exited with {}", output.status),
            });
            (version, error)
        }
        Ok(Err(error)) => (None, Some(format!("run --version: {error}"))),
        Err(_) => (
            None,
            Some(format!(
                "--version timed out after {}ms",
                version_timeout.as_millis()
            )),
        ),
    }
}

fn command_executable(command: &str) -> Option<String> {
    let words = shell_words(command);
    let mut words = words.into_iter();
    let first = words.next()?;
    if first == "env" {
        return words.find(|word| !is_environment_assignment(word));
    }
    if is_environment_assignment(&first) {
        return std::iter::once(first)
            .chain(words)
            .find(|word| !is_environment_assignment(word));
    }
    Some(first)
}

fn shell_words(source: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for character in source.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            started = true;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
        } else if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                word.push(character);
            }
            started = true;
        } else if character.is_whitespace() && quote.is_none() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(character);
            started = true;
        }
    }
    if escaped {
        word.push('\\');
    }
    if started {
        words.push(word);
    }
    words
}

fn is_environment_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
    })
}

fn resolve_executable(executable: &str, path: &OsString) -> Option<PathBuf> {
    let executable_path = Path::new(executable);
    if executable_path.components().count() > 1 {
        return is_executable(executable_path).then(|| executable_path.to_path_buf());
    }
    env::split_paths(path)
        .map(|directory| directory.join(executable))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[derive(Clone, Debug)]
struct ConfigTarget {
    normalized_type: String,
    path: PathBuf,
    detect_directory: bool,
    detect_parent_directory: bool,
    automatic_wiring: bool,
    can_write: bool,
}

impl ConfigTarget {
    fn exists(&self) -> bool {
        if self.detect_parent_directory {
            self.path.parent().is_some_and(Path::is_dir)
        } else if self.detect_directory {
            self.path.is_dir()
        } else {
            self.path.is_file()
        }
    }
}

fn config_target(tool: &AgentTool, home: &Path) -> ConfigTarget {
    let normalized_type = normalize_tool_type(&tool.tool_type);
    let automatic_wiring =
        crate::mcp::agent_spawning::mcp_launch_capability(&tool.tool_type).supported;
    match normalized_type.as_str() {
        "claude" | "claude_code" => ConfigTarget {
            normalized_type,
            path: home.join(".claude"),
            detect_directory: true,
            detect_parent_directory: false,
            automatic_wiring,
            can_write: false,
        },
        "codex" => ConfigTarget {
            normalized_type,
            path: home.join(".codex"),
            detect_directory: true,
            detect_parent_directory: false,
            automatic_wiring,
            can_write: false,
        },
        "gemini" | "gemini_cli" => ConfigTarget {
            normalized_type: "gemini".to_owned(),
            path: home.join(".gemini/settings.json"),
            detect_directory: false,
            detect_parent_directory: false,
            automatic_wiring,
            can_write: false,
        },
        "opencode" | "open_code" => ConfigTarget {
            normalized_type: "opencode".to_owned(),
            path: home.join(".config/opencode/opencode.json"),
            detect_directory: false,
            detect_parent_directory: true,
            automatic_wiring,
            can_write: false,
        },
        "grok" | "grok_cli" | "grok_build" => ConfigTarget {
            normalized_type: "grok".to_owned(),
            path: home.join(".grok/config.toml"),
            detect_directory: false,
            detect_parent_directory: false,
            automatic_wiring,
            can_write: false,
        },
        "kimi" | "kimi_code" => ConfigTarget {
            normalized_type: "kimi".to_owned(),
            path: home.join(".kimi-code/mcp.json"),
            detect_directory: false,
            detect_parent_directory: false,
            automatic_wiring,
            can_write: false,
        },
        _ => {
            let slug = safe_slug(&normalized_type);
            ConfigTarget {
                normalized_type: "custom".to_owned(),
                path: home.join(".config").join(slug).join("mcp.json"),
                detect_directory: false,
                detect_parent_directory: true,
                automatic_wiring: false,
                can_write: true,
            }
        }
    }
}

fn normalize_tool_type(tool_type: &str) -> String {
    tool_type
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
}

fn safe_slug(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect();
    if slug.is_empty() {
        "custom-agent".to_owned()
    } else {
        slug
    }
}

fn install_url(tool_type: &str) -> Option<&'static str> {
    match tool_type {
        "claude" | "claude_code" => Some("https://docs.anthropic.com/en/docs/claude-code/setup"),
        "codex" => Some("https://developers.openai.com/codex/cli/"),
        "gemini" => Some("https://github.com/google-gemini/gemini-cli"),
        "opencode" => Some("https://opencode.ai/docs/"),
        "grok" => Some("https://grok.com/code"),
        "kimi" => Some("https://moonshotai.github.io/kimi-code/"),
        _ => None,
    }
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    if contents.trim().is_empty() {
        return Ok(Map::new());
    }
    serde_json::from_str::<Value>(&contents)
        .map_err(|error| format!("parse {}: {error}", path.display()))?
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{} must contain a JSON object", path.display()))
}

fn desired_server(tool_type: &str, mcp_url: &str) -> Value {
    if tool_type == "opencode" {
        json!({
            "type": "remote",
            "url": mcp_url,
            "headers": { (TOKEN_HEADER): "{env:WORKMAN_MCP_TOKEN}" }
        })
    } else {
        json!({
            "httpUrl": mcp_url,
            "headers": { (TOKEN_HEADER): "$WORKMAN_MCP_TOKEN" }
        })
    }
}

fn current_server<'a>(root: &'a Map<String, Value>, tool_type: &str) -> Option<&'a Value> {
    if tool_type == "opencode" {
        root.get("mcp")?.as_object()?.get("workman")
    } else {
        root.get("mcpServers")?.as_object()?.get("workman")
    }
}

fn put_server(root: &mut Map<String, Value>, tool_type: &str, server: Value) -> Result<(), String> {
    if tool_type == "opencode" {
        object_entry(root, "mcp")?.insert("workman".to_owned(), server);
    } else {
        object_entry(root, "mcpServers")?.insert("workman".to_owned(), server);
    }
    Ok(())
}

fn object_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    let value = object
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    value
        .as_object_mut()
        .ok_or_else(|| format!("config field {key:?} must be an object"))
}

fn sha256(contents: &str) -> String {
    let digest = Sha256::digest(contents.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::{collections::BTreeMap, ffi::OsString, time::Duration};
    use std::{fs, path::Path};

    use workman_core::{AgentTool, AgentToolSource};

    #[cfg(unix)]
    use super::{DoctorEnvironment, check_agent_tools_in, check_agent_tools_with_user_environment};
    use super::{apply_config_in, command_executable, config_preview_in, config_target};

    fn tool(id: i64, name: &str, command: &str, tool_type: &str, enabled: bool) -> AgentTool {
        AgentTool {
            id,
            name: name.to_owned(),
            command: command.to_owned(),
            tool_type: tool_type.to_owned(),
            enabled,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn health_resolves_path_captures_versions_and_rolls_up_launches() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let ready = bin.join("ready-agent");
        fs::write(&ready, "#!/bin/sh\nprintf 'ready-agent 2.4.1\\n'\n").unwrap();
        fs::set_permissions(&ready, fs::Permissions::from_mode(0o700)).unwrap();
        let environment = DoctorEnvironment {
            home: temp.path().join("home"),
            path: std::env::join_paths([&bin]).unwrap(),
            variables: BTreeMap::from([
                (
                    OsString::from("HOME"),
                    temp.path().join("home").into_os_string(),
                ),
                (
                    OsString::from("PATH"),
                    std::env::join_paths([&bin]).unwrap(),
                ),
            ]),
            version_timeout: Duration::from_secs(2),
        };
        let health = check_agent_tools_in(
            vec![
                tool(1, "Ready", "ready-agent --flag", "codex", true),
                tool(2, "Missing", "missing-agent", "custom", true),
                tool(3, "Disabled", "ready-agent", "gemini", false),
            ],
            environment,
        )
        .await;

        assert_eq!(health.summary, "1 of 3 agent tools are MCP-ready");
        assert_eq!(health.enabled_ready_count, 1);
        assert_eq!(health.enabled_count, 2);
        assert!(!health.all_enabled_ready);
        assert_eq!(
            health.tools[0].version.as_deref(),
            Some("ready-agent 2.4.1")
        );
        assert!(!health.tools[1].found_on_path);
        assert!(health.tools[2].found_on_path);
        assert!(!health.tools[2].launch_ready);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn health_uses_the_resolved_login_shell_environment() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("profile-bin");
        fs::create_dir(&bin).unwrap();
        let ready = bin.join("profile-agent");
        fs::write(&ready, "#!/bin/sh\nprintf 'profile-agent 9.1\\n'\n").unwrap();
        fs::set_permissions(&ready, fs::Permissions::from_mode(0o700)).unwrap();

        let shell = temp.path().join("fixture-shell");
        fs::write(
            &shell,
            format!(
                "#!/bin/sh\nexport PATH='{}'\nshift\nexec /bin/sh \"$@\"\n",
                bin.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&shell, fs::Permissions::from_mode(0o700)).unwrap();
        let config = temp.path().join("config.yml");
        fs::write(
            &config,
            format!("terminal:\n  shell: {:?}\n", shell.to_string_lossy()),
        )
        .unwrap();
        let resolved = crate::UserEnvironmentResolver::new(&config).resolve();

        let health = check_agent_tools_with_user_environment(
            vec![tool(8, "Profile", "profile-agent", "codex", true)],
            &resolved,
        )
        .await;
        assert!(health.tools[0].found_on_path);
        assert_eq!(
            health.tools[0].version.as_deref(),
            Some("profile-agent 9.1")
        );
    }

    #[test]
    fn custom_preview_preserves_config_and_write_requires_exact_consent() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let path = home.join(".config/future_agent/mcp.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\"theme\":\"workman-fixture\",\"mcpServers\":{}}\n").unwrap();
        let tool = tool(7, "Future", "future-agent", "future-agent", true);
        let preview = config_preview_in(&tool, "http://127.0.0.1:4100/mcp", home).unwrap();
        let contents = preview.preview.as_deref().unwrap();
        assert!(contents.contains("workman-fixture"));
        assert!(contents.contains("$WORKMAN_MCP_TOKEN"));
        assert!(
            apply_config_in(&tool, "http://127.0.0.1:4100/mcp", false, "", home)
                .unwrap_err()
                .contains("explicit consent")
        );
        assert!(
            apply_config_in(
                &tool,
                "http://127.0.0.1:4100/mcp",
                true,
                "stale-preview",
                home,
            )
            .unwrap_err()
            .contains("changed after preview")
        );

        let result = apply_config_in(
            &tool,
            "http://127.0.0.1:4100/mcp",
            true,
            preview.preview_sha256.as_deref().unwrap(),
            home,
        )
        .unwrap();
        assert!(result.written);
        assert_eq!(fs::read_to_string(path).unwrap(), contents);
    }

    #[test]
    fn command_parser_skips_env_assignments_and_preserves_quoted_paths() {
        assert_eq!(
            command_executable("env FOO=bar '/opt/Agent Tools/codex' --version").as_deref(),
            Some("/opt/Agent Tools/codex")
        );
        assert_eq!(
            command_executable("WORKMAN=1 codex --full-auto").as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn opencode_presence_uses_its_config_directory_but_writes_a_specific_file() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join(".config/opencode");
        fs::create_dir_all(&directory).unwrap();
        let target = config_target(
            &tool(8, "OpenCode", "opencode", "opencode", true),
            temp.path(),
        );
        assert!(target.exists());
        assert_eq!(target.path, directory.join("opencode.json"));
        assert!(!target.path.exists());
        assert!(target.automatic_wiring);
        assert!(!target.can_write);
    }

    #[test]
    fn supported_runtimes_are_automatic_without_writing_user_config() {
        let home = Path::new("/tmp/workman-runtime-doctor-home");
        for tool_type in ["claude", "codex", "gemini", "opencode", "grok", "kimi"] {
            let target = config_target(&tool(1, tool_type, tool_type, tool_type, true), home);
            assert!(target.automatic_wiring, "{tool_type}");
            assert!(!target.can_write, "{tool_type}");
        }

        let kimi = tool(2, "Kimi", "kimi --yolo", "kimi", true);
        let target = config_target(&kimi, home);
        assert_eq!(target.path, home.join(".kimi-code/mcp.json"));
        assert!(target.automatic_wiring);
        assert!(!target.can_write);
        let preview = config_preview_in(&kimi, "http://127.0.0.1:4100/mcp", home).unwrap();
        assert!(!preview.can_write);
        assert!(preview.automatic_wiring);
        assert!(preview.message.contains("no user config change"));
    }
}
