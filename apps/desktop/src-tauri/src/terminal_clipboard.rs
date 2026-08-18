use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use image::{ImageFormat, ImageReader, Limits};
use workmand::default_data_dir;

#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG, NSPasteboardTypeString,
    NSPasteboardTypeTIFF,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSData, NSString};
use serde::Serialize;

const CLIPBOARD_DIRECTORY: &str = "terminal-clipboard";
const DRAFT_ATTACHMENT_DIRECTORY: &str = "draft-attachments";
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_RETAINED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RETAINED_FILES: usize = 64;
const MAX_RETAINED_AGE: Duration = Duration::from_secs(24 * 60 * 60);
static PASTE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TerminalClipboardRead {
    Text {
        text: String,
    },
    Image {
        path: Option<String>,
        clipboard_ready: bool,
    },
    Empty,
}

#[derive(Debug, Serialize)]
pub struct AttachmentImageRead {
    bytes: Vec<u8>,
    mime_type: &'static str,
}

#[tauri::command]
pub async fn terminal_read_clipboard(
    save_image: bool,
    normalize_image: bool,
) -> Result<TerminalClipboardRead, String> {
    tauri::async_runtime::spawn_blocking(move || read_native_clipboard(save_image, normalize_image))
        .await
        .map_err(|error| format!("clipboard read task failed: {error}"))?
}

#[tauri::command]
pub async fn terminal_write_clipboard_image(
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let png = normalize_image_png(&bytes, &mime_type)?;
        write_native_clipboard_png(&png)
    })
    .await
    .map_err(|error| format!("clipboard write task failed: {error}"))?
}

#[tauri::command]
pub async fn terminal_write_clipboard_text(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || write_native_clipboard_text(&text))
        .await
        .map_err(|error| format!("clipboard write task failed: {error}"))?
}

#[cfg(target_os = "macos")]
fn read_native_clipboard(
    save_image: bool,
    normalize_image: bool,
) -> Result<TerminalClipboardRead, String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    // SAFETY: These AppKit globals are immutable NSString constants.
    let image = unsafe {
        pasteboard
            .dataForType(NSPasteboardTypePNG)
            .map(|data| (data.to_vec(), "image/png"))
            .or_else(|| {
                pasteboard
                    .dataForType(NSPasteboardTypeTIFF)
                    .map(|data| (data.to_vec(), "image/tiff"))
            })
    };
    let image = match image {
        Some(image) => Some(image),
        None => read_clipboard_file_url(&pasteboard)?,
    };
    if let Some((bytes, mime_type)) = image {
        if normalize_image {
            let png = normalize_image_png(&bytes, mime_type)?;
            if write_native_clipboard_png(&png).is_ok() {
                return Ok(TerminalClipboardRead::Image {
                    path: None,
                    clipboard_ready: true,
                });
            }
            return Ok(TerminalClipboardRead::Image {
                path: Some(save_clipboard_image(&png, "image/png")?),
                clipboard_ready: false,
            });
        }
        let path = save_image
            .then(|| {
                let png = normalize_image_png(&bytes, mime_type)?;
                save_clipboard_image(&png, "image/png")
            })
            .transpose()?;
        return Ok(TerminalClipboardRead::Image {
            path,
            clipboard_ready: false,
        });
    }

    // SAFETY: NSPasteboardTypeString is an immutable AppKit NSString constant.
    let text = unsafe { pasteboard.stringForType(NSPasteboardTypeString) };
    Ok(text.map_or(TerminalClipboardRead::Empty, |text| {
        TerminalClipboardRead::Text {
            text: text.to_string(),
        }
    }))
}

#[cfg(target_os = "macos")]
fn read_clipboard_file_url(
    pasteboard: &NSPasteboard,
) -> Result<Option<(Vec<u8>, &'static str)>, String> {
    // SAFETY: NSPasteboardTypeFileURL is an immutable AppKit NSString constant.
    let value = unsafe { pasteboard.stringForType(NSPasteboardTypeFileURL) };
    let Some(value) = value else {
        return Ok(None);
    };
    let url = url::Url::parse(&value.to_string())
        .map_err(|error| format!("clipboard file URL is invalid: {error}"))?;
    let path = url
        .to_file_path()
        .map_err(|()| "clipboard file URL is not local".to_owned())?;
    Ok(Some((read_image_path(&path)?, "application/octet-stream")))
}

