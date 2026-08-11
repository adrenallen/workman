use std::{
    error::Error,
    fmt, fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage, imageops::FilterType};
use serde::Serialize;
use uuid::Uuid;
use workman_core::{AgentTool, AgentToolId};

const ICON_DIRECTORY: &str = "agent-icons";
const ICON_SIZE: u32 = 64;
const ICON_CONTENT_SIZE: u32 = 56;
const MAX_SOURCE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentToolView {
    #[serde(flatten)]
    pub tool: AgentTool,
    pub icon_data_url: Option<String>,
}

#[derive(Debug)]
pub enum AgentIconError {
    SourceMissing,
    ImageTooLarge,
    UnsupportedFormat,
    Decode(image::ImageError),
    Io(std::io::Error),
}

impl fmt::Display for AgentIconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceMissing => formatter.write_str("selected image does not exist"),
            Self::ImageTooLarge => formatter.write_str("selected image must be 5 MB or smaller"),
            Self::UnsupportedFormat => {
                formatter.write_str("selected image must be PNG, JPEG, WebP, GIF, BMP, or ICO")
            }
            Self::Decode(error) => write!(formatter, "could not decode selected image: {error}"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl Error for AgentIconError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AgentIconError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<image::ImageError> for AgentIconError {
    fn from(error: image::ImageError) -> Self {
        Self::Decode(error)
    }
}

pub fn views(tools: Vec<AgentTool>, data_dir: &Path) -> Vec<AgentToolView> {
    tools.into_iter().map(|tool| view(tool, data_dir)).collect()
}

pub fn view(tool: AgentTool, data_dir: &Path) -> AgentToolView {
    let icon_data_url = read_icon_data_url(data_dir, tool.id).ok();
    AgentToolView {
        tool,
        icon_data_url,
    }
}

pub fn set_override(
    tool: AgentTool,
    data_dir: &Path,
    source_path: &Path,
) -> Result<AgentToolView, AgentIconError> {
    let source = fs::canonicalize(source_path).map_err(|_| AgentIconError::SourceMissing)?;
    let metadata = fs::metadata(&source).map_err(|_| AgentIconError::SourceMissing)?;
    if !metadata.is_file() {
        return Err(AgentIconError::SourceMissing);
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(AgentIconError::ImageTooLarge);
    }

    let mut reader = ImageReader::open(&source)?;
    reader = reader
        .with_guessed_format()
        .map_err(|_| AgentIconError::UnsupportedFormat)?;
    if !matches!(
        reader.format(),
        Some(
            ImageFormat::Png
                | ImageFormat::Jpeg
                | ImageFormat::WebP
                | ImageFormat::Gif
                | ImageFormat::Bmp
                | ImageFormat::Ico
        )
    ) {
        return Err(AgentIconError::UnsupportedFormat);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode()?;
    let resized = decoded
        .resize(ICON_CONTENT_SIZE, ICON_CONTENT_SIZE, FilterType::Lanczos3)
        .to_rgba8();
    let mut canvas = RgbaImage::new(ICON_SIZE, ICON_SIZE);
    let x = i64::from((ICON_SIZE - resized.width()) / 2);
    let y = i64::from((ICON_SIZE - resized.height()) / 2);
    image::imageops::overlay(&mut canvas, &resized, x, y);

    let directory = icon_directory(data_dir);
    fs::create_dir_all(&directory)?;
    restrict_directory_permissions(&directory)?;
    let destination = icon_path(data_dir, tool.id);
    let temporary = directory.join(format!(".{}.{}.tmp", tool.id, Uuid::new_v4()));
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    restrict_file_permissions(&output)?;
    {
        let mut writer = BufWriter::new(&mut output);
        DynamicImage::ImageRgba8(canvas).write_to(&mut writer, ImageFormat::Png)?;
        writer.flush()?;
    }
    output.sync_all()?;
    fs::rename(&temporary, &destination)?;

    Ok(view(tool, data_dir))
}

pub fn remove_override(tool: AgentTool, data_dir: &Path) -> Result<AgentToolView, AgentIconError> {
    match fs::remove_file(icon_path(data_dir, tool.id)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(view(tool, data_dir))
}

pub fn delete_override(data_dir: &Path, tool_id: AgentToolId) -> Result<(), AgentIconError> {
    match fs::remove_file(icon_path(data_dir, tool_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn read_icon_data_url(data_dir: &Path, tool_id: AgentToolId) -> Result<String, AgentIconError> {
    let bytes = fs::read(icon_path(data_dir, tool_id))?;
    Ok(format!("data:image/png;base64,{}", BASE64.encode(bytes)))
}

fn icon_directory(data_dir: &Path) -> PathBuf {
    data_dir.join(ICON_DIRECTORY)
}

fn icon_path(data_dir: &Path, tool_id: AgentToolId) -> PathBuf {
    icon_directory(data_dir).join(format!("{tool_id}.png"))
}

#[cfg(unix)]
fn restrict_directory_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file_permissions(file: &fs::File) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file_permissions(_file: &fs::File) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use workman_core::AgentToolSource;

    fn tool(id: AgentToolId) -> AgentTool {
        AgentTool {
            id,
            name: "Custom".to_owned(),
            command: "custom-agent".to_owned(),
            tool_type: "custom".to_owned(),
            enabled: true,
            source: AgentToolSource::Config,
            resume_args: None,
            continue_args: None,
        }
    }

    #[test]
    fn override_is_normalized_into_the_workman_data_directory_and_removable() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("wide.png");
        RgbaImage::from_pixel(120, 30, image::Rgba([30, 60, 90, 255]))
            .save(&source)
            .unwrap();
        let data_dir = temp.path().join("state");

        let view = set_override(tool(42), &data_dir, &source).unwrap();
        assert!(
            view.icon_data_url
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        let stored = image::open(data_dir.join("agent-icons/42.png")).unwrap();
        assert_eq!((stored.width(), stored.height()), (ICON_SIZE, ICON_SIZE));
        assert_eq!(stored.get_pixel(0, 0).0[3], 0);
        assert_eq!(stored.get_pixel(32, 32).0, [30, 60, 90, 255]);

        let view = remove_override(tool(42), &data_dir).unwrap();
        assert_eq!(view.icon_data_url, None);
        assert!(!data_dir.join("agent-icons/42.png").exists());
    }

    #[test]
    fn override_rejects_svg_instead_of_persisting_untrusted_markup() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("mark.svg");
        fs::write(&source, "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").unwrap();
        let error = set_override(tool(7), &temp.path().join("state"), &source).unwrap_err();
        assert!(matches!(error, AgentIconError::UnsupportedFormat));
    }
}
