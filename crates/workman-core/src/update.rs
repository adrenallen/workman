//! GitHub Release checks and verified, same-directory binary replacement.

use std::{
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
use reqwest::{Client, Response, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// GitHub's latest non-draft, non-prerelease endpoint for awm.
pub const DEFAULT_RELEASES_API: &str =
    "https://api.github.com/repos/adrenallen/awm/releases/latest";
/// GitHub's ordered release listing, including prereleases, for the latest channel.
pub const LATEST_RELEASES_API: &str =
    "https://api.github.com/repos/adrenallen/awm/releases?per_page=20";
/// Courtesy interval used by the optional startup checker.
pub const UPDATE_CHECK_INTERVAL_SECS: i64 = 7 * 24 * 60 * 60;
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
            Self::UnsupportedPlatform(platform) => {
                write!(formatter, "awm updates are not packaged for {platform}")
            }
            Self::InvalidRelease(message) => write!(formatter, "invalid awm release: {message}"),
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
    /// Browser-facing URL shown in update reports and desktop instructions.
    pub url: String,
    /// Authenticated GitHub API download URL, when supplied by the release response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
}

impl ReleaseAsset {
    fn download_url(&self, authenticated: bool) -> (&str, bool) {
        match (authenticated, self.api_url.as_deref()) {
            (true, Some(api_url)) => (api_url, true),
            _ => (&self.url, false),
        }
    }
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
            url: "https://github.com/adrenallen/awm/releases".to_owned(),
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
                binary_asset_name: "awm-macos-arm64.zip".to_owned(),
                desktop_asset_name: "awm-macos-arm64.zip".to_owned(),
                platform_label: "macOS arm64".to_owned(),
            }),
            ("linux", "x86_64") => Ok(Self {
                binary_asset_name: "awm-linux-x86_64.tar.gz".to_owned(),
                desktop_asset_name: "awm-linux-x86_64.tar.gz".to_owned(),
                platform_label: "Linux x86_64".to_owned(),
            }),
            ("linux", "aarch64") => Ok(Self {
                binary_asset_name: "awm-linux-arm64.tar.gz".to_owned(),
                desktop_asset_name: "awm-linux-arm64.tar.gz".to_owned(),
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
    token: Option<String>,
    channel: UpdateChannel,
}

impl UpdateClient {
    pub fn github() -> UpdateResult<Self> {
        Self::github_for(UpdateChannel::Stable)
    }

    pub fn github_for(channel: UpdateChannel) -> UpdateResult<Self> {
        let api_url = match channel {
            UpdateChannel::Stable => DEFAULT_RELEASES_API,
            UpdateChannel::Latest => LATEST_RELEASES_API,
        };
        Self::new_for_channel(api_url, channel)
    }

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
            .user_agent(concat!("awm/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            api_url: api_url.into(),
            target,
            token: env::var("WORKMAN_GITHUB_TOKEN")
                .or_else(|_| env::var("GITHUB_TOKEN"))
                .or_else(|_| env::var("GH_TOKEN"))
                .ok()
                .filter(|token| !token.trim().is_empty()),
            channel,
        })
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
        let response = self
            .request(&self.api_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        if self.channel == UpdateChannel::Stable && response.status() == StatusCode::NOT_FOUND {
            let mut check = UpdateCheck::current_for(current, self.channel);
            check.checked_at = unix_timestamp();
            return Ok(check);
        }
        let response = response.error_for_status()?;
        let release: GithubRelease = match self.channel {
            UpdateChannel::Stable => response.json().await?,
            UpdateChannel::Latest => response
                .json::<Vec<GithubRelease>>()
                .await?
                .into_iter()
                .find(|release| !release.draft)
                .ok_or_else(|| {
                    UpdateError::InvalidRelease(
                        "latest channel contains no published releases".to_owned(),
                    )
                })?,
        };
        let current_version = parse_version(current)?;
        let latest_version = parse_version(&release.tag_name)?;
        let available = latest_version > current_version;
        let asset = |name: &str| {
            release
                .assets
                .iter()
                .find(|asset| asset.name == name)
                .map(|asset| ReleaseAsset {
                    name: asset.name.clone(),
                    url: asset.browser_download_url.clone(),
                    api_url: asset.api_url.clone(),
                })
        };
        Ok(UpdateCheck {
            channel: self.channel,
            prerelease: release.prerelease,
            current: current_version.to_string(),
            latest: latest_version.to_string(),
            url: release.html_url,
            notes: release.body.unwrap_or_default(),
            available,
            checked_at: unix_timestamp(),
            binary_asset: asset(&self.target.binary_asset_name),
            desktop_asset: asset(&self.target.desktop_asset_name),
            checksums_asset: asset("SHA256SUMS"),
        })
    }

    /// Download, verify, extract, and atomically replace only the awm/awmd pair in install_dir.
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
        let checksums_asset = check
            .checksums_asset
            .as_ref()
            .ok_or_else(|| UpdateError::MissingAsset("SHA256SUMS".to_owned()))?;
        let install_dir = install_dir.as_ref().canonicalize()?;
        let awm_target = install_dir.join(executable_name("awm"));
        let awmd_target = install_dir.join(executable_name("awmd"));
        ensure_existing_binary(&awm_target)?;
        ensure_existing_binary(&awmd_target)?;

        let sums = self.download(checksums_asset).await?;
        let sums = std::str::from_utf8(&sums)
            .map_err(|_| UpdateError::InvalidRelease("SHA256SUMS is not UTF-8".to_owned()))?;
        let expected = checksum_for(sums, &binary_asset.name)
            .ok_or_else(|| UpdateError::MissingAsset(format!("{} checksum", binary_asset.name)))?;
        let archive = self.download(binary_asset).await?;
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
            let awm_source = staged_binary(&staging, "awm")?;
            let awmd_source = staged_binary(&staging, "awmd")?;
            ensure_staged_binary(&awm_source, &staging)?;
            ensure_staged_binary(&awmd_source, &staging)?;
            let quarantine_cleared = clear_macos_quarantine(&staging);
            atomic_replace(&awm_source, &awm_target)?;
            atomic_replace(&awmd_source, &awmd_target)?;

            Ok(UpdateInstallReport {
                current: check.current.clone(),
                latest: check.latest.clone(),
                install_dir: install_dir.to_string_lossy().into_owned(),
                updated_files: vec![
                    awm_target.to_string_lossy().into_owned(),
                    awmd_target.to_string_lossy().into_owned(),
                ],
                desktop_instruction: check.desktop_asset.as_ref().map(|asset| {
                    if asset.name == binary_asset.name {
                        format!(
                            "Desktop app: close awm, open the platform bundle {} from {}, and replace the installed app. The running app is not replaced in place.",
                            asset.name, asset.url
                        )
                    } else {
                        format!(
                            "Desktop app: close awm, download {} from {}, and replace the installed app. The running app is not replaced in place.",
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
        let (url, api_download) = asset.download_url(self.token.is_some());
        let mut request = self.request(url);
        if api_download {
            request = request.header("Accept", "application/octet-stream");
        }
        let response = request.send().await?.error_for_status()?;
        limited_body(response).await
    }

    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        let request = self
            .http
            .get(url)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            request.bearer_auth(token)
        } else {
            request
        }
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
            ".awm-update-{}-{}-{attempt}",
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
        ".{}.awm-update-new-{}",
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

fn checksum_for(manifest: &str, asset_name: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let checksum = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == asset_name && checksum.len() == 64).then(|| checksum.to_ascii_lowercase())
    })
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
struct GithubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    assets: Vec<GithubAsset>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default, rename = "url")]
    api_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parser_accepts_common_sha256sum_forms() {
        let hash = "a".repeat(64);
        let manifest = format!("{hash}  awm-macos-arm64.zip\n{hash} *other.tar.gz\n");
        assert_eq!(checksum_for(&manifest, "awm-macos-arm64.zip"), Some(hash));
        assert_eq!(checksum_for(&manifest, "missing"), None);
    }

    #[test]
    fn semver_comparison_ignores_tag_prefix() {
        assert!(parse_version("v0.2.0").unwrap() > parse_version("0.1.9").unwrap());
    }

    #[test]
    fn release_targets_cover_both_static_linux_archives() {
        let macos = ReleaseTarget::for_platform("macos", "aarch64").unwrap();
        assert_eq!(macos.binary_asset_name, "awm-macos-arm64.zip");
        assert_eq!(macos.desktop_asset_name, macos.binary_asset_name);
        assert_eq!(
            ReleaseTarget::for_platform("linux", "x86_64")
                .unwrap()
                .binary_asset_name,
            "awm-linux-x86_64.tar.gz"
        );
        assert_eq!(
            ReleaseTarget::for_platform("linux", "aarch64")
                .unwrap()
                .binary_asset_name,
            "awm-linux-arm64.tar.gz"
        );
    }

    #[test]
    fn authenticated_assets_prefer_the_api_download_url() {
        let asset = ReleaseAsset {
            name: "awm-macos-arm64.zip".to_owned(),
            url: "https://github.com/example/download".to_owned(),
            api_url: Some("https://api.github.com/repos/example/assets/1".to_owned()),
        };
        assert_eq!(
            asset.download_url(true),
            ("https://api.github.com/repos/example/assets/1", true)
        );
        assert_eq!(
            asset.download_url(false),
            ("https://github.com/example/download", false)
        );
    }
}
