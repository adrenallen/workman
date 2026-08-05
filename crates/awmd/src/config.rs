//! Parsing, validation, synchronization, and trust hashing for `awm.yml`.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs, io,
    path::{Component, Path, PathBuf},
};

use awm_core::{Process, ProcessId, ProcessKind, ProcessSource, ProcessStatus, ProjectId, Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{ProcessRegistry, RegistryError};

/// Repository-local command configuration filename.
pub const AWM_CONFIG_FILE: &str = "awm.yml";

/// Pre-rename repository config accepted for one compatibility release.
pub const LEGACY_GBUILD_CONFIG_FILE: &str = "gbuild.yml";

/// Parsed top-level `awm.yml` document.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AwmConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub processes: BTreeMap<String, YmlProcess>,
}

/// One command process declared by name in `awm.yml`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct YmlProcess {
    pub command: String,
    #[serde(
        default = "default_working_dir",
        skip_serializing_if = "is_default_working_dir"
    )]
    pub working_dir: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_start: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_restart: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restart_when_changed: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// Canonical working-directory paths returned to the add-command UI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ValidatedWorkingDirectory {
    pub absolute: String,
    pub relative: String,
}

/// IDs affected by one source-of-truth synchronization pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncReport {
    pub project_id: ProjectId,
    pub created: Vec<ProcessId>,
    pub updated: Vec<ProcessId>,
    pub removed: Vec<ProcessId>,
    pub started: Vec<ProcessId>,
    pub awaiting_trust: Vec<ProcessId>,
}

/// The exact configuration fields covered by a YAML process approval.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TrustFields {
    pub command: Option<String>,
    pub working_dir: String,
    pub env: BTreeMap<String, String>,
    pub auto_start: bool,
    pub auto_restart: bool,
    pub restart_when_changed: Vec<String>,
}

impl TrustFields {
    pub fn from_process(process: &Process) -> Self {
        Self {
            command: process.command.clone(),
            working_dir: process.working_dir.clone(),
            env: process.env.clone(),
            auto_start: process.auto_start,
            auto_restart: process.auto_restart,
            restart_when_changed: process.restart_when_changed.clone(),
        }
    }
}

/// One trust-relevant field changed since the last approval.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrustFieldChange {
    pub field: String,
    pub previous: Option<Value>,
    pub current: Value,
}

/// Review payload shown before approving one YAML-backed command.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrustReview {
    pub process_id: ProcessId,
    pub process_name: String,
    pub trusted: bool,
    pub expected_hash: String,
    pub fields: TrustFields,
    pub changes: Vec<TrustFieldChange>,
}

