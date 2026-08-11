//! Cached update checks and verified installs exposed through the daemon control channel.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;
use workman_core::{
    DEFAULT_RELEASES_API, LATEST_RELEASES_API, UPDATE_CHECK_INTERVAL_SECS, UpdateChannel,
    UpdateCheck, UpdateClient, UpdateError, UpdateInstallReport, UpdateInstallTarget,
};

use crate::{RuntimeIdentity, user_config::resolve_update_key};

const CACHE_FILE: &str = "updates.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub automatic_checks: bool,
    pub channel: UpdateChannel,
    pub last_checked_at: Option<i64>,
    pub check: UpdateCheck,
    pub cli_recovery_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct UpdateCache {
    #[serde(default = "enabled")]
    automatic_checks: bool,
    #[serde(default)]
    channel: UpdateChannel,
    last_checked_at: Option<i64>,
    last_check: Option<UpdateCheck>,
}

impl Default for UpdateCache {
    fn default() -> Self {
        Self {
            automatic_checks: true,
            channel: UpdateChannel::Stable,
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
    stable_client: UpdateClient,
    latest_client: UpdateClient,
    current_version: &'static str,
    install_target: UpdateInstallTarget,
    cache_path: PathBuf,
    cache: Arc<Mutex<UpdateCache>>,
    operation: Arc<AsyncMutex<()>>,
    updates_enabled: bool,
}

impl UpdateService {
    pub(crate) fn new(data_dir: &Path) -> Result<Self, UpdateError> {
        let stable_api_url = env::var("WORKMAN_RELEASES_API_URL")
            .unwrap_or_else(|_| DEFAULT_RELEASES_API.to_owned());
        let latest_api_url = env::var("WORKMAN_LATEST_RELEASES_API_URL")
            .unwrap_or_else(|_| LATEST_RELEASES_API.to_owned());
        let update_key = resolve_update_key(None).map_err(|error| {
            UpdateError::InvalidRelease(format!("update key configuration: {error}"))
        })?;
        let stable_client = UpdateClient::new_for_channel(stable_api_url, UpdateChannel::Stable)?
            .with_key(&update_key)?;
        let latest_client = UpdateClient::new_for_channel(latest_api_url, UpdateChannel::Latest)?
            .with_key(&update_key)?;
        let install_target = match env::var_os("WORKMAN_UPDATE_INSTALL_DIR") {
            Some(path) => UpdateInstallTarget::binary_directory(PathBuf::from(path)),
            None => UpdateInstallTarget::discover(env::current_exe()?)?,
        };
        let cache_path = data_dir.join(CACHE_FILE);
        let updates_enabled = !RuntimeIdentity::current().is_dev();
        let mut cache = read_cache(&cache_path).unwrap_or_default();
        if !updates_enabled {
            cache.automatic_checks = false;
            cache.last_checked_at = None;
            cache.last_check = None;
        }
        Ok(Self {
            stable_client,
            latest_client,
            current_version: env!("CARGO_PKG_VERSION"),
            install_target,
            cache_path,
            cache: Arc::new(Mutex::new(cache)),
            operation: Arc::new(AsyncMutex::new(())),
            updates_enabled,
        })
    }

    pub(crate) fn status(&self) -> UpdateStatus {
        let cache = self.cache.lock().expect("update cache lock poisoned");
        status_from_cache(
            &cache,
            self.current_version,
            self.install_target.cli_recovery_required(),
        )
    }

    pub(crate) async fn check(&self, force: bool) -> Result<UpdateStatus, UpdateError> {
        self.check_with_key(force, None).await
    }

    pub(crate) async fn check_with_key(
        &self,
        force: bool,
        key_override: Option<&str>,
    ) -> Result<UpdateStatus, UpdateError> {
        if !self.updates_enabled {
            return Ok(self.status());
        }
        let _operation = self.operation.lock().await;
        {
            let cache = self.cache.lock().expect("update cache lock poisoned");
            if !force && !automatic_check_due(&cache, now()) {
                return Ok(status_from_cache(
                    &cache,
                    self.current_version,
                    self.install_target.cli_recovery_required(),
                ));
            }
        }

        let channel = self
            .cache
            .lock()
            .expect("update cache lock poisoned")
            .channel;
        let override_client = key_override
            .map(|key| self.client(channel).clone().with_key(key))
            .transpose()?;
        let client = override_client
            .as_ref()
            .unwrap_or_else(|| self.client(channel));
        let check = client.check(self.current_version).await?;
        let mut cache = self.cache.lock().expect("update cache lock poisoned");
        if cache.channel != channel {
            return Ok(status_from_cache(
                &cache,
                self.current_version,
                self.install_target.cli_recovery_required(),
            ));
        }
        cache.last_checked_at = Some(check.checked_at);
        cache.last_check = Some(check);
        write_cache(&self.cache_path, &cache)?;
        Ok(status_from_cache(
            &cache,
            self.current_version,
            self.install_target.cli_recovery_required(),
        ))
    }

    pub(crate) fn set_preferences(
        &self,
        automatic_checks: Option<bool>,
        channel: Option<UpdateChannel>,
    ) -> Result<UpdateStatus, UpdateError> {
        let mut cache = self.cache.lock().expect("update cache lock poisoned");
        if let Some(enabled) = automatic_checks
            && self.updates_enabled
        {
            cache.automatic_checks = enabled;
        }
        if let Some(channel) = channel
            && channel != cache.channel
        {
            cache.channel = channel;
            cache.last_checked_at = None;
            cache.last_check = None;
        }
        write_cache(&self.cache_path, &cache)?;
        Ok(status_from_cache(
            &cache,
            self.current_version,
            self.install_target.cli_recovery_required(),
        ))
    }

    pub(crate) async fn install(&self) -> Result<UpdateInstallReport, UpdateError> {
        self.install_with_key(None).await
    }

    pub(crate) async fn install_with_key(
        &self,
        key_override: Option<&str>,
    ) -> Result<UpdateInstallReport, UpdateError> {
        if !self.updates_enabled {
            return Err(UpdateError::InvalidRelease(
                "development install — rebuild it from the current working tree with scripts/dev-install.sh"
                    .to_owned(),
            ));
        }
        let status = self.check_with_key(true, key_override).await?;
        let override_client = key_override
            .map(|key| self.client(status.channel).clone().with_key(key))
            .transpose()?;
        override_client
            .as_ref()
            .unwrap_or_else(|| self.client(status.channel))
            .install_target(&status.check, &self.install_target)
            .await
    }

    fn client(&self, channel: UpdateChannel) -> &UpdateClient {
        match channel {
            UpdateChannel::Stable => &self.stable_client,
            UpdateChannel::Latest => &self.latest_client,
        }
    }
}

fn automatic_check_due(cache: &UpdateCache, current_time: i64) -> bool {
    cache.automatic_checks
        && cache.last_checked_at.is_none_or(|checked| {
            current_time.saturating_sub(checked) >= UPDATE_CHECK_INTERVAL_SECS
        })
}

fn status_from_cache(
    cache: &UpdateCache,
    current_version: &str,
    cli_recovery_required: bool,
) -> UpdateStatus {
    UpdateStatus {
        automatic_checks: cache.automatic_checks,
        channel: cache.channel,
        last_checked_at: cache.last_checked_at,
        check: cache
            .last_check
            .clone()
            .unwrap_or_else(|| UpdateCheck::current_for(current_version, cache.channel)),
        cli_recovery_required,
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
        assert_eq!(cache.channel, UpdateChannel::Stable);
        cache.automatic_checks = false;
        write_cache(&path, &cache).unwrap();
        assert!(!read_cache(&path).unwrap().automatic_checks);
    }

    #[test]
    fn switching_channels_invalidates_the_cached_weekly_check() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CACHE_FILE);
        let mut cache = UpdateCache {
            last_checked_at: Some(123),
            last_check: Some(UpdateCheck::current("0.1.0")),
            ..UpdateCache::default()
        };
        cache.channel = UpdateChannel::Latest;
        cache.last_checked_at = None;
        cache.last_check = None;
        write_cache(&path, &cache).unwrap();

        let loaded = read_cache(&path).unwrap();
        assert_eq!(loaded.channel, UpdateChannel::Latest);
        assert!(automatic_check_due(&loaded, 123));
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

    #[test]
    fn status_exposes_cli_recovery_independently_of_release_availability() {
        let status = status_from_cache(&UpdateCache::default(), "0.1.6", true);
        assert!(status.cli_recovery_required);
        assert!(!status.check.available);
    }
}
