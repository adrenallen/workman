//! Resolve the user's login shell and the environment shared by every spawned runtime.

use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
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
const DEGRADED_CAPTURE_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

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
    captured_at: Instant,
}

#[derive(Debug, Default)]
struct UserEnvironmentCaptureCache {
    cached: Option<CachedUserEnvironment>,
    refresh_in_progress: bool,
}

/// A resolved shell choice plus the environment policy applied to spawned PTYs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedUserEnvironment {
    info: UserEnvironmentInfo,
    pty_environment: BTreeMap<OsString, OsString>,
    interactive_terminal_environment: BTreeMap<OsString, OsString>,
    capture: CapturedUserEnvironment,
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

    /// Terminal shells source their rc files themselves, so do not pre-inject the captured PATH
    /// and let the rc prepend it a second time.
    pub fn interactive_terminal_environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.interactive_terminal_environment
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
        self.capture
            .non_interactive
            .clone()
            .unwrap_or_else(|| Err("non-interactive login environment was not cached".to_owned()))
    }

    /// Return the cached environment captured from the user's interactive login shell, or from
    /// the bounded fallback chain when that capture failed.
    pub fn login_environment(&self) -> Result<BTreeMap<OsString, OsString>, String> {
        Ok(self.capture.environment.clone())
    }

    /// Resolve the complete environment for non-PTY subprocesses such as Git, GitHub CLI,
    /// Herd, and desktop openers. The captured environment supplies user variables and PATH;
    /// Workman's stable LANG/LC_*, TERM, COLORTERM, and SHELL baseline wins last.
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
    capture_cache: Arc<Mutex<UserEnvironmentCaptureCache>>,
    capture_timeout: Duration,
    degraded_retry_cooldown: Duration,
}

impl UserEnvironmentResolver {
    pub fn new(config_path: impl Into<PathBuf>) -> Self {
        Self {
            config_path: config_path.into(),
            capture_cache: Arc::new(Mutex::new(UserEnvironmentCaptureCache::default())),
            capture_timeout: LOGIN_ENVIRONMENT_TIMEOUT,
            degraded_retry_cooldown: DEGRADED_CAPTURE_RETRY_COOLDOWN,
        }
    }

    /// Resolve immediately from the shared cache. A cold or expired cache schedules capture on
    /// Tokio's blocking pool and serves the daemon environment until that capture completes.
    pub fn resolve(&self) -> ResolvedUserEnvironment {
        self.resolve_with_capture(false)
    }

    /// Perform a capture on the calling thread. Tests and blocking-pool workers use this method;
    /// async daemon/control paths must call `prewarm` instead.
    pub fn refresh(&self) -> ResolvedUserEnvironment {
        self.resolve_with_capture(true)
    }

    /// Prewarm or refresh the active shell without blocking the async runtime or a registry lock.
    pub fn prewarm(&self) {
        self.schedule_refresh(false);
    }

    /// Force an asynchronous re-capture, for example immediately after the selected shell changes.
    pub fn refresh_async(&self) {
        self.schedule_refresh(true);
    }

