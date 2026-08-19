//! Resolve the user's login shell and the environment shared by every spawned runtime.

use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(not(windows))]
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    thread,
    time::Instant,
};

#[cfg(not(windows))]
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};

use serde::{Deserialize, Serialize};

use crate::user_config::{UserConfigError, parse_user_config};

#[cfg(not(windows))]
const ENV_START: &[u8] = b"\x1eWORKMAN_ENV_START\x1f";
#[cfg(not(windows))]
const ENV_END: &[u8] = b"\x1eWORKMAN_ENV_END\x1f";

const LOGIN_ENVIRONMENT_TIMEOUT: Duration = Duration::from_secs(10);

/// The shell-capture path that supplied the environment currently used by Workman.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentCaptureMode {
    InteractiveLogin,
    NonInteractiveLoginFallback,
    DaemonFallback,
    DaemonEnvironment,
}

impl EnvironmentCaptureMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InteractiveLogin => "interactive_login",
            Self::NonInteractiveLoginFallback => "non_interactive_login_fallback",
            Self::DaemonFallback => "daemon_fallback",
            Self::DaemonEnvironment => "daemon_environment",
        }
    }
}

/// User-facing description of Workman's active and inferred shell choice.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserEnvironmentInfo {
    pub active_shell: String,
    pub configured_shell: Option<String>,
    pub inferred_shell: String,
    pub inferred_from: String,
    pub using_override: bool,
    pub capture_mode: EnvironmentCaptureMode,
    pub resolved_path: String,
    pub capture_error: Option<String>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CapturedUserEnvironment {
    environment: BTreeMap<OsString, OsString>,
    mode: EnvironmentCaptureMode,
    interactive: Result<BTreeMap<OsString, OsString>, String>,
    non_interactive: Option<Result<BTreeMap<OsString, OsString>, String>>,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct CachedUserEnvironment {
    shell: PathBuf,
    capture: CapturedUserEnvironment,
}

/// A resolved shell choice plus the environment policy applied to spawned PTYs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedUserEnvironment {
    info: UserEnvironmentInfo,
    pty_environment: BTreeMap<OsString, OsString>,
    capture: CapturedUserEnvironment,
    capture_timeout: Duration,
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

    pub fn capture_mode(&self) -> EnvironmentCaptureMode {
        self.capture.mode
    }

    pub(crate) fn interactive_login_environment(
        &self,
    ) -> Result<BTreeMap<OsString, OsString>, String> {
        self.capture.interactive.clone()
    }

    pub(crate) fn non_interactive_login_environment(
        &self,
    ) -> Result<BTreeMap<OsString, OsString>, String> {
        if let Some(environment) = &self.capture.non_interactive {
            return environment.clone();
        }
        #[cfg(windows)]
        {
            return Ok(self.command_environment());
        }
        #[cfg(not(windows))]
        capture_shell_environment(
            self.active_shell(),
            &pty_environment(self.active_shell()),
            false,
            self.capture_timeout,
        )
    }

    /// Return the cached environment captured from the user's interactive login shell, or from
    /// the bounded fallback chain when that capture failed.
    pub fn login_environment(&self) -> Result<BTreeMap<OsString, OsString>, String> {
        Ok(self.capture.environment.clone())
    }

    /// Resolve the complete environment for non-PTY subprocesses such as Git, GitHub CLI,
    /// Herd, and desktop openers. If a profile cannot be inspected, preserve the daemon's
    /// inherited variables while still applying Workman's terminal-safe shell baseline.
    pub fn command_environment(&self) -> BTreeMap<OsString, OsString> {
        let mut environment = self.capture.environment.clone();
        environment.extend(self.pty_environment.clone());
        environment
    }
}

/// Resolves the shell from user settings and operating-system account metadata.
#[derive(Clone, Debug)]
pub struct UserEnvironmentResolver {
    config_path: PathBuf,
    capture_cache: Arc<Mutex<Option<CachedUserEnvironment>>>,
    capture_timeout: Duration,
}

