//! Path handling for user-supplied names and paths.
//!
//! Every path that crosses the JNI boundary is a plain UTF-8 (lossy) absolute
//! filesystem path. Android app-private paths are plain POSIX paths; SAF-backed
//! locations are mounted by the Kotlin layer (`/proc/self/fd/N` after opening a
//! document), so the engine only ever deals with ordinary paths. That keeps the
//! engine platform-agnostic and testable on the host.

use crate::error::{ErrorCode, FsError, Result};
use std::path::{Path, PathBuf};

/// Longest filename most Linux filesystems accept (bytes).
pub const MAX_NAME_BYTES: usize = 255;

/// Validate a single path component supplied by the user (new file/folder name,
/// rename target). Rejects separators, NULs, `.`, `..`, and over-long names.
pub fn validate_name(name: &str) -> Result<&str> {
    if name.is_empty() {
        return Err(FsError::new(
            ErrorCode::InvalidName,
            "Name must not be empty",
        ));
    }
    if name == "." || name == ".." {
        return Err(FsError::new(ErrorCode::InvalidName, "Reserved name"));
    }
    if name.contains('/') || name.contains('\0') {
        return Err(FsError::new(
            ErrorCode::InvalidName,
            "Name must not contain '/' or NUL",
        ));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(FsError::new(ErrorCode::InvalidName, "Name is too long"));
    }
    Ok(name)
}

/// Join `parent` and `name`, validating the name first.
pub fn join_checked(parent: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    Ok(parent.join(name))
}

/// Sanitize an entry name taken from an archive.
///
/// Strips any leading separators, resolves `.`/`..` components, and rejects
/// absolute paths. Returns `None` when the entry would escape the destination
/// (zip-slip) or has no usable name.
pub fn safe_archive_relative(raw: &str) -> Option<PathBuf> {
    let normalized = raw.replace('\\', "/");
    let mut out = PathBuf::new();
    for comp in normalized.split('/') {
        match comp {
            "" | "." => continue,
            ".." => {
                if !out.pop() {
                    return None;
                }
            }
            other => {
                if other.contains('\0') {
                    return None;
                }
                out.push(other);
            }
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Directory name (single component) of a path, lossily.
pub fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Ensure `path` has no `..` components and is absolute-ish; used for paths
/// coming from the UI so a compromised caller cannot rely on traversal quirks.
pub fn normalize_absolute(path: &str) -> Result<PathBuf> {
    if path.is_empty() {
        return Err(FsError::new(ErrorCode::InvalidPath, "Empty path"));
    }
    let p = PathBuf::from(path);
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(
            FsError::new(ErrorCode::InvalidPath, "Path must not contain '..'").with_path(path),
        );
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names() {
        assert!(validate_name("hello.txt").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\0b").is_err());
        assert!(validate_name(&"x".repeat(256)).is_err());
        assert!(validate_name(&"x".repeat(255)).is_ok());
    }

    #[test]
    fn joins_checked() {
        let p = join_checked(Path::new("/sdcard/DCIM"), "a.jpg").unwrap();
        assert_eq!(p, PathBuf::from("/sdcard/DCIM/a.jpg"));
        assert!(join_checked(Path::new("/sdcard"), "../etc/passwd").is_err());
    }

    #[test]
    fn archive_paths_are_contained() {
        assert_eq!(
            safe_archive_relative("a/b.txt"),
            Some(PathBuf::from("a/b.txt"))
        );
        assert_eq!(safe_archive_relative("./a/./b"), Some(PathBuf::from("a/b")));
        assert_eq!(
            safe_archive_relative("/etc/passwd"),
            Some(PathBuf::from("etc/passwd"))
        );
        assert_eq!(safe_archive_relative("a/../../b"), None);
        assert_eq!(safe_archive_relative(".."), None);
        assert_eq!(
            safe_archive_relative("a\\b\\c"),
            Some(PathBuf::from("a/b/c"))
        );
        assert_eq!(safe_archive_relative(""), None);
        assert_eq!(safe_archive_relative("a/b/.."), Some(PathBuf::from("a")));
    }

    #[test]
    fn rejects_parent_paths() {
        assert!(normalize_absolute("/sdcard/a/../b").is_err());
        assert!(normalize_absolute("/sdcard/a").is_ok());
    }
}