#[cfg(target_os = "macos")]
fn write_native_clipboard_text(text: &str) -> Result<(), String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: NSPasteboardTypeString is an immutable AppKit NSString constant.
    let written =
        unsafe { pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) };
    written
        .then_some(())
        .ok_or_else(|| "the system pasteboard rejected terminal text".to_owned())
}

#[cfg(target_os = "macos")]
fn write_native_clipboard_png(png: &[u8]) -> Result<(), String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    let data = NSData::with_bytes(png);
    // SAFETY: NSPasteboardTypePNG is an immutable AppKit NSString constant.
    let written = pasteboard.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG });
    written
        .then_some(())
        .ok_or_else(|| "the system pasteboard rejected the normalized PNG".to_owned())
}

#[cfg(not(target_os = "macos"))]
fn read_native_clipboard(
    _save_image: bool,
    _normalize_image: bool,
) -> Result<TerminalClipboardRead, String> {
    Err("terminal context-menu paste is not supported on this platform".to_owned())
}

#[cfg(not(target_os = "macos"))]
fn write_native_clipboard_png(_png: &[u8]) -> Result<(), String> {
    Err("terminal image clipboard writes are not supported on this platform".to_owned())
}

#[cfg(not(target_os = "macos"))]
fn write_native_clipboard_text(_text: &str) -> Result<(), String> {
    Err("terminal context-menu copy is not supported on this platform".to_owned())
}

#[tauri::command]
pub async fn terminal_save_clipboard_image(
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || save_clipboard_image(&bytes, &mime_type))
        .await
        .map_err(|error| format!("clipboard image task failed: {error}"))?
}

#[tauri::command]
pub async fn terminal_save_clipboard_png(
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let png = normalize_image_png(&bytes, &mime_type)?;
        save_clipboard_image(&png, "image/png")
    })
    .await
    .map_err(|error| format!("clipboard image task failed: {error}"))?
}

#[tauri::command]
pub async fn terminal_save_draft_image(
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let png = normalize_image_png(&bytes, &mime_type)?;
        save_draft_png(&png)
    })
    .await
    .map_err(|error| format!("draft image task failed: {error}"))?
}

#[tauri::command]
pub async fn terminal_import_draft_image(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = read_image_path(Path::new(&path))?;
        let png = normalize_image_png(&bytes, "application/octet-stream")?;
        save_draft_png(&png)
    })
    .await
    .map_err(|error| format!("draft image import task failed: {error}"))?
}

#[tauri::command]
pub async fn terminal_read_attachment_image(path: String) -> Result<AttachmentImageRead, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = read_image_path(Path::new(&path))?;
        let png = normalize_image_png(&bytes, "application/octet-stream")?;
        Ok(AttachmentImageRead {
            bytes: png,
            mime_type: "image/png",
        })
    })
    .await
    .map_err(|error| format!("attachment image read task failed: {error}"))?
}

fn read_image_path(path: &Path) -> Result<Vec<u8>, String> {
    if !path.is_absolute() {
        return Err("image path must be absolute".to_owned());
    }
    let mut file =
        fs::File::open(path).map_err(|error| format!("could not open image: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not inspect image: {error}"))?;
    if !metadata.is_file() {
        return Err("image path is not a regular file".to_owned());
    }
    if metadata.len() > MAX_IMAGE_BYTES as u64 {
        return Err(format!(
            "image exceeds the {} MiB limit",
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read image: {error}"))?;
    (bytes.len() <= MAX_IMAGE_BYTES)
        .then_some(bytes)
        .ok_or_else(|| {
            format!(
                "image exceeds the {} MiB limit",
                MAX_IMAGE_BYTES / 1024 / 1024
            )
        })
}

fn save_draft_png(png: &[u8]) -> Result<String, String> {
    let _guard = CLIPBOARD_LOCK
        .lock()
        .map_err(|_| "draft image lock is poisoned".to_owned())?;
    let directory = default_data_dir().join(DRAFT_ATTACHMENT_DIRECTORY);
    save_draft_png_in(&directory, png, SystemTime::now())?
        .into_os_string()
        .into_string()
        .map_err(|_| "draft image path is not valid UTF-8".to_owned())
}

fn save_draft_png_in(directory: &Path, png: &[u8], now: SystemTime) -> Result<PathBuf, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create draft attachment directory: {error}"))?;
    secure_directory(directory)?;
    let millis = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    for _ in 0..16 {
        let sequence = PASTE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            "draft-{millis}-{}-{sequence}.png",
            std::process::id()
        ));
        match create_private_file(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(png).and_then(|_| file.sync_data()) {
                    let _ = fs::remove_file(&path);
                    return Err(format!("could not save draft image: {error}"));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("could not create draft image: {error}")),
        }
    }
    Err("could not allocate a unique draft image path".to_owned())
}