impl UserEnvironmentResolver {
    pub fn new(config_path: impl Into<PathBuf>) -> Self {
        Self {
            config_path: config_path.into(),
            capture_cache: Arc::new(Mutex::new(None)),
            capture_timeout: LOGIN_ENVIRONMENT_TIMEOUT,
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
        let capture = self.capture(active_shell);
        let mut pty_environment = pty_environment(active_shell);
        if let Some(path) = capture.environment.get(OsStr::new("PATH")) {
            pty_environment.insert(OsString::from("PATH"), path.clone());
        }
        let resolved_path = capture
            .environment
            .get(OsStr::new("PATH"))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let info = UserEnvironmentInfo {
            active_shell: active_shell.to_string_lossy().into_owned(),
            configured_shell,
            inferred_shell: inferred.path.to_string_lossy().into_owned(),
            inferred_from: inferred.source.to_owned(),
            using_override: valid_override.is_some(),
            capture_mode: capture.mode,
            resolved_path,
            capture_error: capture.error.clone(),
            warning,
        };
        ResolvedUserEnvironment {
            pty_environment,
            info,
            capture,
            capture_timeout: self.capture_timeout,
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    fn capture(&self, shell: &Path) -> CapturedUserEnvironment {
        if let Some(cached) = self
            .capture_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .filter(|cached| cached.shell == shell)
        {
            return cached.capture.clone();
        }

        // Capture outside the cache lock. The first daemon-startup resolve warms this path;
        // concurrent cold callers may duplicate one capture instead of blocking a hot lock.
        let capture = capture_user_environment(shell, self.capture_timeout);
        let mut cache = self
            .capture_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.as_ref().filter(|cached| cached.shell == shell) {
            return cached.capture.clone();
        }
        *cache = Some(CachedUserEnvironment {
            shell: shell.to_owned(),
            capture: capture.clone(),
        });
        capture
    }

    #[cfg(test)]
    fn with_capture_timeout(mut self, timeout: Duration) -> Self {
        self.capture_timeout = timeout;
        self
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

    #[cfg(windows)]
    {
        windows_fallback_shell()
    }
    #[cfg(not(windows))]
    {
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
}

/// Prefer PowerShell 7, then Windows PowerShell, then the command processor.
///
/// `SHELL` and the configured override still win through the shared inference
/// order; this only chooses the default when nothing else names a shell.
#[cfg(windows)]
fn windows_fallback_shell() -> InferredShell {
    let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push((
            PathBuf::from(program_files)
                .join("PowerShell")
                .join("7")
                .join("pwsh.exe"),
            "PowerShell 7",
        ));
    }
    if let Some(system_root) = env::var_os("SystemRoot") {
        candidates.push((
            PathBuf::from(system_root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
            "Windows PowerShell",
        ));
    }
    if let Some(comspec) = env::var_os("COMSPEC") {
        candidates.push((PathBuf::from(comspec), "COMSPEC"));
    }
    for (path, source) in &candidates {
        if executable_shell(path) {
            return InferredShell {
                path: path.clone(),
                source: *source,
                warning: None,
            };
        }
    }
    let path = candidates
        .into_iter()
        .map(|(path, _)| path)
        .next()
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    InferredShell {
        warning: Some(format!(
            "Workman could not find PowerShell or the command processor; using fallback {}.",
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

fn capture_user_environment(shell: &Path, timeout: Duration) -> CapturedUserEnvironment {
    #[cfg(not(windows))]
    let baseline = pty_environment(shell);

    #[cfg(windows)]
    {
        let _ = (shell, timeout);
        let environment = env::vars_os().collect::<BTreeMap<_, _>>();
        return CapturedUserEnvironment {
            environment: environment.clone(),
            mode: EnvironmentCaptureMode::DaemonEnvironment,
            interactive: Ok(environment),
            non_interactive: None,
            error: None,
        };
    }

    #[cfg(not(windows))]
    {
        let interactive = capture_shell_environment(shell, &baseline, true, timeout);
        match interactive.clone() {
            Ok(environment) => CapturedUserEnvironment {
                environment,
                mode: EnvironmentCaptureMode::InteractiveLogin,
                interactive,
                non_interactive: None,
                error: None,
            },
            Err(interactive_error) => {
                let non_interactive = capture_shell_environment(shell, &baseline, false, timeout);
                match non_interactive.clone() {
                    Ok(environment) => CapturedUserEnvironment {
                        environment,
                        mode: EnvironmentCaptureMode::NonInteractiveLoginFallback,
                        interactive,
                        non_interactive: Some(non_interactive),
                        error: Some(format!(
                            "interactive login environment capture failed: {interactive_error}"
                        )),
                    },
                    Err(non_interactive_error) => CapturedUserEnvironment {
                        environment: env::vars_os().collect(),
                        mode: EnvironmentCaptureMode::DaemonFallback,
                        interactive,
                        non_interactive: Some(non_interactive),
                        error: Some(format!(
                            "interactive login environment capture failed: {interactive_error}; non-interactive fallback failed: {non_interactive_error}"
                        )),
                    },
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn capture_shell_environment(
    shell: &Path,
    baseline: &BTreeMap<OsString, OsString>,
    interactive: bool,
    timeout: Duration,
) -> Result<BTreeMap<OsString, OsString>, String> {
    let capture_command = concat!(
        "printf '\\036WORKMAN_ENV_START\\037'; ",
        "/usr/bin/env -0; ",
        "printf '\\036WORKMAN_ENV_END\\037'",
    );
    let mode = if interactive {
        "interactive login"
    } else {
        "non-interactive login"
    };
    let mut command = Command::new(shell);
    command.arg("-l");
    if interactive {
        command.arg("-i");
    }
    command
        .args(["-c", capture_command])
        .envs(baseline)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A daemon started manually can retain its terminal as a controlling TTY. Detach the
    // interactive capture into a fresh session so zsh/bash cannot stop themselves on background
    // job-control signals. The session leader is also its process-group leader, letting timeout
    // cleanup kill rc-file descendants such as `sleep`, `read`, or `exec tmux` helpers.
    // SAFETY: `setsid` is a single async-signal-safe syscall with no captured Rust state.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(|error| io::Error::from_raw_os_error(error as i32))
        });
    }
    let mut child = command.spawn().map_err(|error| {
        format!(
            "could not inspect {mode} environment with {}: {error}",
            shell.display()
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{mode} shell did not expose stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{mode} shell did not expose stderr"))?;
    let stdout_reader = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                if let Ok(pid) = i32::try_from(child.id()) {
                    let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!(
                    "{mode} shell {} timed out after {}ms",
                    shell.display(),
                    timeout.as_millis()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!(
                    "could not wait for {mode} shell {}: {error}",
                    shell.display()
                ));
            }
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| format!("{mode} shell stdout reader panicked"))?
        .map_err(|error| format!("could not read {mode} shell stdout: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| format!("{mode} shell stderr reader panicked"))?
        .map_err(|error| format!("could not read {mode} shell stderr: {error}"))?;
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| format!(": {}", line.chars().take(300).collect::<String>()))
            .unwrap_or_default();
        return Err(format!(
            "{mode} shell {} exited with {status}{detail}",
            shell.display()
        ));
    }
    parse_environment_capture(&stdout)
        .map_err(|error| format!("could not parse {mode} shell environment: {error}"))
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::{os::unix::fs::PermissionsExt, thread};

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn interactive_capture_is_cached_and_supplies_the_pty_path() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("interactive-bin");
        fs::create_dir(&bin).unwrap();
        let count = temp.path().join("capture-count");
        let shell = temp.path().join("fixture-shell");
        fs::write(
            &shell,
            format!(
                concat!(
                    "#!/bin/sh\n",
                    "printf x >> {:?}\n",
                    "for arg in \"$@\"; do\n",
                    "  if [ \"$arg\" = -i ]; then export PATH={:?}:/usr/bin:/bin; fi\n",
                    "done\n",
                    "while [ \"$#\" -gt 0 ]; do\n",
                    "  if [ \"$1\" = -c ]; then shift; exec /bin/sh -c \"$1\"; fi\n",
                    "  shift\n",
                    "done\n",
                ),
                count, bin,
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

        let resolver = UserEnvironmentResolver::new(config);
        let first = resolver.resolve();
        let second = resolver.resolve();
        let expected_path = format!("{}:/usr/bin:/bin", bin.display());
        assert_eq!(
            first.capture_mode(),
            EnvironmentCaptureMode::InteractiveLogin
        );
        assert_eq!(first.info().resolved_path, expected_path);
        assert_eq!(
            first.pty_environment().get(OsStr::new("PATH")),
            Some(&OsString::from(&expected_path))
        );
        assert_eq!(second.info().resolved_path, expected_path);
        assert_eq!(fs::read_to_string(count).unwrap(), "x");
    }

    #[cfg(unix)]
    #[test]
    fn blocking_interactive_capture_times_out_kills_its_group_and_falls_back() {
        let temp = tempfile::tempdir().unwrap();
        let child_pid = temp.path().join("blocking-child-pid");
        let shell = temp.path().join("blocking-shell");
        fs::write(
            &shell,
            format!(
                concat!(
                    "#!/bin/sh\n",
                    "for arg in \"$@\"; do\n",
                    "  if [ \"$arg\" = -i ]; then\n",
                    "    /bin/sleep 60 &\n",
                    "    printf '%s' \"$!\" > {:?}\n",
                    "    wait\n",
                    "  fi\n",
                    "done\n",
                    "export PATH=/fallback/bin:/usr/bin:/bin\n",
                    "while [ \"$#\" -gt 0 ]; do\n",
                    "  if [ \"$1\" = -c ]; then shift; exec /bin/sh -c \"$1\"; fi\n",
                    "  shift\n",
                    "done\n",
                ),
                child_pid,
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

        let started = Instant::now();
        let resolved = UserEnvironmentResolver::new(config)
            .with_capture_timeout(Duration::from_millis(300))
            .resolve();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            resolved.capture_mode(),
            EnvironmentCaptureMode::NonInteractiveLoginFallback,
            "{:?}",
            resolved.info()
        );
        assert_eq!(resolved.info().resolved_path, "/fallback/bin:/usr/bin:/bin");
        assert!(
            resolved
                .info()
                .capture_error
                .as_deref()
                .is_some_and(|error| error.contains("timed out after 300ms"))
        );

        let pid = fs::read_to_string(child_pid)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        for _ in 0..100 {
            if nix::sys::signal::kill(Pid::from_raw(pid), None).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("blocking capture child {pid} was not reaped");
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
