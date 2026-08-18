use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt, fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::Serialize;
use workman_core::Project;

pub const CUSTOM_ICON_PREFIX: &str = "image:";

const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
const AUTO_ICON_DIRS: &[(&str, usize)] = &[
    ("public", 2),
    ("static", 2),
    ("priv/static", 2),
    ("web", 2),
    ("app/assets", 3),
    ("assets", 2),
    ("src/assets", 2),
    ("resources", 2),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectIconImage {
    pub data_url: String,
    pub source: &'static str,
    pub path: String,
}

#[derive(Debug)]
pub enum ProjectIconError {
    ProjectMissing,
    SourceMissing,
    UnsupportedFormat,
    ImageTooLarge,
    InvalidReference,
    Io(std::io::Error),
}

impl fmt::Display for ProjectIconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectMissing => formatter.write_str("project directory does not exist"),
            Self::SourceMissing => formatter.write_str("selected image does not exist"),
            Self::UnsupportedFormat => {
                formatter.write_str("selected image must be PNG, JPEG, WebP, GIF, BMP, ICO, or SVG")
            }
            Self::ImageTooLarge => formatter.write_str("selected image must be 5 MB or smaller"),
            Self::InvalidReference => formatter.write_str("project image reference is invalid"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl Error for ProjectIconError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ProjectIconError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone)]
struct CacheEntry {
    marker: Option<String>,
    image: Option<ProjectIconImage>,
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    relative_path: String,
    score: (bool, u64, u64),
}

static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();

pub fn resolve(project: &Project) -> Option<ProjectIconImage> {
    let marker = project.icon.clone();
    if let Some(entry) = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&project.path)
        .filter(|entry| entry.marker == marker)
        .filter(|entry| cache_entry_is_current(project, entry))
        .cloned()
    {
        return entry.image;
    }

    let image = match marker.as_deref() {
        Some(reference) if reference.starts_with(CUSTOM_ICON_PREFIX) => {
            custom_icon_path(Path::new(&project.path), reference)
                .ok()
                .and_then(|(path, relative_path)| read_image(&path, "custom", relative_path).ok())
        }
        Some(_) => None,
        None => scan_auto(Path::new(&project.path)),
    };

    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            project.path.clone(),
            CacheEntry {
                marker,
                image: image.clone(),
            },
        );
    image
}

pub fn refresh_auto(project: &Project) -> Option<ProjectIconImage> {
    invalidate(&project.path);
    let image = scan_auto(Path::new(&project.path));
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            project.path.clone(),
            CacheEntry {
                marker: None,
                image: image.clone(),
            },
        );
    image
}

pub fn copy_custom_image(
    project: &Project,
    source_path: &Path,
) -> Result<String, ProjectIconError> {
    let root = workman_core::canonical_path(&project.path)
        .map_err(|_| ProjectIconError::ProjectMissing)?;
    if !root.is_dir() {
        return Err(ProjectIconError::ProjectMissing);
    }
    let source =
        workman_core::canonical_path(source_path).map_err(|_| ProjectIconError::SourceMissing)?;
    if !source.is_file() {
        return Err(ProjectIconError::SourceMissing);
    }
    let extension = image_extension(&source).ok_or(ProjectIconError::UnsupportedFormat)?;
    if fs::metadata(&source)?.len() > MAX_IMAGE_BYTES {
        return Err(ProjectIconError::ImageTooLarge);
    }

    let icon_dir = root.join(".workman");
    fs::create_dir_all(&icon_dir)?;
    let destination = icon_dir.join(format!("icon.{extension}"));
    let same_file = workman_core::canonical_path(&destination)
        .ok()
        .is_some_and(|existing| existing == source);
    if !same_file {
        let temporary = icon_dir.join(format!(".icon.{extension}.tmp"));
        fs::copy(&source, &temporary)?;
        fs::rename(&temporary, &destination)?;
    }

    if let Some(previous) = project
        .icon
        .as_deref()
        .filter(|icon| icon.starts_with(CUSTOM_ICON_PREFIX))
        && let Ok((previous_path, _)) = custom_icon_path(&root, previous)
        && previous_path != destination
    {
        let _ = fs::remove_file(previous_path);
    }

    invalidate(&project.path);
    Ok(format!("{CUSTOM_ICON_PREFIX}.workman/icon.{extension}"))
}