    fn resolve_with_capture(&self, refresh: bool) -> ResolvedUserEnvironment {
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
        let capture = if refresh {
            self.capture(active_shell)
        } else {
            self.cached_capture(active_shell)
        };
        let interactive_terminal_environment = pty_environment(active_shell);
        let mut pty_environment = interactive_terminal_environment.clone();
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
            interactive_terminal_environment,
            info,
            capture,
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    fn capture(&self, shell: &Path) -> CapturedUserEnvironment {
        let capture = capture_user_environment(shell, self.capture_timeout);
        let mut cache = self
            .capture_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.cached = Some(CachedUserEnvironment {
            shell: shell.to_owned(),
            capture: capture.clone(),
            captured_at: Instant::now(),
        });
        cache.refresh_in_progress = false;
        capture
    }

    fn cached_capture(&self, shell: &Path) -> CapturedUserEnvironment {
        let cached = self
            .capture_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .cached
            .as_ref()
            .filter(|cached| cached.shell == shell)
            .map(|cached| cached.capture.clone());
        self.prewarm();
        cached.unwrap_or_else(|| pending_user_environment_capture(shell))
    }

    fn schedule_refresh(&self, force: bool) {
        let shell = self.active_shell();
        let mut cache = self
            .capture_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let due = force
            || cache
                .cached
                .as_ref()
                .filter(|cached| cached.shell == shell)
                .is_none_or(|cached| {
                    cached.capture.mode != EnvironmentCaptureMode::InteractiveLogin
                        && cached.captured_at.elapsed() >= self.degraded_retry_cooldown
                });
        if !due || cache.refresh_in_progress {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        cache.refresh_in_progress = true;
        drop(cache);
        let resolver = self.clone();
        runtime.spawn_blocking(move || {
            resolver.capture(&shell);
        });
    }

    fn active_shell(&self) -> PathBuf {
        let configured_shell = configured_shell(&self.config_path)
            .ok()
            .and_then(|(configured, _)| configured)
            .map(PathBuf::from)
            .filter(|path| executable_shell(path));
        configured_shell.unwrap_or_else(|| infer_shell().path)
    }

    #[cfg(test)]
    fn with_capture_timeout(mut self, timeout: Duration) -> Self {
        self.capture_timeout = timeout;
        self
    }

    #[cfg(test)]
    fn with_degraded_retry_cooldown(mut self, cooldown: Duration) -> Self {
        self.degraded_retry_cooldown = cooldown;
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
            non_interactive: Some(Ok(env::vars_os().collect())),
            error: None,
        };
    }

    #[cfg(not(windows))]
    {
        capture_user_environment_with_baseline(shell, &baseline, timeout)
    }
}

#[cfg(not(windows))]
fn capture_user_environment_with_baseline(
    shell: &Path,
    baseline: &BTreeMap<OsString, OsString>,
    timeout: Duration,
) -> CapturedUserEnvironment {
    let deadline = Instant::now() + timeout;
    let non_interactive = capture_shell_environment(
        shell,
        baseline,
        ShellCaptureKind::NonInteractiveLogin,
        remaining_until(deadline),
    );
    let interactive_baseline = non_interactive.as_ref().unwrap_or(baseline);
    let interactive_kind = if is_bash(shell) {
        // Bash login shells do not read ~/.bashrc. Capture the login profile first, then feed
        // that environment into a real non-login interactive shell so both rc chains apply.
        ShellCaptureKind::Interactive
    } else {
        ShellCaptureKind::InteractiveLogin
    };
    let interactive = capture_shell_environment(
        shell,
        interactive_baseline,
        interactive_kind,
        remaining_until(deadline),
    );
    match interactive.clone() {
        Ok(environment) => CapturedUserEnvironment {
            environment,
            mode: EnvironmentCaptureMode::InteractiveLogin,
            interactive,
            non_interactive: Some(non_interactive),
            error: None,
        },
        Err(interactive_error) => match non_interactive.clone() {
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
        },
    }
}

fn pending_user_environment_capture(shell: &Path) -> CapturedUserEnvironment {
    let reason = format!(
        "interactive environment capture for {} has not completed; refresh Runtime Doctor or restart Workman to re-capture it",
        shell.display()
    );
    CapturedUserEnvironment {
        environment: env::vars_os().collect(),
        mode: EnvironmentCaptureMode::DaemonFallback,
        interactive: Err(reason.clone()),
        non_interactive: Some(Err(reason.clone())),
        error: Some(reason),
    }
}

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug)]
enum ShellCaptureKind {
    InteractiveLogin,
    Interactive,
    NonInteractiveLogin,
}

#[cfg(not(windows))]
impl ShellCaptureKind {
    fn label(self) -> &'static str {
        match self {
            Self::InteractiveLogin => "interactive login",
            Self::Interactive => "interactive",
            Self::NonInteractiveLogin => "non-interactive login",
        }
    }
}

#[cfg(not(windows))]
fn is_bash(shell: &Path) -> bool {
    shell
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name == "bash" || name.eq_ignore_ascii_case("bash.exe"))
}

#[cfg(not(windows))]
fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

