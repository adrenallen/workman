//! Hosted release-manifest checks and verified, same-directory binary replacement.

use std::{
    collections::HashSet,
    env,
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Cursor},
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "macos")]
use std::process::Command;

use flate2::read::GzDecoder;
use reqwest::{Client, Response, StatusCode, Url};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hosted Workman release manifest. Both stable and latest channels live in this document.
pub const DEFAULT_RELEASES_API: &str = "https://workman.userdefined.io/releases.json";
/// Latest-channel compatibility alias; the hosted manifest contains both channel pointers.
pub const LATEST_RELEASES_API: &str = DEFAULT_RELEASES_API;
/// Shared application download key embedded in shipped clients.
///
/// This is intentionally a lightweight download gate, not a user credential. The release host
/// keeps a separate friends key, while explicit/configured keys can override this application key.
pub const DEFAULT_UPDATE_KEY: &str = "2d0bc1d424deae875c3b3ec80fee422942b59c0b0b10ac8b";
/// Environment fallback used when config.yml does not define `update.key`.
pub const WORKMAN_UPDATE_KEY_ENV: &str = "WORKMAN_UPDATE_KEY";
/// Courtesy interval used by the optional startup checker.
pub const UPDATE_CHECK_INTERVAL_SECS: i64 = 7 * 24 * 60 * 60;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

pub type UpdateResult<T> = Result<T, UpdateError>;

/// Release stream used for update checks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Published releases only. This is the safe default.
    #[default]
    Stable,
    /// The newest published release, including prereleases.
    Latest,
}

impl UpdateChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Latest => "latest",
        }
    }
}

impl fmt::Display for UpdateChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for UpdateChannel {
    type Err = UpdateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "stable" => Ok(Self::Stable),
            "latest" => Ok(Self::Latest),
            _ => Err(UpdateError::InvalidRelease(format!(
                "unknown update channel {value:?}; expected stable or latest"
            ))),
        }
    }
}

#[derive(Debug)]
pub enum UpdateError {
    Http(reqwest::Error),
    Io(io::Error),
    Json(serde_json::Error),
    Version(semver::Error),
    CheckFailed(String),
    RejectedKey,
    UnsupportedPlatform(String),
    InvalidRelease(String),
    MissingAsset(String),
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(formatter, "release request failed: {error}"),
            Self::Io(error) => write!(formatter, "update file operation failed: {error}"),
            Self::Json(error) => write!(formatter, "release response was not valid JSON: {error}"),
            Self::Version(error) => write!(formatter, "release version is invalid: {error}"),
            Self::CheckFailed(message) => {
                write!(formatter, "couldn't check for updates: {message}")
            }
            Self::RejectedKey => formatter.write_str("update server rejected our key"),
            Self::UnsupportedPlatform(platform) => {
                write!(formatter, "Workman updates are not packaged for {platform}")
            }
            Self::InvalidRelease(message) => {
                write!(formatter, "invalid Workman release: {message}")
            }
            Self::MissingAsset(asset) => write!(formatter, "release is missing {asset}"),
            Self::ChecksumMismatch {
                asset,
                expected,
                actual,
            } => write!(
                formatter,
                "SHA256 mismatch for {asset}: expected {expected}, got {actual}"
            ),
        }
    }
}

impl Error for UpdateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Version(error) => Some(error),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for UpdateError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error)
    }
}

impl From<io::Error> for UpdateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for UpdateError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<semver::Error> for UpdateError {
    fn from(error: semver::Error) -> Self {
        Self::Version(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    /// Hosted artifact URL shown in update reports and used for authenticated downloads.
    pub url: String,
    /// Expected SHA256 from the signed-off release manifest.
    #[serde(default)]
    pub sha256: String,
    /// Expected byte length from the release manifest.
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateCheck {
    #[serde(default)]
    pub channel: UpdateChannel,
    #[serde(default)]
    pub prerelease: bool,
    pub current: String,
    pub latest: String,
    pub url: String,
    pub notes: String,
    pub available: bool,
    pub checked_at: i64,
    pub binary_asset: Option<ReleaseAsset>,
    pub desktop_asset: Option<ReleaseAsset>,
    pub checksums_asset: Option<ReleaseAsset>,
}

/// Ordered stages emitted while a verified release is installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStage {
    Checking,
    Downloading,
    Verifying,
    Installing,
    Restarting,
}

/// User-facing progress for one update install.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateProgress {
    pub stage: UpdateStage,
    pub message: String,
    pub bytes_done: Option<u64>,
    pub bytes_total: Option<u64>,
    pub percent: Option<u8>,
    #[serde(default)]
    pub failed: bool,
}

impl UpdateProgress {
    pub fn stage(stage: UpdateStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            bytes_done: None,
            bytes_total: None,
            percent: None,
            failed: false,
        }
    }

    fn download(message: impl Into<String>, bytes_done: u64, bytes_total: u64) -> Self {
        Self {
            stage: UpdateStage::Downloading,
            message: message.into(),
            bytes_done: Some(bytes_done),
            bytes_total: Some(bytes_total),
            percent: Some(download_percent(bytes_done, bytes_total)),
            failed: false,
        }
    }

    pub fn failed(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self.failed = true;
        self
    }
}

impl UpdateCheck {
    pub fn current(current: impl Into<String>) -> Self {
        Self::current_for(current, UpdateChannel::Stable)
    }