/// Errors raised before or during YAML synchronization.
#[derive(Debug)]
pub enum ConfigError {
    Parse(serde_yaml::Error),
    Registry(RegistryError),
    ProjectNotFound(ProjectId),
    InvalidProjectName,
    InvalidProcessName,
    MissingCommand(String),
    WrittenProcessMissing(String),
    LocalNameConflict(String),
    ReadConfig {
        path: PathBuf,
        source: io::Error,
    },
    WriteConfig {
        path: PathBuf,
        source: io::Error,
    },
    ParentTraversal {
        process: String,
        path: String,
    },
    WorkingDirectory {
        process: String,
        path: PathBuf,
        source: io::Error,
    },
    NotDirectory {
        process: String,
        path: PathBuf,
    },
    OutsideProject {
        process: String,
        path: PathBuf,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "invalid awm.yml: {error}"),
            Self::Registry(error) => error.fmt(formatter),
            Self::ProjectNotFound(id) => write!(formatter, "project {id} was not found"),
            Self::InvalidProjectName => formatter.write_str("project name must not be empty"),
            Self::InvalidProcessName => formatter.write_str("process name must not be empty"),
            Self::MissingCommand(name) => {
                write!(formatter, "awm.yml process {name:?} has an empty command")
            }
            Self::WrittenProcessMissing(name) => {
                write!(
                    formatter,
                    "written awm.yml process {name:?} was not synchronized"
                )
            }
            Self::LocalNameConflict(name) => write!(
                formatter,
                "awm.yml process {name:?} conflicts with a local process"
            ),
            Self::ReadConfig { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::WriteConfig { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
            Self::ParentTraversal { process, path } => write!(
                formatter,
                "awm.yml process {process:?} working_dir {path:?} contains '..'"
            ),
            Self::WorkingDirectory {
                process,
                path,
                source,
            } => write!(
                formatter,
                "cannot resolve working_dir {} for awm.yml process {process:?}: {source}",
                path.display()
            ),
            Self::NotDirectory { process, path } => write!(
                formatter,
                "working_dir {} for awm.yml process {process:?} is not a directory",
                path.display()
            ),
            Self::OutsideProject { process, path } => write!(
                formatter,
                "working_dir {} for awm.yml process {process:?} is outside the project root",
                path.display()
            ),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::ReadConfig { source, .. } => Some(source),
            Self::WriteConfig { source, .. } => Some(source),
            Self::WorkingDirectory { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<serde_yaml::Error> for ConfigError {
    fn from(error: serde_yaml::Error) -> Self {
        Self::Parse(error)
    }
}

impl From<RegistryError> for ConfigError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

/// Parse YAML, treating an empty document as an empty configuration.
pub fn parse_awm_yml(yaml: &str) -> Result<AwmConfig, ConfigError> {
    if yaml.trim().is_empty() {
        Ok(AwmConfig::default())
    } else {
        Ok(serde_yaml::from_str(yaml)?)
    }
}

/// Resolve the canonical config, falling back to the pre-rename filename.
pub fn project_config_path(root: &Path) -> Option<PathBuf> {
    let canonical = root.join(AWM_CONFIG_FILE);
    if canonical.is_file() {
        return Some(canonical);
    }
    let legacy = root.join(LEGACY_GBUILD_CONFIG_FILE);
    legacy.is_file().then_some(legacy)
}

fn warn_legacy_config(path: &Path) {
    eprintln!(
        "awm: {} is deprecated; rename it to awm.yml",
        path.display()
    );
}

/// Resolve an add-command working directory to canonical absolute and project-relative paths.
pub fn validate_project_working_dir(
    store: &Store,
    project_id: ProjectId,
    configured: &str,
) -> Result<ValidatedWorkingDirectory, ConfigError> {
    let project = store
        .get_project(project_id)
        .map_err(RegistryError::from)?
        .ok_or(ConfigError::ProjectNotFound(project_id))?;
    let root = canonical_directory("<project>", Path::new(&project.path))?;
    let configured = configured.trim();
    let configured = if configured.is_empty() {
        "."
    } else {
        configured
    };
    let absolute = resolve_working_dir("<new command>", &root, configured)?;
    let relative = absolute
        .strip_prefix(&root)
        .expect("validated command working directory is contained by the project root");
    Ok(ValidatedWorkingDirectory {
        absolute: absolute.to_string_lossy().into_owned(),
        relative: if relative.as_os_str().is_empty() {
            ".".into()
        } else {
            relative.to_string_lossy().into_owned()
        },
    })
}

/// Add or update one user-authored YAML command, trust that exact write, and honor auto-start.
pub fn write_awm_yml_command(
    registry: &mut ProcessRegistry,
    project_id: ProjectId,
    name: String,
    command: String,
    working_dir: String,
    auto_start: bool,
    auto_restart: bool,
) -> Result<Process, ConfigError> {
    let project = registry
        .store()
        .get_project(project_id)
        .map_err(RegistryError::from)?
        .ok_or(ConfigError::ProjectNotFound(project_id))?;
    let root = canonical_directory("<project>", Path::new(&project.path))?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(ConfigError::InvalidProcessName);
    }
    let command = command.trim().to_owned();
    if command.is_empty() {
        return Err(ConfigError::MissingCommand(name));
    }
    let validated = validate_project_working_dir(registry.store(), project_id, &working_dir)?;
    let path = root.join(AWM_CONFIG_FILE);
    let source_path = project_config_path(&root);
    let mut config = if let Some(source_path) = source_path {
        if source_path.file_name() == Some(LEGACY_GBUILD_CONFIG_FILE.as_ref()) {
            warn_legacy_config(&source_path);
        }
        let yaml = fs::read_to_string(&source_path).map_err(|source| ConfigError::ReadConfig {
            path: source_path,
            source,
        })?;
        parse_awm_yml(&yaml)?
    } else {
        AwmConfig {
            name: Some(project.name.clone()),
            ..AwmConfig::default()
        }
    };
    let retained = config.processes.get(&name);
    let restart_when_changed = retained
        .map(|process| process.restart_when_changed.clone())
        .unwrap_or_default();
    let env = retained
        .map(|process| process.env.clone())
        .unwrap_or_default();
    config.processes.insert(
        name.clone(),
        YmlProcess {
            command,
            working_dir: validated.relative,
            auto_start,
            auto_restart,
            restart_when_changed,
            env,
        },
    );

    if config
        .name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(ConfigError::InvalidProjectName);
    }
    let prepared = prepare_processes(project_id, &root, config.processes.clone())?;
    let existing = registry.list(Some(project_id))?;
    for process in &prepared {
        if existing.iter().any(|existing| {
            existing.name == process.name && existing.source == ProcessSource::Local
        }) {
            return Err(ConfigError::LocalNameConflict(process.name.clone()));
        }
    }

    let yaml = serde_yaml::to_string(&config)?;
    fs::write(&path, &yaml).map_err(|source| ConfigError::WriteConfig {
        path: path.clone(),
        source,
    })?;
    sync_awm_yml(registry, project_id, &yaml)?;

    let process = registry
        .list(Some(project_id))?
        .into_iter()
        .find(|process| process.source == ProcessSource::Yml && process.name == name)
        .ok_or_else(|| ConfigError::WrittenProcessMissing(name.clone()))?;
    let expected_hash = trust_hash_for_process(&process);
    Ok(registry.trust_yml_process(process.id, &expected_hash)?)
}

/// Read `<project root>/awm.yml` and synchronize it into the registry.
pub fn sync_awm_yml_file(
    registry: &mut ProcessRegistry,
    project_id: ProjectId,
) -> Result<SyncReport, ConfigError> {
    let project = registry
        .store()
        .get_project(project_id)
        .map_err(RegistryError::from)?
        .ok_or(ConfigError::ProjectNotFound(project_id))?;
    let root = Path::new(&project.path);
    let path = project_config_path(root).unwrap_or_else(|| root.join(AWM_CONFIG_FILE));
    if path.file_name() == Some(LEGACY_GBUILD_CONFIG_FILE.as_ref()) {
        warn_legacy_config(&path);
    }
    let yaml = fs::read_to_string(&path).map_err(|source| ConfigError::ReadConfig {
        path: path.clone(),
        source,
    })?;
    sync_awm_yml(registry, project_id, &yaml)
}

/// Make YAML-backed command rows exactly match one validated `awm.yml` document.
pub fn sync_awm_yml(
    registry: &mut ProcessRegistry,
    project_id: ProjectId,
    yaml: &str,
) -> Result<SyncReport, ConfigError> {
    let config = parse_awm_yml(yaml)?;
    let mut project = registry
        .store()
        .get_project(project_id)
        .map_err(RegistryError::from)?
        .ok_or(ConfigError::ProjectNotFound(project_id))?;
    let root = canonical_directory("<project>", Path::new(&project.path))?;
    let prepared = prepare_processes(project_id, &root, config.processes)?;
    let existing = registry.list(Some(project_id))?;

    for process in &prepared {
        if existing.iter().any(|existing| {
            existing.name == process.name && existing.source == ProcessSource::Local
        }) {
            return Err(ConfigError::LocalNameConflict(process.name.clone()));
        }
    }

    if let Some(name) = config.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(ConfigError::InvalidProjectName);
        }
        project.name = name.to_owned();
    }
    if let Some(icon) = config.icon {
        project.icon = Some(icon);
    }
    registry
        .store()
        .put_project(&project)
        .map_err(RegistryError::from)?;