#[cfg(not(windows))]
fn capture_shell_environment(
    shell: &Path,
    baseline: &BTreeMap<OsString, OsString>,
    kind: ShellCaptureKind,
    timeout: Duration,
) -> Result<BTreeMap<OsString, OsString>, String> {
    let capture_command = concat!(
        "printf '\\036WORKMAN_ENV_START\\037'; ",
        "/usr/bin/env -0; ",
        "workman_env_status=$?; ",
        "printf '\\036WORKMAN_ENV_END\\037'; ",
        "exit $workman_env_status",
    );
    let mode = kind.label();
    if timeout.is_zero() {
        return Err(format!(
            "{mode} shell {} timed out before capture started",
            shell.display()
        ));
    }
    let mut command = Command::new(shell);
    match kind {
        ShellCaptureKind::InteractiveLogin => {
            command.args(["-l", "-i"]);
        }
        ShellCaptureKind::Interactive => {
            command.arg("-i");
        }
        ShellCaptureKind::NonInteractiveLogin => {
            command.arg("-l");
        }
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
    // cleanup kill rc-file descendants such as `sleep`, `read`, or `exec tmux` helpers. This
    // deliberately kills the whole capture group: rc-started background work is an accepted
    // trade-off for a hard upper bound and must not outlive a failed environment probe.
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
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = Vec::new();
        let _ = stdout_sender.send(stdout.read_to_end(&mut bytes).map(|_| bytes));
    });
    let (stderr_sender, stderr_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        let _ = stderr_sender.send(stderr.read_to_end(&mut bytes).map(|_| bytes));
    });
    let started = Instant::now();
    let deadline = started + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(remaining_until(deadline).min(Duration::from_millis(10)));
            }
            Ok(None) => {
                terminate_capture_group(&mut child);
                return Err(format!(
                    "{mode} shell {} timed out after {}ms",
                    shell.display(),
                    timeout.as_millis()
                ));
            }
            Err(error) => {
                terminate_capture_group(&mut child);
                return Err(format!(
                    "could not wait for {mode} shell {}: {error}",
                    shell.display()
                ));
            }
        }
    };
    let stdout =
        receive_capture_stream(&stdout_receiver, deadline, mode, "stdout").inspect_err(|_| {
            terminate_capture_group(&mut child);
        })?;
    let stderr =
        receive_capture_stream(&stderr_receiver, deadline, mode, "stderr").inspect_err(|_| {
            terminate_capture_group(&mut child);
        })?;
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

#[cfg(not(windows))]
fn receive_capture_stream(
    receiver: &mpsc::Receiver<io::Result<Vec<u8>>>,
    deadline: Instant,
    mode: &str,
    stream: &str,
) -> Result<Vec<u8>, String> {
    match receiver.recv_timeout(remaining_until(deadline)) {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(error)) => Err(format!("could not read {mode} shell {stream}: {error}")),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "{mode} shell {stream} remained open past the capture deadline"
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("{mode} shell {stream} reader stopped unexpectedly"))
        }
    }
}

