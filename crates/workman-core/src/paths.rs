//! Canonicalization that keeps Windows paths in their familiar drive-letter form.

use std::io;
use std::path::{Path, PathBuf};

/// Canonicalize like [`std::fs::canonicalize`], but return the familiar
/// drive-letter form on Windows instead of the `\\?\` verbatim form whenever the
/// path stays unambiguous without the prefix. Every Workman canonicalization
/// funnels through here so stored paths, lookups, and display always agree.
pub fn canonical_path(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        dunce::canonicalize(path)
    }
    #[cfg(not(windows))]
    {
        std::fs::canonicalize(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_paths_do_not_carry_a_verbatim_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = canonical_path(temp.path()).unwrap();
        assert!(
            !canonical.to_string_lossy().starts_with(r"\\?\"),
            "canonical form must stay in the familiar drive form: {}",
            canonical.display()
        );
        assert!(canonical.is_absolute());
    }
}
