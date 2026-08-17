#![cfg(unix)]

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use tempfile::TempDir;
use tokio::sync::Mutex;
use workman_core::{Project, Store};
use workmand::{
    ProcessRegistry, SharedProcessRegistry, UserEnvironmentResolver,
    worktrees::{self, AdoptWorktree, CreateWorktree, EnvPortPolicy, ForkWorktree, RemoveWorktree},
};

struct GitFixture {
    _temp: TempDir,
    main: PathBuf,
    origin: PathBuf,
    managed: PathBuf,
    external: PathBuf,
    registry: SharedProcessRegistry,
}

#[derive(Clone, Copy)]
enum GitRemoveFixture {
    AlwaysFail,
    SwapPathOnce,
    RecreateOnPrune,
}

impl GitFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        Self::new_with_remove_fixture(None)
    }

    fn new_with_failing_worktree_remove() -> Result<Self, Box<dyn Error>> {
        Self::new_with_remove_fixture(Some(GitRemoveFixture::AlwaysFail))
    }

    fn new_with_retryable_path_swap() -> Result<Self, Box<dyn Error>> {
        Self::new_with_remove_fixture(Some(GitRemoveFixture::SwapPathOnce))
    }

    fn new_with_path_recreated_during_prune() -> Result<Self, Box<dyn Error>> {
        Self::new_with_remove_fixture(Some(GitRemoveFixture::RecreateOnPrune))
    }

    fn new_with_remove_fixture(
        remove_fixture: Option<GitRemoveFixture>,
    ) -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::Builder::new()
            .prefix("com.workman.todo126.")
            .tempdir_in("/tmp")?;
        let main = temp.path().join("sample-repo");
        let origin = temp.path().join("origin.git");
        let managed = temp.path().join("managed");
        let external = temp.path().join("outside");
        let state = temp.path().join("state.sqlite3");

        git(temp.path(), &["init", "--bare", origin.to_str().unwrap()])?;
        git(temp.path(), &["init", "-b", "main", main.to_str().unwrap()])?;
        git(
            &main,
            &[
                "config",
                "user.email",
                "com.workman.todo126@example.invalid",
            ],
        )?;
        git(&main, &["config", "user.name", "com.workman.todo126"])?;
        git(&main, &["config", "branch.autoSetupMerge", "false"])?;
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
        let registry = if let Some(remove_fixture) = remove_fixture {
            let profile_bin = temp.path().join("profile-bin");
            fs::create_dir(&profile_bin)?;
            let git_executable =
                executable_on_test_path("git").ok_or("test git executable missing")?;
            let git_wrapper = profile_bin.join("git");
            let remove_script = match remove_fixture {
                GitRemoveFixture::AlwaysFail => {
                    "echo \"error: failed to delete worktree: Directory not empty\" >&2\nexit 255"
                        .to_owned()
                }
                GitRemoveFixture::SwapPathOnce => format!(
                    "if [ ! -e '{marker}' ]; then\n  : > '{marker}'\n  target=''\n  for argument in \"$@\"; do target=\"$argument\"; done\n  /bin/mv \"$target\" \"$target.todo126-retry\"\n  /bin/ln -s \"$target.todo126-retry\" \"$target\"\n  echo \"error: simulated path swap during Git removal\" >&2\n  exit 255\nfi\nexec '{git}' \"$@\"",
                    marker = temp.path().join("remove-failed-once").display(),
                    git = git_executable.display()
                ),
                GitRemoveFixture::RecreateOnPrune => format!(
                    "target=''\nfor argument in \"$@\"; do target=\"$argument\"; done\n'{git}' \"$@\"\nstatus=$?\nif [ \"$status\" -eq 0 ]; then printf '%s' \"$target\" > '{marker}'; fi\nexit \"$status\"",
                    git = git_executable.display(),
                    marker = temp.path().join("removed-worktree-path").display(),
                ),
            };
            let prune_script = match remove_fixture {
                GitRemoveFixture::RecreateOnPrune => format!(
                    "if [ -s '{marker}' ]; then\n  target=$(/bin/cat '{marker}')\n  /bin/mkdir -p \"$target\"\n  printf 'replacement created after initial verification\\n' > \"$target/reappeared.txt\"\n  /bin/rm '{marker}'\nfi",
                    marker = temp.path().join("removed-worktree-path").display(),
                ),
                GitRemoveFixture::AlwaysFail | GitRemoveFixture::SwapPathOnce => ":".to_owned(),
            };
            fs::write(
                &git_wrapper,
                format!(
                    "#!/bin/sh\nif [ \"$3\" = worktree ] && [ \"$4\" = remove ]; then\n{remove_script}\nfi\nif [ \"$3\" = worktree ] && [ \"$4\" = prune ]; then\n{prune_script}\nfi\nexec '{}' \"$@\"\n",
                    git_executable.display()
                ),
            )?;
            fs::set_permissions(&git_wrapper, fs::Permissions::from_mode(0o700))?;
            let shell = temp.path().join("fixture-shell");
            fs::write(
                &shell,
                format!(
                    "#!/bin/sh\nexport PATH='{}'\nshift\nexec /bin/sh \"$@\"\n",
                    profile_bin.display()
                ),
            )?;
            fs::set_permissions(&shell, fs::Permissions::from_mode(0o700))?;
            let config = temp.path().join("config.yml");
            fs::write(
                &config,
                format!("terminal:\n  shell: {:?}\n", shell.to_string_lossy()),
            )?;
            Arc::new(Mutex::new(ProcessRegistry::with_user_environment(
                store,
                UserEnvironmentResolver::new(config),
            )?))
        } else {
            Arc::new(Mutex::new(ProcessRegistry::new(store)?))
        };

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
async fn branch_picker_lists_unchecked_local_and_origin_branches() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    git(&fixture.main, &["branch", "feature/local-nested"])?;

    let branches = worktrees::origin_branches_for_project(&fixture.registry, 1).await?;
    assert_eq!(branches.repository_id, 1);
    assert!(
        branches
            .options
            .iter()
            .any(|option| option.name == "feature/local-nested" && option.source == "local")
    );
    assert!(
        branches
            .options
            .iter()
            .any(|option| option.name == "remote-only" && option.source == "origin")
    );
    assert!(!branches.branches.iter().any(|branch| branch == "main"));
    assert_eq!(branches.default_ref.as_deref(), Some("origin/main"));
    assert!(
        branches
            .ref_options
            .iter()
            .any(|option| option.name == "HEAD" && option.source == "current")
    );
    assert!(
        branches
            .ref_options
            .iter()
            .any(|option| option.name == "origin/main" && option.source == "default")
    );
    assert!(
        branches
            .ref_options
            .iter()
            .any(|option| option.name == "feature/local-nested" && option.source == "local")
    );
    assert!(
        branches
            .ref_options
            .iter()
            .any(|option| option.name == "origin/remote-only" && option.source == "remote")
    );

    let validated =
        worktrees::validate_ref_for_project(&fixture.registry, 1, "origin/remote-only").await?;
    assert_eq!(validated.requested_ref, "origin/remote-only");
    assert_eq!(validated.resolved_ref, "origin/remote-only");
    assert_eq!(
        validated.commit,
        git(&fixture.main, &["rev-parse", "origin/remote-only"])?.trim()
    );
    let invalid = worktrees::validate_ref_for_project(&fixture.registry, 1, "missing/ref")
        .await
        .expect_err("unknown refs must fail before create");
    assert_eq!(invalid.code(), "invalid_branch");
    assert!(invalid.to_string().contains("was not found"));

    // A remote ref is a first-class base even when no same-named local branch
    // or remote-tracking ref exists. Validation fetches the exact ref so the
    // preview and the eventual worktree use the current remote commit.
    git(&fixture.main, &["checkout", "--detach", "origin/main"])?;
    git(&fixture.main, &["branch", "-D", "main"])?;
    std::fs::write(fixture.main.join("remote-main.txt"), "new remote main\n")?;
    git(&fixture.main, &["add", "remote-main.txt"])?;
    git(&fixture.main, &["commit", "-m", "advance remote main"])?;
    let remote_main = git(&fixture.main, &["rev-parse", "HEAD"])?;
    git(&fixture.main, &["push", "origin", "HEAD:main"])?;
    git(&fixture.main, &["reset", "--hard", "HEAD^"])?;
    git(
        &fixture.main,
        &["update-ref", "-d", "refs/remotes/origin/main"],
    )?;
    assert!(git(&fixture.main, &["branch", "--list", "main"])?.is_empty());
    assert!(git(&fixture.main, &["branch", "-r", "--list", "origin/main"])?.is_empty());

    let remote_validation =
        worktrees::validate_ref_for_project(&fixture.registry, 1, "origin/main").await?;
    assert_eq!(remote_validation.resolved_ref, "origin/main");
    assert_eq!(remote_validation.commit, remote_main.trim());
    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "qa/no-local-main".into(),
            display_name: Some("   ".into()),
            from_ref: Some("origin/main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([("herd_enabled".into(), "no".into())]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    assert_eq!(
        git(Path::new(&created.worktree.path), &["rev-parse", "HEAD"])?.trim(),
        remote_main.trim()
    );
    assert!(created.project.project.display_name.is_none());
    Ok(())
}