    let desired_names = prepared
        .iter()
        .map(|process| process.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut report = SyncReport {
        project_id,
        ..SyncReport::default()
    };

    for process in existing.iter().filter(|process| {
        process.source == ProcessSource::Yml && !desired_names.contains(process.name.as_str())
    }) {
        registry.close(process.id)?;
        report.removed.push(process.id);
    }

    for desired in prepared {
        let existing = existing
            .iter()
            .find(|process| process.source == ProcessSource::Yml && process.name == desired.name);
        let process = if let Some(current) = existing {
            let changed = trust_hash_for_process(current) != trust_hash_for_process(&desired);
            if changed && is_active(current.status) {
                registry.stop(current.id)?;
            }
            let mut desired = desired;
            desired.id = current.id;
            desired.trust_hash = current.trust_hash.clone();
            let process = registry.update(desired)?;
            report.updated.push(process.id);
            process
        } else {
            let process = registry.create(desired)?;
            report.created.push(process.id);
            process
        };

        if !is_process_trusted(&process) {
            report.awaiting_trust.push(process.id);
        } else if process.auto_start && !is_active(process.status) {
            let process = registry.start(process.id)?;
            report.started.push(process.id);
        }
    }

    Ok(report)
}

/// Calculate the canonical approval hash for one process's trust-relevant fields.
pub fn trust_hash_for_process(process: &Process) -> String {
    let fields = TrustFields::from_process(process);
    let bytes = serde_json::to_vec(&fields).expect("trust fields always serialize");
    let digest = Sha256::digest(bytes);
    let mut hash = String::with_capacity(7 + digest.len() * 2);
    hash.push_str("sha256:");
    for byte in digest {
        use fmt::Write as _;
        write!(&mut hash, "{byte:02x}").expect("writing to String cannot fail");
    }
    hash
}

/// A YAML process is trusted only while its stored approval equals its current hash.
pub fn is_process_trusted(process: &Process) -> bool {
    process.source != ProcessSource::Yml
        || process.trust_hash.as_deref() == Some(trust_hash_for_process(process).as_str())
}

/// Revalidate the canonical persisted working directory immediately before launch.
pub(crate) fn validate_process_working_dir(store: &Store, process: &Process) -> Result<(), String> {
    let project = store
        .get_project(process.project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("project {} was not found", process.project_id))?;
    let root = fs::canonicalize(&project.path)
        .map_err(|error| format!("cannot resolve project root: {error}"))?;
    let configured = Path::new(&process.working_dir);
    if configured
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err("working_dir contains '..'".into());
    }
    let actual = fs::canonicalize(configured)
        .map_err(|error| format!("cannot resolve working_dir: {error}"))?;
    if !actual.is_dir() {
        return Err("working_dir is not a directory".into());
    }
    if !actual.starts_with(&root) {
        return Err("working_dir is outside the project root".into());
    }
    if actual != configured {
        return Err("working_dir no longer resolves to its reviewed canonical path".into());
    }
    Ok(())
}

