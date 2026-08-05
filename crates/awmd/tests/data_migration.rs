use std::{env, error::Error, ffi::OsString, fs};

use awm_core::{AgentTool, AgentToolSource, Project, Store};
use awmd::{DATABASE_FILE, DaemonConfig, DaemonServer, default_data_dir};

struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    fn fixture(home: &std::path::Path, config: &std::path::Path) -> Self {
        let names = ["HOME", "AWM_CONFIG", "AWM_DATA_DIR", "XDG_DATA_HOME"];
        let previous = names
            .iter()
            .map(|name| (*name, env::var_os(name)))
            .collect();
        // SAFETY: this integration binary contains one test, so no sibling test
        // can observe the temporary process environment.
        unsafe {
            env::set_var("HOME", home);
            env::set_var("AWM_CONFIG", config);
            env::remove_var("AWM_DATA_DIR");
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
async fn first_default_boot_migrates_projects_agents_config_and_regenerates_discovery()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    fs::create_dir_all(&home)?;
    let destination = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/awm")
    } else {
        home.join(".local/share/awm")
    };
    let legacy = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/gbuild")
    } else {
        home.join(".local/share/gbuild")
    };
    fs::create_dir_all(&legacy)?;
    let workspace = temp.path().join("workspace");
    fs::create_dir(&workspace)?;

    let store = Store::open(legacy.join("gbuild.sqlite3"))?;
    store.put_project(&Project {
        id: 17,
        path: workspace.to_string_lossy().into_owned(),
        name: "migrated-project".into(),
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
        "agent_tools:\n  - name: Config survivor\n    command: config-agent\n    tool_type: future-tool\n    enabled: true\n",
    )?;
    fs::write(
        legacy.join("daemon.json"),
        r#"{"port":1,"token":"stale","pid":1}"#,
    )?;

    let _environment = EnvGuard::fixture(&home, &destination.join("config.yml"));
    assert_eq!(default_data_dir(), destination);
    let server = DaemonServer::bind(DaemonConfig::default()).await?;

    assert!(legacy.join("gbuild.sqlite3").is_file());
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
    assert_eq!(projects[0].name, "migrated-project");
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
    assert!(
        tools.iter().any(|tool| {
            tool.name == "Config survivor" && tool.source == AgentToolSource::Config
        })
    );

    Ok(())
}