#[tokio::test]
async fn swm_semantics_cover_remote_discovery_adoption_and_safe_removal()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    let remote_refs_before_removal = git(
        &fixture.origin,
        &[
            "for-each-ref",
            "--format=%(refname):%(objectname)",
            "refs/heads",
        ],
    )?;
    let initial = worktrees::list_for_project(&fixture.registry, 1).await?;
    assert_eq!(initial.repository.name, "sample-repo");
    assert_eq!(initial.worktrees.len(), 1);
    assert_eq!(initial.worktrees[0].kind, "main");
    assert_eq!(initial.worktrees[0].branch, "main");

    let unconfirmed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: 1,
            confirm_remove: false,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: Some("main".into()),
        },
    )
    .await
    .expect_err("every project removal requires explicit confirmation");
    assert_eq!(unconfirmed.code(), "confirmation_required");
    assert!(fixture.main.exists());

    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "feature/new-ui".into(),
            display_name: Some("New UI checkout".into()),
            from_ref: Some("origin/main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([
                ("copy_env".into(), "no".into()),
                ("herd_enabled".into(), "no".into()),
            ]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    assert_eq!(created.project.project.name, "sample-repo: feature/new-ui");
    assert_eq!(
        created.project.project.display_name.as_deref(),
        Some("New UI checkout")
    );
    assert_eq!(created.project.parent_project_id, Some(1));
    assert_eq!(created.project.branch.as_deref(), Some("feature/new-ui"));
    assert!(created.project.worktree_managed);
    assert_eq!(created.worktree.kind, "managed");
    assert_eq!(
        Path::new(&created.worktree.path).file_name().unwrap(),
        "feature-new-ui"
    );
    assert_eq!(created.repository.preferences["copy_env"], "no");

    // "Fork again" must use the selected child worktree's exact HEAD, even
    // when that commit exists on neither the root branch nor a remote.
    std::fs::write(
        Path::new(&created.worktree.path).join("child-only.txt"),
        "selected HEAD\n",
    )?;
    git(
        Path::new(&created.worktree.path),
        &["add", "child-only.txt"],
    )?;
    git(
        Path::new(&created.worktree.path),
        &["commit", "-m", "child head"],
    )?;
    let selected_head = git(Path::new(&created.worktree.path), &["rev-parse", "HEAD"])?;
    let forked = worktrees::fork(
        &fixture.registry,
        ForkWorktree {
            source_project_id: created.project.project.id,
            branch: "feature/fork-again".into(),
            display_name: Some("Fork follow-up".into()),
            managed_root: None,
            preferences: BTreeMap::new(),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    assert_eq!(
        git(Path::new(&forked.worktree.path), &["rev-parse", "HEAD"])?.trim(),
        selected_head.trim()
    );
    assert_eq!(
        forked.project.project.display_name.as_deref(),
        Some("Fork follow-up")
    );

    // Existing SWM projects were registered before workman knew about worktree
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
            display_name: None,
            from_ref: None,
            managed_root: None,
            preferences: BTreeMap::from([("herd_enabled".into(), "no".into())]),
            env_policy: None,
            remember_env_policy: false,
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
    let remote_path = PathBuf::from(&remote.worktree.path);
    let unregistered = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: remote.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: false,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(unregistered.removed && unregistered.project_unregistered);
    assert!(!unregistered.deleted_from_disk && !unregistered.metadata_pruned);
    assert!(
        remote_path.exists(),
        "default removal must keep the checkout"
    );
    assert!(
        fixture
            .registry
            .lock()
            .await
            .store()
            .get_project_by_path_any(remote_path.to_str().unwrap())?
            .is_some(),
        "registration-only removal keeps the canonical project available to other profiles"
    );
    assert!(
        git(&fixture.main, &["worktree", "list", "--porcelain"])?
            .contains(remote_path.to_str().unwrap()),
        "default removal must keep Git worktree metadata"
    );
    let readopted = worktrees::adopt(
        &fixture.registry,
        AdoptWorktree {
            path: remote_path.clone(),
            display_name: None,
            preferences: BTreeMap::new(),
        },
    )
    .await?;
    assert_eq!(
        readopted.project.project.id, remote.project.project.id,
        "a kept checkout must reattach its canonical project instead of duplicating it"
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
            display_name: Some("Outside checkout".into()),
            preferences: BTreeMap::new(),
        },
    )
    .await?;
    assert_eq!(adopted.project.project.name, "sample-repo: outside-branch");
    assert_eq!(
        adopted.project.project.display_name.as_deref(),
        Some("Outside checkout")
    );
    assert_eq!(adopted.worktree.kind, "adopted");
    let unregistered_adopted = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: adopted.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: false,
            force_dirty: true,
            confirm_branch: Some("outside-branch".into()),
        },
    )
    .await?;
    assert!(unregistered_adopted.project_unregistered);
    assert!(!unregistered_adopted.deleted_from_disk);
    assert!(fixture.external.exists());
    fixture
        .registry
        .lock()
        .await
        .store()
        .put_project(&adopted.project.project)?;
    let deleted_adopted = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: adopted.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(deleted_adopted.deleted_from_disk && deleted_adopted.metadata_pruned);
    assert!(deleted_adopted.branch_kept);
    assert!(!fixture.external.exists());

    std::fs::write(
        Path::new(&created.worktree.path).join("dirty.txt"),
        "unsaved\n",
    )?;
    let pending = worktrees::list_for_project(&fixture.registry, 1).await?;
    let pending = pending
        .worktrees
        .iter()
        .find(|entry| entry.project_id == Some(created.project.project.id))
        .and_then(|entry| entry.delete_safety.as_ref())
        .expect("pending work has concrete deletion safety details");
    assert_eq!(pending.unpushed_subjects, ["child head"]);
    assert_eq!(pending.unmerged_subjects, ["child head"]);
    let dirty = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await
    .expect_err("dirty managed worktree requires an explicit force confirmation");
    assert_eq!(dirty.code(), "dirty_worktree");
    let dirty_warning = dirty.to_string();
    assert!(dirty_warning.contains("feature/new-ui"));
    assert!(dirty_warning.contains("dirty file(s)"));
    assert!(dirty_warning.contains("dirty.txt"));
    assert!(
        dirty_warning.contains("commit(s) not pushed")
            || dirty_warning.contains("commit(s) have no branch upstream")
    );
    assert!(dirty_warning.contains("commit(s) not merged"));
    assert!(Path::new(&created.worktree.path).exists());

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.removed && removed.project_unregistered && removed.branch_kept);
    assert!(removed.deleted_from_disk && removed.metadata_pruned);
    assert!(!Path::new(&removed.path).exists());
    assert!(
        !git(&fixture.main, &["worktree", "list", "--porcelain"])?.contains(&removed.path),
        "removed checkout must not remain in Git worktree metadata"
    );
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
    assert!(
        fixture
            .registry
            .lock()
            .await
            .store()
            .get_project_by_path_any(&removed.path)?
            .is_none(),
        "disk deletion must remove canonical registration from every profile"
    );

    assert_eq!(
        git(
            &fixture.origin,
            &[
                "for-each-ref",
                "--format=%(refname):%(objectname)",
                "refs/heads",
            ],
        )?,
        remote_refs_before_removal,
        "linked worktree removals must leave every scratch-remote branch ref untouched"
    );
    Ok(())
}

