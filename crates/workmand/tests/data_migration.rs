use std::{env, error::Error, ffi::OsString, fs};

use workman_core::{AgentTool, AgentToolSource, Project, Store};
use workmand::{DATABASE_FILE, DaemonConfig, DaemonServer, default_data_dir};

struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    fn fixture(home: &std::path::Path, config: &std::path::Path) -> Self {
        let names = [
            "HOME",
            "WORKMAN_CONFIG",
            "WORKMAN_DATA_DIR",
            "XDG_DATA_HOME",
        ];
        let previous = names
            .iter()
            .map(|name| (*name, env::var_os(name)))
            .collect();
        // SAFETY: this integration binary contains one test, so no sibling test
        // can observe the temporary process environment.
        unsafe {
            env::set_var("HOME", home);
            env::set_var("WORKMAN_CONFIG", config);
            env::remove_var("WORKMAN_DATA_DIR");
            env::remove_var("XDG_DATA_HOME");
        }
        Self(previous)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            // SAFETY: see `fixture`; this binary has no parallel tests.
            unsafe {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }
}

#[tokio::test]
async fn first_default_boot_prefers_awm_then_falls_back_to_gbuild() -> Result<(), Box<dyn Error>> {
    run_migration_case("awm", "awm.sqlite3", true).await?;
    run_migration_case("gbuild", "gbuild.sqlite3", false).await?;
    Ok(())
}

async fn run_migration_case(
    source_name: &str,
    source_database: &str,
    add_gbuild_decoy: bool,
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    fs::create_dir_all(&home)?;
    let destination = platform_data_dir(&home, "workman");
    let legacy = platform_data_dir(&home, source_name);
    fs::create_dir_all(&legacy)?;
    let workspace = temp.path().join("workspace");
    fs::create_dir(&workspace)?;

    let store = Store::open(legacy.join(source_database))?;
    store.put_project(&Project {
        id: 17,
        path: workspace.to_string_lossy().into_owned(),
        name: format!("migrated-from-{source_name}"),
        display_name: Some("Migrated Project".into()),
        icon: Some("rocket".into()),
        selected: true,
        sort_order: 0,
    })?;
    store.put_agent_tool(&AgentTool {
        id: 41,
        name: "Local survivor".into(),
        command: "local-agent --safe".into(),
        tool_type: "custom".into(),
        enabled: true,
        source: AgentToolSource::Local,
    })?;
    let legacy_tool_count = store.list_agent_tools()?.len();
    drop(store);
    fs::write(
        legacy.join("config.yml"),
        format!(
            "agent_tools:\n  - name: Config survivor {source_name}\n    command: config-agent\n    tool_type: future-tool\n    enabled: true\n"
        ),
    )?;
    fs::write(
        legacy.join("daemon.json"),
        r#"{"port":1,"token":"stale","pid":1}"#,
    )?;

    if add_gbuild_decoy {
        let decoy = platform_data_dir(&home, "gbuild");
        fs::create_dir_all(&decoy)?;
        let decoy_store = Store::open(decoy.join("gbuild.sqlite3"))?;
        decoy_store.put_project(&Project {
            id: 99,
            path: workspace.to_string_lossy().into_owned(),
            name: "must-not-win".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
    }

    let _environment = EnvGuard::fixture(&home, &destination.join("config.yml"));
    assert_eq!(default_data_dir(), destination);
    let server = DaemonServer::bind(DaemonConfig::default()).await?;

    assert!(legacy.join(source_database).is_file());
    assert!(destination.join(DATABASE_FILE).is_file());
    assert_eq!(
        fs::read_to_string(destination.join("config.yml"))?,
        fs::read_to_string(legacy.join("config.yml"))?
    );
    let discovery = server.discovery();
    assert_ne!(discovery.port, 1);
    assert_ne!(discovery.token, "stale");
    assert_eq!(discovery.pid, std::process::id());

    let registry = server.registry();
    let registry = registry.lock().await;
    let projects = registry.store().list_projects()?;
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, format!("migrated-from-{source_name}"));
    assert_eq!(
        projects[0].display_name.as_deref(),
        Some("Migrated Project")
    );
    let tools = registry.store().list_agent_tools()?;
    assert_eq!(tools.len(), legacy_tool_count + 1);
    assert!(
        tools
            .iter()
            .any(|tool| { tool.name == "Local survivor" && tool.source == AgentToolSource::Local })
    );
    assert!(tools.iter().any(|tool| {
        tool.name == format!("Config survivor {source_name}")
            && tool.source == AgentToolSource::Config
    }));

    Ok(())
}

fn platform_data_dir(home: &std::path::Path, app_name: &str) -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support").join(app_name)
    } else {
        home.join(".local/share").join(app_name)
    }
}
