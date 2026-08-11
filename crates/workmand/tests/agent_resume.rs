use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use workman_core::{
    AgentLaunchMode, AgentTool, AgentToolSource, Process, ProcessKind, ProcessSource,
    ProcessStatus, Project, Store,
};
use workmand::ProcessRegistry;

fn wait_for_lines(path: &Path, count: usize) -> Result<Vec<String>, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let lines = fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if lines.len() >= count {
            return Ok(lines);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for {count} launches in {}",
                path.display()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[allow(clippy::too_many_arguments)]
fn process(
    id: i64,
    project: &Project,
    name: &str,
    command: String,
    tool_id: i64,
    home: &Path,
    session_file: &Path,
    launch_log: &Path,
) -> Process {
    Process {
        id,
        project_id: project.id,
        kind: ProcessKind::Agent,
        name: name.into(),
        command: Some(command),
        working_dir: project.path.clone(),
        env: BTreeMap::from([
            ("HOME".into(), home.to_string_lossy().into_owned()),
            (
                "FIXTURE_SESSION_FILE".into(),
                session_file.to_string_lossy().into_owned(),
            ),
            (
                "FIXTURE_LAUNCH_LOG".into(),
                launch_log.to_string_lossy().into_owned(),
            ),
        ]),
        auto_start: false,
        auto_restart: false,
        restart_when_changed: Vec::new(),
        source: ProcessSource::Local,
        trust_hash: None,
        status: ProcessStatus::Stopped,
        pid: None,
        exit_code: None,
        exit_signal: None,
        exited_at: None,
        agent_tool_id: Some(tool_id),
        spawned_by_process_id: None,
        sort_order: 0,
    }
}

fn claude_slug(path: &Path) -> String {
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

#[test]
fn standard_start_captures_and_resumes_exact_session_while_custom_stays_fresh()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repository = temp.path().join("repo");
    fs::create_dir_all(&repository)?;
    let project = Project {
        id: 1,
        path: repository.to_string_lossy().into_owned(),
        name: "resume-fixture".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };
    let script = temp.path().join("fake-claude");
    fs::write(
        &script,
        "#!/bin/sh\nmkdir -p \"$(dirname \"$FIXTURE_SESSION_FILE\")\"\nsession_id=${FIXTURE_SESSION_ID:-fixture-session}\nprintf '{\"sessionId\":\"%s\",\"cwd\":\"%s\"}\\n' \"$session_id\" \"$PWD\" > \"$FIXTURE_SESSION_FILE\"\nprintf '%s\\n' \"$*\" >> \"$FIXTURE_LAUNCH_LOG\"\nwhile :; do sleep 1; done\n",
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))?;

    let store = Store::open_in_memory()?;
    store.put_project(&project)?;
    store.put_agent_tool(&AgentTool {
        id: 41,
        name: "Fixture Claude".into(),
        command: script.to_string_lossy().into_owned(),
        tool_type: "claude_code".into(),
        enabled: true,
        source: AgentToolSource::Local,
        resume_args: Some("--resume {session_id}".into()),
        continue_args: Some("--continue".into()),
    })?;
    store.put_agent_tool(&AgentTool {
        id: 42,
        name: "Fixture custom".into(),
        command: script.to_string_lossy().into_owned(),
        tool_type: "custom".into(),
        enabled: true,
        source: AgentToolSource::Local,
        resume_args: None,
        continue_args: None,
    })?;
    let mut registry = ProcessRegistry::with_stop_grace(store, Duration::from_millis(100))?;

    let home = temp.path().join("home");
    let session_file = home
        .join(".claude/projects")
        .join(claude_slug(&repository))
        .join("fixture-session.jsonl");
    let resume_log = temp.path().join("resume-launches.log");
    registry.create(process(
        10,
        &project,
        "resumable",
        format!(
            "{} --model fixture --dangerously-skip-permissions",
            script.display()
        ),
        41,
        &home,
        &session_file,
        &resume_log,
    ))?;

    let first = registry.start(10)?;
    assert_eq!(first.status, ProcessStatus::Running);
    wait_for_lines(&resume_log, 1)?;
    registry.stop(10)?;
    let captured = registry.get_status(10)?;
    assert_eq!(
        captured.agent_session_id.as_deref(),
        Some("fixture-session")
    );
    assert_eq!(captured.agent_launch_mode, Some(AgentLaunchMode::Fresh));

    registry.start(10)?;
    let launches = wait_for_lines(&resume_log, 2)?;
    assert_eq!(
        launches[0],
        "--model fixture --dangerously-skip-permissions"
    );
    assert_eq!(
        launches[1],
        "--model fixture --dangerously-skip-permissions --resume fixture-session"
    );
    let resumed = registry.get_status(10)?;
    assert_eq!(
        resumed.agent_launch_mode,
        Some(AgentLaunchMode::ResumedSession)
    );
    registry.stop(10)?;

    // Simulate a legacy dead agent that predates persisted session IDs. The existing
    // cwd-scoped Claude file makes the preset's continue-latest command the honest fallback.
    let continue_log = temp.path().join("continue-launches.log");
    let mut legacy = process(
        12,
        &project,
        "legacy-with-cwd-session",
        format!("{} --model fixture", script.display()),
        41,
        &home,
        &session_file,
        &continue_log,
    );
    legacy
        .env
        .insert("FIXTURE_SESSION_ID".into(), "legacy-session".into());
    let mut legacy = registry.create(legacy)?;
    legacy.status = ProcessStatus::Exited;
    legacy.exited_at = Some(1);
    registry.store().put_process(&legacy)?;
    registry.start(12)?;
    let launches = wait_for_lines(&continue_log, 1)?;
    assert_eq!(launches, ["--model fixture --continue"]);
    let continued = registry.get_status(12)?;
    assert_eq!(
        continued.agent_launch_mode,
        Some(AgentLaunchMode::ContinuedLatest)
    );
    registry.stop(12)?;

    let fresh_log = temp.path().join("fresh-launches.log");
    registry.create(process(
        11,
        &project,
        "always-fresh",
        format!("{} --flag retained", script.display()),
        42,
        &home,
        &session_file,
        &fresh_log,
    ))?;
    registry.start(11)?;
    wait_for_lines(&fresh_log, 1)?;
    registry.stop(11)?;
    registry.start(11)?;
    let launches = wait_for_lines(&fresh_log, 2)?;
    assert_eq!(launches, ["--flag retained", "--flag retained"]);
    let fresh = registry.get_status(11)?;
    assert_eq!(fresh.agent_launch_mode, Some(AgentLaunchMode::Fresh));
    registry.stop(11)?;

    Ok(())
}

