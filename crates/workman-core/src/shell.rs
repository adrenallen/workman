//! Shared shell startup policy for agent launches and command discovery.

use std::{ffi::OsStr, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentShellMode {
    #[default]
    Auto,
    Login,
    Interactive,
    InteractiveLogin,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileTerminalSettings {
    pub shell: Option<String>,
    pub agent_shell_mode: AgentShellMode,
}

impl AgentShellMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Login => "login",
            Self::Interactive => "interactive",
            Self::InteractiveLogin => "interactive_login",
        }
    }

    pub const fn invocation(self) -> ShellInvocation {
        match self {
            Self::Auto | Self::InteractiveLogin => ShellInvocation::INTERACTIVE_LOGIN,
            Self::Login => ShellInvocation::LOGIN,
            Self::Interactive => ShellInvocation::INTERACTIVE,
        }
    }

    pub fn stored_value(self) -> Option<&'static str> {
        (self != Self::Auto).then_some(self.as_str())
    }
}

impl FromStr for AgentShellMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "login" => Ok(Self::Login),
            "interactive" => Ok(Self::Interactive),
            "interactive_login" => Ok(Self::InteractiveLogin),
            _ => Err(format!(
                "Unknown agent shell mode {value:?}; expected auto, login, interactive, or interactive_login."
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShellInvocation {
    pub login: bool,
    pub interactive: bool,
}

impl ShellInvocation {
    pub const LOGIN: Self = Self {
        login: true,
        interactive: false,
    };
    pub const INTERACTIVE: Self = Self {
        login: false,
        interactive: true,
    };
    pub const INTERACTIVE_LOGIN: Self = Self {
        login: true,
        interactive: true,
    };

    pub fn startup_args(self) -> &'static [&'static str] {
        if !cfg!(unix) {
            return &[];
        }
        match (self.login, self.interactive) {
            (true, true) => &["-l", "-i"],
            (true, false) => &["-l"],
            (false, true) => &["-i"],
            (false, false) => &[],
        }
    }

    pub fn command_args(self, shell: &Path) -> Vec<&'static str> {
        let mut args = self.startup_args().to_vec();
        args.extend(command_prelude(shell));
        args.push(command_flag(shell));
        args
    }

    pub fn label(self) -> &'static str {
        match (self.login, self.interactive) {
            (true, true) => "interactive login",
            (true, false) => "non-interactive login",
            (false, true) => "interactive",
            (false, false) => "non-interactive",
        }
    }
}

pub fn is_fish(shell: &Path) -> bool {
    shell
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name == "fish")
}

/// Quote a literal word for the selected Unix shell, including Fish's backslash rules.
pub fn quote_word(shell: &Path, word: &str) -> String {
    if is_fish(shell) {
        format!("'{}'", word.replace('\\', "\\\\").replace('\'', "\\'"))
    } else {
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }
}

/// Inspect a name without invoking its alias/function or printing its definition.
pub fn command_probe(shell: &Path, name: &str) -> String {
    let lookup = if is_fish(shell) {
        "type -q"
    } else {
        "command -v"
    };
    format!("{lookup} -- {} >/dev/null 2>&1", quote_word(shell, name))
}

/// Permit npm's PowerShell script launchers for explicit command sessions only.
/// Interactive terminal sessions retain the account's execution policy.
pub(crate) fn command_prelude(shell: &Path) -> &'static [&'static str] {
    #[cfg(windows)]
    if shell.file_name().is_some_and(|name| {
        ["powershell.exe", "powershell", "pwsh.exe", "pwsh"]
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
    }) {
        return &["-ExecutionPolicy", "Bypass"];
    }
    let _ = shell;
    &[]
}

pub(crate) fn command_flag(shell: &Path) -> &'static str {
    #[cfg(windows)]
    if shell.file_name().is_some_and(|name| {
        name.eq_ignore_ascii_case("cmd.exe") || name.eq_ignore_ascii_case("cmd")
    }) {
        return "/c";
    }
    let _ = shell;
    "-c"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_modes_preserve_platform_command_flags() {
        for mode in [
            AgentShellMode::Auto,
            AgentShellMode::Login,
            AgentShellMode::Interactive,
            AgentShellMode::InteractiveLogin,
        ] {
            assert_eq!(mode.as_str().parse(), Ok(mode));
            #[cfg(unix)]
            assert_eq!(
                mode.invocation().command_args(Path::new("/bin/zsh")),
                match mode {
                    AgentShellMode::Auto | AgentShellMode::InteractiveLogin =>
                        vec!["-l", "-i", "-c"],
                    AgentShellMode::Login => vec!["-l", "-c"],
                    AgentShellMode::Interactive => vec!["-i", "-c"],
                }
            );
            #[cfg(windows)]
            {
                assert_eq!(
                    mode.invocation().command_args(Path::new("pwsh.exe")),
                    ["-ExecutionPolicy", "Bypass", "-c"]
                );
                assert_eq!(mode.invocation().command_args(Path::new("cmd.exe")), ["/c"]);
            }
        }
        assert!("invalid".parse::<AgentShellMode>().is_err());
    }
}