pub fn is_custom_reference(reference: &str) -> bool {
    custom_icon_path(Path::new("."), reference).is_ok()
}

pub fn invalidate(project_path: &str) {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(project_path);
}

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_entry_is_current(project: &Project, entry: &CacheEntry) -> bool {
    match &entry.image {
        Some(image) => Path::new(&project.path).join(&image.path).is_file(),
        None => !entry
            .marker
            .as_deref()
            .is_some_and(|marker| marker.starts_with(CUSTOM_ICON_PREFIX)),
    }
}

fn scan_auto(root: &Path) -> Option<ProjectIconImage> {
    if !root.is_dir() {
        return None;
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    collect_candidates(root, root, 0, &mut seen, &mut candidates);
    for (relative, depth) in AUTO_ICON_DIRS {
        let directory = root.join(relative);
        collect_candidates(root, &directory, *depth, &mut seen, &mut candidates);
    }
    let candidate = candidates
        .into_iter()
        .max_by(|left, right| left.score.cmp(&right.score))?;
    read_image(&candidate.path, "auto", candidate.relative_path).ok()
}

fn collect_candidates(
    root: &Path,
    directory: &Path,
    remaining_depth: usize,
    seen: &mut HashSet<PathBuf>,
    candidates: &mut Vec<Candidate>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && !file_type.is_symlink() && remaining_depth > 0 {
            collect_candidates(root, &path, remaining_depth - 1, seen, candidates);
            continue;
        }
        if !file_type.is_file() || !is_auto_icon_name(&path) || !seen.insert(path.clone()) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > MAX_IMAGE_BYTES {
            continue;
        }
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let score = image_score(&path, metadata.len());
        candidates.push(Candidate {
            path,
            relative_path,
            score,
        });
    }
}

fn is_auto_icon_name(path: &Path) -> bool {
    let Some(extension) = image_extension(path) else {
        return false;
    };
    if !matches!(
        extension.as_str(),
        "ico" | "png" | "svg" | "jpg" | "jpeg" | "webp"
    ) {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    stem == "favicon"
        || stem.starts_with("favicon-")
        || stem.starts_with("favicon_")
        || stem.starts_with("apple-touch-icon")
}

fn image_score(path: &Path, byte_len: u64) -> (bool, u64, u64) {
    let extension = image_extension(path).unwrap_or_default();
    if extension == "svg" {
        return (true, u64::MAX, byte_len);
    }
    let bytes = fs::read(path).unwrap_or_default();
    let area = match extension.as_str() {
        "png" => png_area(&bytes),
        "ico" => ico_area(&bytes),
        _ => None,
    }
    .or_else(|| dimension_hint(path))
    .unwrap_or(0);
    (false, area, byte_len)
}

fn png_area(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 24 || bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some(u64::from(width) * u64::from(height))
}

fn ico_area(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 6 || bytes.get(..4)? != [0, 0, 1, 0] {
        return None;
    }
    let count = usize::from(u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?));
    (0..count)
        .filter_map(|index| {
            let offset = 6 + index * 16;
            let width = match *bytes.get(offset)? {
                0 => 256_u64,
                value => u64::from(value),
            };
            let height = match *bytes.get(offset + 1)? {
                0 => 256_u64,
                value => u64::from(value),
            };
            Some(width * height)
        })
        .max()
}

fn dimension_hint(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?;
    stem.split(|character: char| !character.is_ascii_digit() && character != 'x')
        .filter_map(|part| part.split_once('x'))
        .filter_map(|(width, height)| {
            Some(width.parse::<u64>().ok()? * height.parse::<u64>().ok()?)
        })
        .max()
}

fn read_image(
    path: &Path,
    source: &'static str,
    relative_path: String,
) -> Result<ProjectIconImage, ProjectIconError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(ProjectIconError::SourceMissing);
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(ProjectIconError::ImageTooLarge);
    }
    let extension = image_extension(path).ok_or(ProjectIconError::UnsupportedFormat)?;
    let mime = mime_for_extension(&extension).ok_or(ProjectIconError::UnsupportedFormat)?;
    let bytes = fs::read(path)?;
    Ok(ProjectIconImage {
        data_url: format!("data:{mime};base64,{}", BASE64.encode(bytes)),
        source,
        path: relative_path,
    })
}