#[test]
fn concurrent_codex_agents_in_one_cwd_capture_their_own_sessions() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repository = temp.path().join("repo");
    fs::create_dir_all(&repository)?;
    let project = Project {
        id: 1,
        path: repository.to_string_lossy().into_owned(),
        name: "concurrent-resume-fixture".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };
    let script = temp.path().join("fake-codex");
    fs::write(
        &script,
        "#!/bin/sh\nwhile [ ! -f \"$FIXTURE_RELEASE_FILE\" ]; do sleep 0.01; done\nsession_id=codex-session-$WORKMAN_PROCESS_ID\nsession_file=\"$CODEX_HOME/sessions/2026/08/11/rollout-$WORKMAN_PROCESS_ID.jsonl\"\nmkdir -p \"$(dirname \"$session_file\")\"\nexec 9>\"$session_file\"\nprintf '{\"type\":\"session_meta\",\"payload\":{\"id\":\"%s\",\"cwd\":\"%s\"}}\\n' \"$session_id\" \"$PWD\" >&9\nprintf '%s:%s\\n' \"$WORKMAN_PROCESS_ID\" \"$*\" >> \"$FIXTURE_LAUNCH_LOG\"\nif [ -n \"$FIXTURE_CRASH_TRIGGER\" ]; then\n  while [ ! -f \"$FIXTURE_CRASH_TRIGGER\" ]; do sleep 0.01; done\n  exit 17\nfi\nwhile :; do sleep 1; done\n",
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))?;

    let database = temp.path().join("workman.db");
    let store = Store::open(&database)?;
    store.put_project(&project)?;
    store.put_agent_tool(&AgentTool {
        id: 51,
        name: "Fixture Codex".into(),
        command: script.to_string_lossy().into_owned(),
        tool_type: "codex".into(),
        enabled: true,
        source: AgentToolSource::Local,
        resume_args: Some("resume {session_id}".into()),
        continue_args: Some("resume --last".into()),
    })?;
    let mut registry = ProcessRegistry::with_stop_grace(store, Duration::from_millis(100))?;
    let home = temp.path().join("home");
    let codex_home = home.join(".codex");
    let release_file = temp.path().join("release");
    let crash_trigger = temp.path().join("crash");
    let launch_log = temp.path().join("launches.log");
    let unused_session_file = temp.path().join("unused.jsonl");

    for process_id in [20, 21, 22, 23] {
        let mut agent = process(
            process_id,
            &project,
            &format!("codex-{process_id}"),
            script.to_string_lossy().into_owned(),
            51,
            &home,
            &unused_session_file,
            &launch_log,
        );
        agent.env.insert(
            "CODEX_HOME".into(),
            codex_home.to_string_lossy().into_owned(),
        );
        agent.env.insert(
            "FIXTURE_RELEASE_FILE".into(),
            release_file.to_string_lossy().into_owned(),
        );
        if process_id == 21 {
            agent.spawned_by_process_id = Some(20);
        }
        if process_id == 23 {
            agent.env.insert(
                "FIXTURE_CRASH_TRIGGER".into(),
                crash_trigger.to_string_lossy().into_owned(),
            );
        }
        registry.create(agent)?;
        registry.start(process_id)?;
    }

    fs::write(&release_file, "go\n")?;
    wait_for_lines(&launch_log, 4)?;
    thread::sleep(Duration::from_millis(1_050));
    for process_id in [20, 21, 22, 23] {
        let status = registry.get_status(process_id)?;
        assert_eq!(
            status.agent_session_id.as_deref(),
            Some(format!("codex-session-{process_id}").as_str())
        );
    }

    fs::write(&crash_trigger, "crash\n")?;
    let crash_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if registry.get_status(23)?.process.status == ProcessStatus::Crashed {
            break;
        }
        assert!(Instant::now() < crash_deadline, "agent did not crash");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        registry.get_status(23)?.agent_session_id.as_deref(),
        Some("codex-session-23")
    );

    registry.kill(20)?;
    assert_eq!(
        registry.get_status(21)?.process.status,
        ProcessStatus::Stopped
    );
    registry.kill(22)?;
    drop(registry);

    let store = Store::open(&database)?;
    let mut registry = ProcessRegistry::with_stop_grace(store, Duration::from_millis(100))?;
    for process_id in [20, 21, 22, 23] {
        assert_eq!(
            registry.get_status(process_id)?.agent_session_id,
            Some(format!("codex-session-{process_id}"))
        );
    }
    for process_id in [20, 21, 22] {
        registry.start(process_id)?;
    }
    let launches = wait_for_lines(&launch_log, 7)?;
    for process_id in [20, 21, 22] {
        let prefix = format!("{process_id}:");
        let launch = launches
            .iter()
            .rfind(|launch| launch.starts_with(&prefix))
            .expect("resumed launch for process");
        assert_eq!(
            launch,
            &format!("{process_id}:resume codex-session-{process_id}")
        );
        assert_eq!(
            registry.get_status(process_id)?.agent_launch_mode,
            Some(AgentLaunchMode::ResumedSession)
        );
    }

    // A stopped Codex agent with no captured ID must start fresh instead of
    // invoking `resume --last`, which can fail or select a sibling session.
    let mut fallback = process(
        30,
        &project,
        "codex-without-capture",
        script.to_string_lossy().into_owned(),
        51,
        &home,
        &unused_session_file,
        &launch_log,
    );
    fallback.env.insert(
        "CODEX_HOME".into(),
        codex_home.to_string_lossy().into_owned(),
    );
    fallback.env.insert(
        "FIXTURE_RELEASE_FILE".into(),
        release_file.to_string_lossy().into_owned(),
    );
    let mut fallback = registry.create(fallback)?;
    fallback.status = ProcessStatus::Exited;
    fallback.exited_at = Some(1);
    registry.store().put_process(&fallback)?;
    registry.start(30)?;
    let launches = wait_for_lines(&launch_log, 8)?;
    assert_eq!(
        launches
            .iter()
            .rfind(|launch| launch.starts_with("30:"))
            .map(String::as_str),
        Some("30:")
    );
    let fallback = registry.get_status(30)?;
    assert_eq!(fallback.agent_launch_mode, Some(AgentLaunchMode::Fresh));
    assert!(fallback.events.iter().any(|event| {
        event.kind == "agent_launch"
            && event.message
                == "Started a fresh agent conversation because no captured session ID was available"
    }));

    // Dropping the registry models a clean app/daemon shutdown. IDs already
    // captured for all live agents remain durable across the restart.
    drop(registry);
    let store = Store::open(&database)?;
    let mut registry = ProcessRegistry::with_stop_grace(store, Duration::from_millis(100))?;
    for process_id in [20, 21, 22, 23] {
        assert_eq!(
            registry.get_status(process_id)?.agent_session_id,
            Some(format!("codex-session-{process_id}"))
        );
    }

    Ok(())
}
