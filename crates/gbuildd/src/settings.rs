//! Daemon runtime and MCP connection details exposed to authenticated local clients.

use std::path::PathBuf;

use serde::Serialize;
use tokio::time::Instant;

use crate::Discovery;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpConnectionInfo {
    pub endpoint: String,
    pub token: String,
    pub claude_command: String,
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
    let authorization = format!("Authorization: Bearer {}", discovery.token);
    let command = [
        "claude".to_owned(),
        "mcp".to_owned(),
        "add".to_owned(),
        "--transport".to_owned(),
        "http".to_owned(),
        "gbuild".to_owned(),
        endpoint.clone(),
        "--header".to_owned(),
        authorization,
    ]
    .iter()
    .map(|argument| shell_quote(argument))
    .collect::<Vec<_>>()
    .join(" ");
    McpConnectionInfo {
        endpoint,
        token: discovery.token.clone(),
        claude_command: command,
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
    fn mcp_connection_info_matches_the_cli_contract() {
        let info = mcp_connection_info(&Discovery {
            port: 41731,
            token: "secret-token".into(),
            pid: 42,
        });
        assert_eq!(info.endpoint, "http://127.0.0.1:41731/mcp");
        assert_eq!(
            info.claude_command,
            "claude mcp add --transport http gbuild http://127.0.0.1:41731/mcp --header 'Authorization: Bearer secret-token'"
        );
    }
}
