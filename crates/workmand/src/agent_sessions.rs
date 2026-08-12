//! Passive discovery of conversation IDs written by supported agent CLIs.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "macos")]
use std::process::Command;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionAdapter {
    Claude,
    Codex,
    Gemini,
    Grok,
    Kimi,
    OpenCode,
}

/// One spawn-time watermark plus the CLI-owned roots that may receive its session record.
#[derive(Clone, Debug)]
pub(crate) struct SessionCapture {
    adapter: SessionAdapter,
    working_dir: PathBuf,
    started_at_ms: i64,
    home: PathBuf,
    claude_config: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    grok_home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
}

impl SessionCapture {
    pub(crate) fn new(
        tool_type: &str,
        working_dir: &str,
        process_env: &BTreeMap<String, String>,
        started_at_ms: i64,
    ) -> Option<Self> {
        let adapter = adapter(tool_type)?;
        let variable = |name: &str| {
            process_env
                .get(name)
                .map(OsString::from)
                .or_else(|| env::var_os(name))
                .map(PathBuf::from)
        };
        let home = variable("HOME").or_else(dirs::home_dir)?;
        Some(Self {
            adapter,
            working_dir: PathBuf::from(working_dir),
            started_at_ms,
            home,
            claude_config: variable("CLAUDE_CONFIG_DIR"),
            codex_home: variable("CODEX_HOME"),
            grok_home: variable("GROK_HOME"),
            xdg_data_home: variable("XDG_DATA_HOME"),
        })
    }

    pub(crate) fn discover(&self) -> Result<Option<String>, String> {
        let result = match self.adapter {
            SessionAdapter::Claude => discover_claude(self),
            SessionAdapter::Codex => discover_codex(self),
            SessionAdapter::Gemini => discover_gemini(self),
            SessionAdapter::Grok => discover_grok(self),
            SessionAdapter::Kimi => discover_kimi(self),
            SessionAdapter::OpenCode => discover_opencode(self),
        };
        result.map_err(|error| error.to_string())
    }

    /// Discover the session file that is owned by this Workman process tree.
    ///
    /// Codex and Grok keep active session files open for the life of the TUI. That
    /// OS-level ownership is the only reliable discriminator when several
    /// sessions start in the same cwd at nearly the same time. Other adapters
    /// retain their existing cwd-and-watermark discovery until they expose an
    /// equally strong process-specific signal.
    pub(crate) fn discover_for_process(&self, root_pid: u32) -> Result<Option<String>, String> {
        let result = match self.adapter {
            SessionAdapter::Codex => discover_codex_for_process(self, root_pid),
            SessionAdapter::Grok => discover_grok_for_process(self, root_pid),
            _ => return self.discover(),
        };
        result.map_err(|error| error.to_string())
    }

    /// Codex and Grok continue-latest can select a concurrently-created sibling in the
    /// same cwd. Without an exact captured ID, a fresh launch is safer.
    pub(crate) fn supports_continue_latest_fallback(&self) -> bool {
        !matches!(self.adapter, SessionAdapter::Codex | SessionAdapter::Grok)
    }

    /// Resolve whether this CLI has any cwd-scoped session that its continue-latest
    /// command could select. Unsupported layouts stay unknown at construction time.
    pub(crate) fn latest_existing(&self) -> Result<Option<String>, String> {
        let mut unbounded = self.clone();
        unbounded.started_at_ms = i64::MIN;
        unbounded.discover()
    }

    #[cfg(test)]
    fn fixture(
        adapter: SessionAdapter,
        home: &Path,
        working_dir: &Path,
        started_at_ms: i64,
    ) -> Self {
        Self {
            adapter,
            working_dir: working_dir.to_owned(),
            started_at_ms,
            home: home.to_owned(),
            claude_config: None,
            codex_home: None,
            grok_home: None,
            xdg_data_home: None,
        }
    }
}

fn adapter(tool_type: &str) -> Option<SessionAdapter> {
    match tool_type
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "claude" | "claude_code" => Some(SessionAdapter::Claude),
        "codex" => Some(SessionAdapter::Codex),
        "gemini" | "gemini_cli" => Some(SessionAdapter::Gemini),
        "grok" | "grok_cli" | "grok_build" => Some(SessionAdapter::Grok),
        "kimi" | "kimi_code" => Some(SessionAdapter::Kimi),
        "opencode" | "open_code" => Some(SessionAdapter::OpenCode),
        _ => None,
    }
}

