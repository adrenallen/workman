use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 5 * 1024 * 1024;

fn editor_root() -> PathBuf {
    workmand::default_data_dir().join("scratchpad-editor")
}

#[tauri::command]
pub fn scratchpad_editor_create(name: String, markdown: String) -> Result<String, String> {
    create(&editor_root(), &name, &markdown)
}

#[tauri::command]
pub fn scratchpad_editor_read(path: String) -> Result<String, String> {
    read(&editor_root(), Path::new(&path))
}

fn create(root: &Path, name: &str, markdown: &str) -> Result<String, String> {
    if markdown.len() as u64 > MAX_BYTES {
        return Err("Scratchpads opened in an editor must be smaller than 5 MB.".into());
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let title: String = name
        .chars()
        .take(80)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let path = root.join(format!(
        "{}-{}.md",
        if title.is_empty() {
            "scratchpad"
        } else {
            &title
        },
        uuid::Uuid::new_v4()
    ));
    let mut created = false;
    let result = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        created = true;
        file.write_all(markdown.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = result {
        if created {
            let _ = fs::remove_file(&path);
        }
        return Err(error.to_string());
    }
    Ok(path.to_string_lossy().into_owned())
}

fn read(root: &Path, path: &Path) -> Result<String, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if path.parent() != Some(root.as_path()) || path.extension().is_none_or(|ext| ext != "md") {
        return Err("This file is not a Workman scratchpad editor file.".into());
    }
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("The scratchpad editor path is not a file.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The editor file is larger than 5 MB.".into());
    }
    String::from_utf8(bytes)
        .map_err(|_| "Save the editor file as UTF-8 to sync it with Workman.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_saves_are_read_without_overwriting_previous_exports() {
        let dir = tempfile::tempdir().unwrap();
        let first = create(dir.path(), "../../Notes", "# Notes\n\nOriginal").unwrap();
        let second = create(dir.path(), "../../Notes", "New export").unwrap();
        assert_ne!(first, second);
        fs::write(&first, "# Notes\n\nEditor save 📝").unwrap();
        assert_eq!(
            read(dir.path(), Path::new(&first)).unwrap(),
            "# Notes\n\nEditor save 📝"
        );
        assert_eq!(read(dir.path(), Path::new(&second)).unwrap(), "New export");
    }

    #[test]
    fn rejects_unowned_invalid_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("outside.md");
        fs::write(&file, "secret").unwrap();
        assert!(read(dir.path(), &file).is_err());
        let owned = dir.path().join("owned.md");
        fs::write(&owned, [0xff]).unwrap();
        assert!(read(dir.path(), &owned).unwrap_err().contains("UTF-8"));
        fs::write(&owned, vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        assert!(read(dir.path(), &owned).unwrap_err().contains("5 MB"));
        assert!(create(dir.path(), "big", &"a".repeat(MAX_BYTES as usize + 1)).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("link.md");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert!(read(dir.path(), &link).is_err());
        }
    }
}