fn normalize_image_png(bytes: &[u8], mime_type: &str) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("clipboard image is empty".to_owned());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "clipboard image exceeds the {} MiB limit",
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }
    let format = image_format(mime_type)
        .or_else(|| image::guess_format(bytes).ok())
        .ok_or_else(|| "clipboard item is not a supported raster image".to_owned())?;
    if !matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::WebP
            | ImageFormat::Bmp
            | ImageFormat::Tiff
    ) {
        return Err("clipboard item is not a supported raster image".to_owned());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| format!("could not decode clipboard image: {error}"))?;
    let mut output = Cursor::new(Vec::new());
    decoded
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|error| format!("could not encode clipboard image as PNG: {error}"))?;
    let png = output.into_inner();
    if png.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "normalized clipboard image exceeds the {} MiB limit",
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }
    Ok(png)
}

fn image_format(mime_type: &str) -> Option<ImageFormat> {
    match mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/tiff" => Some(ImageFormat::Tiff),
        _ => None,
    }
}

fn save_clipboard_image(bytes: &[u8], mime_type: &str) -> Result<String, String> {
    let _guard = CLIPBOARD_LOCK
        .lock()
        .map_err(|_| "clipboard image retention lock is poisoned".to_owned())?;
    let directory = default_data_dir().join(CLIPBOARD_DIRECTORY);
    let path = save_image_in(&directory, bytes, mime_type, SystemTime::now())?;
    path.into_os_string()
        .into_string()
        .map_err(|_| "clipboard image path is not valid UTF-8".to_owned())
}

fn save_image_in(
    directory: &Path,
    bytes: &[u8],
    mime_type: &str,
    now: SystemTime,
) -> Result<PathBuf, String> {
    if bytes.is_empty() {
        return Err("clipboard image is empty".to_owned());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "clipboard image exceeds the {} MiB limit",
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }
    let extension = image_extension(mime_type)
        .ok_or_else(|| "clipboard item is not a supported raster image".to_owned())?;

    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create clipboard image directory: {error}"))?;
    secure_directory(directory)?;
    sweep_managed_images(
        directory,
        now,
        MAX_RETAINED_AGE,
        MAX_RETAINED_FILES.saturating_sub(1),
        MAX_RETAINED_BYTES.saturating_sub(bytes.len() as u64),
        None,
    )?;

    let millis = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut saved = None;
    for _ in 0..16 {
        let sequence = PASTE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            "paste-{millis}-{}-{sequence}.{extension}",
            std::process::id()
        ));
        let file = create_private_file(&path);
        match file {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_data()) {
                    let _ = fs::remove_file(&path);
                    return Err(format!("could not save clipboard image: {error}"));
                }
                saved = Some(path);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("could not create clipboard image: {error}")),
        }
    }
    let path =
        saved.ok_or_else(|| "could not allocate a unique clipboard image path".to_owned())?;

    if let Err(error) = sweep_managed_images(
        directory,
        now,
        MAX_RETAINED_AGE,
        MAX_RETAINED_FILES,
        MAX_RETAINED_BYTES,
        Some(&path),
    ) {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

fn image_extension(mime_type: &str) -> Option<&'static str> {
    match mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tiff"),
        _ => None,
    }
}

#[cfg(unix)]
fn secure_directory(directory: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not secure clipboard image directory: {error}"))
}