fn discover_claude(capture: &SessionCapture) -> io::Result<Option<String>> {
    let config = capture
        .claude_config
        .clone()
        .unwrap_or_else(|| capture.home.join(".claude"));
    let live = config.join("sessions");
    if let Some(session_id) = latest_session_file(&live, "json", capture.started_at_ms, |path| {
        let Some(value) = first_json_document(path)? else {
            return Ok(None);
        };
        if !value
            .get("cwd")
            .and_then(Value::as_str)
            .is_some_and(|cwd| working_dirs_match(cwd, &capture.working_dir))
        {
            return Ok(None);
        }
        Ok(value
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned))
    })? {
        return Ok(Some(session_id));
    }
    let root = config
        .join("projects")
        .join(claude_project_slug(&capture.working_dir));
    latest_session_file(&root, "jsonl", capture.started_at_ms, |path| {
        let value = first_json_line_matching(path, |value| {
            value
                .get("cwd")
                .and_then(Value::as_str)
                .is_some_and(|cwd| working_dirs_match(cwd, &capture.working_dir))
        })?;
        Ok(value
            .as_ref()
            .and_then(|value| value.get("sessionId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| value.and_then(|_| path.file_stem()?.to_str().map(str::to_owned))))
    })
}

fn discover_codex(capture: &SessionCapture) -> io::Result<Option<String>> {
    let root = codex_sessions_root(capture);
    latest_session_file(&root, "jsonl", capture.started_at_ms, |path| {
        codex_session_id(path, &capture.working_dir)
    })
}

fn discover_codex_for_process(
    capture: &SessionCapture,
    root_pid: u32,
) -> io::Result<Option<String>> {
    let root = codex_sessions_root(capture);
    let mut files = open_files_for_process_tree(root_pid)?
        .into_iter()
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
                && path_is_within(path, &root)
        })
        .filter_map(|path| {
            let modified = modified_millis(&path).ok()?;
            (modified >= capture.started_at_ms).then_some((path, modified))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(_, modified)| *modified);
    for (path, _) in files.into_iter().rev() {
        if let Some(session_id) = codex_session_id(&path, &capture.working_dir)? {
            return Ok(Some(session_id));
        }
    }
    Ok(None)
}

fn codex_sessions_root(capture: &SessionCapture) -> PathBuf {
    capture
        .codex_home
        .clone()
        .unwrap_or_else(|| capture.home.join(".codex"))
        .join("sessions")
}

fn codex_session_id(path: &Path, working_dir: &Path) -> io::Result<Option<String>> {
    let Some(value) = first_json_line(path)? else {
        return Ok(None);
    };
    let payload = value.get("payload").unwrap_or(&value);
    if !payload
        .get("cwd")
        .and_then(Value::as_str)
        .is_some_and(|cwd| working_dirs_match(cwd, working_dir))
    {
        return Ok(None);
    }
    Ok(payload
        .get("id")
        .or_else(|| payload.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn discover_grok(capture: &SessionCapture) -> io::Result<Option<String>> {
    let root = grok_sessions_root(capture);
    latest_session_file(&root, "json", capture.started_at_ms, |path| {
        if path.file_name().and_then(|name| name.to_str()) != Some("summary.json") {
            return Ok(None);
        }
        grok_session_id(path, &capture.working_dir)
    })
}

fn discover_grok_for_process(
    capture: &SessionCapture,
    root_pid: u32,
) -> io::Result<Option<String>> {
    let root = grok_sessions_root(capture);
    let mut files = open_files_for_process_tree(root_pid)?
        .into_iter()
        .filter(|path| path_is_within(path, &root))
        .filter_map(|path| {
            let modified = modified_millis(&path).ok()?;
            (modified >= capture.started_at_ms).then_some((path, modified))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(_, modified)| *modified);
    for (path, _) in files.into_iter().rev() {
        for directory in path.ancestors().skip(1) {
            let Some(parent) = directory.parent() else {
                break;
            };
            if parent == root || !path_is_within(directory, &root) {
                continue;
            }
            let summary = directory.join("summary.json");
            if summary.is_file()
                && let Some(session_id) = grok_session_id(&summary, &capture.working_dir)?
            {
                return Ok(Some(session_id));
            }
            if directory
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_uuid_like)
            {
                return Ok(directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned));
            }
        }
    }
    Ok(None)
}

fn grok_sessions_root(capture: &SessionCapture) -> PathBuf {
    capture
        .grok_home
        .clone()
        .unwrap_or_else(|| capture.home.join(".grok"))
        .join("sessions")
}

fn grok_session_id(path: &Path, working_dir: &Path) -> io::Result<Option<String>> {
    let Some(value) = first_json_document(path)? else {
        return Ok(None);
    };
    let info = value.get("info").unwrap_or(&value);
    if !info
        .get("cwd")
        .or_else(|| value.get("cwd"))
        .and_then(Value::as_str)
        .is_some_and(|cwd| working_dirs_match(cwd, working_dir))
    {
        return Ok(None);
    }
    Ok(info
        .get("session_id")
        .or_else(|| info.get("sessionId"))
        .or_else(|| info.get("id"))
        .or_else(|| value.get("session_id"))
        .or_else(|| value.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| path.parent()?.file_name()?.to_str().map(str::to_owned)))
}

fn is_uuid_like(value: &str) -> bool {
    value.len() == 36
        && value
            .chars()
            .enumerate()
            .all(|(index, character)| match index {
                8 | 13 | 18 | 23 => character == '-',
                _ => character.is_ascii_hexdigit(),
            })
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
        || path
            .canonicalize()
            .ok()
            .zip(root.canonicalize().ok())
            .is_some_and(|(path, root)| path.starts_with(root))
}

fn process_tree_pids(root_pid: u32) -> HashSet<u32> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().without_tasks(),
    );
    let parents = system
        .processes()
        .values()
        .filter_map(|process| {
            process
                .parent()
                .map(|parent| (process.pid().as_u32(), parent.as_u32()))
        })
        .collect::<HashMap<_, _>>();
    let mut tree = HashSet::from([root_pid]);
    loop {
        let before = tree.len();
        for (pid, parent) in &parents {
            if tree.contains(parent) {
                tree.insert(*pid);
            }
        }
        if tree.len() == before {
            return tree;
        }
    }
}

#[cfg(target_os = "macos")]
fn open_files_for_process_tree(root_pid: u32) -> io::Result<HashSet<PathBuf>> {
    let mut pids = process_tree_pids(root_pid).into_iter().collect::<Vec<_>>();
    pids.sort_unstable();
    let pid_list = pids
        .into_iter()
        .map(|pid| pid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let output = Command::new("/usr/sbin/lsof")
        .args(["-w", "-a", "-Fn", "-p", &pid_list])
        .output()?;
    Ok(output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(b"n"))
        .filter_map(|path| std::str::from_utf8(path).ok())
        .map(PathBuf::from)
        .collect())
}

#[cfg(target_os = "linux")]
fn open_files_for_process_tree(root_pid: u32) -> io::Result<HashSet<PathBuf>> {
    let mut files = HashSet::new();
    for pid in process_tree_pids(root_pid) {
        let descriptors = match fs::read_dir(format!("/proc/{pid}/fd")) {
            Ok(descriptors) => descriptors,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        for descriptor in descriptors.flatten() {
            if let Ok(path) = fs::read_link(descriptor.path()) {
                files.insert(path);
            }
        }
    }
    Ok(files)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn open_files_for_process_tree(_root_pid: u32) -> io::Result<HashSet<PathBuf>> {
    Ok(HashSet::new())
}

fn discover_kimi(capture: &SessionCapture) -> io::Result<Option<String>> {
    let index = capture.home.join(".kimi-code/session_index.jsonl");
    let file = match File::open(index) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut latest = None;
    for line in BufReader::new(file).lines() {
        let Ok(value) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if !value
            .get("workDir")
            .and_then(Value::as_str)
            .is_some_and(|cwd| working_dirs_match(cwd, &capture.working_dir))
        {
            continue;
        }
        let Some(session_id) = value.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        let Some(session_dir) = value.get("sessionDir").and_then(Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(session_dir);
        let modified =
            modified_millis(&path.join("state.json")).or_else(|_| modified_millis(&path))?;
        if modified >= capture.started_at_ms
            && latest.as_ref().is_none_or(|(seen, _)| modified > *seen)
        {
            latest = Some((modified, session_id.to_owned()));
        }
    }
    Ok(latest.map(|(_, id)| id))
}

fn discover_gemini(capture: &SessionCapture) -> io::Result<Option<String>> {
    let Some(name) = capture.working_dir.file_name() else {
        return Ok(None);
    };
    let root = capture.home.join(".gemini/tmp").join(name).join("chats");
    latest_session_file(&root, "jsonl", capture.started_at_ms, |path| {
        Ok(first_json_line(path)?.and_then(|value| {
            value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }))
    })
}

fn discover_opencode(capture: &SessionCapture) -> io::Result<Option<String>> {
    let data_home = capture
        .xdg_data_home
        .clone()
        .unwrap_or_else(|| capture.home.join(".local/share"));
    let path = data_home.join("opencode/opencode.db");
    if !path.is_file() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sqlite_io)?;
    connection
        .query_row(
            "SELECT id FROM session
             WHERE directory = ?1 AND time_updated >= ?2
             ORDER BY time_updated DESC LIMIT 1",
            (capture.working_dir.to_string_lossy(), capture.started_at_ms),
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_io)
}

fn latest_session_file(
    root: &Path,
    extension: &str,
    watermark_ms: i64,
    mut session_id: impl FnMut(&Path) -> io::Result<Option<String>>,
) -> io::Result<Option<String>> {
    let mut files = Vec::new();
    collect_session_files(root, extension, watermark_ms, &mut files)?;
    files.sort_by_key(|(_, modified)| *modified);
    for (path, _) in files.into_iter().rev() {
        if let Some(session_id) = session_id(&path)? {
            return Ok(Some(session_id));
        }
    }
    Ok(None)
}

fn collect_session_files(
    root: &Path,
    extension: &str,
    watermark_ms: i64,
    files: &mut Vec<(PathBuf, i64)>,
) -> io::Result<()> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_session_files(&path, extension, watermark_ms, files)?;
        } else if file_type.is_file()
            && path.extension().and_then(|candidate| candidate.to_str()) == Some(extension)
        {
            let modified = modified_millis(&path)?;
            if modified >= watermark_ms {
                files.push((path, modified));
            }
        }
    }
    Ok(())
}

