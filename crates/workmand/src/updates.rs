//! Cached update checks and verified installs exposed through the daemon control channel.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as AsyncMutex, broadcast};
use workman_core::{
    DEFAULT_RELEASES_API, LATEST_RELEASES_API, UPDATE_CHECK_INTERVAL_SECS, UpdateChannel,
    UpdateCheck, UpdateClient, UpdateError, UpdateInstallReport, UpdateInstallTarget,
    UpdateProgress, UpdateStage,
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

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpdateProgressEvent {
    pub request_id: String,
    pub progress: UpdateProgress,
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
    installing: Arc<AtomicBool>,
    progress: Arc<Mutex<Option<UpdateProgress>>>,
    progress_events: broadcast::Sender<UpdateProgressEvent>,
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
        let (progress_events, _) = broadcast::channel(64);
        Ok(Self {
            stable_client,
            latest_client,
            current_version: env!("CARGO_PKG_VERSION"),
            install_target,
            cache_path,
            cache: Arc::new(Mutex::new(cache)),
            operation: Arc::new(AsyncMutex::new(())),
            installing: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(None)),
            progress_events,
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
            if !force
                && (!cache.automatic_checks
                    || (cached_check_is_current(&cache, self.current_version)
                        && !automatic_check_due(&cache, now())))
            {
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
        self.install_with_key_for(None, None).await
    }

    pub(crate) async fn install_with_key(
        &self,
        key_override: Option<&str>,
    ) -> Result<UpdateInstallReport, UpdateError> {
        self.install_with_key_for(key_override, None).await
    }

    pub(crate) async fn install_with_key_for(
        &self,
        key_override: Option<&str>,
        request_id: Option<String>,
    ) -> Result<UpdateInstallReport, UpdateError> {
        if !self.updates_enabled {
            return Err(UpdateError::InvalidRelease(
                "development install — rebuild it from the current working tree with scripts/dev-install.sh"
                    .to_owned(),
            ));
        }
        let _install = self.begin_install()?;
        self.publish_progress(
            request_id.as_deref(),
            UpdateProgress::stage(
                UpdateStage::Checking,
                "Checking for the latest Workman release",
            ),
        );
        let status = match self.check_with_key(true, key_override).await {
            Ok(status) => status,
            Err(error) => {
                self.publish_failure(request_id.as_deref(), &error);
                return Err(error);
            }
        };
        let override_client = match key_override
            .map(|key| self.client(status.channel).clone().with_key(key))
            .transpose()
        {
            Ok(client) => client,
            Err(error) => {
                self.publish_failure(request_id.as_deref(), &error);
                return Err(error);
            }
        };
        let client = override_client
            .as_ref()
            .unwrap_or_else(|| self.client(status.channel));
        let publish = |progress| self.publish_progress(request_id.as_deref(), progress);
        match client
            .install_target_with_progress(&status.check, &self.install_target, &publish)
            .await
        {
            // The daemon cannot know whether the caller will relaunch an app, restart only the
            // daemon, or present a manual fallback. The successful RPC report is the terminal
            // event; the caller narrates its own restart plan.
            Ok(report) => Ok(report),
            Err(error) => {
                self.publish_failure(request_id.as_deref(), &error);
                Err(error)
            }
        }
    }

    pub(crate) fn subscribe_progress(&self) -> broadcast::Receiver<UpdateProgressEvent> {
        self.progress_events.subscribe()
    }

    fn begin_install(&self) -> Result<UpdateInstallGuard, UpdateError> {
        self.installing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                UpdateError::InvalidRelease(
                    "another update installation is already in progress".to_owned(),
                )
            })?;
        Ok(UpdateInstallGuard(self.installing.clone()))
    }

    fn publish_progress(&self, request_id: Option<&str>, progress: UpdateProgress) {
        *self.progress.lock().expect("update progress lock poisoned") = Some(progress.clone());
        if let Some(request_id) = request_id {
            let _ = self.progress_events.send(UpdateProgressEvent {
                request_id: request_id.to_owned(),
                progress,
            });
        }
    }

    fn publish_failure(&self, request_id: Option<&str>, error: &UpdateError) {
        let progress = self
            .progress
            .lock()
            .expect("update progress lock poisoned")
            .clone()
            .unwrap_or_else(|| UpdateProgress::stage(UpdateStage::Checking, "Checking for updates"))
            .failed(error.to_string());
        self.publish_progress(request_id, progress);
    }

    fn client(&self, channel: UpdateChannel) -> &UpdateClient {
        match channel {
            UpdateChannel::Stable => &self.stable_client,
            UpdateChannel::Latest => &self.latest_client,
        }
    }
}

struct UpdateInstallGuard(Arc<AtomicBool>);

impl Drop for UpdateInstallGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn automatic_check_due(cache: &UpdateCache, current_time: i64) -> bool {
    cache.automatic_checks
        && cache.last_checked_at.is_none_or(|checked| {
            current_time.saturating_sub(checked) >= UPDATE_CHECK_INTERVAL_SECS
        })
}

fn cached_check_is_current(cache: &UpdateCache, current_version: &str) -> bool {
    cache
        .last_check
        .as_ref()
        .is_some_and(|check| check.current == current_version)
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
            .filter(|check| check.current == current_version)
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

    #[test]
    fn stale_pre_update_cache_never_reannounces_an_installed_release() {
        let mut old_check = UpdateCheck::current("0.1.9");
        old_check.latest = "0.1.11".to_owned();
        old_check.available = true;
        let cache = UpdateCache {
            last_checked_at: Some(123),
            last_check: Some(old_check),
            ..UpdateCache::default()
        };

        assert!(!cached_check_is_current(&cache, "0.1.11"));
        let status = status_from_cache(&cache, "0.1.11", false);
        assert_eq!(status.check.current, "0.1.11");
        assert_eq!(status.check.latest, "0.1.11");
        assert!(!status.check.available);
    }
}