fn custom_icon_path(
    project_root: &Path,
    reference: &str,
) -> Result<(PathBuf, String), ProjectIconError> {
    let relative = reference
        .strip_prefix(CUSTOM_ICON_PREFIX)
        .ok_or(ProjectIconError::InvalidReference)?;
    let relative_path = Path::new(relative);
    let components = relative_path.components().collect::<Vec<_>>();
    if components.len() != 2
        || components[0] != Component::Normal(".workman".as_ref())
        || !matches!(components[1], Component::Normal(_))
        || !relative_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().starts_with("icon."))
        || image_extension(relative_path).is_none()
    {
        return Err(ProjectIconError::InvalidReference);
    }
    Ok((
        project_root.join(relative_path),
        relative.replace('\\', "/"),
    ))
}

fn image_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "ico" | "svg"
    )
    .then_some(extension)
}

fn mime_for_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn project(root: &Path, icon: Option<&str>) -> Project {
        Project {
            id: 1,
            path: root.to_string_lossy().into_owned(),
            name: "demo".to_owned(),
            display_name: None,
            icon: icon.map(str::to_owned),
            selected: true,
            sort_order: 0,
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn auto_icon_prefers_the_highest_resolution_candidate_and_caches_it() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("favicon.png"), png(16, 16)).unwrap();
        fs::create_dir(root.path().join("public")).unwrap();
        fs::write(
            root.path().join("public/apple-touch-icon-180x180.png"),
            png(180, 180),
        )
        .unwrap();
        let project = project(root.path(), None);

        let first = resolve(&project).unwrap();
        assert_eq!(first.source, "auto");
        assert_eq!(first.path, "public/apple-touch-icon-180x180.png");

        fs::create_dir(root.path().join("static")).unwrap();
        fs::write(root.path().join("static/favicon.png"), png(256, 256)).unwrap();
        assert_eq!(resolve(&project).unwrap().path, first.path);
        assert_eq!(refresh_auto(&project).unwrap().path, "static/favicon.png");
    }

    #[test]
    fn custom_image_is_copied_replaced_and_resolved_from_workman_directory() {
        let root = TempDir::new().unwrap();
        let source_png = root.path().join("picked.png");
        fs::write(&source_png, png(64, 64)).unwrap();
        let mut project = project(root.path(), None);

        let reference = copy_custom_image(&project, &source_png).unwrap();
        assert_eq!(reference, "image:.workman/icon.png");
        assert!(root.path().join(".workman/icon.png").is_file());
        project.icon = Some(reference);
        let image = resolve(&project).unwrap();
        assert_eq!(image.source, "custom");
        assert_eq!(image.path, ".workman/icon.png");
        assert!(image.data_url.starts_with("data:image/png;base64,"));

        fs::remove_file(root.path().join(".workman/icon.png")).unwrap();
        assert_eq!(resolve(&project), None);
        fs::write(root.path().join(".workman/icon.png"), png(64, 64)).unwrap();

        let source_jpg = root.path().join("picked.jpg");
        fs::write(&source_jpg, b"jpeg-placeholder").unwrap();
        let reference = copy_custom_image(&project, &source_jpg).unwrap();
        assert_eq!(reference, "image:.workman/icon.jpg");
        assert!(!root.path().join(".workman/icon.png").exists());
        assert_eq!(
            fs::read(root.path().join(".workman/icon.jpg")).unwrap(),
            b"jpeg-placeholder"
        );
    }

    #[test]
    fn missing_custom_image_falls_back_without_error() {
        let root = TempDir::new().unwrap();
        let project = project(root.path(), Some("image:.workman/icon.png"));
        assert_eq!(resolve(&project), None);
    }

    #[test]
    fn custom_reference_cannot_escape_the_project_icon_directory() {
        assert!(!is_custom_reference("image:../secret.png"));
        assert!(!is_custom_reference("image:/tmp/icon.png"));
        assert!(!is_custom_reference("image:.workman/avatar.png"));
        assert!(is_custom_reference("image:.workman/icon.webp"));
    }
}
