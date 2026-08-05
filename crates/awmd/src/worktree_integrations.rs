//! Optional integrations layered onto the core Git-worktree engine.
//!
//! Git lifecycle ownership stays in `worktrees`: Herd only parks a parent,
//! GitHub reads are cached/optional, and `.env` handling never shells out.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    process::Output,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;
use tokio::{process::Command, sync::Mutex, time::timeout};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const PR_CACHE_TTL_SECONDS: i64 = 300;

#[derive(Clone, Debug, Serialize)]
pub struct HerdView {
    pub available: bool,
    pub parked: bool,
    pub tld: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PullRequestView {
    pub number: u64,
    pub state: &'static str,
    pub url: String,
    pub checks: &'static str,
    pub mergeable: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct PullRequestCacheView {
    pub available: bool,
    pub checked_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeHealth {
    pub summary: String,
    pub all_required_ready: bool,
    pub checks: Vec<WorktreeHealthCheck>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeHealthCheck {
    pub id: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub status: &'static str,
    pub detail: String,
    pub version: Option<String>,
    pub fix_hint: Option<String>,
}

#[derive(Debug)]
pub struct IntegrationError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone)]
struct PrCacheEntry {
    checked_at: i64,
    branches: HashMap<String, PullRequestView>,
    error: Option<String>,
}

static PR_CACHE: OnceLock<Mutex<HashMap<PathBuf, PrCacheEntry>>> = OnceLock::new();

pub(crate) async fn herd_for_root(
    managed_root: &Path,
    park_when_needed: bool,
) -> Result<HerdView, IntegrationError> {
    if executable_on_path("herd").is_none() {
        return Ok(HerdView {
            available: false,
            parked: false,
            tld: None,
            error: None,
        });
    }
    let tld = run_text("herd", ["tld"])
        .await
        .map_err(|message| IntegrationError {
            code: "herd_error",
            message: format!("Laravel Herd is installed but its TLD could not be read: {message}"),
        })?;
    if tld.is_empty() {
        return Err(IntegrationError {
            code: "herd_error",
            message: "Laravel Herd returned an empty site TLD".into(),
        });
    }
    let mut parked = herd_paths()
        .await
        .map_err(|message| IntegrationError {
            code: "herd_error",
            message: format!("Laravel Herd's parked paths could not be read: {message}"),
        })?
        .iter()
        .any(|path| same_path(path, managed_root));
    if park_when_needed && !parked {
        let root = managed_root.to_string_lossy().into_owned();
        run_text("herd", ["park", root.as_str()])
            .await
            .map_err(|message| IntegrationError {
                code: "herd_error",
                message: format!(
                    "Laravel Herd could not park {}: {message}",
                    managed_root.display()
                ),
            })?;
        parked = true;
    }
    Ok(HerdView {
        available: true,
        parked,
        tld: Some(tld),
        error: None,
    })
}

pub(crate) fn site_url(site_name: &str, herd: &HerdView) -> Option<String> {
    herd.available
        .then_some(())
        .filter(|_| herd.parked)
        .and_then(|_| herd.tld.as_ref())
        .map(|tld| format!("http://{site_name}.{tld}"))
}

pub(crate) async fn pull_requests(
    repository: &Path,
    refresh: bool,
) -> (HashMap<String, PullRequestView>, PullRequestCacheView) {
    let now = unix_time();
    let cache = PR_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if !refresh {
        if let Some(hit) = cache.lock().await.get(repository).cloned() {
            if now.saturating_sub(hit.checked_at) < PR_CACHE_TTL_SECONDS {
                return cache_result(hit);
            }
        }
    }
    let entry = match fetch_pull_requests(repository).await {
        Ok(branches) => PrCacheEntry {
            checked_at: now,
            branches,
            error: None,
        },
        Err(error) => PrCacheEntry {
            checked_at: now,
            branches: HashMap::new(),
            error: Some(error),
        },
    };
    cache
        .lock()
        .await
        .insert(repository.to_path_buf(), entry.clone());
    cache_result(entry)
}

fn cache_result(entry: PrCacheEntry) -> (HashMap<String, PullRequestView>, PullRequestCacheView) {
    let view = PullRequestCacheView {
        available: entry.error.is_none(),
        checked_at: Some(entry.checked_at),
        expires_at: Some(entry.checked_at + PR_CACHE_TTL_SECONDS),
        error: entry.error.clone(),
    };
    (entry.branches, view)
}

async fn fetch_pull_requests(
    repository: &Path,
) -> Result<HashMap<String, PullRequestView>, String> {
    if executable_on_path("gh").is_none() {
        return Err("GitHub CLI is not installed; PR status is unavailable".into());
    }
    let output = command_output(Command::new("gh").current_dir(repository).args([
        "pr",
        "list",
        "--state",
        "all",
        "--limit",
        "200",
        "--json",
        "number,state,isDraft,headRefName,url,mergeable,statusCheckRollup",
    ]))
    .await
    .map_err(|error| format!("could not run gh: {error}"))?;
    if !output.status.success() {
        return Err(clean_command_error(&output));
    }
    parse_pull_requests(&output.stdout)
}

fn parse_pull_requests(bytes: &[u8]) -> Result<HashMap<String, PullRequestView>, String> {
    let values: Vec<Value> = serde_json::from_slice(bytes)
        .map_err(|error| format!("gh returned invalid PR JSON: {error}"))?;
    let mut result = HashMap::new();
    for value in values {
        let Some(branch) = value.get("headRefName").and_then(Value::as_str) else {
            continue;
        };
        let state = if value.get("isDraft").and_then(Value::as_bool) == Some(true) {
            "draft"
        } else {
            match value.get("state").and_then(Value::as_str).unwrap_or("") {
                "OPEN" => "open",
                "MERGED" => "merged",
                _ => "closed",
            }
        };
        let candidate = PullRequestView {
            number: value.get("number").and_then(Value::as_u64).unwrap_or(0),
            state,
            url: value
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            checks: checks_state(value.get("statusCheckRollup")),
            mergeable: match value.get("mergeable").and_then(Value::as_str).unwrap_or("") {
                "MERGEABLE" => "mergeable",
                "CONFLICTING" => "conflicting",
                _ => "unknown",
            },
        };
        let replace = result.get(branch).is_none_or(|current: &PullRequestView| {
            pr_rank(candidate.state) > pr_rank(current.state)
        });
        if replace {
            result.insert(branch.to_owned(), candidate);
        }
    }
    Ok(result)
}

fn checks_state(value: Option<&Value>) -> &'static str {
    let Some(checks) = value.and_then(Value::as_array) else {
        return "none";
    };
    if checks.is_empty() {
        return "none";
    }
    let mut pending = false;
    for check in checks {
        let conclusion = check
            .get("conclusion")
            .or_else(|| check.get("state"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let status = check.get("status").and_then(Value::as_str).unwrap_or("");
        if matches!(
            conclusion,
            "FAILURE"
                | "ERROR"
                | "CANCELLED"
                | "TIMED_OUT"
                | "ACTION_REQUIRED"
                | "failure"
                | "error"
        ) {
            return "failing";
        }
        if conclusion.is_empty()
            || matches!(status, "QUEUED" | "IN_PROGRESS" | "PENDING" | "EXPECTED")
        {
            pending = true;
        }
    }
    if pending { "pending" } else { "passing" }
}

fn pr_rank(state: &str) -> u8 {
    match state {
        "open" => 4,
        "draft" => 3,
        "merged" => 2,
        _ => 1,
    }
}

pub(crate) async fn health(managed_roots: Vec<PathBuf>) -> WorktreeHealth {
    let roots = if managed_roots.is_empty() {
        vec![super::worktrees::default_worktree_root()]
    } else {
        managed_roots
    };
    let mut checks = vec![
        command_health(
            "git",
            "Git",
            true,
            &["--version"],
            "Install Git with Xcode Command Line Tools (`xcode-select --install`) or Homebrew.",
        )
        .await,
    ];

    let mut gh = command_health(
        "gh",
        "GitHub CLI",
        false,
        &["--version"],
        "Install with `brew install gh`, then authenticate with `gh auth login`.",
    )
    .await;
    if gh.status == "ready" {
        match run_text("gh", ["auth", "status"]).await {
            Ok(_) => gh.detail = "Installed and authenticated; PR status is available.".into(),
            Err(error) => {
                gh.status = "attention";
                gh.detail = format!("Installed, but not authenticated: {error}");
                gh.fix_hint = Some("Run `gh auth login` to enable PR and merge status.".into());
            }
        }
    }
    checks.push(gh);

    let mut herd = command_health(
        "herd",
        "Laravel Herd",
        false,
        &["--version"],
        "Install Laravel Herd to serve managed worktrees at http://<name>.test.",
    )
    .await;
    if herd.status == "ready" {
        match (herd_paths().await, run_text("herd", ["tld"]).await) {
            (Ok(paths), Ok(tld)) => {
                let unparked = roots
                    .iter()
                    .filter(|root| !paths.iter().any(|path| same_path(path, root)))
                    .count();
                if unparked == 0 {
                    herd.detail = format!("Available on .{tld}; every managed root is parked.");
                } else {
                    herd.status = "attention";
                    herd.detail =
                        format!("Available on .{tld}; {unparked} managed root(s) are not parked.");
                    herd.fix_hint = Some("Creating the next managed worktree parks its root automatically, or run `herd park <managed-root>`.".into());
                }
            }
            (Err(error), _) => {
                herd.status = "attention";
                herd.detail = format!("Installed, but parked paths could not be read: {error}");
            }
            (_, Err(error)) => {
                herd.status = "attention";
                herd.detail = format!("Installed, but its TLD could not be read: {error}");
            }
        }
    }
    checks.push(herd);

    let root_issue = roots.iter().find_map(|root| root_health_error(root));
    checks.push(if let Some(detail) = root_issue {
        WorktreeHealthCheck {
            id: "managed_root", label: "Managed roots", required: true, status: "missing",
            detail, version: None,
            fix_hint: Some("Create the directory and grant your user write access, or set AWM_WORKTREE_ROOT to a writable parent.".into()),
        }
    } else {
        WorktreeHealthCheck {
            id: "managed_root", label: "Managed roots", required: true, status: "ready",
            detail: format!("{} managed root(s) are available or can be created.", roots.len()),
            version: None, fix_hint: None,
        }
    });

    let all_required_ready = checks
        .iter()
        .filter(|check| check.required)
        .all(|check| check.status == "ready");
    let ready = checks
        .iter()
        .filter(|check| check.status == "ready")
        .count();
    WorktreeHealth {
        summary: format!("{ready} of {} worktree integrations ready", checks.len()),
        all_required_ready,
        checks,
    }
}

async fn command_health(
    command: &'static str,
    label: &'static str,
    required: bool,
    version_args: &[&str],
    hint: &str,
) -> WorktreeHealthCheck {
    if executable_on_path(command).is_none() {
        return WorktreeHealthCheck {
            id: command,
            label,
            required,
            status: if required { "missing" } else { "attention" },
            detail: format!(
                "{label} {} not found on PATH.",
                if required {
                    "is required but was"
                } else {
                    "is optional and was"
                }
            ),
            version: None,
            fix_hint: Some(hint.into()),
        };
    }
    match run_text(command, version_args.iter().copied()).await {
        Ok(version) => WorktreeHealthCheck {
            id: command,
            label,
            required,
            status: "ready",
            detail: format!("{label} is available."),
            version: version.lines().next().map(str::to_owned),
            fix_hint: None,
        },
        Err(error) => WorktreeHealthCheck {
            id: command,
            label,
            required,
            status: if required { "missing" } else { "attention" },
            detail: format!("{label} could not run: {error}"),
            version: None,
            fix_hint: Some(hint.into()),
        },
    }
}

fn root_health_error(root: &Path) -> Option<String> {
    if root.exists() {
        if !root.is_dir() {
            return Some(format!("{} exists but is not a directory.", root.display()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(root).ok()?.permissions().mode() & 0o200 == 0 {
                return Some(format!("{} is not writable.", root.display()));
            }
        }
        return None;
    }
    let Some(parent) = root.ancestors().skip(1).find(|parent| parent.exists()) else {
        return Some(format!(
            "{} cannot be created because no parent exists.",
            root.display()
        ));
    };
    if !parent.is_dir() {
        Some(format!(
            "{} cannot be created because {} is not a directory.",
            root.display(),
            parent.display()
        ))
    } else {
        None
    }
}

async fn herd_paths() -> Result<Vec<PathBuf>, String> {
    let text = run_text("herd", ["paths"]).await?;
    serde_json::from_str::<Vec<String>>(&text)
        .map(|paths| paths.into_iter().map(PathBuf::from).collect())
        .map_err(|error| format!("invalid JSON from `herd paths`: {error}"))
}

async fn run_text<I, S>(command: &str, args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = command_output(Command::new(command).args(args))
        .await
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(clean_command_error(&output))
    }
}

async fn command_output(command: &mut Command) -> io::Result<Output> {
    timeout(COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "command timed out"))?
}

fn clean_command_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    }
}

fn executable_on_path(command: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(command))
            .find(|candidate| candidate.is_file())
    })
}

fn same_path(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| PathBuf::from(path.to_string_lossy().trim_end_matches('/')))
    };
    normalize(left) == normalize(right)
}

fn unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pr_priority_checks_and_mergeability() {
        let parsed = parse_pull_requests(br#"[
          {"number":1,"state":"CLOSED","isDraft":false,"headRefName":"feature","url":"https://example/1","mergeable":"UNKNOWN","statusCheckRollup":[]},
          {"number":2,"state":"OPEN","isDraft":true,"headRefName":"feature","url":"https://example/2","mergeable":"MERGEABLE","statusCheckRollup":[{"status":"IN_PROGRESS","conclusion":""}]},
          {"number":3,"state":"OPEN","isDraft":false,"headRefName":"ready","url":"https://example/3","mergeable":"CONFLICTING","statusCheckRollup":[{"status":"COMPLETED","conclusion":"FAILURE"}]}
        ]"#).unwrap();
        assert_eq!(parsed["feature"].number, 2);
        assert_eq!(parsed["feature"].state, "draft");
        assert_eq!(parsed["feature"].checks, "pending");
        assert_eq!(parsed["ready"].checks, "failing");
        assert_eq!(parsed["ready"].mergeable, "conflicting");
    }
}