fn prepare_processes(
    project_id: ProjectId,
    root: &Path,
    processes: BTreeMap<String, YmlProcess>,
) -> Result<Vec<Process>, ConfigError> {
    processes
        .into_iter()
        .map(|(name, config)| {
            let name = name.trim().to_owned();
            if name.is_empty() {
                return Err(ConfigError::InvalidProcessName);
            }
            if config.command.trim().is_empty() {
                return Err(ConfigError::MissingCommand(name));
            }
            let working_dir = resolve_working_dir(&name, root, &config.working_dir)?;
            Ok(Process {
                id: 0,
                project_id,
                kind: ProcessKind::Command,
                name,
                command: Some(config.command),
                working_dir: working_dir.to_string_lossy().into_owned(),
                env: config.env,
                auto_start: config.auto_start,
                auto_restart: config.auto_restart,
                restart_when_changed: config.restart_when_changed,
                source: ProcessSource::Yml,
                trust_hash: None,
                status: ProcessStatus::Stopped,
                pid: None,
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
        })
        .collect()
}

fn resolve_working_dir(
    process: &str,
    root: &Path,
    configured: &str,
) -> Result<PathBuf, ConfigError> {
    let configured_path = Path::new(configured);
    if configured_path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(ConfigError::ParentTraversal {
            process: process.into(),
            path: configured.into(),
        });
    }
    let candidate = if configured_path.is_absolute() {
        configured_path.to_owned()
    } else {
        root.join(configured_path)
    };
    let canonical = canonical_directory(process, &candidate)?;
    if !canonical.starts_with(root) {
        return Err(ConfigError::OutsideProject {
            process: process.into(),
            path: canonical,
        });
    }
    Ok(canonical)
}

fn canonical_directory(process: &str, path: &Path) -> Result<PathBuf, ConfigError> {
    let canonical = fs::canonicalize(path).map_err(|source| ConfigError::WorkingDirectory {
        process: process.into(),
        path: path.to_owned(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(ConfigError::NotDirectory {
            process: process.into(),
            path: canonical,
        });
    }
    Ok(canonical)
}

fn default_working_dir() -> String {
    ".".into()
}

fn is_default_working_dir(value: &String) -> bool {
    value == "."
}

fn is_false(value: &bool) -> bool {
    !value
}

fn is_active(status: ProcessStatus) -> bool {
    matches!(status, ProcessStatus::Starting | ProcessStatus::Running)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use awm_core::{ProcessSource, Project, Store};
    use tempfile::TempDir;

    use super::*;

    struct Fixture {
        root: TempDir,
        outside: TempDir,
        registry: ProcessRegistry,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("frontend")).unwrap();
            let store = Store::open_in_memory().unwrap();
            store
                .put_project(&Project {
                    id: 1,
                    path: root.path().to_string_lossy().into_owned(),
                    name: "fixture".into(),
                    display_name: None,
                    icon: None,
                    selected: true,
                    sort_order: 0,
                })
                .unwrap();
            Self {
                root,
                outside,
                registry: ProcessRegistry::new(store).unwrap(),
            }
        }

        fn process(&mut self, name: &str) -> Process {
            self.registry
                .list(Some(1))
                .unwrap()
                .into_iter()
                .find(|process| process.name == name)
                .unwrap()
        }
    }

    #[test]
    fn parser_defaults_missing_processes_and_ignores_unknown_keys() {
        let parsed = parse_awm_yml(
            "name: Demo\nicon: rocket\nunknown: ignored\nprocesses:\n  Web:\n    command: npm run dev\n    extra: ignored\n",
        )
        .unwrap();
        assert_eq!(parsed.name.as_deref(), Some("Demo"));
        assert_eq!(parsed.icon.as_deref(), Some("rocket"));
        assert_eq!(parsed.processes["Web"].working_dir, ".");
        assert!(!parsed.processes["Web"].auto_start);

        let parsed = parse_awm_yml("name: Empty\n").unwrap();
        assert!(parsed.processes.is_empty());
        assert_eq!(parse_awm_yml("").unwrap(), AwmConfig::default());
    }

    #[test]
    fn legacy_gbuild_yml_falls_back_and_awm_yml_takes_precedence() {
        let mut fixture = Fixture::new();
        fs::write(
            fixture.root.path().join(LEGACY_GBUILD_CONFIG_FILE),
            "processes:\n  Legacy:\n    command: printf legacy\n",
        )
        .unwrap();

        sync_awm_yml_file(&mut fixture.registry, 1).unwrap();
        assert_eq!(
            fixture.process("Legacy").command.as_deref(),
            Some("printf legacy")
        );

        fs::write(
            fixture.root.path().join(AWM_CONFIG_FILE),
            "processes:\n  Canonical:\n    command: printf canonical\n",
        )
        .unwrap();
        sync_awm_yml_file(&mut fixture.registry, 1).unwrap();
        assert_eq!(
            fixture.process("Canonical").command.as_deref(),
            Some("printf canonical")
        );
        assert!(
            fixture
                .registry
                .list(Some(1))
                .unwrap()
                .iter()
                .all(|process| process.name != "Legacy")
        );
    }

    #[test]
    fn in_app_write_imports_legacy_config_into_canonical_awm_yml() {
        let mut fixture = Fixture::new();
        fs::write(
            fixture.root.path().join(LEGACY_GBUILD_CONFIG_FILE),
            "name: Legacy project\nprocesses:\n  Existing:\n    command: printf keep\n",
        )
        .unwrap();

        write_awm_yml_command(
            &mut fixture.registry,
            1,
            "New".into(),
            "printf new".into(),
            ".".into(),
            false,
            false,
        )
        .unwrap();

        let canonical = fixture.root.path().join(AWM_CONFIG_FILE);
        let parsed = parse_awm_yml(&fs::read_to_string(canonical).unwrap()).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("Legacy project"));
        assert!(parsed.processes.contains_key("Existing"));
        assert!(parsed.processes.contains_key("New"));
        assert!(
            fixture
                .root
                .path()
                .join(LEGACY_GBUILD_CONFIG_FILE)
                .is_file()
        );
    }

    #[test]
    fn in_app_write_creates_trusted_command_and_external_edit_revokes_trust() {
        let mut fixture = Fixture::new();
        let running = write_awm_yml_command(
            &mut fixture.registry,
            1,
            "Web".into(),
            "trap 'exit 0' TERM; sleep 30".into(),
            "frontend".into(),
            true,
            false,
        )
        .unwrap();
        assert_eq!(running.status, ProcessStatus::Running);
        assert!(is_process_trusted(&running));

        let path = fixture.root.path().join(AWM_CONFIG_FILE);
        let written = fs::read_to_string(&path).unwrap();
        let parsed = parse_awm_yml(&written).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("fixture"));
        assert_eq!(
            parsed.processes["Web"].command,
            "trap 'exit 0' TERM; sleep 30"
        );
        assert_eq!(parsed.processes["Web"].working_dir, "frontend");
        assert!(parsed.processes["Web"].auto_start);

        fs::write(
            &path,
            "name: fixture\nprocesses:\n  Web:\n    command: printf externally-edited\n    working_dir: frontend\n    auto_start: true\n",
        )
        .unwrap();
        sync_awm_yml_file(&mut fixture.registry, 1).unwrap();
        let pending = fixture.process("Web");
        assert_eq!(pending.id, running.id);
        assert_eq!(pending.status, ProcessStatus::Stopped);
        assert!(pending.trust_hash.is_none());
        let review = fixture.registry.trust_review(pending.id).unwrap();
        assert!(!review.trusted);
        assert_eq!(review.changes.len(), 1);
        assert_eq!(review.changes[0].field, "command");
    }

    #[test]
    fn in_app_write_preserves_other_yml_entries_and_updates_by_name() {
        let mut fixture = Fixture::new();
        let path = fixture.root.path().join(AWM_CONFIG_FILE);
        fs::write(
            &path,
            "name: Existing project\nicon: ship\nprocesses:\n  Existing:\n    command: printf keep\n  Deploy:\n    command: printf old\n    env:\n      TOKEN: retained\n    restart_when_changed:\n      - src/**\n",
        )
        .unwrap();

        let updated = write_awm_yml_command(
            &mut fixture.registry,
            1,
            "Deploy".into(),
            "printf new".into(),
            "".into(),
            false,
            true,
        )
        .unwrap();
        assert!(is_process_trusted(&updated));
        assert!(updated.auto_restart);

        let parsed = parse_awm_yml(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("Existing project"));
        assert_eq!(parsed.icon.as_deref(), Some("ship"));
        assert_eq!(parsed.processes["Existing"].command, "printf keep");
        assert_eq!(parsed.processes["Deploy"].command, "printf new");
        assert!(parsed.processes["Deploy"].auto_restart);
        assert_eq!(parsed.processes["Deploy"].env["TOKEN"], "retained");
        assert_eq!(parsed.processes["Deploy"].restart_when_changed, ["src/**"]);
        assert_eq!(parsed.processes.len(), 2);
    }

    #[test]
    fn add_command_working_directory_validation_returns_both_path_forms() {
        let fixture = Fixture::new();
        let validated =
            validate_project_working_dir(fixture.registry.store(), 1, "frontend").unwrap();
        assert_eq!(
            validated.absolute,
            fs::canonicalize(fixture.root.path().join("frontend"))
                .unwrap()
                .to_string_lossy()
        );
        assert_eq!(validated.relative, "frontend");

        let outside = fixture.outside.path().to_string_lossy().into_owned();
        assert!(matches!(
            validate_project_working_dir(fixture.registry.store(), 1, &outside),
            Err(ConfigError::OutsideProject { .. })
        ));
    }

    #[test]
    fn sync_is_source_of_truth_only_for_yml_processes() {
        let mut fixture = Fixture::new();
        fixture
            .registry
            .create(Process {
                id: 10,
                project_id: 1,
                kind: ProcessKind::Command,
                name: "local".into(),
                command: Some("true".into()),
                working_dir: fixture.root.path().to_string_lossy().into_owned(),
                env: BTreeMap::new(),
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
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .unwrap();
        let report = sync_awm_yml(
            &mut fixture.registry,
            1,
            "name: Synced\nicon: terminal\nprocesses:\n  Web:\n    command: printf web\n    working_dir: frontend\n    env: { NODE_ENV: development }\n  Worker:\n    command: printf worker\n",
        )
        .unwrap();
        assert_eq!(report.created.len(), 2);
        assert_eq!(report.awaiting_trust.len(), 2);
        let web = fixture.process("Web");
        assert_eq!(web.source, ProcessSource::Yml);
        assert_eq!(
            web.working_dir,
            fs::canonicalize(fixture.root.path().join("frontend"))
                .unwrap()
                .to_string_lossy()
        );
        assert!(matches!(
            fixture.registry.start(web.id),
            Err(RegistryError::Untrusted(_))
        ));
        let bulk = fixture.registry.start_all_commands(1);
        assert!(bulk.failures.iter().any(|failure| {
            failure.process_id == web.id && failure.code == "process_untrusted"
        }));

        let report = sync_awm_yml(&mut fixture.registry, 1, "processes: {}\n").unwrap();
        assert_eq!(report.removed.len(), 2);
        let remaining = fixture.registry.list(Some(1)).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "local");
        let project = fixture.registry.store().get_project(1).unwrap().unwrap();
        assert_eq!(project.name, "Synced");
        assert_eq!(project.icon.as_deref(), Some("terminal"));
    }

    #[test]
    fn trust_is_preserved_only_while_relevant_fields_match() {
        let mut fixture = Fixture::new();
        let original = "processes:\n  Server:\n    command: sleep 30\n    auto_start: true\n";
        let first_sync = sync_awm_yml(&mut fixture.registry, 1, original).unwrap();
        assert!(first_sync.started.is_empty());
        let pending = fixture.process("Server");
        assert_eq!(pending.status, ProcessStatus::Stopped);
        let hash = trust_hash_for_process(&pending);
        let running = fixture
            .registry
            .trust_yml_process(pending.id, &hash)
            .unwrap();
        assert_eq!(running.status, ProcessStatus::Running);
        assert_eq!(running.trust_hash.as_deref(), Some(hash.as_str()));

        let unchanged = sync_awm_yml(&mut fixture.registry, 1, original).unwrap();
        assert!(unchanged.awaiting_trust.is_empty());
        assert_eq!(fixture.process("Server").status, ProcessStatus::Running);

        let changed = sync_awm_yml(
            &mut fixture.registry,
            1,
            "processes:\n  Server:\n    command: printf changed\n    auto_start: true\n",
        )
        .unwrap();
        assert_eq!(changed.awaiting_trust.len(), 1);
        let pending = fixture.process("Server");
        assert_eq!(pending.status, ProcessStatus::Stopped);
        assert!(pending.trust_hash.is_none());
        assert!(matches!(
            fixture.registry.start(pending.id),
            Err(RegistryError::Untrusted(_))
        ));
        assert!(matches!(
            fixture.registry.trust_yml_process(pending.id, &hash),
            Err(RegistryError::TrustHashMismatch(_))
        ));
    }

    #[test]
    fn trust_hash_covers_exactly_the_reviewed_fields() {
        let fixture = Fixture::new();
        let base = Process {
            id: 1,
            project_id: 1,
            kind: ProcessKind::Command,
            name: "name".into(),
            command: Some("command".into()),
            working_dir: fs::canonicalize(fixture.root.path())
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            env: BTreeMap::from([("A".into(), "1".into())]),
            auto_start: false,
            auto_restart: false,
            restart_when_changed: vec!["src/**".into()],
            source: ProcessSource::Yml,
            trust_hash: None,
            status: ProcessStatus::Stopped,
            pid: None,
            exit_code: None,
            exit_signal: None,
            exited_at: None,
            agent_tool_id: None,
            spawned_by_process_id: None,
            sort_order: 0,
        };
        let hash = trust_hash_for_process(&base);
        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), 71);

        let variants = [
            Process {
                command: Some("changed".into()),
                ..base.clone()
            },
            Process {
                working_dir: fixture
                    .root
                    .path()
                    .join("frontend")
                    .to_string_lossy()
                    .into(),
                ..base.clone()
            },
            Process {
                env: BTreeMap::from([("A".into(), "2".into())]),
                ..base.clone()
            },
            Process {
                auto_start: true,
                ..base.clone()
            },
            Process {
                auto_restart: true,
                ..base.clone()
            },
            Process {
                restart_when_changed: vec!["other/**".into()],
                ..base.clone()
            },
        ];
        for variant in variants {
            assert_ne!(trust_hash_for_process(&variant), hash);
        }
        assert_eq!(
            trust_hash_for_process(&Process {
                name: "renamed".into(),
                ..base.clone()
            }),
            hash
        );
    }

    #[test]
    fn working_directory_rejects_parent_absolute_and_symlink_escapes() {
        let mut fixture = Fixture::new();
        let parent = sync_awm_yml(
            &mut fixture.registry,
            1,
            "processes:\n  Bad:\n    command: true\n    working_dir: ../outside\n",
        )
        .unwrap_err();
        assert!(matches!(parent, ConfigError::ParentTraversal { .. }));

        let absolute = format!(
            "processes:\n  Bad:\n    command: true\n    working_dir: {}\n",
            fixture.outside.path().display()
        );
        assert!(matches!(
            sync_awm_yml(&mut fixture.registry, 1, &absolute),
            Err(ConfigError::OutsideProject { .. })
        ));

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fixture.outside.path(), fixture.root.path().join("escape"))
                .unwrap();
            assert!(matches!(
                sync_awm_yml(
                    &mut fixture.registry,
                    1,
                    "processes:\n  Bad:\n    command: true\n    working_dir: escape\n"
                ),
                Err(ConfigError::OutsideProject { .. })
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn launch_revalidates_a_reviewed_directory_after_symlink_retargeting() {
        let mut fixture = Fixture::new();
        sync_awm_yml(
            &mut fixture.registry,
            1,
            "processes:\n  Server:\n    command: true\n    working_dir: frontend\n",
        )
        .unwrap();
        let pending = fixture.process("Server");
        let hash = trust_hash_for_process(&pending);
        fixture
            .registry
            .trust_yml_process(pending.id, &hash)
            .unwrap();

        fs::remove_dir(fixture.root.path().join("frontend")).unwrap();
        std::os::unix::fs::symlink(fixture.outside.path(), fixture.root.path().join("frontend"))
            .unwrap();
        assert!(matches!(
            fixture.registry.start(pending.id),
            Err(RegistryError::InvalidWorkingDirectory { .. })
        ));
    }
}
