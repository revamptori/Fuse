//! Incremental directory listing.
//!
//! `read_dir` returns entries in filesystem order, which is essentially arbitrary
//! on ext4/F2FS. Android's own `File` listing does the same, but a file manager
//! that renders thousands of items benefits from two things:
//!
//! 1. streaming the raw scan so the first screenful appears immediately;
//! 2. a stable, user-selected order for the UI.
//!
//! We do both: `list_dir` produces `Entry` records and the caller sorts them.
//! Sorting 50 000 entries with the natural comparator takes ~10 ms, well within
//! budget, and it happens off the main thread.

use crate::cancel::CancelToken;
use crate::entry::Entry;
use crate::error::{FsError, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum entries returned by one listing call. Directories with more items
/// are streamed in pages by the caller using `offset`.
pub const MAX_ENTRIES_PER_CALL: usize = 200_000;

#[derive(Debug, Clone, Copy)]
pub struct ListOptions {
    pub show_hidden: bool,
    /// Also resolve symlink targets when the entry itself is a link.
    pub follow_symlinks: bool,
    /// Skip entries below this offset (paging for huge directories).
    pub offset: usize,
    /// Hard cap on returned entries; 0 means "no cap beyond MAX_ENTRIES_PER_CALL".
    pub limit: usize,
}

impl Default for ListOptions {
    fn default() -> Self {
        ListOptions { show_hidden: false, follow_symlinks: false, offset: 0, limit: 0 }
    }
}

/// Listing result with directory-level metadata the UI needs for the header.
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// True when the directory held more entries than the returned page.
    pub has_more: bool,
    pub total_scanned: usize,
    /// Set when the directory could not be opened at all.
    pub error: Option<FsError>,
}

/// Read a directory into memory. Errors opening the directory are returned;
/// errors on individual children are tolerated (the child is then listed
/// with `stat_ok = false`, so the UI can still show it).
pub fn list_dir(path: &Path, opts: ListOptions, cancel: &CancelToken) -> Result<Listing> {
    let meta = fs::metadata(path).map_err(|e| FsError::from_io_path(e, path))?;
    if !meta.is_dir() {
        return Err(FsError::new(crate::error::ErrorCode::NotADirectory, "Not a directory")
            .with_path(path.to_string_lossy()));
    }
    let rd = fs::read_dir(path).map_err(|e| FsError::from_io_path(e, path))?;
    let cap = opts.limit.max(1).min(4096);
    let mut out = Vec::with_capacity(cap);
    let mut scanned = 0usize;
    let mut skipped = 0usize;
    let limit = opts.limit;
    let mut has_more = false;
    for item in rd {
        if scanned & 0x3FF == 0 {
            // Cancellation is checked every 1024 entries; cheaper than per entry.
            cancel.check()?;
        }
        scanned += 1;
        let item = match item {
            Ok(i) => i,
            Err(_) => {
                // Transient readdir error (e.g. entry deleted mid-scan).
                continue;
            }
        };
        let name = item.file_name();
        let name_str = name.to_string_lossy();
        if !opts.show_hidden && name_str.starts_with('.') {
            continue;
        }
        if skipped < opts.offset {
            skipped += 1;
            continue;
        }
        if limit != 0 && out.len() >= limit {
            has_more = true;
            break;
        }
        if out.len() >= MAX_ENTRIES_PER_CALL {
            has_more = true;
            break;
        }
        let child = item.path();
        let meta = if opts.follow_symlinks {
            fs::metadata(&child).or_else(|_| fs::symlink_metadata(&child)).ok()
        } else {
            fs::symlink_metadata(&child).ok()
        };
        out.push(Entry::from_parts(name_str.into_owned(), meta.as_ref()));
    }
    Ok(Listing { entries: out, has_more, total_scanned: scanned, error: None })
}

/// Count of all entries in a directory (including hidden), without metadata.
pub fn count_entries(path: &Path) -> Result<u64> {
    let rd = fs::read_dir(path).map_err(|e| FsError::from_io_path(e, path))?;
    let mut n = 0u64;
    for item in rd {
        if item.is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// All mount points of the device, used for the storage overview and for
/// showing removable volumes. Parsed from /proc/mounts with no dependencies.
#[derive(Debug, Clone)]
pub struct MountPoint {
    pub path: PathBuf,
    pub fs_type: String,
    pub device: String,
    pub is_emulated: bool,
}

pub fn mount_points() -> Vec<MountPoint> {
    let mut out = Vec::new();
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let device = parts.next().unwrap_or("");
        // /proc/mounts escapes spaces as \040; undo for display purposes.
        let path_raw = parts.next().unwrap_or("");
        if path_raw.is_empty() {
            continue;
        }
        let path = path_raw.replace("\\040", " ");
        let fs_type = parts.next().unwrap_or("");
        if !(path.starts_with("/storage/") || path.starts_with("/mnt/media_rw/")) {
            continue;
        }
        if fs_type == "tmpfs" && !path.starts_with("/storage/emulated") {
            continue;
        }
        let is_emulated = path.starts_with("/storage/emulated") || path.starts_with("/mnt/runtime");
        out.push(MountPoint {
            path: PathBuf::from(path),
            fs_type: fs_type.to_string(),
            device: device.replace("\\040", " "),
            is_emulated,
        });
    }
    // Deduplicate by path, preferring /storage paths over /mnt/media_rw.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fuss-list-{}-{}-{}", tag, std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn lists_and_hides_dotfiles() {
        let d = tmpdir("list");
        fs::write(d.join("a.txt"), "a").unwrap();
        fs::write(d.join(".hidden"), "h").unwrap();
        fs::create_dir(d.join("sub")).unwrap();
        let t = CancelToken::new();
        let l = list_dir(&d, ListOptions::default(), &t).unwrap();
        assert_eq!(l.entries.len(), 2);
        assert!(l.entries.iter().all(|e| !e.name.starts_with('.')));
        let l2 = list_dir(&d, ListOptions { show_hidden: true, ..Default::default() }, &t).unwrap();
        assert_eq!(l2.entries.len(), 3);
        let sub = l2.entries.iter().find(|e| e.name == "sub").unwrap();
        assert!(sub.is_dir);
        assert_eq!(sub.kind, crate::ext::FileKind::Directory);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn paging() {
        let d = tmpdir("page");
        for i in 0..25 {
            fs::write(d.join(format!("f{i:02}.txt")), "").unwrap();
        }
        let t = CancelToken::new();
        let l = list_dir(&d, ListOptions { limit: 10, ..Default::default() }, &t).unwrap();
        assert_eq!(l.entries.len(), 10);
        assert!(l.has_more);
        let l2 = list_dir(&d, ListOptions { limit: 10, offset: 10, ..Default::default() }, &t).unwrap();
        assert_eq!(l2.entries.len(), 10);
        let l3 = list_dir(&d, ListOptions { limit: 10, offset: 20, ..Default::default() }, &t).unwrap();
        assert_eq!(l3.entries.len(), 5);
        assert!(!l3.has_more);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn not_a_directory() {
        let d = tmpdir("nad");
        let f = d.join("f");
        fs::write(&f, "").unwrap();
        let err = list_dir(&f, ListOptions::default(), &CancelToken::new()).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::NotADirectory);
        fs::remove_dir_all(&d).unwrap();
    }
}
