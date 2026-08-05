//! Cached update checks and verified installs exposed through the daemon control channel.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use awm_core::{
    DEFAULT_RELEASES_API, UPDATE_CHECK_INTERVAL_SECS, UpdateCheck, UpdateClient, UpdateError,
    UpdateInstallReport, install_dir_from_executable,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

const CACHE_FILE: &str = "updates.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UpdateStatus {
    pub automatic_checks: bool,
    pub last_checked_at: Option<i64>,
    pub check: UpdateCheck,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct UpdateCache {
    #[serde(default = "enabled")]
    automatic_checks: bool,
    last_checked_at: Option<i64>,
    last_check: Option<UpdateCheck>,
}

impl Default for UpdateCache {
    fn default() -> Self {
        Self {
            automatic_checks: true,
            last_checked_at: None,
            last_check: None,
        }
    }
}

fn enabled() -> bool {
    true
}

#[derive(Clone)]
pub(crate) struct UpdateService {
    client: UpdateClient,
    current_version: &'static str,
    install_dir: PathBuf,
    cache_path: PathBuf,
    cache: Arc<Mutex<UpdateCache>>,
    operation: Arc<AsyncMutex<()>>,
}

impl UpdateService {
    pub(crate) fn new(data_dir: &Path) -> Result<Self, UpdateError> {
        let api_url =
            env::var("AWM_RELEASES_API_URL").unwrap_or_else(|_| DEFAULT_RELEASES_API.to_owned());
        let client = UpdateClient::new(api_url)?;
        let install_dir = match env::var_os("AWM_UPDATE_INSTALL_DIR") {
            Some(path) => PathBuf::from(path),
            None => install_dir_from_executable(env::current_exe()?)?,
        };
        let cache_path = data_dir.join(CACHE_FILE);
        let cache = read_cache(&cache_path).unwrap_or_default();
        Ok(Self {
            client,
            current_version: env!("CARGO_PKG_VERSION"),
            install_dir,
            cache_path,
            cache: Arc::new(Mutex::new(cache)),
            operation: Arc::new(AsyncMutex::new(())),
        })
    }

    pub(crate) fn status(&self) -> UpdateStatus {
        let cache = self.cache.lock().expect("update cache lock poisoned");
        status_from_cache(&cache, self.current_version)
    }

    pub(crate) async fn check(&self, force: bool) -> Result<UpdateStatus, UpdateError> {
        let _operation = self.operation.lock().await;
        {
            let cache = self.cache.lock().expect("update cache lock poisoned");
            if !force && !automatic_check_due(&cache, now()) {
                return Ok(status_from_cache(&cache, self.current_version));
            }
        }

        let check = self.client.check(self.current_version).await?;
        let mut cache = self.cache.lock().expect("update cache lock poisoned");
        cache.last_checked_at = Some(check.checked_at);
        cache.last_check = Some(check);
        write_cache(&self.cache_path, &cache)?;
        Ok(status_from_cache(&cache, self.current_version))
    }

    pub(crate) fn set_automatic_checks(&self, enabled: bool) -> Result<UpdateStatus, UpdateError> {
        let mut cache = self.cache.lock().expect("update cache lock poisoned");
        cache.automatic_checks = enabled;
        write_cache(&self.cache_path, &cache)?;
        Ok(status_from_cache(&cache, self.current_version))
    }

    pub(crate) async fn install(&self) -> Result<UpdateInstallReport, UpdateError> {
        let status = self.check(true).await?;
        self.client.install(&status.check, &self.install_dir).await
    }
}

fn automatic_check_due(cache: &UpdateCache, current_time: i64) -> bool {
    cache.automatic_checks
        && cache.last_checked_at.is_none_or(|checked| {
            current_time.saturating_sub(checked) >= UPDATE_CHECK_INTERVAL_SECS
        })
}

fn status_from_cache(cache: &UpdateCache, current_version: &str) -> UpdateStatus {
    UpdateStatus {
        automatic_checks: cache.automatic_checks,
        last_checked_at: cache.last_checked_at,
        check: cache
            .last_check
            .clone()
            .unwrap_or_else(|| UpdateCheck::current(current_version)),
    }
}

fn read_cache(path: &Path) -> io::Result<UpdateCache> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_cache(path: &Path, cache: &UpdateCache) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(cache).expect("update cache serializes"),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_defaults_to_weekly_checks_and_persists_opt_out() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CACHE_FILE);
        let mut cache = read_cache(&path).unwrap_or_default();
        assert!(cache.automatic_checks);
        cache.automatic_checks = false;
        write_cache(&path, &cache).unwrap();
        assert!(!read_cache(&path).unwrap().automatic_checks);
    }

    #[test]
    fn automatic_check_is_due_only_after_the_weekly_cache_interval() {
        let mut cache = UpdateCache::default();
        assert!(automatic_check_due(&cache, 1_000_000));
        cache.last_checked_at = Some(1_000_000);
        assert!(!automatic_check_due(
            &cache,
            1_000_000 + UPDATE_CHECK_INTERVAL_SECS - 1
        ));
        assert!(automatic_check_due(
            &cache,
            1_000_000 + UPDATE_CHECK_INTERVAL_SECS
        ));
        cache.automatic_checks = false;
        assert!(!automatic_check_due(&cache, i64::MAX));
    }
}
