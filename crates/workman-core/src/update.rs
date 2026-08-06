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
        let install_dir = install_dir.as_ref().canonicalize()?;
        let (wrk_target, workmand_target) = installed_binary_targets(&install_dir)?;

        let expected = validate_sha256(&binary_asset.sha256, &binary_asset.name)?;
        let archive = self.download(binary_asset).await?;
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
        let result = (|| -> UpdateResult<UpdateInstallReport> {
            extract_release_archive(&archive, &binary_asset.name, &staging)?;
            let wrk_source = staged_binary(&staging, "wrk")?;
            let workmand_source = staged_binary(&staging, "workmand")?;
            ensure_staged_binary(&wrk_source, &staging)?;
            ensure_staged_binary(&workmand_source, &staging)?;
            let quarantine_cleared = clear_macos_quarantine(&staging);
            atomic_replace(&wrk_source, &wrk_target)?;
            atomic_replace(&workmand_source, &workmand_target)?;

            Ok(UpdateInstallReport {
                current: check.current.clone(),
                latest: check.latest.clone(),
                install_dir: install_dir.to_string_lossy().into_owned(),
                updated_files: vec![
                    wrk_target.to_string_lossy().into_owned(),
                    workmand_target.to_string_lossy().into_owned(),
                ],
                desktop_instruction: check.desktop_asset.as_ref().map(|asset| {
                    if asset.name == binary_asset.name {
                        format!(
                            "Desktop app: close Workman, open the platform bundle {} from {}, and replace the installed app. The running app is not replaced in place.",
                            asset.name, asset.url
                        )
                    } else {
                        format!(
                            "Desktop app: close Workman, download {} from {}, and replace the installed app. The running app is not replaced in place.",
                            asset.name, asset.url
                        )
                    }
                }),
                quarantine_cleared,
            })
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    async fn download(&self, asset: &ReleaseAsset) -> UpdateResult<Vec<u8>> {
        let response = self.successful_response(
            self.http
                .get(&asset.url)
                .bearer_auth(&self.key)
                .send()
                .await?,
        )?;
        limited_body(response).await
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
    pub quarantine_cleared: bool,
}

/// Resolve the directory containing the actual executable target. current_exe generally
/// resolves launcher symlinks; canonicalization makes that behavior explicit for installers.
pub fn install_dir_from_executable(executable: impl AsRef<Path>) -> UpdateResult<PathBuf> {
    let executable = executable.as_ref().canonicalize()?;
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| UpdateError::InvalidRelease("executable has no parent directory".to_owned()))
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
    fs::rename(&temporary, target)?;
    sync_directory(parent)?;
    Ok(())
}

fn set_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
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

async fn limited_body(response: Response) -> UpdateResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
    {
        return Err(UpdateError::InvalidRelease(format!(
            "asset exceeds the {} MiB update limit",
            MAX_DOWNLOAD_BYTES / (1024 * 1024)
        )));
    }
    let bytes = response.bytes().await?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(UpdateError::InvalidRelease(format!(
            "asset exceeds the {} MiB update limit",
            MAX_DOWNLOAD_BYTES / (1024 * 1024)
        )));
    }
    Ok(bytes.to_vec())
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

    #[test]
    fn release_targets_cover_both_static_linux_archives() {
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
}