#[cfg(not(windows))]
fn terminate_capture_group(child: &mut std::process::Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
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
            if !valid_environment_name(&entry[..separator]) {
                continue;
            }
            environment.insert(
                OsString::from_vec(entry[..separator].to_vec()),
                OsString::from_vec(entry[separator + 1..].to_vec()),
            );
        }
        for required in ["PATH", "HOME"] {
            if !environment.contains_key(OsStr::new(required)) {
                return Err(format!(
                    "login shell environment did not contain required variable {required}"
                ));
            }
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
fn valid_environment_name(name: &[u8]) -> bool {
    name.first()
        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphabetic())
        && name
            .iter()
            .all(|byte| *byte == b'_' || byte.is_ascii_alphanumeric())
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
        bytes.extend_from_slice(
            b"PATH=/profile/bin:/usr/bin\0HOME=/Users/example\0QUOTED=two words and ' apostrophe\0",
        );
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
    fn captured_environment_rejects_missing_required_variables_and_malformed_chatter() {
        let mut empty = ENV_START.to_vec();
        empty.extend_from_slice(ENV_END);
        assert!(
            parse_environment_capture(&empty)
                .unwrap_err()
                .contains("required variable PATH")
        );

        let mut bytes = ENV_START.to_vec();
        bytes.extend_from_slice(
            b"banner\nPATH=/corrupted\0HOME=/Users/example\0PATH=/valid\0BAD-NAME=value\0",
        );
        bytes.extend_from_slice(ENV_END);
        let environment = parse_environment_capture(&bytes).unwrap();
        assert_eq!(
            environment.get(OsStr::new("PATH")),
            Some(&OsString::from("/valid"))
        );
        assert!(!environment.contains_key(OsStr::new("banner\nPATH")));
        assert!(!environment.contains_key(OsStr::new("BAD-NAME")));
    }

    #[cfg(unix)]
    #[test]
    fn empty_capture_falls_back_instead_of_being_cached_as_success() {
        let temp = tempfile::tempdir().unwrap();
        let shell = temp.path().join("empty-environment-shell");
        fs::write(
            &shell,
            concat!(
                "#!/bin/sh\n",
                "printf '\\036WORKMAN_ENV_START\\037'\n",
                "printf '\\036WORKMAN_ENV_END\\037'\n",
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
        let resolved = resolver.refresh();
        assert_eq!(
            resolved.capture_mode(),
            EnvironmentCaptureMode::DaemonFallback
        );
        assert!(
            resolved
                .info()
                .capture_error
                .as_deref()
                .is_some_and(|error| error.contains("required variable PATH"))
        );
        assert_ne!(
            resolver.resolve().capture_mode(),
            EnvironmentCaptureMode::InteractiveLogin
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
        let first = resolver.refresh();
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
        assert!(
            !first
                .interactive_terminal_environment()
                .contains_key(OsStr::new("PATH")),
            "interactive terminals source their rc and must not receive the captured PATH twice"
        );
        assert_eq!(second.info().resolved_path, expected_path);
        assert_eq!(fs::read_to_string(count).unwrap(), "xx");
    }

    #[cfg(unix)]
    #[test]
    fn cold_hot_path_resolve_never_executes_the_capture_shell() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("capture-ran");
        let shell = temp.path().join("blocking-shell");
        fs::write(
            &shell,
            format!("#!/bin/sh\nprintf ran > {:?}\n/bin/sleep 60\n", marker),
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
        let resolved = UserEnvironmentResolver::new(config).resolve();
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(
            resolved.capture_mode(),
            EnvironmentCaptureMode::DaemonFallback
        );
        assert!(!marker.exists());
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
            .with_capture_timeout(Duration::from_millis(750))
            .refresh();
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
                .is_some_and(|error| error.contains("timed out")),
            "{:?}",
            resolved.info()
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

    #[cfg(unix)]
    #[test]
    fn background_fd_holder_cannot_extend_the_capture_deadline() {
        let temp = tempfile::tempdir().unwrap();
        let shell = temp.path().join("fd-holder-shell");
        fs::write(
            &shell,
            concat!(
                "#!/bin/sh\n",
                "interactive=0\n",
                "for arg in \"$@\"; do [ \"$arg\" = -i ] && interactive=1; done\n",
                "if [ \"$interactive\" = 1 ]; then /bin/sleep 60 & fi\n",
                "export PATH=/fallback/bin:/usr/bin:/bin\n",
                "while [ \"$#\" -gt 0 ]; do\n",
                "  if [ \"$1\" = -c ]; then shift; exec /bin/sh -c \"$1\"; fi\n",
                "  shift\n",
                "done\n",
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
            .with_capture_timeout(Duration::from_millis(750))
            .refresh();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            resolved.capture_mode(),
            EnvironmentCaptureMode::NonInteractiveLoginFallback,
            "{:?}",
            resolved.info()
        );
        assert!(
            resolved
                .info()
                .capture_error
                .as_deref()
                .is_some_and(|error| error.contains("remained open past the capture deadline"))
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn degraded_capture_retries_after_the_cooldown() {
        let temp = tempfile::tempdir().unwrap();
        let block = temp.path().join("block-interactive");
        fs::write(&block, "blocked").unwrap();
        let shell = temp.path().join("retry-shell");
        fs::write(
            &shell,
            format!(
                concat!(
                    "#!/bin/sh\n",
                    "interactive=0\n",
                    "for arg in \"$@\"; do [ \"$arg\" = -i ] && interactive=1; done\n",
                    "if [ \"$interactive\" = 1 ] && [ -f {:?} ]; then exit 42; fi\n",
                    "if [ \"$interactive\" = 1 ]; then export PATH=/retried/bin:/usr/bin:/bin; else export PATH=/fallback/bin:/usr/bin:/bin; fi\n",
                    "while [ \"$#\" -gt 0 ]; do\n",
                    "  if [ \"$1\" = -c ]; then shift; exec /bin/sh -c \"$1\"; fi\n",
                    "  shift\n",
                    "done\n",
                ),
                block,
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
        let resolver = UserEnvironmentResolver::new(config)
            .with_capture_timeout(Duration::from_millis(500))
            .with_degraded_retry_cooldown(Duration::ZERO);
        assert_eq!(
            resolver.refresh().capture_mode(),
            EnvironmentCaptureMode::NonInteractiveLoginFallback
        );

        fs::remove_file(block).unwrap();
        assert_eq!(
            resolver.resolve().capture_mode(),
            EnvironmentCaptureMode::NonInteractiveLoginFallback
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let resolved = resolver.resolve();
            if resolved.capture_mode() == EnvironmentCaptureMode::InteractiveLogin {
                assert_eq!(resolved.info().resolved_path, "/retried/bin:/usr/bin:/bin");
                break;
            }
            assert!(Instant::now() < deadline, "retry did not refresh the cache");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(unix)]
    #[test]
    fn real_bash_capture_merges_login_profile_and_bashrc() {
        let bash = Path::new("/bin/bash");
        if !bash.is_file() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let profile_bin = temp.path().join("profile-bin");
        let rc_bin = temp.path().join("rc-bin");
        fs::create_dir(&profile_bin).unwrap();
        fs::create_dir(&rc_bin).unwrap();
        fs::write(
            temp.path().join(".bash_profile"),
            format!(
                "export WORKMAN_BASH_PROFILE=loaded\nexport PATH={}:$PATH\n",
                profile_bin.display()
            ),
        )
        .unwrap();
        fs::write(
            temp.path().join(".bashrc"),
            format!(
                "export WORKMAN_BASHRC=loaded\nexport PATH={}:$PATH\n",
                rc_bin.display()
            ),
        )
        .unwrap();
        let mut baseline = pty_environment(bash);
        baseline.insert(OsString::from("HOME"), temp.path().as_os_str().to_owned());
        baseline.insert(OsString::from("PATH"), OsString::from("/usr/bin:/bin"));

        let captured =
            capture_user_environment_with_baseline(bash, &baseline, Duration::from_secs(4));
        assert_eq!(captured.mode, EnvironmentCaptureMode::InteractiveLogin);
        let non_interactive = captured.non_interactive.unwrap().unwrap();
        let interactive = captured.interactive.unwrap();

        assert_eq!(
            non_interactive.get(OsStr::new("WORKMAN_BASH_PROFILE")),
            Some(&OsString::from("loaded"))
        );
        assert!(!non_interactive.contains_key(OsStr::new("WORKMAN_BASHRC")));
        assert_eq!(
            interactive.get(OsStr::new("WORKMAN_BASH_PROFILE")),
            Some(&OsString::from("loaded"))
        );
        assert_eq!(
            interactive.get(OsStr::new("WORKMAN_BASHRC")),
            Some(&OsString::from("loaded"))
        );
        assert!(interactive.get(OsStr::new("PATH")).is_some_and(|path| {
            path.to_string_lossy()
                .starts_with(&*rc_bin.to_string_lossy())
        }));
    }

    #[cfg(unix)]
    #[test]
    fn command_environment_pins_the_terminal_locale_overlay() {
        let shell = Path::new("/bin/sh");
        let terminal = pty_environment(shell);
        let mut captured = env::vars_os().collect::<BTreeMap<_, _>>();
        captured.insert(OsString::from("LANG"), OsString::from("profile_LANG"));
        captured.insert(OsString::from("TERM"), OsString::from("profile_TERM"));
        let resolved = ResolvedUserEnvironment {
            info: UserEnvironmentInfo {
                active_shell: shell.display().to_string(),
                configured_shell: None,
                inferred_shell: shell.display().to_string(),
                inferred_from: "test".to_owned(),
                using_override: false,
                capture_mode: EnvironmentCaptureMode::InteractiveLogin,
                resolved_path: String::new(),
                capture_error: None,
                warning: None,
            },
            pty_environment: terminal.clone(),
            interactive_terminal_environment: terminal.clone(),
            capture: CapturedUserEnvironment {
                environment: captured,
                mode: EnvironmentCaptureMode::InteractiveLogin,
                interactive: Ok(BTreeMap::new()),
                non_interactive: Some(Ok(BTreeMap::new())),
                error: None,
            },
        };

        let command = resolved.command_environment();
        assert_eq!(
            command.get(OsStr::new("LANG")),
            terminal.get(OsStr::new("LANG"))
        );
        assert_eq!(
            command.get(OsStr::new("TERM")),
            terminal.get(OsStr::new("TERM"))
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
        let resolved = UserEnvironmentResolver::new(config).refresh();
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