#[tokio::test]
async fn clean_merged_worktree_is_deleted_and_pruned_without_force() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "feature/merged-cleanly".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([
                ("copy_env".into(), "no".into()),
                ("herd_enabled".into(), "no".into()),
            ]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    let path = PathBuf::from(&created.worktree.path);
    std::fs::write(path.join("merged.txt"), "merged\n")?;
    git(&path, &["add", "merged.txt"])?;
    git(&path, &["commit", "-m", "merged worktree commit"])?;
    git(&path, &["push", "-u", "origin", "feature/merged-cleanly"])?;
    git(
        &fixture.main,
        &[
            "merge",
            "--no-ff",
            "feature/merged-cleanly",
            "-m",
            "merge feature",
        ],
    )?;
    git(&fixture.main, &["push", "origin", "main"])?;

    let listed = worktrees::list_for_project(&fixture.registry, 1).await?;
    let safety = listed
        .worktrees
        .iter()
        .find(|worktree| worktree.project_id == Some(created.project.project.id))
        .and_then(|worktree| worktree.delete_safety.as_ref())
        .expect("managed worktree has deletion safety details");
    assert_eq!(safety.dirty_files, 0);
    assert_eq!(safety.unpushed_commits, 0);
    assert_eq!(safety.unmerged_commits, 0);
    assert!(!safety.requires_force);

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.deleted_from_disk);
    assert!(removed.metadata_pruned);
    assert!(removed.project_unregistered);
    assert!(!path.exists());
    assert!(
        !git(&fixture.main, &["worktree", "list", "--porcelain"])?.contains(path.to_str().unwrap())
    );
    assert!(
        git(
            &fixture.origin,
            &["show-ref", "--verify", "refs/heads/feature/merged-cleanly"]
        )
        .is_ok(),
        "local deletion must leave the scratch remote branch untouched"
    );
    Ok(())
}