#[cfg(not(unix))]
fn secure_directory(_directory: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[derive(Debug)]
struct ManagedImage {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
    keep: bool,
}

fn sweep_managed_images(
    directory: &Path,
    now: SystemTime,
    max_age: Duration,
    max_files: usize,
    max_bytes: u64,
    keep: Option<&Path>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not inspect clipboard image directory: {error}"))?;
    let mut images = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not inspect clipboard image: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect clipboard image type: {error}"))?;
        if !file_type.is_file() || !is_managed_image_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("could not inspect clipboard image metadata: {error}"))?;
        let path = entry.path();
        images.push(ManagedImage {
            keep: keep.is_some_and(|candidate| candidate == path),
            path,
            modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            bytes: metadata.len(),
        });
    }

    for image in &images {
        let expired = now
            .duration_since(image.modified)
            .is_ok_and(|age| age > max_age);
        if expired && !image.keep {
            fs::remove_file(&image.path)
                .map_err(|error| format!("could not expire clipboard image: {error}"))?;
        }
    }
    images.retain(|image| {
        image.keep
            || !now
                .duration_since(image.modified)
                .is_ok_and(|age| age > max_age)
    });
    images.sort_by(|left, right| {
        left.modified
            .cmp(&right.modified)
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut count = images.len();
    let mut bytes = images.iter().map(|image| image.bytes).sum::<u64>();
    for image in &images {
        if count <= max_files && bytes <= max_bytes {
            break;
        }
        if image.keep {
            continue;
        }
        fs::remove_file(&image.path)
            .map_err(|error| format!("could not prune clipboard image: {error}"))?;
        count = count.saturating_sub(1);
        bytes = bytes.saturating_sub(image.bytes);
    }
    if count > max_files || bytes > max_bytes {
        return Err("clipboard image retention limit could not be satisfied".to_owned());
    }
    Ok(())
}

fn is_managed_image_name(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    stem.starts_with("paste-")
        && matches!(extension, "png" | "jpg" | "gif" | "webp" | "bmp" | "tiff")
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn saves_private_raster_image_in_managed_directory() {
        let root = tempdir().unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let path = save_image_in(root.path(), b"png-bytes", "image/png", now).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"png-bytes");
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("paste-")
        );
        assert_eq!(path.extension().unwrap(), "png");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn rejects_non_images_empty_images_and_oversized_images() {
        let root = tempdir().unwrap();
        let now = SystemTime::now();

        assert!(save_image_in(root.path(), b"text", "text/plain", now).is_err());
        assert!(save_image_in(root.path(), b"", "image/png", now).is_err());
        assert!(
            save_image_in(root.path(), &vec![0; MAX_IMAGE_BYTES + 1], "image/png", now).is_err()
        );
    }

    #[test]
    fn normalizes_tiff_bytes_to_png_for_claude_clipboard_delivery() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([12, 34, 56, 255]),
        ));
        let mut tiff = Cursor::new(Vec::new());
        image.write_to(&mut tiff, ImageFormat::Tiff).unwrap();

        let png = normalize_image_png(&tiff.into_inner(), "image/tiff").unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn draft_images_use_a_private_unswept_store() {
        let root = tempdir().unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let first = save_draft_png_in(root.path(), b"first", now).unwrap();
        let second = save_draft_png_in(root.path(), b"second", now).unwrap();

        assert_eq!(fs::read(first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"second");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in fs::read_dir(root.path()).unwrap() {
                assert_eq!(
                    entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }

    #[test]
    fn retention_expires_and_caps_only_managed_images() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("keep.txt"), b"sentinel").unwrap();
        for index in 0..4 {
            fs::write(root.path().join(format!("paste-{index}.png")), [index; 4]).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        sweep_managed_images(
            root.path(),
            SystemTime::now(),
            Duration::from_secs(60),
            2,
            8,
            None,
        )
        .unwrap();

        let retained = fs::read_dir(root.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(retained.contains(&"keep.txt".to_owned()));
        assert_eq!(
            retained
                .iter()
                .filter(|name| name.starts_with("paste-"))
                .count(),
            2
        );

        sweep_managed_images(
            root.path(),
            SystemTime::now() + Duration::from_secs(120),
            Duration::from_secs(60),
            2,
            8,
            None,
        )
        .unwrap();
        assert!(root.path().join("keep.txt").exists());
        assert_eq!(
            fs::read_dir(root.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("paste-"))
                .count(),
            0
        );
    }
}
