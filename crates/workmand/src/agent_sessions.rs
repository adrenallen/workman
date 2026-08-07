//! Passive discovery of conversation IDs written by supported agent CLIs.

use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionAdapter {
    Claude,
    Codex,
    Gemini,
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
            xdg_data_home: variable("XDG_DATA_HOME"),
        })
    }

    pub(crate) fn discover(&self) -> Result<Option<String>, String> {
        let result = match self.adapter {
            SessionAdapter::Claude => discover_claude(self),
            SessionAdapter::Codex => discover_codex(self),
            SessionAdapter::Gemini => discover_gemini(self),
            SessionAdapter::Kimi => discover_kimi(self),
            SessionAdapter::OpenCode => discover_opencode(self),
        };
        result.map_err(|error| error.to_string())
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
    let root = capture
        .codex_home
        .clone()
        .unwrap_or_else(|| capture.home.join(".codex"))
        .join("sessions");
    latest_session_file(&root, "jsonl", capture.started_at_ms, |path| {
        let Some(value) = first_json_line(path)? else {
            return Ok(None);
        };
        let payload = value.get("payload").unwrap_or(&value);
        if !payload
            .get("cwd")
            .and_then(Value::as_str)
            .is_some_and(|cwd| working_dirs_match(cwd, &capture.working_dir))
        {
            return Ok(None);
        }
        Ok(payload
            .get("id")
            .or_else(|| payload.get("session_id"))
            .and_then(Value::as_str)
            .map(str::to_owned))
    })
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