#[tokio::test]
async fn plain_project_removal_defaults_to_registration_only_then_deletes_exact_folder()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::Builder::new()
        .prefix("com.workman.todo126.plain.")
        .tempdir_in("/tmp")?;
    let folder = temp.path().join("plain-project");
    fs::create_dir(&folder)?;
    fs::write(folder.join("kept.txt"), "local fixture\n")?;
    let canonical = fs::canonicalize(&folder)?;
    let store = Store::open(temp.path().join("state.sqlite3"))?;
    let project = Project {
        id: 41,
        path: canonical.to_string_lossy().into_owned(),
        name: "plain-project".into(),
        display_name: Some("Plain Fixture".into()),
        icon: None,
        selected: true,
        sort_order: 0,
    };
    store.put_project(&project)?;
    let registry = Arc::new(Mutex::new(ProcessRegistry::new(store)?));

    let kept = worktrees::remove(
        &registry,
        RemoveWorktree {
            project_id: project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: false,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(kept.project_unregistered && !kept.deleted_from_disk);
    assert!(folder.join("kept.txt").is_file());

    registry.lock().await.store().put_project(&project)?;
    let deleted = worktrees::remove(
        &registry,
        RemoveWorktree {
            project_id: project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(deleted.project_unregistered && deleted.deleted_from_disk);
    assert!(!folder.exists());
    assert!(
        registry
            .lock()
            .await
            .store()
            .get_project_by_path_any(canonical.to_str().unwrap())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn plain_git_clone_deletion_removes_checkout_without_changing_scratch_remote_refs()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    let remote_refs_before = git(
        &fixture.origin,
        &[
            "for-each-ref",
            "--format=%(refname):%(objectname)",
            "refs/heads",
        ],
    )?;

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: 1,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;

    assert!(removed.deleted_from_disk && removed.project_unregistered);
    assert!(!fixture.main.exists());
    assert_eq!(
        git(
            &fixture.origin,
            &[
                "for-each-ref",
                "--format=%(refname):%(objectname)",
                "refs/heads",
            ],
        )?,
        remote_refs_before,
        "plain clone deletion must leave every scratch-remote branch ref untouched"
    );
    Ok(())
}

#[tokio::test]
async fn plain_folder_permission_failure_is_loud_keeps_registration_and_allows_retry()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::Builder::new()
        .prefix("com.workman.todo126.permission.")
        .tempdir_in("/tmp")?;
    let locked_parent = temp.path().join("locked-parent");
    let folder = locked_parent.join("plain-project");
    fs::create_dir_all(&folder)?;
    fs::write(folder.join("local.txt"), "must not be silently orphaned\n")?;
    let canonical = fs::canonicalize(&folder)?;
    let store = Store::open(temp.path().join("state.sqlite3"))?;
    let project = Project {
        id: 51,
        path: canonical.to_string_lossy().into_owned(),
        name: "plain-project".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };
    store.put_project(&project)?;
    let registry = Arc::new(Mutex::new(ProcessRegistry::new(store)?));

    fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o500))?;
    let attempted = worktrees::remove(
        &registry,
        RemoveWorktree {
            project_id: project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await;
    fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o700))?;

    let failure = attempted.expect_err("parent permissions must prevent directory removal");
    assert_eq!(failure.code(), "invalid_worktree_path");
    assert!(failure.to_string().contains("direct deletion"));
    assert!(failure.to_string().contains("remains registered"));
    assert!(folder.exists());
    assert!(
        registry
            .lock()
            .await
            .store()
            .get_project(project.id)?
            .is_some()
    );

    let retried = worktrees::remove(
        &registry,
        RemoveWorktree {
            project_id: project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(retried.deleted_from_disk && retried.project_unregistered);
    assert!(!folder.exists());
    Ok(())
}

#[tokio::test]
async fn trailing_separator_symlink_is_refused_and_keeps_target_and_registration()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::Builder::new()
        .prefix("com.workman.todo126.symlink.")
        .tempdir_in("/tmp")?;
    let target = temp.path().join("real-project");
    let alias = temp.path().join("project-link");
    fs::create_dir(&target)?;
    fs::write(target.join("local.txt"), "must survive\n")?;
    symlink(&target, &alias)?;
    let store = Store::open(temp.path().join("state.sqlite3"))?;
    let project = Project {
        id: 52,
        path: format!("{}/", alias.display()),
        name: "project-link".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };
    store.put_project(&project)?;
    let registry = Arc::new(Mutex::new(ProcessRegistry::new(store)?));

    let failure = worktrees::remove(
        &registry,
        RemoveWorktree {
            project_id: project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await
    .expect_err("a trailing separator must not bypass final-symlink refusal");

    assert_eq!(failure.code(), "invalid_worktree_path");
    assert!(failure.to_string().contains("not a real directory"));
    assert!(alias.is_symlink());
    assert_eq!(
        fs::read_to_string(target.join("local.txt"))?,
        "must survive\n"
    );
    assert!(
        registry
            .lock()
            .await
            .store()
            .get_project(project.id)?
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn trailing_separator_and_apfs_case_alias_delete_only_the_canonical_plain_folder()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::Builder::new()
        .prefix("com.workman.todo126.case.")
        .tempdir_in("/tmp")?;
    let folder = temp.path().join("CaseProject");
    fs::create_dir(&folder)?;
    fs::write(folder.join("local.txt"), "delete me\n")?;
    let case_alias = temp.path().join("caseproject");
    let registered = if case_alias.is_dir() {
        case_alias
    } else {
        folder.clone()
    };
    let store = Store::open(temp.path().join("state.sqlite3"))?;
    let project = Project {
        id: 53,
        path: format!("{}/", registered.display()),
        name: "CaseProject".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };
    store.put_project(&project)?;
    let registry = Arc::new(Mutex::new(ProcessRegistry::new(store)?));

    let removed = worktrees::remove(
        &registry,
        RemoveWorktree {
            project_id: project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.deleted_from_disk && removed.project_unregistered);
    assert!(!folder.exists());
    Ok(())
}

#[tokio::test]
async fn primary_checkout_with_linked_worktree_requires_force_and_never_changes_remote()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    let remote_refs_before_removal = git(
        &fixture.origin,
        &[
            "for-each-ref",
            "--format=%(refname):%(objectname)",
            "refs/heads",
        ],
    )?;
    git(
        &fixture.main,
        &[
            "worktree",
            "add",
            "-b",
            "dependent-local",
            fixture.external.to_str().unwrap(),
            "main",
        ],
    )?;

    let listed = worktrees::list_for_project(&fixture.registry, 1).await?;
    let safety = listed.worktrees[0]
        .delete_safety
        .as_ref()
        .expect("primary checkout has deletion safety");
    assert!(safety.requires_force);
    assert_eq!(safety.dependent_worktrees.len(), 1);
    assert!(safety.dependent_worktrees[0].contains("outside"));

    let refused = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: 1,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await
    .expect_err("dependent linked worktrees require force");
    assert_eq!(refused.code(), "dirty_worktree");
    assert!(refused.to_string().contains("linked worktree(s) depend"));
    assert!(fixture.main.exists());

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: 1,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.deleted_from_disk && removed.project_unregistered);
    assert!(!fixture.main.exists());
    assert!(
        fixture.external.exists(),
        "dependent checkout is warned about, not deleted"
    );
    assert_eq!(
        git(
            &fixture.origin,
            &[
                "for-each-ref",
                "--format=%(refname):%(objectname)",
                "refs/heads",
            ],
        )?,
        remote_refs_before_removal,
        "primary checkout deletion must leave every scratch-remote branch ref untouched"
    );
    Ok(())
}

#[tokio::test]
async fn ignored_local_files_require_force_before_deletion() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    std::fs::write(fixture.main.join(".gitignore"), ".env\n")?;
    git(&fixture.main, &["add", ".gitignore"])?;
    git(&fixture.main, &["commit", "-m", "ignore local env"])?;
    git(&fixture.main, &["push", "origin", "main"])?;
    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "ignored-local".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([
                ("copy_env".into(), "no".into()),
                ("herd_enabled".into(), "no".into()),
            ]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    let path = PathBuf::from(&created.worktree.path);
    std::fs::write(path.join(".env"), "LOCAL_SECRET=fixture\n")?;

    let refused = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: false,
            confirm_branch: None,
        },
    )
    .await
    .expect_err("ignored local files must require force");
    assert_eq!(refused.code(), "dirty_worktree");
    assert!(refused.to_string().contains("ignored local path(s)"));
    assert!(refused.to_string().contains(".env"));
    assert!(path.exists());

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.deleted_from_disk && removed.metadata_pruned);
    assert!(!path.exists());
    Ok(())
}

#[tokio::test]
async fn git_directory_not_empty_falls_back_to_verified_deletion_and_prunes()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new_with_failing_worktree_remove()?;
    fs::write(
        fixture.main.join(".gitignore"),
        "node_modules/\nvendor/\n.env\n",
    )?;
    git(&fixture.main, &["add", ".gitignore"])?;
    git(
        &fixture.main,
        &["commit", "-m", "ignore generated dependencies"],
    )?;
    git(&fixture.main, &["push", "origin", "main"])?;
    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "feature/vendor-junk".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([
                ("copy_env".into(), "no".into()),
                ("herd_enabled".into(), "no".into()),
            ]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    let path = PathBuf::from(&created.worktree.path);
    fs::create_dir_all(path.join("node_modules/pkg/cache"))?;
    fs::create_dir_all(path.join("vendor/bundle"))?;
    fs::write(path.join("node_modules/pkg/cache/blob"), "cache\n")?;
    fs::write(path.join("vendor/bundle/library"), "vendor\n")?;
    fs::write(path.join(".env"), "LOCAL_ONLY=1\n")?;
    fs::write(path.join("notes.tmp"), "untracked\n")?;

    let listed = worktrees::list_for_project(&fixture.registry, 1).await?;
    let safety = listed
        .worktrees
        .iter()
        .find(|entry| entry.project_id == Some(created.project.project.id))
        .and_then(|entry| entry.delete_safety.as_ref())
        .expect("ignored and untracked content is summarized before deletion");
    assert_eq!(safety.untracked_files, 1);
    assert!(safety.ignored_files >= 3);
    assert!(
        safety
            .ignored_paths
            .iter()
            .any(|entry| entry == "node_modules/")
    );
    assert!(safety.ignored_paths.iter().any(|entry| entry == "vendor/"));
    let remote_refs_before = git(
        &fixture.origin,
        &[
            "for-each-ref",
            "--format=%(refname):%(objectname)",
            "refs/heads",
        ],
    )?;

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.deleted_from_disk && removed.metadata_pruned);
    assert!(removed.branch_kept && removed.project_unregistered);
    assert!(!path.exists());
    assert!(!git(&fixture.main, &["worktree", "list", "--porcelain"])?.contains(&removed.path));
    assert_eq!(
        git(
            &fixture.origin,
            &[
                "for-each-ref",
                "--format=%(refname):%(objectname)",
                "refs/heads",
            ],
        )?,
        remote_refs_before,
        "fallback deletion must leave every scratch-remote ref untouched"
    );
    Ok(())
}

