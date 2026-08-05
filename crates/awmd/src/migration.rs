//! One-time migration from the pre-rename gbuild application directories.

use std::{
    env,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{DATABASE_FILE, DISCOVERY_FILE, user_config};

const LEGACY_APP_NAME: &str = "gbuild";
const LEGACY_DATABASE_FILE: &str = "gbuild.sqlite3";

/// Migrate the platform-default pre-rename data and user config on first awm boot.
pub(crate) fn migrate_default_paths_if_needed(data_dir: &Path) -> io::Result<()> {
    let using_default_data =
        env::var_os("AWM_DATA_DIR").is_none() && data_dir == platform_data_dir("awm");
    if !using_default_data {
        return Ok(());
    }

    let legacy = platform_data_dir(LEGACY_APP_NAME);
    if migrate_legacy_data_dir(&legacy, data_dir)? {
        eprintln!(
            "awm: migrated legacy data from {} to {}; daemon.json will be regenerated",
            legacy.display(),
            data_dir.display()
        );
    }

    if env::var_os(user_config::AWM_CONFIG_ENV).is_none() {
        let destination = user_config::default_user_config_path("awm");
        let legacy = user_config::default_user_config_path(LEGACY_APP_NAME);
        if migrate_legacy_user_config(&legacy, &destination)? {
            eprintln!(
                "awm: migrated legacy user config from {} to {}",
                legacy.display(),
                destination.display()
            );
        }
    }

    Ok(())
}

pub(crate) fn platform_data_dir(app_name: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(app_name)
}

/// Copy a legacy data directory into an absent or empty awm directory.
///
/// The copy is staged beside `destination` and renamed into place so a failed
/// copy never leaves a partially migrated database. The old discovery record
/// is intentionally omitted; the new daemon always publishes its own token,
/// port, and PID. SQLite files are renamed to the awm filename as they move.
pub fn migrate_legacy_data_dir(legacy: &Path, destination: &Path) -> io::Result<bool> {
    if !legacy.is_dir() || !destination_is_absent_or_empty(destination)? {
        return Ok(false);
    }
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "awm data directory must have a parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!(
        ".awm-migration-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    fs::create_dir(&stage)?;

    let copied = copy_directory_contents(legacy, &stage, true);
    if let Err(error) = copied {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    let destination_existed = destination.exists();
    if destination_existed {
        fs::remove_dir(destination)?;
    }
    if let Err(error) = fs::rename(&stage, destination) {
        if destination_existed {
            let _ = fs::create_dir(destination);
        }
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    Ok(true)
}

fn destination_is_absent_or_empty(path: &Path) -> io::Result<bool> {
    match fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn copy_directory_contents(source: &Path, destination: &Path, root: bool) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let original_name = entry.file_name();
        if root && original_name == OsStr::new(DISCOVERY_FILE) {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("refusing to migrate symlink {}", entry.path().display()),
            ));
        }
        let migrated_name = if root {
            migrated_root_name(&original_name)
        } else {
            original_name
        };
        let target = destination.join(migrated_name);
        if file_type.is_dir() {
            fs::create_dir(&target)?;
            copy_directory_contents(&entry.path(), &target, false)?;
            fs::set_permissions(&target, entry.metadata()?.permissions())?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
            fs::set_permissions(&target, entry.metadata()?.permissions())?;
        }
    }
    Ok(())
}

fn migrated_root_name(name: &OsStr) -> OsString {
    let Some(name) = name.to_str() else {
        return name.to_owned();
    };
    if name == LEGACY_DATABASE_FILE {
        return OsString::from(DATABASE_FILE);
    }
    if let Some(suffix) = name.strip_prefix(LEGACY_DATABASE_FILE) {
        return OsString::from(format!("{DATABASE_FILE}{suffix}"));
    }
    OsString::from(name)
}

fn migrate_legacy_user_config(legacy: &Path, destination: &Path) -> io::Result<bool> {
    if destination.exists() || !legacy.is_file() {
        return Ok(false);
    }
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "awm user config path must have a parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".config.yml.{}.{}.tmp",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    fs::copy(legacy, &temporary)?;
    fs::set_permissions(&temporary, fs::metadata(legacy)?.permissions())?;
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_renames_sqlite_and_excludes_discovery() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("gbuild");
        let destination = root.path().join("awm");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join(LEGACY_DATABASE_FILE), b"database").unwrap();
        fs::write(legacy.join(format!("{LEGACY_DATABASE_FILE}-wal")), b"wal").unwrap();
        fs::write(legacy.join("config.yml"), b"agent_tools: []\n").unwrap();
        fs::write(legacy.join(DISCOVERY_FILE), b"stale").unwrap();

        assert!(migrate_legacy_data_dir(&legacy, &destination).unwrap());
        assert_eq!(
            fs::read(destination.join(DATABASE_FILE)).unwrap(),
            b"database"
        );
        assert_eq!(
            fs::read(destination.join(format!("{DATABASE_FILE}-wal"))).unwrap(),
            b"wal"
        );
        assert!(destination.join("config.yml").is_file());
        assert!(!destination.join(DISCOVERY_FILE).exists());
        assert!(legacy.join(LEGACY_DATABASE_FILE).is_file());
        assert!(!migrate_legacy_data_dir(&legacy, &destination).unwrap());
    }
}
