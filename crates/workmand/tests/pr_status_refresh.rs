#![cfg(unix)]

use std::{
    error::Error,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use tokio::sync::Mutex;
use workman_core::{Project, Store};
use workmand::{ProcessRegistry, UserEnvironmentResolver, worktrees};

#[tokio::test]
async fn manual_refresh_round_trips_terminal_pr_state_and_preserves_the_link()
-> Result<(), Box<dyn Error>> {
    let fixture = tempfile::Builder::new()
        .prefix("com.workman.todo89.")
        .tempdir_in("/tmp")?;
    let repository = fixture.path().join("scratch-repository");
    let data_dir = fixture.path().join("fresh-data");
    let profile_bin = fixture.path().join("fake-github-bin");
    fs::create_dir(&data_dir)?;
    fs::create_dir(&profile_bin)?;

    git(
        fixture.path(),
        &["init", "-b", "main", repository.to_str().unwrap()],
    )?;
    git(
        &repository,
        &["config", "user.email", "fixture@example.test"],
    )?;
    git(&repository, &["config", "user.name", "Todo 89 Fixture"])?;
    fs::write(repository.join("README.md"), "isolated todo 89 fixture\n")?;
    git(&repository, &["add", "README.md"])?;
    git(&repository, &["commit", "-m", "fixture"])?;

    let git_executable = executable_on_test_path("git").ok_or("test git executable missing")?;
    symlink(git_executable, profile_bin.join("git"))?;
    let gh = profile_bin.join("gh");
    write_gh(
        &gh,
        r#"[{"number":42,"state":"OPEN","isDraft":false,"headRefName":"main","url":"https://example.test/pr/42","mergeable":"MERGEABLE","statusCheckRollup":[]}]"#,
    )?;

    let shell = fixture.path().join("isolated-login-shell");
    fs::write(
        &shell,
        format!(
            "#!/bin/sh\nexport PATH='{}'\nshift\nexec /bin/sh \"$@\"\n",
            profile_bin.display()
        ),
    )?;
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o700))?;
    let config = fixture.path().join("isolated-config.yml");
    fs::write(
        &config,
        format!("terminal:\n  shell: {:?}\n", shell.to_string_lossy()),
    )?;

    let store = Store::open(data_dir.join("state.sqlite3"))?;
    store.put_project(&Project {
        id: 1,
        path: fs::canonicalize(&repository)?
            .to_string_lossy()
            .into_owned(),
        name: "scratch-repository".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    })?;
    worktrees::reconcile_existing_projects(&store)?;
    let registry = Arc::new(Mutex::new(ProcessRegistry::with_user_environment(
        store,
        UserEnvironmentResolver::new(config),
    )?));

    let open = worktrees::list_for_project_refresh(&registry, 1, true).await?;
    let open_pr = pull_request(&open)?;
    assert_eq!(open_pr["state"], "open");
    assert_eq!(open_pr["mergeable"], "mergeable");

    write_gh(
        &gh,
        r#"[{"number":42,"state":"MERGED","isDraft":false,"headRefName":"main","url":"https://example.test/pr/42","mergeable":"MERGEABLE","statusCheckRollup":[]}]"#,
    )?;
    let cached = worktrees::list_for_project(&registry, 1).await?;
    assert_eq!(pull_request(&cached)?["state"], "open");

    let merged = worktrees::list_for_project_refresh(&registry, 1, true).await?;
    let merged_pr = pull_request(&merged)?;
    assert_eq!(merged_pr["state"], "merged");
    assert_eq!(merged_pr["mergeable"], "unknown");
    assert_eq!(merged_pr["url"], "https://example.test/pr/42");

    write_gh(
        &gh,
        r#"[{"number":43,"state":"CLOSED","isDraft":true,"headRefName":"main","url":"https://example.test/pr/43","mergeable":"MERGEABLE","statusCheckRollup":[]}]"#,
    )?;
    let closed = worktrees::list_for_project_refresh(&registry, 1, true).await?;
    let closed_pr = pull_request(&closed)?;
    assert_eq!(closed_pr["state"], "closed");
    assert_eq!(closed_pr["mergeable"], "unknown");
    assert_eq!(closed_pr["url"], "https://example.test/pr/43");
    Ok(())
}

fn pull_request(list: &worktrees::WorktreeList) -> Result<serde_json::Value, Box<dyn Error>> {
    serde_json::to_value(list)?["worktrees"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["branch"] == "main"))
        .and_then(|entry| entry.get("pull_request"))
        .filter(|pull_request| !pull_request.is_null())
        .cloned()
        .ok_or_else(|| "main worktree PR link is missing".into())
}

fn write_gh(path: &Path, json: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' '{json}'\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn executable_on_test_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn git(directory: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