#[tokio::test]
async fn path_recreated_during_git_cleanup_fails_loudly_and_keeps_registration_for_retry()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new_with_path_recreated_during_prune()?;
    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "feature/reappearing-path".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([
                ("copy_env".into(), "no".into()),
                ("herd_enabled".into(), "no".into()),
            ]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    let path = PathBuf::from(&created.worktree.path);
    let remote_refs_before = git(
        &fixture.origin,
        &[
            "for-each-ref",
            "--format=%(refname):%(objectname)",
            "refs/heads",
        ],
    )?;

    let failure = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await
    .expect_err("a path recreated after initial deletion must prevent unregistration");

    assert_eq!(failure.code(), "invalid_worktree_path");
    assert!(failure.to_string().contains("reappeared"));
    assert!(failure.to_string().contains("remains registered"));
    assert_eq!(
        fs::read_to_string(path.join("reappeared.txt"))?,
        "replacement created after initial verification\n"
    );
    assert!(
        fixture
            .registry
            .lock()
            .await
            .store()
            .get_project(created.project.project.id)?
            .is_some(),
        "the registration must remain available for retry"
    );
    assert_eq!(
        git(
            &fixture.origin,
            &[
                "for-each-ref",
                "--format=%(refname):%(objectname)",
                "refs/heads",
            ],
        )?,
        remote_refs_before,
        "failed local deletion must not change scratch-remote refs"
    );

    let retried = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(retried.deleted_from_disk && retried.project_unregistered);
    assert!(!path.exists());
    assert_eq!(
        git(
            &fixture.origin,
            &[
                "for-each-ref",
                "--format=%(refname):%(objectname)",
                "refs/heads",
            ],
        )?,
        remote_refs_before,
        "retry must remain local-only"
    );
    Ok(())
}

