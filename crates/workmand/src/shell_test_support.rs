//! Hermetic fixtures for native shell behavior, shared by daemon unit tests.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn installed_shells() -> Vec<PathBuf> {
    let mut shells = ["/bin/zsh", "/bin/bash"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    let fish = std::env::var_os("WORKMAN_TEST_FISH")
        .map(PathBuf::from)
        .or_else(|| {
            crate::runtime_doctor::resolve_executable(
                "fish",
                &std::env::var_os("PATH").unwrap_or_default(),
            )
        });
    if let Some(fish) = fish {
        assert!(fish.is_file(), "test fish must exist: {}", fish.display());
        shells.push(fish);
    }
    shells
}

pub struct ShellFixture {
    pub home: tempfile::TempDir,
    pub shell: PathBuf,
    pub config: PathBuf,
    pub variables: BTreeMap<OsString, OsString>,
}

impl ShellFixture {
    pub fn new(shell: &Path) -> Self {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join(".zshenv"), "unsetopt GLOBAL_RCS\n").unwrap();
        let posix_rc = concat!(
            "case $- in *i*) ;; *) return ;; esac\n",
            "workman_fixture_fn() { printf ran > \"$HOME/executed\"; printf 'ARGS:<%s>\\n' \"$@\"; }\n",
            "alias workman-fixture-alias='workman_fixture_fn \"alias arg\"'\n",
        );
        fs::write(home.path().join(".zshrc"), posix_rc).unwrap();
        fs::write(home.path().join(".bashrc"), posix_rc).unwrap();
        fs::write(
            home.path().join(".bash_profile"),
            "alias workman-profile-alias='printf profile'\n. \"$HOME/.bashrc\"\n",
        )
        .unwrap();
        fs::create_dir(home.path().join("fish")).unwrap();
        fs::write(
            home.path().join("fish/config.fish"),
            concat!(
                "if status is-interactive\n",
                "  function workman_fixture_fn\n",
                "    printf ran > \"$HOME/executed\"\n",
                "    printf 'ARGS:<%s>\\n' $argv\n",
                "  end\n",
                "  alias workman-fixture-alias 'workman_fixture_fn \"alias arg\"'\n",
                "end\n",
            ),
        )
        .unwrap();
        let mut variables = BTreeMap::new();
        for key in ["HOME", "ZDOTDIR", "XDG_CONFIG_HOME"] {
            variables.insert(OsString::from(key), home.path().as_os_str().to_owned());
        }
        for key in ["ENV", "BASH_ENV"] {
            variables.insert(OsString::from(key), OsString::from("/dev/null"));
        }
        variables.insert(
            OsString::from("PATH"),
            std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
        );
        variables.insert(OsString::from("TERM"), OsString::from("xterm-256color"));
        // A same-name wrapper also isolates asynchronous environment captures before a PTY
        // exists, without changing HOME or SHELL in the test runner's global environment.
        let bin = home.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let wrapper = bin.join(shell.file_name().unwrap());
        let mut script = String::from("#!/bin/sh\n");
        for (key, value) in &variables {
            script.push_str(&format!(
                "export {}={}\n",
                key.to_string_lossy(),
                workman_core::shell::quote_word(Path::new("/bin/sh"), &value.to_string_lossy())
            ));
        }
        script.push_str(&format!(
            "exec {} \"$@\"\n",
            workman_core::shell::quote_word(Path::new("/bin/sh"), &shell.to_string_lossy())
        ));
        fs::write(&wrapper, script).unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
        let config = home.path().join("workman.yml");
        fs::write(&config, format!("terminal:\n  shell: {:?}\n", wrapper)).unwrap();
        Self {
            home,
            shell: shell.to_owned(),
            config,
            variables,
        }
    }
}