fn first_json_line(path: &Path) -> io::Result<Option<Value>> {
    first_json_line_matching(path, |_| true)
}

fn first_json_document(path: &Path) -> io::Result<Option<Value>> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(serde_json::from_slice(&contents).ok())
}

fn first_json_line_matching(
    path: &Path,
    predicate: impl Fn(&Value) -> bool,
) -> io::Result<Option<Value>> {
    let file = File::open(path)?;
    for line in BufReader::new(file).lines().take(128) {
        let Ok(value) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if predicate(&value) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn claude_project_slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn working_dirs_match(recorded: &str, expected: &Path) -> bool {
    let recorded = Path::new(recorded);
    recorded == expected
        || recorded
            .canonicalize()
            .ok()
            .zip(expected.canonicalize().ok())
            .is_some_and(|(recorded, expected)| recorded == expected)
}

fn modified_millis(path: &Path) -> io::Result<i64> {
    let modified = fs::metadata(path)?.modified()?;
    system_time_millis(modified)
}

fn system_time_millis(time: SystemTime) -> io::Result<i64> {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis();
    Ok(i64::try_from(millis).unwrap_or(i64::MAX))
}

fn sqlite_io(error: rusqlite::Error) -> io::Error {
    io::Error::other(error)
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use std::{fs, thread, time::Duration};

    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn now_ms() -> i64 {
        system_time_millis(SystemTime::now()).unwrap()
    }

    #[test]
    fn discovers_claude_and_codex_by_cwd_after_watermark() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("repo with space");
        fs::create_dir_all(&cwd).unwrap();
        let watermark = now_ms();
        thread::sleep(Duration::from_millis(2));

        let claude = temp
            .path()
            .join(".claude/projects")
            .join(claude_project_slug(&cwd));
        fs::create_dir_all(&claude).unwrap();
        fs::write(
            claude.join("claude-session.jsonl"),
            format!(
                "{{\"sessionId\":\"claude-session\",\"cwd\":{}}}\n",
                serde_json::to_string(&cwd).unwrap()
            ),
        )
        .unwrap();
        let codex = temp.path().join(".codex/sessions/2026/08/07");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("rollout.jsonl"),
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"codex-session\",\"cwd\":{}}}}}\n",
                serde_json::to_string(&cwd).unwrap()
            ),
        )
        .unwrap();

        assert_eq!(
            SessionCapture::fixture(SessionAdapter::Claude, temp.path(), &cwd, watermark)
                .discover()
                .unwrap()
                .as_deref(),
            Some("claude-session")
        );
        assert_eq!(
            SessionCapture::fixture(SessionAdapter::Codex, temp.path(), &cwd, watermark)
                .discover()
                .unwrap()
                .as_deref(),
            Some("codex-session")
        );
    }

    #[test]
    fn discovers_current_claude_live_session_registry() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("repo");
        fs::create_dir_all(&cwd).unwrap();
        let watermark = now_ms();
        thread::sleep(Duration::from_millis(2));
        let sessions = temp.path().join(".claude/sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("4242.json"),
            format!(
                "{{\"cwd\":{},\"sessionId\":\"live-claude-session\",\"pid\":4242}}\n",
                serde_json::to_string(&cwd).unwrap()
            ),
        )
        .unwrap();

        assert_eq!(
            SessionCapture::fixture(SessionAdapter::Claude, temp.path(), &cwd, watermark)
                .discover()
                .unwrap()
                .as_deref(),
            Some("live-claude-session")
        );
    }

    #[test]
    fn discovers_kimi_and_opencode_without_reading_pty_output() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("repo");
        fs::create_dir_all(&cwd).unwrap();
        let watermark = now_ms();
        thread::sleep(Duration::from_millis(2));

        let kimi_session = temp.path().join(".kimi-code/sessions/repo/session-kimi");
        fs::create_dir_all(&kimi_session).unwrap();
        fs::write(kimi_session.join("state.json"), "{}\n").unwrap();
        fs::write(
            temp.path().join(".kimi-code/session_index.jsonl"),
            format!(
                "{{\"sessionId\":\"session-kimi\",\"sessionDir\":{},\"workDir\":{}}}\n",
                serde_json::to_string(&kimi_session).unwrap(),
                serde_json::to_string(&cwd).unwrap()
            ),
        )
        .unwrap();

        let opencode_dir = temp.path().join(".local/share/opencode");
        fs::create_dir_all(&opencode_dir).unwrap();
        let database = Connection::open(opencode_dir.join("opencode.db")).unwrap();
        database
            .execute_batch(
                "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, time_updated INTEGER);",
            )
            .unwrap();
        database
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3)",
                ("ses-opencode", cwd.to_string_lossy(), now_ms()),
            )
            .unwrap();
        drop(database);

        assert_eq!(
            SessionCapture::fixture(SessionAdapter::Kimi, temp.path(), &cwd, watermark)
                .discover()
                .unwrap()
                .as_deref(),
            Some("session-kimi")
        );
        assert_eq!(
            SessionCapture::fixture(SessionAdapter::OpenCode, temp.path(), &cwd, watermark)
                .discover()
                .unwrap()
                .as_deref(),
            Some("ses-opencode")
        );
    }

    #[test]
    fn discovers_grok_summary_by_cwd_and_honors_grok_home() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("repo with space");
        fs::create_dir_all(&cwd).unwrap();
        let grok_home = temp.path().join("isolated-grok");
        let session = grok_home
            .join("sessions")
            .join("%2Ftmp%2Frepo%20with%20space")
            .join("01989cb4-471e-7cd0-8c9f-3ee5e46f475d");
        fs::create_dir_all(&session).unwrap();
        let watermark = now_ms();
        thread::sleep(Duration::from_millis(2));
        fs::write(
            session.join("summary.json"),
            serde_json::to_vec(&json!({
                "info": {
                    "session_id": "01989cb4-471e-7cd0-8c9f-3ee5e46f475d",
                    "cwd": cwd
                },
                "generated_title": "fixture"
            }))
            .unwrap(),
        )
        .unwrap();

        let mut capture =
            SessionCapture::fixture(SessionAdapter::Grok, temp.path(), &cwd, watermark);
        capture.grok_home = Some(grok_home);
        assert_eq!(
            capture.discover().unwrap().as_deref(),
            Some("01989cb4-471e-7cd0-8c9f-3ee5e46f475d")
        );
        assert!(!capture.supports_continue_latest_fallback());
        assert_eq!(adapter("grok-build"), Some(SessionAdapter::Grok));
    }

    #[test]
    fn ignores_sessions_before_the_spawn_watermark() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("repo");
        let claude = temp
            .path()
            .join(".claude/projects")
            .join(claude_project_slug(&cwd));
        fs::create_dir_all(&claude).unwrap();
        fs::write(
            claude.join("old.jsonl"),
            format!(
                "{{\"sessionId\":\"old\",\"cwd\":{}}}\n",
                serde_json::to_string(&cwd).unwrap()
            ),
        )
        .unwrap();
        let watermark = now_ms() + 1;
        assert_eq!(
            SessionCapture::fixture(SessionAdapter::Claude, temp.path(), &cwd, watermark)
                .discover()
                .unwrap(),
            None
        );
    }
}