#[tokio::test]
async fn failed_verified_deletion_keeps_registration_and_retries_cleanly()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new_with_retryable_path_swap()?;
    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "feature/retry-removal".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([
                ("copy_env".into(), "no".into()),
                ("herd_enabled".into(), "no".into()),
            ]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    let path = PathBuf::from(&created.worktree.path);
    let displaced = PathBuf::from(format!("{}.todo126-retry", path.display()));

    let failure = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await
    .expect_err("a path swap must stop the direct deletion fallback");
    assert_eq!(failure.code(), "invalid_worktree_path");
    assert!(fs::symlink_metadata(&path)?.file_type().is_symlink());
    assert!(displaced.is_dir());
    assert!(
        fixture
            .registry
            .lock()
            .await
            .store()
            .get_project(created.project.project.id)?
            .is_some(),
        "registration remains until the verified directory is actually gone"
    );

    fs::remove_file(&path)?;
    fs::rename(&displaced, &path)?;
    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.deleted_from_disk && removed.project_unregistered);
    assert!(!path.exists());
    Ok(())
}

#[tokio::test]
async fn retry_recovers_after_git_already_dropped_linked_worktree_metadata()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    let created = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "feature/metadata-retry".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([
                ("copy_env".into(), "no".into()),
                ("herd_enabled".into(), "no".into()),
            ]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await?;
    let path = PathBuf::from(&created.worktree.path);
    fs::write(path.join("remaining.tmp"), "left after partial deletion\n")?;
    fs::remove_file(path.join(".git"))?;
    git(&fixture.main, &["worktree", "prune"])?;
    assert!(path.exists());
    assert!(
        fixture
            .registry
            .lock()
            .await
            .store()
            .get_project(created.project.project.id)?
            .is_some()
    );

    let removed = worktrees::remove(
        &fixture.registry,
        RemoveWorktree {
            project_id: created.project.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: true,
            force_dirty: true,
            confirm_branch: None,
        },
    )
    .await?;
    assert!(removed.deleted_from_disk && removed.metadata_pruned);
    assert!(removed.branch_kept && removed.project_unregistered);
    assert!(!path.exists());
    Ok(())
}

