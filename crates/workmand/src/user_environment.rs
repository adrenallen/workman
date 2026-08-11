//! Resolve the user's login shell and the environment shared by every spawned runtime.

use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::user_config::{UserConfigError, parse_user_config};

const ENV_START: &[u8] = b"\x1eWORKMAN_ENV_START\x1f";
const ENV_END: &[u8] = b"\x1eWORKMAN_ENV_END\x1f";

/// User-facing description of Workman's active and inferred shell choice.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserEnvironmentInfo {
    pub active_shell: String,
    pub configured_shell: Option<String>,
    pub inferred_shell: String,
    pub inferred_from: String,
    pub using_override: bool,
    pub warning: Option<String>,
}

/// A resolved shell choice plus the environment policy applied to spawned PTYs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedUserEnvironment {
    info: UserEnvironmentInfo,
    pty_environment: BTreeMap<OsString, OsString>,
}

impl ResolvedUserEnvironment {
    pub fn info(&self) -> &UserEnvironmentInfo {
        &self.info
    }

    pub fn active_shell(&self) -> &Path {
        Path::new(&self.info.active_shell)
    }

    pub fn pty_environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.pty_environment
    }

    /// Capture the environment after the same login shell used for PTY launches has sourced
    /// the user's profiles. Profile chatter is ignored using explicit byte sentinels.
    pub fn login_environment(&self) -> Result<BTreeMap<OsString, OsString>, String> {
        let command = concat!(
            "printf '\\036WORKMAN_ENV_START\\037'; ",
            "/usr/bin/env -0; ",
            "printf '\\036WORKMAN_ENV_END\\037'",
        );
        let output = Command::new(self.active_shell())
            .args(["-l", "-c", command])
            .envs(&self.pty_environment)
            .output()
            .map_err(|error| {
                format!(
                    "could not inspect login environment with {}: {error}",
                    self.active_shell().display()
                )
            })?;
        if !output.status.success() {
            return Err(format!(
                "login shell {} exited with {} while inspecting its environment",
                self.active_shell().display(),
                output.status
            ));
        }
        parse_environment_capture(&output.stdout)
    }

    /// Resolve the complete environment for non-PTY subprocesses such as Git, GitHub CLI,
    /// Herd, and desktop openers. If a profile cannot be inspected, preserve the daemon's
    /// inherited variables while still applying Workman's terminal-safe shell baseline.
    pub fn command_environment(&self) -> BTreeMap<OsString, OsString> {
        self.login_environment().unwrap_or_else(|_| {
            let mut environment = env::vars_os().collect::<BTreeMap<_, _>>();
            environment.extend(self.pty_environment.clone());
            environment
        })
    }
}

/// Resolves the shell from user settings and operating-system account metadata.
#[derive(Clone, Debug)]
pub struct UserEnvironmentResolver {
    config_path: PathBuf,
}

impl UserEnvironmentResolver {
    pub fn new(config_path: impl Into<PathBuf>) -> Self {
        Self {
            config_path: config_path.into(),
        }
    }