    pub fn current_for(current: impl Into<String>, channel: UpdateChannel) -> Self {
        let current = current.into();
        Self {
            channel,
            prerelease: false,
            latest: current.clone(),
            current,
            url: "https://workman.userdefined.io/".to_owned(),
            notes: String::new(),
            available: false,
            checked_at: 0,
            binary_asset: None,
            desktop_asset: None,
            checksums_asset: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseTarget {
    pub binary_asset_name: String,
    pub desktop_asset_name: String,
    pub platform_label: String,
}

impl ReleaseTarget {
    pub fn current() -> UpdateResult<Self> {
        Self::for_platform(env::consts::OS, env::consts::ARCH)
    }

    pub fn for_platform(os: &str, arch: &str) -> UpdateResult<Self> {
        match (os, arch) {
            ("macos", "aarch64") => Ok(Self {
                binary_asset_name: "workman-macos-arm64.zip".to_owned(),
                desktop_asset_name: "workman-macos-arm64.zip".to_owned(),
                platform_label: "macOS arm64".to_owned(),
            }),
            ("linux", "x86_64") => Ok(Self {
                binary_asset_name: "workman-linux-x86_64.tar.gz".to_owned(),
                desktop_asset_name: "workman-linux-x86_64.tar.gz".to_owned(),
                platform_label: "Linux x86_64".to_owned(),
            }),
            ("linux", "aarch64") => Ok(Self {
                binary_asset_name: "workman-linux-arm64.tar.gz".to_owned(),
                desktop_asset_name: "workman-linux-arm64.tar.gz".to_owned(),
                platform_label: "Linux arm64".to_owned(),
            }),
            ("windows", "x86_64") => Ok(Self {
                binary_asset_name: "workman-windows-x86_64.zip".to_owned(),
                desktop_asset_name: "workman-windows-x86_64.zip".to_owned(),
                platform_label: "Windows x86_64".to_owned(),
            }),
            (os, arch) => Err(UpdateError::UnsupportedPlatform(format!("{os}/{arch}"))),
        }
    }
}

#[derive(Clone)]
pub struct UpdateClient {
    http: Client,
    api_url: String,
    target: ReleaseTarget,
    channel: UpdateChannel,
    key: String,
}

impl UpdateClient {
    pub fn new(api_url: impl Into<String>) -> UpdateResult<Self> {
        Self::new_for_channel(api_url, UpdateChannel::Stable)
    }

    pub fn new_for_channel(
        api_url: impl Into<String>,
        channel: UpdateChannel,
    ) -> UpdateResult<Self> {
        Self::with_target_for_channel(api_url, ReleaseTarget::current()?, channel)
    }

    pub fn with_target(api_url: impl Into<String>, target: ReleaseTarget) -> UpdateResult<Self> {
        Self::with_target_for_channel(api_url, target, UpdateChannel::Stable)
    }

    pub fn with_target_for_channel(
        api_url: impl Into<String>,
        target: ReleaseTarget,
        channel: UpdateChannel,
    ) -> UpdateResult<Self> {
        let http = Client::builder()
            .user_agent(concat!("workman/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            api_url: api_url.into(),
            target,
            channel,
            key: DEFAULT_UPDATE_KEY.to_owned(),
        })
    }

    /// Override the shared download key for every request made by this client.
    pub fn with_key(mut self, key: impl Into<String>) -> UpdateResult<Self> {
        let key = key.into();
        let key = key.trim();
        if key.is_empty() {
            return Err(UpdateError::InvalidRelease(
                "update key must not be empty".to_owned(),
            ));
        }
        self.key = key.to_owned();
        Ok(self)
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    pub fn target(&self) -> &ReleaseTarget {
        &self.target
    }

    pub fn channel(&self) -> UpdateChannel {
        self.channel
    }

    pub async fn check(&self, current: &str) -> UpdateResult<UpdateCheck> {
        self.check_manifest(current)
            .await
            .map_err(|error| match error {
                UpdateError::CheckFailed(_) => error,
                error => UpdateError::CheckFailed(error.to_string()),
            })
    }

    async fn check_manifest(&self, current: &str) -> UpdateResult<UpdateCheck> {
        let manifest_url = Url::parse(&self.api_url).map_err(|error| {
            UpdateError::InvalidRelease(format!("manifest URL is not valid: {error}"))
        })?;
        let response = self.successful_response(
            self.http
                .get(&self.api_url)
                .bearer_auth(&self.key)
                .send()
                .await?,
        )?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MANIFEST_BYTES)
        {
            return Err(UpdateError::InvalidRelease(
                "manifest exceeds the 1 MiB limit".to_owned(),
            ));
        }
        let body = response.bytes().await?;
        if body.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(UpdateError::InvalidRelease(
                "manifest exceeds the 1 MiB limit".to_owned(),
            ));
        }
        let manifest: HostedManifest = serde_json::from_slice(&body)?;
        let stable_version = parse_version(&manifest.channels.stable.version)?;
        let latest_version = parse_version(&manifest.channels.latest.version)?;
        if latest_version < stable_version {
            return Err(UpdateError::InvalidRelease(format!(
                "latest channel {} is older than stable channel {}",
                latest_version, stable_version
            )));
        }
        let release = match self.channel {
            UpdateChannel::Stable => &manifest.channels.stable,
            UpdateChannel::Latest => &manifest.channels.latest,
        };
        let current_version = parse_version(current)?;
        let selected_version = parse_version(&release.version)?;
        let notes_url = validate_url(&release.notes_url, "notes_url")?;
        if release.published_at.trim().is_empty() {
            return Err(UpdateError::InvalidRelease(
                "published_at must not be empty".to_owned(),
            ));
        }
        let mut names = HashSet::new();
        let assets = release
            .assets
            .iter()
            .map(|asset| {
                if !names.insert(asset.name.as_str()) {
                    return Err(UpdateError::InvalidRelease(format!(
                        "manifest contains duplicate asset {}",
                        asset.name
                    )));
                }
                hosted_asset(asset, &manifest_url)
            })
            .collect::<UpdateResult<Vec<_>>>()?;
        let asset = |name: &str| assets.iter().find(|asset| asset.name == name).cloned();
        Ok(UpdateCheck {
            channel: self.channel,
            prerelease: self.channel == UpdateChannel::Latest && selected_version != stable_version,
            current: current_version.to_string(),
            latest: selected_version.to_string(),
            url: notes_url,
            notes: String::new(),
            available: selected_version > current_version,
            checked_at: unix_timestamp(),
            binary_asset: asset(&self.target.binary_asset_name),
            desktop_asset: asset(&self.target.desktop_asset_name),
            checksums_asset: asset("SHA256SUMS"),
        })
    }

    /// Download, verify, extract, and atomically replace only the wrk/workmand pair in install_dir.
    ///
    /// The directory must already contain both binaries. This guard makes an accidental broad
    /// destination fail before a downloaded byte is installed.
    pub async fn install(
        &self,
        check: &UpdateCheck,
        install_dir: impl AsRef<Path>,
    ) -> UpdateResult<UpdateInstallReport> {
        let ignore_progress = |_: UpdateProgress| {};
        self.install_with_progress(check, install_dir, &ignore_progress)
            .await
    }

    async fn install_with_progress(
        &self,
        check: &UpdateCheck,
        install_dir: impl AsRef<Path>,
        on_progress: &(dyn Fn(UpdateProgress) + Send + Sync),
    ) -> UpdateResult<UpdateInstallReport> {
        if !check.available {
            return Err(UpdateError::InvalidRelease(format!(
                "{} is already current",
                check.current
            )));
        }
        let binary_asset = check
            .binary_asset
            .as_ref()
            .ok_or_else(|| UpdateError::MissingAsset(self.target.binary_asset_name.clone()))?;
        let install_dir = crate::canonical_path(install_dir.as_ref())?;
        let (wrk_target, workmand_target) = installed_binary_targets(&install_dir)?;
        let desktop_candidate = install_dir.join(executable_name("workman-desktop"));
        remove_retired_binaries(&[&wrk_target, &workmand_target, &desktop_candidate]);

        let expected = validate_sha256(&binary_asset.sha256, &binary_asset.name)?;
        let archive = self.download(binary_asset, on_progress).await?;
        on_progress(UpdateProgress::stage(
            UpdateStage::Verifying,
            format!("Verifying {}", binary_asset.name),
        ));
        if archive.len() as u64 != binary_asset.size {
            return Err(UpdateError::InvalidRelease(format!(
                "{} size mismatch: expected {} bytes, got {}",
                binary_asset.name,
                binary_asset.size,
                archive.len()
            )));
        }
        let actual = sha256_hex(&archive);
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(UpdateError::ChecksumMismatch {
                asset: binary_asset.name.clone(),
                expected,
                actual,
            });
        }

        let staging = unique_staging_dir(&install_dir)?;
        on_progress(UpdateProgress::stage(
            UpdateStage::Installing,
            "Installing command-line tools and daemon",
        ));
        let result = (|| -> UpdateResult<UpdateInstallReport> {
            extract_release_archive(&archive, &binary_asset.name, &staging)?;
            let wrk_source = staged_binary(&staging, "wrk")?;
            let workmand_source = staged_binary(&staging, "workmand")?;
            ensure_staged_binary(&wrk_source, &staging)?;
            ensure_staged_binary(&workmand_source, &staging)?;
            set_executable(&wrk_source)?;
            set_executable(&workmand_source)?;
            let quarantine_cleared = clear_macos_quarantine(&staging);
            atomic_replace(&wrk_source, &wrk_target)?;
            atomic_replace(&workmand_source, &workmand_target)?;
            let desktop_target = replace_staged_desktop(&staging, &install_dir)?;

            let mut updated_files = vec![
                wrk_target.to_string_lossy().into_owned(),
                workmand_target.to_string_lossy().into_owned(),
            ];
            if let Some(desktop) = &desktop_target {
                updated_files.push(desktop.to_string_lossy().into_owned());
            }
            Ok(UpdateInstallReport {
                current: check.current.clone(),
                latest: check.latest.clone(),
                install_dir: install_dir.to_string_lossy().into_owned(),
                updated_files,
                desktop_instruction: check.desktop_asset.as_ref().map(|asset| {
                    if let Some(desktop) = &desktop_target {
                        format!(
                            "Updated wrk at {}, workmand at {}, and the desktop app at {}. Restart Workman to load the update.",
                            wrk_target.display(), workmand_target.display(), desktop.display()
                        )
                    } else if asset.name == binary_asset.name {
                        format!(
                            "Updated wrk at {} and workmand at {}. The desktop app bundle was not replaced: close Workman, open the platform bundle {} from {}, and replace the installed app.",
                            wrk_target.display(), workmand_target.display(), asset.name, asset.url
                        )
                    } else {
                        format!(
                            "Updated wrk at {} and workmand at {}. The desktop app bundle was not replaced: close Workman, download {} from {}, and replace the installed app.",
                            wrk_target.display(), workmand_target.display(), asset.name, asset.url
                        )
                    }
                }),
                installed_app_bundle: None,
                quarantine_cleared,
                restart_plan: UpdateRestartPlan {
                    daemon: true,
                    app: false,
                },
            })
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    /// Install through either a CLI binary directory or a Dock-launched application layout.
    pub async fn install_target(
        &self,
        check: &UpdateCheck,
        target: &UpdateInstallTarget,
    ) -> UpdateResult<UpdateInstallReport> {
        let ignore_progress = |_: UpdateProgress| {};
        self.install_target_with_progress(check, target, &ignore_progress)
            .await
    }

    /// Install a release while synchronously publishing each semantic progress transition.
    pub async fn install_target_with_progress(
        &self,
        check: &UpdateCheck,
        target: &UpdateInstallTarget,
        on_progress: &(dyn Fn(UpdateProgress) + Send + Sync),
    ) -> UpdateResult<UpdateInstallReport> {
        match target {
            UpdateInstallTarget::BinaryDirectory(directory) => {
                self.install_with_progress(check, directory, on_progress)
                    .await
            }
            UpdateInstallTarget::VersionedBinary(target) => {
                self.install_versioned_binary(check, target, on_progress)
                    .await
            }
            UpdateInstallTarget::Application(application) => {
                self.install_application(check, application, on_progress)
                    .await
            }
        }
    }

    async fn install_versioned_binary(
        &self,
        check: &UpdateCheck,
        target: &VersionedBinaryInstallTarget,
        on_progress: &(dyn Fn(UpdateProgress) + Send + Sync),
    ) -> UpdateResult<UpdateInstallReport> {
        if !check.available {
            return Err(UpdateError::InvalidRelease(format!(
                "{} is already current",
                check.current
            )));
        }
        let current_binary_dir = target.current_binary_dir.canonicalize()?;
        installed_binary_targets(&current_binary_dir)?;
        let binary_asset = check
            .binary_asset
            .as_ref()
            .ok_or_else(|| UpdateError::MissingAsset(self.target.binary_asset_name.clone()))?;
        let expected = validate_sha256(&binary_asset.sha256, &binary_asset.name)?;
        let archive = self.download(binary_asset, on_progress).await?;
        on_progress(UpdateProgress::stage(
            UpdateStage::Verifying,
            format!("Verifying {}", binary_asset.name),
        ));
        if archive.len() as u64 != binary_asset.size {
            return Err(UpdateError::InvalidRelease(format!(
                "{} size mismatch: expected {} bytes, got {}",
                binary_asset.name,
                binary_asset.size,
                archive.len()
            )));
        }
        let actual = sha256_hex(&archive);
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(UpdateError::ChecksumMismatch {
                asset: binary_asset.name.clone(),
                expected,
                actual,
            });
        }

        let inventory = discover_versioned_binary_inventory(target);
        fs::create_dir_all(&target.versioned_root)?;
        let staging = unique_staging_dir(&target.versioned_root)?;
        on_progress(UpdateProgress::stage(
            UpdateStage::Installing,
            "Installing command-line tools and daemon",
        ));
        let result = (|| -> UpdateResult<UpdateInstallReport> {
            extract_release_archive(&archive, &binary_asset.name, &staging)?;
            let wrk_source = staged_binary(&staging, "wrk")?;
            let workmand_source = staged_binary(&staging, "workmand")?;
            ensure_staged_binary(&wrk_source, &staging)?;
            ensure_staged_binary(&workmand_source, &staging)?;
            set_executable(&wrk_source)?;
            set_executable(&workmand_source)?;
            let quarantine_cleared = clear_macos_quarantine(&staging);

            let version = parse_version(&check.latest)?.to_string();
            let install_dir = target.versioned_root.join(version);
            commit_staging_directory(&staging, &install_dir)?;
            let wrk_target = install_dir.join("bin").join(executable_name("wrk"));
            let workmand_target = install_dir.join("bin").join(executable_name("workmand"));
            ensure_existing_binary(&wrk_target)?;
            ensure_existing_binary(&workmand_target)?;

            let mut launchers = inventory.launchers;
            let canonical_launcher_dir = target.home_dir.join(".local/bin");
            for (name, binary) in [
                ("wrk", InstalledProgram::Wrk),
                ("workmand", InstalledProgram::Workmand),
            ] {
                let path = canonical_launcher_dir.join(executable_name(name));
                if !launchers.iter().any(|launcher| launcher.path == path) {
                    launchers.push(DiscoveredLauncher { path, binary });
                }
            }
            launchers.sort_by(|left, right| left.path.cmp(&right.path));

            let mut updated_files = vec![wrk_target.clone(), workmand_target.clone()];
            for launcher in launchers {
                let destination = match launcher.binary {
                    InstalledProgram::Wrk => &wrk_target,
                    InstalledProgram::Workmand => &workmand_target,
                };
                if ensure_launcher(&launcher.path, destination)? {
                    updated_files.push(launcher.path);
                }
            }

            Ok(UpdateInstallReport {
                current: check.current.clone(),
                latest: check.latest.clone(),
                install_dir: install_dir.to_string_lossy().into_owned(),
                updated_files: updated_files
                    .into_iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect(),
                desktop_instruction: check.desktop_asset.as_ref().map(|asset| {
                    if asset.name == binary_asset.name {
                        format!(
                            "Updated wrk at {} and workmand at {}. The desktop app bundle was not replaced: close Workman, open the platform bundle {} from {}, and replace the installed app.",
                            wrk_target.display(), workmand_target.display(), asset.name, asset.url
                        )
                    } else {
                        format!(
                            "Updated wrk at {} and workmand at {}. The desktop app bundle was not replaced: close Workman, download {} from {}, and replace the installed app.",
                            wrk_target.display(), workmand_target.display(), asset.name, asset.url
                        )
                    }
                }),
                installed_app_bundle: None,
                quarantine_cleared,
                restart_plan: UpdateRestartPlan {
                    daemon: true,
                    app: false,
                },
            })
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    async fn install_application(
        &self,
        check: &UpdateCheck,
        target: &ApplicationInstallTarget,
        on_progress: &(dyn Fn(UpdateProgress) + Send + Sync),
    ) -> UpdateResult<UpdateInstallReport> {
        let inventory = discover_application_inventory(target);
        let cli_recovery_required = install_inventory_requires_cli_recovery(&inventory);
        if !check.available
            && (!cli_recovery_required
                || parse_version(&check.latest)? != parse_version(&check.current)?)
        {
            return Err(UpdateError::InvalidRelease(format!(
                "{} is already current",
                check.current
            )));
        }
        let binary_asset = check
            .binary_asset
            .as_ref()
            .ok_or_else(|| UpdateError::MissingAsset(self.target.binary_asset_name.clone()))?;
        let expected = validate_sha256(&binary_asset.sha256, &binary_asset.name)?;
        let archive = self.download(binary_asset, on_progress).await?;
        on_progress(UpdateProgress::stage(
            UpdateStage::Verifying,
            format!("Verifying {}", binary_asset.name),
        ));
        if archive.len() as u64 != binary_asset.size {
            return Err(UpdateError::InvalidRelease(format!(
                "{} size mismatch: expected {} bytes, got {}",
                binary_asset.name,
                binary_asset.size,
                archive.len()
            )));
        }
        let actual = sha256_hex(&archive);
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(UpdateError::ChecksumMismatch {
                asset: binary_asset.name.clone(),
                expected,
                actual,
            });
        }

        fs::create_dir_all(&target.versioned_root)?;
        let staging = unique_staging_dir(&target.versioned_root)?;
        on_progress(UpdateProgress::stage(
            UpdateStage::Installing,
            "Installing command-line tools, daemon, and desktop app",
        ));
        let result = (|| -> UpdateResult<UpdateInstallReport> {
            extract_release_archive(&archive, &binary_asset.name, &staging)?;
            let wrk_source = staged_binary(&staging, "wrk")?;
            let workmand_source = staged_binary(&staging, "workmand")?;
            ensure_staged_binary(&wrk_source, &staging)?;
            ensure_staged_binary(&workmand_source, &staging)?;
            set_executable(&wrk_source)?;
            set_executable(&workmand_source)?;

            let staged_app = staging.join("Workman.app");
            let source_identifier = app_bundle_identifier(&staged_app).map_err(|error| {
                UpdateError::InvalidRelease(format!(
                    "the verified update cannot refresh the desktop app: {error}"
                ))
            })?;
            let installed_identifier =
                app_bundle_identifier(&target.app_bundle).map_err(|error| {
                    UpdateError::InvalidRelease(format!(
                        "the installed app cannot be refreshed safely: {error}"
                    ))
                })?;
            if source_identifier != installed_identifier {
                return Err(UpdateError::InvalidRelease(format!(
                    "refusing to refresh {}: update bundle identifier {:?} does not match installed identifier {:?}",
                    target.app_bundle.display(),
                    source_identifier,
                    installed_identifier
                )));
            }

            let quarantine_cleared = clear_macos_quarantine(&staging);
            let version = parse_version(&check.latest)?.to_string();
            let install_dir = target.versioned_root.join(version);
            commit_staging_directory(&staging, &install_dir)?;

            let installed_app = install_dir.join("Workman.app");
            refresh_application_bundle(&installed_app, &target.app_bundle, &installed_identifier)?;

            let wrk_target = install_dir.join("bin").join(executable_name("wrk"));
            let workmand_target = install_dir.join("bin").join(executable_name("workmand"));
            ensure_existing_binary(&wrk_target)?;
            ensure_existing_binary(&workmand_target)?;

            let mut launchers = inventory.launchers.clone();
            let canonical_launcher_dir = target.home_dir.join(".local/bin");
            for (name, binary) in [
                ("wrk", InstalledProgram::Wrk),
                ("workmand", InstalledProgram::Workmand),
            ] {
                let path = canonical_launcher_dir.join(executable_name(name));
                if !launchers.iter().any(|launcher| launcher.path == path) {
                    launchers.push(DiscoveredLauncher { path, binary });
                }
            }
            launchers.sort_by(|left, right| left.path.cmp(&right.path));

            let mut updated_files = vec![
                wrk_target.clone(),
                workmand_target.clone(),
                target.app_bundle.clone(),
            ];
            let mut updated_launcher_count = 0;
            for launcher in launchers {
                let destination = match launcher.binary {
                    InstalledProgram::Wrk => &wrk_target,
                    InstalledProgram::Workmand => &workmand_target,
                };
                if ensure_launcher(&launcher.path, destination)? {
                    updated_launcher_count += 1;
                    updated_files.push(launcher.path);
                }
            }

            let launcher_note = if cli_recovery_required {
                format!(
                    "Reinstalled the command-line tools and repaired the wrk and workmand launchers in {}.",
                    canonical_launcher_dir.display()
                )
            } else if updated_launcher_count > 0 {
                format!(
                    "Updated {updated_launcher_count} command-line launcher{}.",
                    if updated_launcher_count == 1 { "" } else { "s" }
                )
            } else {
                format!(
                    "The command-line tools remain available through {}.",
                    canonical_launcher_dir.display()
                )
            };
            let incomplete_note = if inventory.incomplete_launchers.is_empty() {
                String::new()
            } else if cli_recovery_required {
                format!(
                    " Found {} incomplete launcher entr{} while repairing the canonical pair.",
                    inventory.incomplete_launchers.len(),
                    if inventory.incomplete_launchers.len() == 1 {
                        "y"
                    } else {
                        "ies"
                    }
                )
            } else {
                format!(
                    " Ignored {} incomplete launcher location{} without a wrk/workmand pair.",
                    inventory.incomplete_launchers.len(),
                    if inventory.incomplete_launchers.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                )
            };

            Ok(UpdateInstallReport {
                current: check.current.clone(),
                latest: check.latest.clone(),
                install_dir: install_dir.to_string_lossy().into_owned(),
                updated_files: updated_files
                    .into_iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect(),
                desktop_instruction: Some(format!(
                    "Updated the Workman app bundle at {} to {}. The running app must restart to use the replaced bundle. {launcher_note}{incomplete_note}",
                    target.app_bundle.display(),
                    check.latest
                )),
                installed_app_bundle: Some(target.app_bundle.to_string_lossy().into_owned()),
                quarantine_cleared,
                restart_plan: UpdateRestartPlan {
                    daemon: true,
                    app: true,
                },
            })
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    async fn download(
        &self,
        asset: &ReleaseAsset,
        on_progress: &(dyn Fn(UpdateProgress) + Send + Sync),
    ) -> UpdateResult<Vec<u8>> {
        on_progress(UpdateProgress::download(
            format!("Downloading {}", asset.name),
            0,
            asset.size,
        ));
        let mut response = self.successful_response(
            self.http
                .get(&asset.url)
                .bearer_auth(&self.key)
                .send()
                .await?,
        )?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
        {
            return Err(UpdateError::InvalidRelease(format!(
                "asset exceeds the {} MiB update limit",
                MAX_DOWNLOAD_BYTES / (1024 * 1024)
            )));
        }
        let mut body = Vec::new();
        let mut last_percent = 0;
        while let Some(chunk) = response.chunk().await? {
            let next_len = body.len().saturating_add(chunk.len());
            if next_len as u64 > MAX_DOWNLOAD_BYTES {
                return Err(UpdateError::InvalidRelease(format!(
                    "asset exceeds the {} MiB update limit",
                    MAX_DOWNLOAD_BYTES / (1024 * 1024)
                )));
            }
            body.extend_from_slice(&chunk);
            let progress = UpdateProgress::download(
                format!("Downloading {}", asset.name),
                body.len() as u64,
                asset.size,
            );
            let percent = progress.percent.unwrap_or_default();
            // Body chunks are an implementation detail and can number in the thousands. One
            // event per meaningful percentage change keeps the UI fluid without flooding every
            // subscribed transport, while preserving the initial 0% and final 100% events.
            if percent > last_percent {
                last_percent = percent;
                on_progress(progress);
            }
        }
        Ok(body)
    }

    fn successful_response(&self, response: Response) -> UpdateResult<Response> {
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(UpdateError::RejectedKey);
        }
        Ok(response.error_for_status()?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateInstallReport {
    pub current: String,
    pub latest: String,
    pub install_dir: String,
    pub updated_files: Vec<String>,
    pub desktop_instruction: Option<String>,
    /// The application bundle replaced by this install, if any.
    #[serde(default)]
    pub installed_app_bundle: Option<String>,
    pub quarantine_cleared: bool,
    #[serde(default)]
    pub restart_plan: UpdateRestartPlan,
}

/// Explicit post-install ownership: the caller restarts only the surfaces that were replaced.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateRestartPlan {
    pub daemon: bool,
    pub app: bool,
}

impl Default for UpdateRestartPlan {
    fn default() -> Self {
        // Reports from legacy daemons did not carry a restart plan. Treat those installs as
        // requiring a daemon restart, but never claim that the desktop bundle was replaced.
        Self {
            daemon: true,
            app: false,
        }
    }
}

/// Destination selected for an update. Installed CLI binaries hop to a new durable versioned
/// directory, while Dock-launched apps also refresh the installed application bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateInstallTarget {
    BinaryDirectory(PathBuf),
    VersionedBinary(VersionedBinaryInstallTarget),
    Application(ApplicationInstallTarget),
}

impl UpdateInstallTarget {
    pub fn binary_directory(path: impl Into<PathBuf>) -> Self {
        Self::BinaryDirectory(path.into())
    }

    /// Resolve the update destination from a process executable. Paths inside a macOS app bundle
    /// use app-surface discovery; ordinary installed CLI/daemon executables hop to the durable
    /// versioned layout and repoint their launchers.
    pub fn discover(executable: impl AsRef<Path>) -> UpdateResult<Self> {
        let executable = executable.as_ref().canonicalize()?;
        if let Some(app_bundle) = application_bundle_from_executable(&executable) {
            return Ok(Self::Application(
                ApplicationInstallTarget::from_environment(app_bundle)?,
            ));
        }
        let current_binary_dir = executable.parent().map(Path::to_path_buf).ok_or_else(|| {
            UpdateError::InvalidRelease("executable has no parent directory".to_owned())
        })?;
        Ok(Self::VersionedBinary(
            VersionedBinaryInstallTarget::from_environment(current_binary_dir)?,
        ))
    }

    /// Report whether a Dock-launched app is the only usable Workman surface because no complete
    /// CLI launcher pair can be found. The app updater uses this to offer a verified reinstall.
    pub fn cli_recovery_required(&self) -> bool {
        let Self::Application(target) = self else {
            return false;
        };
        install_inventory_requires_cli_recovery(&discover_application_inventory(target))
    }
}

/// Injectable CLI discovery inputs used to migrate updates into a durable versioned directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionedBinaryInstallTarget {
    pub current_binary_dir: PathBuf,
    pub home_dir: PathBuf,
    pub search_path: Vec<PathBuf>,
    pub known_launcher_dirs: Vec<PathBuf>,
    pub versioned_root: PathBuf,
}

impl VersionedBinaryInstallTarget {
    pub fn new(
        current_binary_dir: impl Into<PathBuf>,
        home_dir: impl Into<PathBuf>,
        search_path: Vec<PathBuf>,
        known_launcher_dirs: Vec<PathBuf>,
        versioned_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            current_binary_dir: current_binary_dir.into(),
            home_dir: home_dir.into(),
            search_path,
            known_launcher_dirs,
            versioned_root: versioned_root.into(),
        }
    }

    fn from_environment(current_binary_dir: PathBuf) -> UpdateResult<Self> {
        let environment = UpdateInstallEnvironment::from_environment()?;
        Ok(Self::new(
            current_binary_dir,
            environment.home_dir,
            environment.search_path,
            environment.known_launcher_dirs,
            environment.versioned_root,
        ))
    }
}

/// Injectable app-surface discovery inputs used by both runtime installation and regression tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationInstallTarget {
    pub app_bundle: PathBuf,
    pub home_dir: PathBuf,
    pub search_path: Vec<PathBuf>,
    pub known_launcher_dirs: Vec<PathBuf>,
    pub versioned_root: PathBuf,
}

impl ApplicationInstallTarget {
    pub fn new(
        app_bundle: impl Into<PathBuf>,
        home_dir: impl Into<PathBuf>,
        search_path: Vec<PathBuf>,
        known_launcher_dirs: Vec<PathBuf>,
        versioned_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            app_bundle: app_bundle.into(),
            home_dir: home_dir.into(),
            search_path,
            known_launcher_dirs,
            versioned_root: versioned_root.into(),
        }
    }

    fn from_environment(app_bundle: PathBuf) -> UpdateResult<Self> {
        let environment = UpdateInstallEnvironment::from_environment()?;
        Ok(Self::new(
            app_bundle,
            environment.home_dir,
            environment.search_path,
            environment.known_launcher_dirs,
            environment.versioned_root,
        ))
    }
}

struct UpdateInstallEnvironment {
    home_dir: PathBuf,
    search_path: Vec<PathBuf>,
    known_launcher_dirs: Vec<PathBuf>,
    versioned_root: PathBuf,
}

impl UpdateInstallEnvironment {
    fn from_environment() -> UpdateResult<Self> {
        let home_dir = env::var_os("WORKMAN_UPDATE_HOME")
            .or_else(|| env::var_os("HOME"))
            // Windows sessions carry the home directory in USERPROFILE, not HOME.
            .or_else(|| env::var_os("USERPROFILE"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                UpdateError::InvalidRelease(
                    "update cannot determine HOME; rerun the keyed installer".to_owned(),
                )
            })?;
        let search_path = env::var_os("WORKMAN_UPDATE_PATH")
            .or_else(|| env::var_os("PATH"))
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default();
        let mut known_launcher_dirs = vec![home_dir.join(".local/bin")];
        if let Some(test_root) = env::var_os("WORKMAN_INSTALL_TEST_ROOT") {
            let test_root = PathBuf::from(test_root);
            known_launcher_dirs.push(test_root.join("usr/local/bin"));
            known_launcher_dirs.push(test_root.join("opt/homebrew/bin"));
        } else {
            known_launcher_dirs.push(PathBuf::from("/usr/local/bin"));
            known_launcher_dirs.push(PathBuf::from("/opt/homebrew/bin"));
        }
        let versioned_root = env::var_os("WORKMAN_UPDATE_VERSION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join(".local/share/workman/dist"));
        Ok(Self {
            home_dir,
            search_path,
            known_launcher_dirs,
            versioned_root,
        })
    }
}

/// Resolve the directory containing the actual executable target. current_exe generally
/// resolves launcher symlinks; canonicalization makes that behavior explicit for installers.
pub fn install_dir_from_executable(executable: impl AsRef<Path>) -> UpdateResult<PathBuf> {
    let executable = crate::canonical_path(executable.as_ref())?;
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| UpdateError::InvalidRelease("executable has no parent directory".to_owned()))
}

fn application_bundle_from_executable(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle
            .extension()
            .is_some_and(|extension| extension == "app"))
    .then(|| bundle.to_path_buf())
}

fn parse_version(value: &str) -> UpdateResult<Version> {
    Ok(Version::parse(value.trim().trim_start_matches('v'))?)
}

fn validate_url(value: &str, field: &str) -> UpdateResult<String> {
    let url = Url::parse(value).map_err(|error| {
        UpdateError::InvalidRelease(format!("{field} is not a valid URL: {error}"))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(UpdateError::InvalidRelease(format!(
            "{field} must use http or https"
        )));
    }
    Ok(url.to_string())
}

fn validate_sha256(value: &str, asset_name: &str) -> UpdateResult<String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdateError::InvalidRelease(format!(
            "{asset_name} has an invalid SHA256"
        )));
    }
    Ok(value.to_ascii_lowercase())
}

fn hosted_asset(asset: &HostedAsset, manifest_url: &Url) -> UpdateResult<ReleaseAsset> {
    if asset.name.is_empty()
        || asset.name.contains('/')
        || asset.name.contains('\\')
        || asset.target.trim().is_empty()
    {
        return Err(UpdateError::InvalidRelease(format!(
            "invalid hosted asset name or target: {:?}",
            asset.name
        )));
    }
    if asset.size == 0 || asset.size > MAX_DOWNLOAD_BYTES {
        return Err(UpdateError::InvalidRelease(format!(
            "{} has invalid size {}",
            asset.name, asset.size
        )));
    }
    let url = validate_url(&asset.url, &format!("{} URL", asset.name))?;
    let parsed_url = Url::parse(&url).expect("validated asset URL parses");
    if parsed_url.scheme() != manifest_url.scheme()
        || parsed_url.host_str() != manifest_url.host_str()
        || parsed_url.port_or_known_default() != manifest_url.port_or_known_default()
    {
        return Err(UpdateError::InvalidRelease(format!(
            "{} URL must use the release manifest origin",
            asset.name
        )));
    }
    Ok(ReleaseAsset {
        name: asset.name.clone(),
        url,
        sha256: validate_sha256(&asset.sha256, &asset.name)?,
        size: asset.size,
    })
}

fn executable_name(name: &str) -> String {
    format!("{name}{}", env::consts::EXE_SUFFIX)
}

fn ensure_existing_binary(path: &Path) -> UpdateResult<()> {
    if !path.is_file() {
        return Err(UpdateError::InvalidRelease(format!(
            "refusing to update: installed binary is missing at {}",
            path.display()
        )));
    }
    Ok(())
}

fn installed_binary_targets(install_dir: &Path) -> UpdateResult<(PathBuf, PathBuf)> {
    let wrk = install_dir.join(executable_name("wrk"));
    let workmand = install_dir.join(executable_name("workmand"));
    if wrk.is_file() && workmand.is_file() {
        return Ok((wrk, workmand));
    }

    // A v0.1.0 updater installs the transitional release beneath its original filenames. Keep
    // that pair functional until the user replaces the installation with a Workman bundle.
    let legacy_awm = install_dir.join(executable_name("awm"));
    let legacy_awmd = install_dir.join(executable_name("awmd"));
    if legacy_awm.is_file() && legacy_awmd.is_file() {
        return Ok((legacy_awm, legacy_awmd));
    }

    ensure_existing_binary(&wrk)?;
    ensure_existing_binary(&workmand)?;
    unreachable!("existing Workman targets returned above")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstalledProgram {
    Wrk,
    Workmand,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiscoveredLauncher {
    path: PathBuf,
    binary: InstalledProgram,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct InstallInventory {
    launchers: Vec<DiscoveredLauncher>,
    incomplete_launchers: Vec<PathBuf>,
}

fn install_inventory_requires_cli_recovery(inventory: &InstallInventory) -> bool {
    !inventory
        .launchers
        .iter()
        .any(|launcher| launcher.binary == InstalledProgram::Wrk)
        || !inventory
            .launchers
            .iter()
            .any(|launcher| launcher.binary == InstalledProgram::Workmand)
}

fn discover_application_inventory(target: &ApplicationInstallTarget) -> InstallInventory {
    discover_install_inventory(
        &target.home_dir,
        &target.search_path,
        &target.known_launcher_dirs,
    )
}

fn discover_versioned_binary_inventory(target: &VersionedBinaryInstallTarget) -> InstallInventory {
    discover_install_inventory(
        &target.home_dir,
        &target.search_path,
        &target.known_launcher_dirs,
    )
}

fn discover_install_inventory(
    home_dir: &Path,
    search_path: &[PathBuf],
    known_launcher_dirs: &[PathBuf],
) -> InstallInventory {
    let mut directories = Vec::new();
    let mut seen_directories = HashSet::new();
    for directory in search_path.iter().chain(known_launcher_dirs.iter()) {
        let directory = if directory.as_os_str().is_empty() {
            home_dir.to_path_buf()
        } else {
            directory.clone()
        };
        if seen_directories.insert(directory.clone()) {
            directories.push(directory);
        }
    }

    let mut launchers = Vec::new();
    let mut incomplete_launchers = Vec::new();
    let mut seen_launchers = HashSet::new();
    for directory in directories {
        for (name, binary) in [
            ("wrk", InstalledProgram::Wrk),
            ("awm", InstalledProgram::Wrk),
            ("workmand", InstalledProgram::Workmand),
            ("awmd", InstalledProgram::Workmand),
        ] {
            let path = directory.join(executable_name(name));
            if fs::symlink_metadata(&path).is_err() || !seen_launchers.insert(path.clone()) {
                continue;
            }
            let Ok(resolved) = path.canonicalize() else {
                incomplete_launchers.push(path);
                continue;
            };
            if !resolved.is_file() || !resolved_binary_has_pair(&resolved, binary) {
                incomplete_launchers.push(path);
                continue;
            }
            launchers.push(DiscoveredLauncher { path, binary });
        }
    }

    launchers.sort_by(|left, right| left.path.cmp(&right.path));
    incomplete_launchers.sort();
    InstallInventory {
        launchers,
        incomplete_launchers,
    }
}

fn resolved_binary_has_pair(path: &Path, binary: InstalledProgram) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let name = path.file_name().and_then(|name| name.to_str());
    let sibling = match (binary, name) {
        (InstalledProgram::Wrk, Some("awm")) => "awmd",
        (InstalledProgram::Wrk, _) => "workmand",
        (InstalledProgram::Workmand, Some("awmd")) => "awm",
        (InstalledProgram::Workmand, _) => "wrk",
    };
    parent.join(executable_name(sibling)).is_file()
}

fn app_bundle_identifier(bundle: &Path) -> Result<String, String> {
    let plist_path = bundle.join("Contents/Info.plist");
    if !plist_path.is_file() {
        return Err(format!("{} is missing", plist_path.display()));
    }
    let value = plist::Value::from_file(&plist_path)
        .map_err(|error| format!("could not read {}: {error}", plist_path.display()))?;
    value
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("CFBundleIdentifier"))
        .and_then(plist::Value::as_string)
        .filter(|identifier| !identifier.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{} has no CFBundleIdentifier", plist_path.display()))
}

fn commit_staging_directory(staging: &Path, destination: &Path) -> UpdateResult<()> {
    let parent = destination.parent().ok_or_else(|| {
        UpdateError::InvalidRelease(format!("{} has no parent", destination.display()))
    })?;
    fs::create_dir_all(parent)?;
    let backup = next_update_path(destination, "old")?;
    let had_destination = fs::symlink_metadata(destination).is_ok();
    if had_destination {
        fs::rename(destination, &backup)?;
    }
    if let Err(error) = fs::rename(staging, destination) {
        if had_destination {
            let _ = fs::rename(&backup, destination);
        }
        return Err(error.into());
    }
    if had_destination {
        remove_path(&backup)?;
    }
    sync_directory(parent)?;
    Ok(())
}

fn refresh_application_bundle(
    source: &Path,
    destination: &Path,
    expected_identifier: &str,
) -> UpdateResult<()> {
    let parent = destination.parent().ok_or_else(|| {
        UpdateError::InvalidRelease(format!("{} has no parent", destination.display()))
    })?;
    fs::create_dir_all(parent)?;
    let temporary = next_update_path(destination, "new")?;
    let backup = next_update_path(destination, "old")?;
    copy_application_bundle(source, &temporary)?;
    let copied_identifier = app_bundle_identifier(&temporary).map_err(|error| {
        UpdateError::InvalidRelease(format!("copied desktop app is invalid: {error}"))
    })?;
    if copied_identifier != expected_identifier {
        let _ = remove_path(&temporary);
        return Err(UpdateError::InvalidRelease(format!(
            "copied app bundle identifier {:?} does not match installed identifier {:?}",
            copied_identifier, expected_identifier
        )));
    }

    let had_destination = fs::symlink_metadata(destination).is_ok();
    if had_destination {
        fs::rename(destination, &backup)?;
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        if had_destination {
            let _ = fs::rename(&backup, destination);
        }
        let _ = remove_path(&temporary);
        return Err(error.into());
    }
    if had_destination {
        remove_path(&backup)?;
    }
    sync_directory(parent)?;
    Ok(())
}

fn copy_application_bundle(source: &Path, destination: &Path) -> UpdateResult<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("ditto")
            .arg(source)
            .arg(destination)
            .status()?;
        if !status.success() {
            return Err(UpdateError::InvalidRelease(format!(
                "ditto could not copy {} to {}",
                source.display(),
                destination.display()
            )));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        copy_directory_tree(source, destination)
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_directory_tree(source: &Path, destination: &Path) -> UpdateResult<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_directory_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, fs::metadata(&source_path)?.permissions())?;
        } else {
            return Err(UpdateError::InvalidRelease(format!(
                "desktop app contains an unsupported filesystem entry: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

fn ensure_launcher(path: &Path, target: &Path) -> UpdateResult<bool> {
    if path.canonicalize().is_ok_and(|current| {
        target
            .canonicalize()
            .is_ok_and(|expected| current == expected)
    }) {
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| UpdateError::InvalidRelease(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(parent)?;
    if fs::symlink_metadata(path).is_ok() {
        replace_launcher(path, target)?;
        return Ok(true);
    }

    let temporary = next_update_path(path, "link")?;
    create_launcher(&temporary, target)?;
    fs::rename(&temporary, path)?;
    sync_directory(parent)?;
    Ok(true)
}

fn replace_launcher(path: &Path, target: &Path) -> UpdateResult<Option<PathBuf>> {
    if crate::canonical_path(path).is_ok_and(|current| {
        crate::canonical_path(target).is_ok_and(|expected| current == expected)
    }) {
        return Ok(None);
    }
    let parent = path
        .parent()
        .ok_or_else(|| UpdateError::InvalidRelease(format!("{} has no parent", path.display())))?;
    let temporary = next_update_path(path, "link")?;
    let backup = next_update_path(path, "backup")?;
    create_launcher(&temporary, target)?;
    fs::rename(path, &backup)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    sync_directory(parent)?;
    Ok(Some(backup))
}

fn create_launcher(path: &Path, target: &Path) -> UpdateResult<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, path)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::copy(target, path)?;
        set_executable(path)?;
        Ok(())
    }
}

fn next_update_path(path: &Path, label: &str) -> UpdateResult<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| UpdateError::InvalidRelease(format!("{} has no parent", path.display())))?;
    let name = path.file_name().ok_or_else(|| {
        UpdateError::InvalidRelease(format!("{} has no file name", path.display()))
    })?;
    for attempt in 0..100_u32 {
        let candidate = parent.join(format!(
            ".{}.workman-update-{label}-{}-{attempt}",
            name.to_string_lossy(),
            std::process::id()
        ));
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(UpdateError::InvalidRelease(format!(
        "could not allocate a temporary path beside {}",
        path.display()
    )))
}

fn remove_path(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    }
}

fn ensure_staged_binary(path: &Path, staging: &Path) -> UpdateResult<()> {
    if !path.is_file() {
        return Err(UpdateError::InvalidRelease(format!(
            "{} does not contain {} in bin/ or at archive root",
            staging.display(),
            path.file_name().unwrap_or_default().to_string_lossy()
        )));
    }
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(staging.canonicalize()?) {
        return Err(UpdateError::InvalidRelease(format!(
            "archive entry escapes the staging directory: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Replace the flat Windows desktop executable in place when both the archive
/// and the install directory carry one. A running app is renamed aside by the
/// same retire-and-swap as the other binaries and keeps working until restart.
/// Bundle platforms keep their manual replacement flow, so this is a no-op there.
fn replace_staged_desktop(staging: &Path, install_dir: &Path) -> UpdateResult<Option<PathBuf>> {
    #[cfg(windows)]
    {
        let target = install_dir.join(executable_name("workman-desktop"));
        if target.is_file()
            && let Ok(source) = staged_binary(staging, "workman-desktop")
        {
            set_executable(&source)?;
            atomic_replace(&source, &target)?;
            return Ok(Some(target));
        }
        Ok(None)
    }
    #[cfg(not(windows))]
    {
        let _ = (staging, install_dir);
        Ok(None)
    }
}

fn staged_binary(staging: &Path, name: &str) -> UpdateResult<PathBuf> {
    let executable = executable_name(name);
    let bundled = staging.join("bin").join(&executable);
    if bundled.is_file() {
        return Ok(bundled);
    }
    let legacy = staging.join(&executable);
    if legacy.is_file() {
        return Ok(legacy);
    }
    Err(UpdateError::InvalidRelease(format!(
        "{} does not contain {executable} in bin/ or at archive root",
        staging.display()
    )))
}

fn unique_staging_dir(install_dir: &Path) -> UpdateResult<PathBuf> {
    for attempt in 0..100_u32 {
        let path = install_dir.join(format!(
            ".workman-update-{}-{}-{attempt}",
            std::process::id(),
            unix_timestamp()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(UpdateError::InvalidRelease(
        "could not allocate a unique update staging directory".to_owned(),
    ))
}

fn extract_tar_gz(bytes: &[u8], destination: &Path) -> UpdateResult<()> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.unpack_in(destination)? {
            return Err(UpdateError::InvalidRelease(
                "archive contains a path outside its root".to_owned(),
            ));
        }
    }
    Ok(())
}

fn extract_release_archive(bytes: &[u8], asset_name: &str, destination: &Path) -> UpdateResult<()> {
    if asset_name.ends_with(".tar.gz") {
        return extract_tar_gz(bytes, destination);
    }
    if asset_name.ends_with(".zip") {
        return extract_zip(bytes, destination);
    }
    Err(UpdateError::InvalidRelease(format!(
        "unsupported update archive format: {asset_name}"
    )))
}

fn extract_zip(bytes: &[u8], destination: &Path) -> UpdateResult<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        UpdateError::InvalidRelease(format!("ZIP archive could not be read: {error}"))
    })?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            UpdateError::InvalidRelease(format!("ZIP entry could not be read: {error}"))
        })?;
        let relative = entry.enclosed_name().ok_or_else(|| {
            UpdateError::InvalidRelease(format!(
                "ZIP entry escapes the staging directory: {}",
                entry.name()
            ))
        })?;
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&output)?;
        io::copy(&mut entry, &mut file)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

fn atomic_replace(source: &Path, target: &Path) -> UpdateResult<()> {
    let parent = target.parent().ok_or_else(|| {
        UpdateError::InvalidRelease(format!("{} has no parent", target.display()))
    })?;
    let name = target.file_name().ok_or_else(|| {
        UpdateError::InvalidRelease(format!("{} has no file name", target.display()))
    })?;
    let temporary = parent.join(format!(
        ".{}.workman-update-new-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let _ = fs::remove_file(&temporary);
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    set_executable(&temporary)?;
    replace_file(&temporary, target)?;
    sync_directory(parent)?;
    Ok(())
}

/// Move `source` over `target`, replacing any existing file.
#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> UpdateResult<()> {
    fs::rename(source, target)?;
    Ok(())
}

/// Move `source` over `target`, replacing any existing file.
///
/// Windows cannot replace an executable that is currently running, but it can
/// rename one aside. The retired file is parked beside the target and removed by
/// [`remove_retired_binaries`] on the next install, once its process has exited.
#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> UpdateResult<()> {
    // A running image rejects replacement with access-denied; other open handles
    // surface as a sharing violation (os error 32). Both mean "target is busy".
    fn replace_blocked(error: &io::Error) -> bool {
        error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(32)
    }

    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if replace_blocked(&error) && target.is_file() => {
            let retired = next_update_path(target, "retired")?;
            fs::rename(target, &retired)?;
            match fs::rename(source, target) {
                Ok(()) => {
                    // Best effort: succeeds immediately unless the retired
                    // binary is still running somewhere.
                    let _ = fs::remove_file(&retired);
                    Ok(())
                }
                Err(second) => {
                    let _ = fs::rename(&retired, target);
                    Err(second.into())
                }
            }
        }
        Err(error) => Err(error.into()),
    }
}

/// Remove binaries retired by earlier Windows installs whose processes have
/// since exited. Files still held open by a running process are left in place.
fn remove_retired_binaries(targets: &[&Path]) {
    for target in targets {
        let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
            continue;
        };
        let prefix = format!(".{}.workman-update-retired-", name.to_string_lossy());
        let Ok(entries) = fs::read_dir(parent) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn set_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn clear_macos_quarantine(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(path)
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn download_percent(bytes_done: u64, bytes_total: u64) -> u8 {
    if bytes_total == 0 {
        return 0;
    }
    bytes_done
        .saturating_mul(100)
        .checked_div(bytes_total)
        .unwrap_or_default()
        .min(100) as u8
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[derive(Deserialize)]
struct HostedManifest {
    channels: HostedChannels,
}

#[derive(Deserialize)]
struct HostedChannels {
    stable: HostedRelease,
    latest: HostedRelease,
}

#[derive(Deserialize)]
struct HostedRelease {
    version: String,
    published_at: String,
    notes_url: String,
    assets: Vec<HostedAsset>,
}

#[derive(Deserialize)]
struct HostedAsset {
    name: String,
    target: String,
    sha256: String,
    size: u64,
    url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_ignores_tag_prefix() {
        assert!(parse_version("v0.2.0").unwrap() > parse_version("0.1.9").unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn staged_desktop_replaces_the_flat_windows_executable_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let install_dir = temp.path().join("install");
        let staging = temp.path().join("staging");
        fs::create_dir_all(install_dir.join("ignored")).unwrap();
        fs::create_dir_all(staging.join("bin")).unwrap();
        fs::write(install_dir.join("workman-desktop.exe"), b"old").unwrap();
        fs::write(staging.join("bin").join("workman-desktop.exe"), b"new").unwrap();

        let replaced = replace_staged_desktop(&staging, &install_dir).unwrap();
        assert_eq!(
            replaced.as_deref(),
            Some(install_dir.join("workman-desktop.exe").as_path())
        );
        assert_eq!(
            fs::read(install_dir.join("workman-desktop.exe")).unwrap(),
            b"new"
        );

        // Without an installed desktop executable the update leaves it alone.
        let cli_only = temp.path().join("cli-only");
        fs::create_dir_all(&cli_only).unwrap();
        assert_eq!(replace_staged_desktop(&staging, &cli_only).unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn replace_file_retires_a_running_target_and_installs_the_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("wrk.exe");
        let system32 =
            PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot")).join("System32");
        fs::copy(system32.join("ping.exe"), &target).unwrap();
        let mut running = std::process::Command::new(&target)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let retired_exists = || {
            fs::read_dir(temp.path()).unwrap().flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".wrk.exe.workman-update-retired-")
            })
        };

        let source = temp.path().join("incoming.exe");
        fs::write(&source, b"replacement").unwrap();
        replace_file(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"replacement");
        assert!(
            retired_exists(),
            "running original must be parked beside the target"
        );

        running.kill().unwrap();
        running.wait().unwrap();
        remove_retired_binaries(&[&target]);
        assert!(
            !retired_exists(),
            "retired binary must be removed once its process exits"
        );
    }

    #[test]
    fn release_targets_cover_every_packaged_platform() {
        let macos = ReleaseTarget::for_platform("macos", "aarch64").unwrap();
        assert_eq!(macos.binary_asset_name, "workman-macos-arm64.zip");
        assert_eq!(macos.desktop_asset_name, macos.binary_asset_name);
        assert_eq!(
            ReleaseTarget::for_platform("linux", "x86_64")
                .unwrap()
                .binary_asset_name,
            "workman-linux-x86_64.tar.gz"
        );
        assert_eq!(
            ReleaseTarget::for_platform("linux", "aarch64")
                .unwrap()
                .binary_asset_name,
            "workman-linux-arm64.tar.gz"
        );
        assert_eq!(
            ReleaseTarget::for_platform("windows", "x86_64")
                .unwrap()
                .binary_asset_name,
            "workman-windows-x86_64.zip"
        );
    }

    #[test]
    fn manifest_sha256_must_be_exact_hex() {
        assert_eq!(
            validate_sha256(&"A".repeat(64), "asset").unwrap(),
            "a".repeat(64)
        );
        assert!(validate_sha256(&"z".repeat(64), "asset").is_err());
        assert!(validate_sha256("abc", "asset").is_err());
    }

    #[test]
    fn app_surface_detection_stops_at_the_bundle_root() {
        let executable =
            Path::new("/tmp/workman-todo467/Workman Todo 467.app/Contents/MacOS/workman-desktop");
        assert_eq!(
            application_bundle_from_executable(executable),
            Some(PathBuf::from("/tmp/workman-todo467/Workman Todo 467.app"))
        );
        assert_eq!(
            application_bundle_from_executable(Path::new("/tmp/workman/bin/workmand")),
            None
        );
    }
}