#[tokio::test]
async fn ignored_environment_is_asked_once_copied_and_rewritten_safely()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    std::fs::write(fixture.main.join(".gitignore"), ".env\n")?;
    git(&fixture.main, &["add", ".gitignore"])?;
    git(&fixture.main, &["commit", "-m", "ignore local env"])?;
    git(&fixture.main, &["push", "origin", "main"])?;
    std::fs::write(
        fixture.main.join(".env"),
        "APP_NAME=Original\nAPP_URL=http://original.test\nSECRET=fixture\n",
    )?;

    let required = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "env/needs-choice".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([("herd_enabled".into(), "no".into())]),
            env_policy: None,
            remember_env_policy: false,
        },
    )
    .await
    .expect_err("the first ignored .env requires a repository choice");
    assert_eq!(required.code(), "env_preference_required");
    assert!(!fixture.managed.join("env-needs-choice").exists());

    let initial_head = git(&fixture.main, &["rev-parse", "HEAD^"])?;
    git(
        &fixture.main,
        &["branch", "env-target-does-not-ignore", initial_head.trim()],
    )?;
    let target_unsafe = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "env-target-does-not-ignore".into(),
            display_name: None,
            from_ref: None,
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([("herd_enabled".into(), "no".into())]),
            env_policy: Some(EnvPortPolicy::Copy),
            remember_env_policy: false,
        },
    )
    .await
    .expect_err("target branches that do not ignore .env must be rolled back");
    assert_eq!(target_unsafe.code(), "unsafe_env_file");
    assert!(target_unsafe.to_string().contains("target branch"));
    assert!(!fixture.managed.join("env-target-does-not-ignore").exists());

    let copied = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "env/copied".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([("herd_enabled".into(), "no".into())]),
            env_policy: Some(EnvPortPolicy::Copy),
            remember_env_policy: true,
        },
    )
    .await?;
    let copied_env = std::fs::read_to_string(Path::new(&copied.worktree.path).join(".env"))?;
    assert!(copied_env.contains("APP_NAME=\"env-copied\""));
    assert!(copied_env.contains("APP_URL=http://original.test"));
    assert!(copied_env.contains("SECRET=fixture"));
    let result = copied.environment.expect("environment receipt");
    assert!(result.copied && result.app_name_rewritten && !result.app_url_rewritten);
    assert_eq!(copied.repository.preferences["copy_env"], "yes");

    let receipt = worktrees::forget_env_preference(&fixture.registry, 1).await?;
    assert!(receipt.cleared);
    let listed = worktrees::list_for_project(&fixture.registry, 1).await?;
    assert!(!listed.repository.preferences.contains_key("copy_env"));
    Ok(())
}

#[tokio::test]
async fn environment_porting_refuses_nonignored_and_tracked_files() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    std::fs::write(fixture.main.join(".env"), "APP_NAME=Unsafe\n")?;
    let nonignored = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "env/nonignored".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([("herd_enabled".into(), "no".into())]),
            env_policy: Some(EnvPortPolicy::Copy),
            remember_env_policy: false,
        },
    )
    .await
    .expect_err("nonignored .env must be refused");
    assert_eq!(nonignored.code(), "unsafe_env_file");
    assert!(nonignored.to_string().contains("does not ignore"));

    git(&fixture.main, &["add", ".env"])?;
    git(&fixture.main, &["commit", "-m", "tracked env fixture"])?;
    let tracked = worktrees::create(
        &fixture.registry,
        CreateWorktree {
            source_project_id: 1,
            branch: "env/tracked".into(),
            display_name: None,
            from_ref: Some("main".into()),
            managed_root: Some(fixture.managed.clone()),
            preferences: BTreeMap::from([("herd_enabled".into(), "no".into())]),
            env_policy: Some(EnvPortPolicy::Copy),
            remember_env_policy: false,
        },
    )
    .await
    .expect_err("tracked .env must be refused");
    assert_eq!(tracked.code(), "unsafe_env_file");
    assert!(tracked.to_string().contains("tracks"));
    Ok(())
}