    pub fn resolve(&self) -> ResolvedUserEnvironment {
        let (configured_shell, config_warning) = configured_shell(&self.config_path)
            .unwrap_or_else(|error| (None, Some(error.to_string())));
        let inferred = infer_shell();
        let configured_path = configured_shell.as_deref().map(Path::new);
        let valid_override = configured_path
            .filter(|path| executable_shell(path))
            .map(Path::to_owned);
        let active_shell = valid_override.as_deref().unwrap_or(&inferred.path);
        let warning = if let Some(configured_path) =
            configured_path.filter(|_| valid_override.is_none())
        {
            Some(format!(
                "Configured shell {} is not an executable absolute path; using inferred shell {}.",
                configured_path.display(),
                inferred.path.display()
            ))
        } else {
            config_warning.or(inferred.warning)
        };
        let info = UserEnvironmentInfo {
            active_shell: active_shell.to_string_lossy().into_owned(),
            configured_shell,
            inferred_shell: inferred.path.to_string_lossy().into_owned(),
            inferred_from: inferred.source.to_owned(),
            using_override: valid_override.is_some(),
            warning,
        };
        ResolvedUserEnvironment {
            pty_environment: pty_environment(active_shell),
            info,
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}

#[derive(Debug)]
struct InferredShell {
    path: PathBuf,
    source: &'static str,
    warning: Option<String>,
}

fn configured_shell(path: &Path) -> Result<(Option<String>, Option<String>), UserConfigError> {
    let yaml = match fs::read_to_string(path) {
        Ok(yaml) => yaml,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let configured = parse_user_config(&yaml)?.terminal.shell;
    Ok((configured, None))
}

fn infer_shell() -> InferredShell {
    let environment = env::var_os("SHELL").map(PathBuf::from);
    let username = username();
    let dscl = username.as_deref().and_then(dscl_shell);
    let passwd = username.as_deref().and_then(passwd_shell);
    infer_shell_from(environment, dscl, passwd)
}

fn infer_shell_from(
    environment: Option<PathBuf>,
    dscl: Option<PathBuf>,
    passwd: Option<PathBuf>,
) -> InferredShell {
    for (path, source) in [
        (environment, "SHELL"),
        (dscl, "macOS account"),
        (passwd, "passwd database"),
    ] {
        if let Some(path) = path.filter(|path| executable_shell(path)) {
            return InferredShell {
                path,
                source,
                warning: None,
            };
        }
    }

    #[cfg(target_os = "macos")]
    let fallbacks = ["/bin/zsh", "/bin/bash", "/bin/sh"];
    #[cfg(not(target_os = "macos"))]
    let fallbacks = ["/bin/bash", "/bin/sh", "/bin/zsh"];
    let path = fallbacks
        .iter()
        .map(PathBuf::from)
        .find(|path| executable_shell(path))
        .unwrap_or_else(|| PathBuf::from(fallbacks[0]));
    InferredShell {
        warning: Some(format!(
            "Workman could not infer the account login shell; using fallback {}.",
            path.display()
        )),
        path,
        source: "fallback",
    }
}

fn username() -> Option<String> {
    ["USER", "LOGNAME"]
        .into_iter()
        .find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
        .or_else(|| {
            env::var_os("HOME")
                .and_then(|home| PathBuf::from(home).file_name().map(OsStr::to_owned))
                .and_then(|name| name.into_string().ok())
        })
}

fn dscl_shell(username: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/bin/dscl")
            .args([".", "-read", &format!("/Users/{username}"), "UserShell"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8(output.stdout).ok()?;
        text.strip_prefix("UserShell:")
            .map(str::trim)
            .filter(|shell| !shell.is_empty())
            .map(PathBuf::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = username;
        None
    }
}

fn passwd_shell(username: &str) -> Option<PathBuf> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == username)
            .then(|| fields.nth(5))
            .flatten()
            .filter(|shell| !shell.trim().is_empty())
            .map(PathBuf::from)
    })
}

fn executable_shell(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

pub(crate) fn validate_shell_override(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("custom shell must be an absolute path".to_owned());
    }
    if !executable_shell(path) {
        return Err(format!(
            "custom shell {} must name an executable file",
            path.display()
        ));
    }
    Ok(path.to_owned())
}

fn pty_environment(shell: &Path) -> BTreeMap<OsString, OsString> {
    let mut environment = BTreeMap::new();
    for (key, value) in env::vars_os() {
        if (key == "LANG" || key.to_string_lossy().starts_with("LC_")) && !value.is_empty() {
            environment.insert(key, value);
        }
    }
    environment
        .entry(OsString::from("LANG"))
        .or_insert_with(default_lang);
    environment.insert(OsString::from("TERM"), OsString::from("xterm-256color"));
    environment.insert(OsString::from("COLORTERM"), OsString::from("truecolor"));
    environment.insert(OsString::from("SHELL"), shell.as_os_str().to_owned());
    environment
}

fn default_lang() -> OsString {
    #[cfg(target_os = "macos")]
    {
        OsString::from("en_US.UTF-8")
    }
    #[cfg(not(target_os = "macos"))]
    {
        OsString::from("C.UTF-8")
    }
}

fn parse_environment_capture(bytes: &[u8]) -> Result<BTreeMap<OsString, OsString>, String> {
    let start = find_bytes(bytes, ENV_START)
        .map(|index| index + ENV_START.len())
        .ok_or_else(|| "login shell did not emit the environment start marker".to_owned())?;
    let end = find_bytes(&bytes[start..], ENV_END)
        .map(|index| start + index)
        .ok_or_else(|| "login shell did not emit the environment end marker".to_owned())?;

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut environment = BTreeMap::new();
        for entry in bytes[start..end].split(|byte| *byte == 0) {
            if entry.is_empty() {
                continue;
            }
            let Some(separator) = entry.iter().position(|byte| *byte == b'=') else {
                continue;
            };
            environment.insert(
                OsString::from_vec(entry[..separator].to_vec()),
                OsString::from_vec(entry[separator + 1..].to_vec()),
            );
        }
        Ok(environment)
    }
    #[cfg(not(unix))]
    {
        let _ = (bytes, start, end);
        Ok(BTreeMap::new())
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inference_precedence_is_environment_then_account_then_passwd() {
        let environment = infer_shell_from(
            Some(PathBuf::from("/bin/sh")),
            Some(PathBuf::from("/bin/bash")),
            None,
        );
        assert_eq!(environment.path, Path::new("/bin/sh"));
        assert_eq!(environment.source, "SHELL");

        let account = infer_shell_from(
            Some(PathBuf::from("relative-shell")),
            Some(PathBuf::from("/bin/sh")),
            Some(PathBuf::from("/bin/bash")),
        );
        assert_eq!(account.path, Path::new("/bin/sh"));
        assert_eq!(account.source, "macOS account");
    }

    #[test]
    fn captured_environment_ignores_profile_output_and_preserves_values() {
        let mut bytes = b"profile said hello\n".to_vec();
        bytes.extend_from_slice(ENV_START);
        bytes.extend_from_slice(b"PATH=/profile/bin:/usr/bin\0QUOTED=two words and ' apostrophe\0");
        bytes.extend_from_slice(ENV_END);
        bytes.extend_from_slice(b"later noise\n");
        let environment = parse_environment_capture(&bytes).unwrap();
        assert_eq!(
            environment.get(OsStr::new("PATH")),
            Some(&OsString::from("/profile/bin:/usr/bin"))
        );
        assert_eq!(
            environment.get(OsStr::new("QUOTED")),
            Some(&OsString::from("two words and ' apostrophe"))
        );
    }

    #[test]
    fn invalid_override_is_visible_and_pty_defaults_are_complete() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.yml");
        fs::write(
            &config,
            "terminal:\n  shell: /definitely/missing/workman-shell\n",
        )
        .unwrap();
        let resolved = UserEnvironmentResolver::new(config).resolve();
        assert!(!resolved.info().using_override);
        assert_eq!(
            resolved.info().configured_shell.as_deref(),
            Some("/definitely/missing/workman-shell")
        );
        assert!(
            resolved
                .info()
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("using inferred shell"))
        );
        assert_eq!(
            resolved.pty_environment().get(OsStr::new("TERM")),
            Some(&OsString::from("xterm-256color"))
        );
        assert_eq!(
            resolved.pty_environment().get(OsStr::new("COLORTERM")),
            Some(&OsString::from("truecolor"))
        );
        assert!(resolved.pty_environment().contains_key(OsStr::new("LANG")));
    }
}
