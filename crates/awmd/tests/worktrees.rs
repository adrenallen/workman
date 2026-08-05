#![cfg(unix)]

use std::{
    collections::BTreeMap,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use awm_core::{Project, Store};
use awmd::{
    ProcessRegistry, SharedProcessRegistry,
    worktrees::{self, AdoptWorktree, CreateWorktree, RemoveWorktree},
};
use tempfile::TempDir;
use tokio::sync::Mutex;

struct GitFixture {
    _temp: TempDir,
    main: PathBuf,
    origin: PathBuf,
    managed: PathBuf,
    external: PathBuf,
    registry: SharedProcessRegistry,
}

impl GitFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let temp = TempDir::new()?;
        let main = temp.path().join("sample-repo");
        let origin = temp.path().join("origin.git");
        let managed = temp.path().join("managed");
        let external = temp.path().join("outside");
        let state = temp.path().join("state.sqlite3");

        git(temp.path(), &["init", "--bare", origin.to_str().unwrap()])?;
        git(temp.path(), &["init", "-b", "main", main.to_str().unwrap()])?;
        git(&main, &["config", "user.email", "fixture@example.test"])?;
        git(&main, &["config", "user.name", "Fixture"])?;
        std::fs::write(main.join("README.md"), "fixture\n")?;
        git(&main, &["add", "README.md"])?;
        git(&main, &["commit", "-m", "initial"])?;
        git(
            &main,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        )?;
        git(&main, &["push", "-u", "origin", "main"])?;
        git(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"])?;

        git(&main, &["checkout", "-b", "remote-only"])?;
        std::fs::write(main.join("remote.txt"), "remote branch\n")?;
        git(&main, &["add", "remote.txt"])?;
        git(&main, &["commit", "-m", "remote branch"])?;
        git(&main, &["push", "origin", "remote-only"])?;
        git(&main, &["checkout", "main"])?;
        git(&main, &["branch", "-D", "remote-only"])?;
        git(
            &main,
            &["update-ref", "-d", "refs/remotes/origin/remote-only"],
        )?;

        let store = Store::open(&state)?;
        store.put_project(&Project {
            id: 1,
            path: std::fs::canonicalize(&main)?.to_string_lossy().into_owned(),
            name: "sample-repo".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        worktrees::reconcile_existing_projects(&store)?;
        let registry = Arc::new(Mutex::new(ProcessRegistry::new(store)?));

        Ok(Self {
            _temp: temp,
            main,
            origin,
            managed,
            external,
            registry,
        })
    }
}

#[tokio::test]
async fn swm_semantics_cover_remote_discovery_adoption_and_safe_removal()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    let initial = worktrees::list_for_project(&fixture.registry, 1).await?;
    assert_eq!(initial.repository.name, "sample-repo");
    assert_eq!(initial.worktrees.len(), 1);
    assert_eq!(initial.worktrees[0].kind, "main");
    assert_eq!(initial.worktrees[0].branch, "main");

    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "feature/new-ui".into(),
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([("copy_env".into(), "no".into())]),
        },
    )
    .await?;
    assert_eq!(created.project.project.name, "sample-repo: feature/new-ui");
    assert_eq!(created.project.parent_project_id, Some(1));
    assert_eq!(created.project.branch.as_deref(), Some("feature/new-ui"));
    assert!(created.project.worktree_managed);
    assert_eq!(created.worktree.kind, "managed");
    assert_eq!(
        Path::new(&created.worktree.path).file_name().unwrap(),
        "feature-new-ui"
    );
    assert_eq!(created.repository.preferences["copy_env"], "no");

    // Existing SWM projects were registered before awm knew about worktree
    // metadata. An exact managed-root/branch-slug layout is linked back to the
    // repository parent and inherits safe-removal ownership.
    let legacy_path = fixture.managed.join("legacy-port");
    git(
        &fixture.main,
        &[
            "worktree",
            "add",
            "-b",
            "legacy/port",
            legacy_path.to_str().unwrap(),
            "main",
        ],
    )?;
    let legacy_id = {
        let registry = fixture.registry.lock().await;
        let legacy_id = registry.store().next_project_id()?;
        registry.store().put_project(&Project {
            id: legacy_id,
            path: std::fs::canonicalize(&legacy_path)?
                .to_string_lossy()
                .into_owned(),
            name: "sample-repo: legacy/port".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: registry.store().next_project_sort_order()?,
        })?;
        worktrees::reconcile_existing_projects(registry.store())?;
        legacy_id
    };
    let legacy = worktrees::list_for_project(&fixture.registry, legacy_id).await?;
    let legacy = legacy
        .worktrees
        .iter()
        .find(|entry| entry.project_id == Some(legacy_id))
        .expect("legacy project was linked");
    assert_eq!(legacy.parent_project_id, Some(1));
    assert_eq!(legacy.kind, "managed");
    assert!(legacy.can_remove);

    let remote = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "remote-only".into(),
            from_ref: None,
            managed_root: None,
            preferences: BTreeMap::new(),
        },
    )
    .await?;
    assert_eq!(remote.project.project.name, "sample-repo: remote-only");
    assert_eq!(
        git(
            Path::new(&remote.worktree.path),
            &["rev-parse", "--abbrev-ref", "@{upstream}"]
        )?
        .trim(),
        "origin/remote-only"
    );

    git(
        &fixture.main,
        &[
            "worktree",
            "add",
            "-b",
            "outside-branch",
            fixture.external.to_str().unwrap(),
            "main",
        ],
    )?;
    std::fs::create_dir(fixture.external.join("nested"))?;
    let discovered = worktrees::list_for_project(&fixture.registry, 1).await?;
    let offer = discovered
        .worktrees
        .iter()
        .find(|entry| entry.branch == "outside-branch")
        .expect("external worktree is offered for import");
    assert_eq!(offer.kind, "external");
    assert!(offer.can_adopt);
    assert!(!offer.can_remove);

    let adopted = worktrees::adopt(
        &fixture.registry,
        AdoptWorktree {
            path: fixture.external.join("nested"),
            preferences: BTreeMap::new(),
        },
    )
    .await?;
    assert_eq!(adopted.project.project.name, "sample-repo: outside-branch");
    assert_eq!(adopted.worktree.kind, "adopted");
    let foreign = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: adopted.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            force_dirty: true,
            confirm_branch: Some("outside-branch".into()),
        },
    )
    .await
    .expect_err("adopted worktree removal must be refused");
    assert_eq!(foreign.code(), "foreign_worktree");
    assert!(fixture.external.exists());

    std::fs::write(
        Path::new(&created.worktree.path).join("dirty.txt"),
        "unsaved\n",
    )?;
    let dirty = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await
    .expect_err("dirty managed worktree requires typed confirmation");
    assert_eq!(dirty.code(), "dirty_worktree");
    assert!(Path::new(&created.worktree.path).exists());

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            force_dirty: true,
            confirm_branch: Some("feature/new-ui".into()),
        },
    )
    .await?;
    assert!(removed.removed && removed.project_unregistered && removed.branch_kept);
    assert!(!Path::new(&removed.path).exists());
    assert!(
        git(
            &fixture.main,
            &["show-ref", "--verify", "refs/heads/feature/new-ui"]
        )
        .is_ok(),
        "removal must preserve the branch"
    );
    assert_eq!(
        fixture
            .registry
            .lock()
            .await
            .store()
            .get_project(created.project.project.id)?,
        None
    );

    // The remote stays a local fixture throughout the test; no user repository is touched.
    assert!(fixture.origin.join("HEAD").exists());
    Ok(())
}

fn git(directory: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git -C {} {} failed: {}",
            directory.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