#[tokio::test]
async fn pr_lookup_uses_login_path_for_plain_and_managed_worktrees_and_refreshes_errors()
-> Result<(), Box<dyn Error>> {
    let fixture = tempfile::tempdir()?;
    let origin = fixture.path().join("origin.git");
    let main = fixture.path().join("plain-repo");
    let managed = fixture.path().join("managed-feature");
    git(
        fixture.path(),
        &["init", "--bare", origin.to_str().unwrap()],
    )?;
    git(
        fixture.path(),
        &["init", "-b", "main", main.to_str().unwrap()],
    )?;
    git(&main, &["config", "user.email", "fixture@example.test"])?;
    git(&main, &["config", "user.name", "Fixture"])?;
    fs::write(main.join("README.md"), "fixture\n")?;
    git(&main, &["add", "README.md"])?;
    git(&main, &["commit", "-m", "initial"])?;
    git(
        &main,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    )?;
    git(&main, &["push", "-u", "origin", "main"])?;
    git(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "feature/managed",
            managed.to_str().unwrap(),
            "main",
        ],
    )?;
    assert_eq!(
        git(&main, &["rev-parse", "--abbrev-ref", "@{upstream}"])?.trim(),
        "origin/main"
    );

    let store = Store::open(fixture.path().join("state.sqlite3"))?;
    for (id, path, name, selected) in [
        (1, &main, "plain-repo", true),
        (2, &managed, "plain-repo: feature/managed", false),
    ] {
        store.put_project(&Project {
            id,
            path: fs::canonicalize(path)?.to_string_lossy().into_owned(),
            name: name.into(),
            display_name: None,
            icon: None,
            selected,
            sort_order: id,
        })?;
    }
    worktrees::reconcile_existing_projects(&store)?;
    let mut managed_link = store
        .get_project_worktree(2)?
        .expect("linked worktree metadata");
    managed_link.parent_project_id = Some(1);
    managed_link.managed = true;
    store.put_project_worktree(&managed_link)?;

    let profile_bin = fixture.path().join("profile-bin");
    fs::create_dir(&profile_bin)?;
    let git_executable = executable_on_test_path("git").ok_or("test git executable missing")?;
    symlink(git_executable, profile_bin.join("git"))?;
    let shell = fixture.path().join("fixture-shell");
    fs::write(
        &shell,
        format!(
            "#!/bin/sh\nexport PATH='{}'\nshift\nexec /bin/sh \"$@\"\n",
            profile_bin.display()
        ),
    )?;
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o700))?;
    let config = fixture.path().join("config.yml");
    fs::write(
        &config,
        format!("terminal:\n  shell: {:?}\n", shell.to_string_lossy()),
    )?;
    let registry = Arc::new(Mutex::new(ProcessRegistry::with_user_environment(
        store,
        UserEnvironmentResolver::new(config),
    )?));

    let unavailable = worktrees::list_for_project_refresh(&registry, 1, true).await?;
    assert!(!unavailable.pull_requests.available);
    assert!(
        unavailable
            .pull_requests
            .error
            .as_deref()
            .is_some_and(|error| error.contains("resolved user PATH"))
    );

    let gh = profile_bin.join("gh");
    fs::write(
        &gh,
        "#!/bin/sh\nprintf '%s\\n' '[{\"number\":51,\"title\":\"Main change\",\"state\":\"OPEN\",\"isDraft\":false,\"headRefName\":\"main\",\"url\":\"https://example.test/pr/51\",\"mergeable\":\"MERGEABLE\",\"statusCheckRollup\":[]},{\"number\":50,\"title\":\"Earlier main change\",\"state\":\"MERGED\",\"isDraft\":false,\"headRefName\":\"main\",\"url\":\"https://example.test/pr/50\",\"mergeable\":\"UNKNOWN\",\"statusCheckRollup\":[]},{\"number\":52,\"title\":\"Managed change\",\"state\":\"OPEN\",\"isDraft\":false,\"headRefName\":\"feature/managed\",\"url\":\"https://example.test/pr/52\",\"mergeable\":\"MERGEABLE\",\"statusCheckRollup\":[]}]'\n",
    )?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o700))?;

    let cached = worktrees::list_for_project(&registry, 1).await?;
    assert!(
        !cached.pull_requests.available,
        "ordinary reads retain the cache"
    );
    let recovered = worktrees::list_for_project_refresh(&registry, 1, true).await?;
    assert!(recovered.pull_requests.available);
    assert_eq!(recovered.pull_requests.error, None);
    let plain = recovered
        .worktrees
        .iter()
        .find(|entry| entry.kind == "main")
        .expect("plain repository row");
    assert_eq!(plain.pull_request.as_ref().map(|pr| pr.number), Some(51));
    assert_eq!(plain.pull_requests.len(), 2);
    assert_eq!(plain.pull_requests[0].title, "Main change");
    assert_eq!(plain.pull_requests[1].state, "merged");
    let managed = recovered
        .worktrees
        .iter()
        .find(|entry| entry.kind == "managed")
        .expect("managed worktree row");
    assert_eq!(managed.pull_request.as_ref().map(|pr| pr.number), Some(52));
    assert_eq!(managed.pull_requests.len(), 1);
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
