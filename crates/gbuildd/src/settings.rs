//! Daemon runtime and MCP connection details exposed to authenticated local clients.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::json;
use tokio::time::Instant;

use crate::Discovery;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpConnectionInfo {
    pub endpoint: String,
    pub token: String,
    pub setups: Vec<McpClientSetup>,
}

impl McpConnectionInfo {
    pub fn setup(&self, client: McpClient) -> Option<&McpClientSetup> {
        self.setups.iter().find(|setup| setup.client == client)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum McpClient {
    Claude,
    Codex,
    Gemini,
    Opencode,
    Generic,
}

impl McpClient {
    pub const ALL: [Self; 5] = [
        Self::Claude,
        Self::Codex,
        Self::Gemini,
        Self::Opencode,
        Self::Generic,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Generic => "generic",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
            Self::Opencode => "OpenCode",
            Self::Generic => "Generic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|client| client.as_str() == value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum McpSetupFormat {
    Shell,
    Toml,
    Json,
    Text,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpSetupField {
    pub label: &'static str,
    pub value: String,
    pub format: McpSetupFormat,
    pub sensitive: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpClientSetup {
    pub client: McpClient,
    pub label: &'static str,
    pub description: &'static str,
    pub fields: Vec<McpSetupField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DaemonSettingsInfo {
    pub data_dir: String,
    pub port: u16,
    pub pid: u32,
    pub version: &'static str,
    pub uptime_ms: u64,
    pub mcp: McpConnectionInfo,
}

#[derive(Clone)]
pub(crate) struct DaemonRuntimeSettings {
    data_dir: PathBuf,
    discovery: Discovery,
    started_at: Instant,
}

impl DaemonRuntimeSettings {
    pub(crate) fn new(data_dir: PathBuf, discovery: Discovery, started_at: Instant) -> Self {
        Self {
            data_dir,
            discovery,
            started_at,
        }
    }

    pub(crate) fn info(&self) -> DaemonSettingsInfo {
        DaemonSettingsInfo {
            data_dir: self.data_dir.to_string_lossy().into_owned(),
            port: self.discovery.port,
            pid: self.discovery.pid,
            version: env!("CARGO_PKG_VERSION"),
            uptime_ms: self
                .started_at
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            mcp: mcp_connection_info(&self.discovery),
        }
    }
}

pub fn mcp_connection_info(discovery: &Discovery) -> McpConnectionInfo {
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let authorization = format!("Bearer {}", discovery.token);
    let claude_command = [
        "claude".to_owned(),
        "mcp".to_owned(),
        "add".to_owned(),
        "--transport".to_owned(),
        "http".to_owned(),
        "gbuild".to_owned(),
        endpoint.clone(),
        "--header".to_owned(),
        format!("Authorization: {authorization}"),
    ]
    .iter()
    .map(|argument| shell_quote(argument))
    .collect::<Vec<_>>()
    .join(" ");
    let codex_environment = format!(
        "export GBUILD_MCP_AUTHORIZATION={}",
        shell_quote(&authorization)
    );
    let codex_config = format!(
        "[mcp_servers.gbuild]\nurl = {endpoint:?}\nenv_http_headers = {{ \"Authorization\" = \"GBUILD_MCP_AUTHORIZATION\" }}"
    );
    let gemini_config = serde_json::to_string_pretty(&json!({
        "mcpServers": {
            "gbuild": {
                "httpUrl": endpoint.clone(),
                "headers": { "Authorization": authorization.clone() }
            }
        }
    }))
    .expect("static MCP setup JSON serializes");
    let opencode_config = serde_json::to_string_pretty(&json!({
        "$schema": "https://opencode.ai/config.json",
        "mcp": {
            "gbuild": {
                "type": "remote",
                "url": endpoint.clone(),
                "enabled": true,
                "headers": { "Authorization": authorization.clone() }
            }
        }
    }))
    .expect("static MCP setup JSON serializes");
    let setups = vec![
        McpClientSetup {
            client: McpClient::Claude,
            label: McpClient::Claude.label(),
            description: "Run once to add gbuild to Claude Code's local MCP configuration.",
            fields: vec![field(
                "Shell command",
                claude_command,
                McpSetupFormat::Shell,
                true,
            )],
        },
        McpClientSetup {
            client: McpClient::Codex,
            label: McpClient::Codex.label(),
            description: "Export the token value, then add the server table to Codex config.toml.",
            fields: vec![
                field(
                    "Environment variable",
                    codex_environment,
                    McpSetupFormat::Shell,
                    true,
                ),
                field(
                    "~/.codex/config.toml",
                    codex_config,
                    McpSetupFormat::Toml,
                    false,
                ),
            ],
        },
        McpClientSetup {
            client: McpClient::Gemini,
            label: McpClient::Gemini.label(),
            description: "Merge this server into ~/.gemini/settings.json or a project settings file.",
            fields: vec![field(
                "settings.json",
                gemini_config,
                McpSetupFormat::Json,
                true,
            )],
        },
        McpClientSetup {
            client: McpClient::Opencode,
            label: McpClient::Opencode.label(),
            description: "Merge this remote server into your OpenCode configuration.",
            fields: vec![field(
                "opencode.json",
                opencode_config,
                McpSetupFormat::Json,
                true,
            )],
        },
        McpClientSetup {
            client: McpClient::Generic,
            label: McpClient::Generic.label(),
            description: "Use these Streamable HTTP values in any MCP-compatible client.",
            fields: vec![
                field("URL", endpoint.clone(), McpSetupFormat::Text, false),
                field(
                    "Header name",
                    "Authorization".to_owned(),
                    McpSetupFormat::Text,
                    false,
                ),
                field(
                    "Header value",
                    authorization.clone(),
                    McpSetupFormat::Text,
                    true,
                ),
            ],
        },
    ];
    McpConnectionInfo {
        endpoint,
        token: discovery.token.clone(),
        setups,
    }
}

fn field(
    label: &'static str,
    value: String,
    format: McpSetupFormat,
    sensitive: bool,
) -> McpSetupField {
    McpSetupField {
        label,
        value,
        format,
        sensitive,
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_+-.,/:=@%".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_connection_info_covers_each_supported_client() {
        let info = mcp_connection_info(&Discovery {
            port: 41731,
            token: "secret-token".into(),
            pid: 42,
        });
        assert_eq!(info.endpoint, "http://127.0.0.1:41731/mcp");
        let claude = info.setup(McpClient::Claude).unwrap();
        assert_eq!(
            claude.fields[0].value,
            "claude mcp add --transport http gbuild http://127.0.0.1:41731/mcp --header 'Authorization: Bearer secret-token'"
        );

        let codex = info.setup(McpClient::Codex).unwrap();
        assert_eq!(
            codex.fields[0].value,
            "export GBUILD_MCP_AUTHORIZATION='Bearer secret-token'"
        );
        assert_eq!(
            codex.fields[1].value,
            "[mcp_servers.gbuild]\nurl = \"http://127.0.0.1:41731/mcp\"\nenv_http_headers = { \"Authorization\" = \"GBUILD_MCP_AUTHORIZATION\" }"
        );

        let gemini: serde_json::Value =
            serde_json::from_str(&info.setup(McpClient::Gemini).unwrap().fields[0].value).unwrap();
        assert_eq!(gemini["mcpServers"]["gbuild"]["httpUrl"], info.endpoint);
        assert_eq!(
            gemini["mcpServers"]["gbuild"]["headers"]["Authorization"],
            "Bearer secret-token"
        );

        let opencode: serde_json::Value =
            serde_json::from_str(&info.setup(McpClient::Opencode).unwrap().fields[0].value)
                .unwrap();
        assert_eq!(opencode["mcp"]["gbuild"]["type"], "remote");
        assert_eq!(opencode["mcp"]["gbuild"]["url"], info.endpoint);

        let generic = info.setup(McpClient::Generic).unwrap();
        assert_eq!(generic.fields[0].value, info.endpoint);
        assert_eq!(generic.fields[1].value, "Authorization");
        assert_eq!(generic.fields[2].value, "Bearer secret-token");
    }
}
