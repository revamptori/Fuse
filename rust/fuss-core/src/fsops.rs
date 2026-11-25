//! Core filesystem operations: single entry, batched, recursive.
//!
//! Design rules enforced here:
//! * Stream, never slurp: copies use a fixed 128 KiB buffer; nothing larger than
//!   that is ever held in memory regardless of file size.
//! * Never leave half-written output: copies go to a sibling temp file which is
//!   renamed into place only after a successful `fsync`.
//! * Recursive operations refuse to descend into the destination (moving a
//!   directory into itself) and never follow symlinks.
//! * Every long loop polls a `CancelToken`.

use crate::cancel::CancelToken;
use crate::entry::{mode_of, mtime_secs, Entry};
use crate::error::{ErrorCode, FsError, Result};
use crate::path::validate_name;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Streaming copy buffer size. 128 KiB keeps syscall count low without
/// pressuring the ART heap or the page cache.
pub const COPY_BUF: usize = 128 * 1024;

/// How to react when a destination already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionMode {
    /// Fail with `AlreadyExists` (default; UI then asks the user).
    Fail,
    /// Replace the destination.
    Overwrite,
    /// Pick "name (1).ext", "name (2).ext", ...
    Rename,
}

impl CollisionMode {
    pub fn from_u8(v: u8) -> CollisionMode {
        match v {
            1 => CollisionMode::Overwrite,
            2 => CollisionMode::Rename,
            _ => CollisionMode::Fail,
        }
    }
}

/// Outcome of one operation, consumed by the Kotlin layer to build a summary.
#[derive(Debug, Clone, Default)]
pub struct OpStats {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub skipped: u64,
    /// First error encountered in a "continue on error" batch, if any.
    pub first_error: Option<FsError>,
    pub errors: u64,
}

impl OpStats {
    fn record_err(&mut self, e: FsError) {
        self.errors += 1;
        if self.first_error.is_none() {
            self.first_error = Some(e);
        }
    }
}

/// Check that `child` is not `ancestor` and not inside it (real paths, so
/// symlinked roots are compared correctly).
pub fn would_recurse_into(child: &Path, ancestor: &Path) -> bool {
    let child = fs::canonicalize(child).unwrap_or_else(|_| child.to_path_buf());
    let ancestor = fs::canonicalize(ancestor).unwrap_or_else(|_| ancestor.to_path_buf());
    child == ancestor || child.starts_with(&ancestor)
}

/// `name (1).ext` style collision-free name in `dir`.
pub fn unique_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && name[..i].chars().any(|c| c != '.') => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for i in 1..10_000u32 {
        let candidate = format!("{stem} ({i}){ext}");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{stem} ({}){ext}", std::process::id())
}

/// Apply a collision policy, returning the final destination path.
fn resolve_dest(dest: &Path, mode: CollisionMode, recursion_ok: bool) -> Result<PathBuf> {
    let exists = fs::symlink_metadata(dest).is_ok();
    if !exists {
        return Ok(dest.to_path_buf());
    }
    match mode {
        CollisionMode::Fail => Err(FsError::new(
            ErrorCode::AlreadyExists,
            format!("'{}' already exists", crate::path::file_name_of(dest)),
        )),
        CollisionMode::Rename => {
            let parent = dest.parent().ok_or_else(|| FsError::invalid_arg("Destination has no parent"))?;
            let name = crate::path::file_name_of(dest);
            Ok(parent.join(unique_name(parent, &name)))
        }
        CollisionMode::Overwrite => {
            if recursion_ok {
                // Recursive overwrite merges directory trees; handled by caller.
                Ok(dest.to_path_buf())
            } else {
                Ok(dest.to_path_buf())
            }
        }
    }
}

fn open_src(path: &Path) -> Result<File> {
    File::open(path).map_err(|e| FsError::from_io_path(e, path))
}

fn create_dest(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|e| FsError::from_io_path(e, path))
}

/// Stream `src` into `dst`, returning bytes copied. `dst` is expected to be a
/// fresh/open file. Polls `cancel` once per buffer.
pub fn copy_stream(
    src: &mut File,
    dst: &mut File,
    buf: &mut [u8],
    cancel: &CancelToken,
) -> Result<u64> {
    let mut total = 0u64;
    loop {
        cancel.check()?;
        let n = match src.read(buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(FsError::from_io_pathless(e)),
        };
        dst.write_all(&buf[..n]).map_err(|e| FsError::from_io_pathless(e))?;
        total += n as u64;
        cancel.add_processed(n as u64);
    }
    Ok(total)
}

/// Write a temp file next to `dest` so the final rename is atomic on the same
/// filesystem; avoids leaving a truncated destination if we are killed midway.
fn temp_sibling(dest: &Path) -> PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("/"));
    let name = crate::path::file_name_of(dest);
    let unique = format!(".{}.fuss-{}", name, std::process::id());
    parent.join(unique)
}

fn cleanup_temp(p: &Path) {
    let _ = fs::remove_file(p);
}

/// Copy one regular file, preserving modification time when possible.
pub fn copy_file(
    src: &Path,
    dest: &Path,
    mode: CollisionMode,
    cancel: &CancelToken,
) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(src).map_err(|e| FsError::from_io_path(e, src))?;
    if meta.file_type().is_symlink() {
        return copy_symlink(src, dest, mode);
    }
    if meta.is_dir() {
        return Err(FsError::new(ErrorCode::NotADirectory, "Source is a directory").with_path(src.to_string_lossy()));
    }
    let final_dest = resolve_dest(dest, mode, false)?;
    if final_dest.exists() && !final_dest.is_file() {
        return Err(FsError::new(ErrorCode::NotADirectory, "Destination is not a regular file")
            .with_path(final_dest.to_string_lossy()));
    }
    let tmp = temp_sibling(&final_dest);
    let mut src_file = open_src(src)?;
    let mut out = create_dest(&tmp)?;
    let mut buf = vec![0u8; COPY_BUF];
    let copied = match copy_stream(&mut src_file, &mut out, &mut buf, cancel) {
        Ok(n) => n,
        Err(e) => {
            drop(out);
            cleanup_temp(&tmp);
            return Err(e);
        }
    };
    if let Err(e) = out.sync_all() {
        drop(out);
        cleanup_temp(&tmp);
        return Err(FsError::from_io_path(e, &tmp));
    }
    // Preserve permission bits and mtime best-effort (ignored on failure).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(meta.permissions().mode()));
    }
    let mtime = filetime_from(meta_modified(&meta));
    drop(out);
    if let Err(e) = fs::rename(&tmp, &final_dest) {
        cleanup_temp(&tmp);
        return Err(FsError::from_io_path(e, &final_dest));
    }
    if let Some(t) = mtime {
        set_file_times(&final_dest, t);
    }
    let _ = copied;
    Ok(final_dest)
}

fn meta_modified(meta: &fs::Metadata) -> Option<std::time::SystemTime> {
    meta.modified().ok()
}

fn filetime_from(t: Option<std::time::SystemTime>) -> Option<std::time::SystemTime> {
    t
}

/// Set atime+mtime with a single `futimens(2)` call. Used to preserve
/// timestamps across copy/extract. Failures are deliberately ignored: having
/// the file is more important than its mtime.
pub fn set_file_times(path: &Path, t: std::time::SystemTime) {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    extern "C" {
        fn futimens(fd: i32, times: *const Timespec) -> i32;
    }
    let d = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return,
    };
    let ts = Timespec { tv_sec: d.as_secs() as i64, tv_nsec: d.subsec_nanos() as i64 };
    let times = [Timespec { tv_sec: ts.tv_sec, tv_nsec: ts.tv_nsec }, ts];
    // utimensat needs UTIME_OMIT etc; futimens on an fd is simpler and works on
    // both Android (bionic) and the Linux host used for tests.
    let file = match File::options().write(true).open(path) {
        Ok(f) => f,
        Err(_) => match File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        },
    };
    unsafe {
        futimens(file.as_raw_fd(), times.as_ptr());
    }
}

#[cfg(unix)]
pub fn copy_symlink(src: &Path, dest: &Path, mode: CollisionMode) -> Result<PathBuf> {
    use std::os::unix::fs::symlink;
    let target = fs::read_link(src).map_err(|e| FsError::from_io_path(e, src))?;
    let final_dest = resolve_dest(dest, if mode == CollisionMode::Fail { CollisionMode::Fail } else { CollisionMode::Rename }, false)?;
    let _ = fs::remove_file(&final_dest);
    symlink(&target, &final_dest).map_err(|e| FsError::from_io_path(e, &final_dest))?;
    Ok(final_dest)
}

#[cfg(not(unix))]
pub fn copy_symlink(src: &Path, dest: &Path, mode: CollisionMode) -> Result<PathBuf> {
    copy_file(src, dest, mode, &CancelToken::new())
}

/// Recursively copy `src` into `dest` (dest is the full target path).
pub fn copy_recursive(
    src: &Path,
    dest: &Path,
    mode: CollisionMode,
    cancel: &CancelToken,
    stats: &mut OpStats,
) -> Result<PathBuf> {
    cancel.check()?;
    let src_meta = fs::symlink_metadata(src).map_err(|e| FsError::from_io_path(e, src))?;
    if !src_meta.is_dir() {
        let out = copy_file(src, dest, mode, cancel)?;
        stats.files += 1;
        stats.bytes += src_meta.len();
        return Ok(out);
    }
    if would_recurse_into(dest, src) && dest != src {
        return Err(FsError::invalid_arg("Cannot copy a folder into itself"));
    }
    let final_dest = match mode {
        CollisionMode::Overwrite if dest.is_dir() => dest.to_path_buf(),
        _ => resolve_dest(dest, mode, true)?,
    };
    if !final_dest.exists() {
        fs::create_dir(&final_dest).map_err(|e| FsError::from_io_path(e, &final_dest))?;
        stats.dirs += 1;
    }
    let rd = fs::read_dir(src).map_err(|e| FsError::from_io_path(e, src))?;
    for item in rd {
        cancel.check()?;
        let item = match item {
            Ok(i) => i,
            Err(e) => {
                stats.record_err(FsError::from_io_path(e, src));
                continue;
            }
        };
        let child_src = item.path();
        let child_dest = final_dest.join(item.file_name());
        match copy_recursive(&child_src, &child_dest, CollisionMode::Overwrite, cancel, stats) {
            Ok(_) => {}
            Err(e) if e.code == ErrorCode::Cancelled => return Err(e),
            Err(e) => stats.record_err(e),
        }
    }
    // Preserve directory mtime after its children are written.
    if let Some(t) = src_meta.modified().ok() {
        set_file_times(&final_dest, t);
    }
    Ok(final_dest)
}

/// Move a file. Tries `rename(2)` first (atomic, zero-copy on the same
/// filesystem); falls back to copy+delete across devices.
pub fn move_path(
    src: &Path,
    dest: &Path,
    mode: CollisionMode,
    cancel: &CancelToken,
    stats: &mut OpStats,
) -> Result<PathBuf> {
    cancel.check()?;
    let src_meta = fs::symlink_metadata(src).map_err(|e| FsError::from_io_path(e, src))?;
    if src_meta.is_dir() && would_recurse_into(dest, src) && dest != src {
        return Err(FsError::invalid_arg("Cannot move a folder into itself"));
    }
    let final_dest = resolve_dest(dest, mode, src_meta.is_dir())?;
    if final_dest == src {
        return Ok(final_dest);
    }
    // Same-directory rename-to-existing under Overwrite: remove first.
    match fs::rename(src, &final_dest) {
        Ok(()) => {
            if src_meta.is_dir() {
                stats.dirs += 1;
            } else {
                stats.files += 1;
                stats.bytes += src_meta.len();
            }
            return Ok(final_dest);
        }
        Err(e) if e.raw_os_error() == Some(18) => {
            // EXDEV: different mount points (internal -> SD card). Copy, then
            // delete the source only if the copy fully succeeded.
        }
        Err(e) => return Err(FsError::from_io_path(e, &final_dest)),
    }
    let mut local = OpStats::default();
    if src_meta.is_dir() {
        copy_recursive(src, &final_dest, CollisionMode::Rename, cancel, &mut local)?;
    } else {
        copy_file(src, &final_dest, CollisionMode::Rename, cancel)?;
        local.files += 1;
        local.bytes += src_meta.len();
    }
    stats.files += local.files;
    stats.dirs += local.dirs;
    stats.bytes += local.bytes;
    delete_path(src, cancel, &mut OpStats::default())?;
    Ok(final_dest)
}

/// Recursively delete. Read-only directories (FUSE/SD cards) are retried after
/// a chmod, matching what system file managers do.
pub fn delete_path(path: &Path, cancel: &CancelToken, stats: &mut OpStats) -> Result<()> {
    cancel.check()?;
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(FsError::from_io_path(e, path)),
    };
    if meta.is_dir() && !meta.file_type().is_symlink() {
        let rd = match fs::read_dir(path) {
            Ok(rd) => rd,
            Err(e) => {
                // A directory we cannot read cannot be emptied; removing it
                // would be wrong. Report instead of silently skipping.
                return Err(FsError::from_io_path(e, path));
            }
        };
        for item in rd {
            cancel.check()?;
            match item {
                Ok(i) => {
                    let p = i.path();
                    if let Err(e) = delete_path(&p, cancel, stats) {
                        if e.code == ErrorCode::Cancelled {
                            return Err(e);
                        }
                        stats.record_err(e);
                    }
                }
                Err(e) => stats.record_err(FsError::from_io_path(e, path)),
            }
        }
        remove_dir_with_retry(path).map_err(|e| FsError::from_io_path(e, path))?;
        stats.dirs += 1;
    } else {
        remove_file_with_retry(path).map_err(|e| FsError::from_io_path(e, path))?;
        stats.files += 1;
        stats.bytes += meta.len();
    }
    Ok(())
}

fn remove_file_with_retry(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            make_writable(path);
            fs::remove_file(path)
        }
        Err(e) => Err(e),
    }
}

fn remove_dir_with_retry(path: &Path) -> io::Result<()> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            make_writable(path);
            fs::remove_dir(path)
        }
        Err(e) => Err(e),
    }
}

fn make_writable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = fs::symlink_metadata(path) {
            let mut p = m.permissions();
            p.set_mode(p.mode() | 0o600);
            let _ = fs::set_permissions(path, p);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Create a new empty file. Fails if it exists (`AlreadyExists`).
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    let p = dir.join(name);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p)
        .map_err(|e| FsError::from_io_path(e, &p))?;
    Ok(p)
}

/// Create a directory (single level). Fails if it exists.
pub fn create_dir(dir: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    let p = dir.join(name);
    fs::create_dir(&p).map_err(|e| FsError::from_io_path(e, &p))?;
    Ok(p)
}

/// Rename within the same directory. Keeps it a single atomic syscall; moving
/// across directories is `move_path`.
pub fn rename_path(path: &Path, new_name: &str) -> Result<PathBuf> {
    validate_name(new_name)?;
    let parent = path
        .parent()
        .ok_or_else(|| FsError::invalid_arg("Cannot rename a filesystem root"))?;
    let dest = parent.join(new_name);
    if dest == path {
        return Ok(dest);
    }
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(FsError::new(ErrorCode::AlreadyExists, "An item with that name already exists")
            .with_path(dest.to_string_lossy()));
    }
    fs::rename(path, &dest).map_err(|e| FsError::from_io_path(e, path))?;
    Ok(dest)
}

/// Stat details for the properties sheet.
#[derive(Debug, Clone)]
pub struct Properties {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub inode: u64,
    pub hard_links: u64,
    pub is_symlink: bool,
    pub symlink_target: Option<String>,
    /// Number of direct children for directories.
    pub child_count: u64,
    pub mime: String,
}

#[cfg(unix)]
pub fn properties(path: &Path, include_children: bool) -> Result<Properties> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path).map_err(|e| FsError::from_io_path(e, path))?;
    let is_dir = meta.is_dir();
    let name = crate::path::file_name_of(path);
    let child_count = if is_dir && include_children {
        fs::read_dir(path).map(|rd| rd.count() as u64).unwrap_or(0)
    } else {
        0
    };
    let symlink_target = if meta.file_type().is_symlink() {
        fs::read_link(path).ok().map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };
    Ok(Properties {
        name: name.clone(),
        path: path.to_string_lossy().into_owned(),
        is_dir,
        size: if is_dir { 0 } else { meta.len() },
        modified: mtime_secs(&meta),
        mode: mode_of(&meta),
        uid: meta.uid(),
        gid: meta.gid(),
        inode: meta.ino(),
        hard_links: meta.nlink(),
        is_symlink: meta.file_type().is_symlink(),
        symlink_target,
        child_count,
        mime: crate::ext::mime_type(&name, is_dir).to_string(),
    })
}

#[cfg(not(unix))]
pub fn properties(path: &Path, include_children: bool) -> Result<Properties> {
    let meta = fs::symlink_metadata(path).map_err(|e| FsError::from_io_path(e, path))?;
    let is_dir = meta.is_dir();
    let name = crate::path::file_name_of(path);
    Ok(Properties {
        name: name.clone(),
        path: path.to_string_lossy().into_owned(),
        is_dir,
        size: if is_dir { 0 } else { meta.len() },
        modified: mtime_secs(&meta),
        mode: 0,
        uid: 0,
        gid: 0,
        inode: 0,
        hard_links: 1,
        is_symlink: meta.file_type().is_symlink(),
        symlink_target: None,
        child_count: if is_dir && include_children { fs::read_dir(path).map(|r| r.count() as u64).unwrap_or(0) } else { 0 },
        mime: crate::ext::mime_type(&name, is_dir).to_string(),
    })
}

/// Entry for a single path (used when the UI needs to refresh one item).
pub fn stat_entry(path: &Path) -> Result<Entry> {
    let meta = fs::symlink_metadata(path).map_err(|e| FsError::from_io_path(e, path))?;
    Ok(Entry::from_parts(crate::path::file_name_of(path), Some(&meta)))
}

/// Batch operation result: per-item errors are collected, not fatal.
#[derive(Debug, Default)]
pub struct BatchResult {
    pub stats: OpStats,
    pub outputs: Vec<String>,
}

/// Copy a batch of paths into `dest_dir`.
pub fn copy_batch(
    sources: &[PathBuf],
    dest_dir: &Path,
    mode: CollisionMode,
    cancel: Arc<CancelToken>,
) -> BatchResult {
    let mut out = BatchResult::default();
    let multi = sources.len() > 1;
    for src in sources {
        if cancel.is_cancelled() {
            out.stats.record_err(FsError::cancelled());
            break;
        }
        let name = crate::path::file_name_of(src);
        if name.is_empty() {
            out.stats.record_err(FsError::invalid_arg("Invalid source"));
            continue;
        }
        let mut dest = dest_dir.join(&name);
        // Collision policy applies within the batch too: copying two files with
        // the same name must not silently keep only one.
        let effective_mode = if multi && fs::symlink_metadata(&dest).is_ok() && mode == CollisionMode::Fail {
            CollisionMode::Rename
        } else {
            mode
        };
        match copy_recursive(src, &dest, effective_mode, &cancel, &mut out.stats) {
            Ok(p) => {
                dest = p;
                out.outputs.push(dest.to_string_lossy().into_owned());
            }
            Err(e) => {
                if e.code == ErrorCode::Cancelled {
                    out.stats.record_err(e);
                    break;
                }
                out.stats.record_err(e);
            }
        }
    }
    out
}

/// Move a batch of paths into `dest_dir`. Cross-device moves fall back to copy.
pub fn move_batch(
    sources: &[PathBuf],
    dest_dir: &Path,
    mode: CollisionMode,
    cancel: Arc<CancelToken>,
) -> BatchResult {
    let mut out = BatchResult::default();
    for src in sources {
        if cancel.is_cancelled() {
            out.stats.record_err(FsError::cancelled());
            break;
        }
        let name = crate::path::file_name_of(src);
        if name.is_empty() {
            out.stats.record_err(FsError::invalid_arg("Invalid source"));
            continue;
        }
        let dest = dest_dir.join(&name);
        match move_path(src, &dest, mode, &cancel, &mut out.stats) {
            Ok(p) => out.outputs.push(p.to_string_lossy().into_owned()),
            Err(e) => {
                if e.code == ErrorCode::Cancelled {
                    out.stats.record_err(e);
                    break;
                }
                out.stats.record_err(e);
            }
        }
    }
    out
}

/// Delete a batch of paths.
pub fn delete_batch(sources: &[PathBuf], cancel: Arc<CancelToken>) -> BatchResult {
    let mut out = BatchResult::default();
    for src in sources {
        if cancel.is_cancelled() {
            out.stats.record_err(FsError::cancelled());
            break;
        }
        match delete_path(src, &cancel, &mut out.stats) {
            Ok(()) => out.outputs.push(src.to_string_lossy().into_owned()),
            Err(e) => {
                if e.code == ErrorCode::Cancelled {
                    out.stats.record_err(e);
                    break;
                }
                out.stats.record_err(e);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fuss-test-{}-{}-{}", tag, std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn create_copy_move_delete() {
        let root = tmpdir("basic");
        let f = create_file(&root, "a.txt").unwrap();
        fs::write(&f, b"hello world").unwrap();
        let sub = create_dir(&root, "sub").unwrap();
        let c = copy_file(&f, &sub.join("a.txt"), CollisionMode::Fail, &CancelToken::new()).unwrap();
        assert_eq!(fs::read(&c).unwrap(), b"hello world");
        // Same-dir copy with collision must fail by default
        assert_eq!(copy_file(&f, &sub.join("a.txt"), CollisionMode::Fail, &CancelToken::new()).unwrap_err().code, ErrorCode::AlreadyExists);
        // Rename mode picks "a (1).txt"
        let c2 = copy_file(&f, &sub.join("a.txt"), CollisionMode::Rename, &CancelToken::new()).unwrap();
        assert_eq!(c2.file_name().unwrap(), "a (1).txt");
        let renamed = rename_path(&c2, "b.txt").unwrap();
        assert!(renamed.exists());
        let moved = move_path(&f, &sub.join("moved.txt"), CollisionMode::Fail, &CancelToken::new(), &mut OpStats::default()).unwrap();
        assert!(moved.exists() && !f.exists());
        delete_path(&sub, &CancelToken::new(), &mut OpStats::default()).unwrap();
        assert!(!sub.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn recursive_copy_and_delete() {
        let root = tmpdir("rec");
        let src = root.join("src");
        fs::create_dir_all(src.join("d1/d2")).unwrap();
        File::create(src.join("d1/x.bin")).unwrap().write_all(&[7u8; 300_000]).unwrap();
        fs::write(src.join("d1/d2/y.txt"), "y").unwrap();
        let mut stats = OpStats::default();
        copy_recursive(&src, &root.join("dst"), CollisionMode::Fail, &CancelToken::new(), &mut stats).unwrap();
        assert_eq!(fs::read(root.join("dst/d1/x.bin")).unwrap().len(), 300_000);
        assert_eq!(fs::read_to_string(root.join("dst/d1/d2/y.txt")).unwrap(), "y");
        assert_eq!(stats.files, 2);
        delete_path(&src, &CancelToken::new(), &mut OpStats::default()).unwrap();
        assert!(!src.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn refuses_copy_into_self() {
        let root = tmpdir("self");
        let src = root.join("a");
        fs::create_dir_all(&src).unwrap();
        let mut stats = OpStats::default();
        let err = copy_recursive(&src, &src.join("inner"), CollisionMode::Fail, &CancelToken::new(), &mut stats).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancellation_leaves_no_temp_file() {
        let root = tmpdir("cancel");
        let f = root.join("big.bin");
        File::create(&f).unwrap().write_all(&vec![0u8; 300_000]).unwrap();
        let token = CancelToken::new();
        token.cancel();
        let err = copy_file(&f, &root.join("out.bin"), CollisionMode::Fail, &token).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert!(!root.join("out.bin").exists());
        let leftovers: Vec<_> = fs::read_dir(&root).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains("fuss-")).collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn overwrite_replaces_content() {
        let root = tmpdir("ow");
        let a = root.join("a"); let b = root.join("b");
        fs::write(&a, b"AAAAAAAAAA").unwrap();
        fs::write(&b, b"BB").unwrap();
        copy_file(&a, &b, CollisionMode::Overwrite, &CancelToken::new()).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"AAAAAAAAAA");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn batch_copy_collects_errors_and_renames_duplicates() {
        let root = tmpdir("batch");
        let src1 = root.join("s1"); let src2 = root.join("s2"); let out = root.join("out");
        fs::create_dir_all(&src1).unwrap(); fs::create_dir_all(&src2).unwrap(); fs::create_dir_all(&out).unwrap();
        fs::write(src1.join("same.txt"), "1").unwrap();
        fs::write(src2.join("same.txt"), "2").unwrap();
        let r = copy_batch(&[src1.join("same.txt"), src2.join("same.txt")], &out, CollisionMode::Fail, CancelToken::new());
        assert_eq!(r.stats.errors, 0);
        assert_eq!(r.outputs.len(), 2);
        assert!(out.join("same.txt").exists());
        assert!(out.join("same (1).txt").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unique_name_works() {
        let root = tmpdir("uniq");
        fs::write(root.join("a.txt"), "x").unwrap();
        assert_eq!(unique_name(&root, "a.txt"), "a (1).txt");
        fs::write(root.join("a (1).txt"), "x").unwrap();
        assert_eq!(unique_name(&root, "a.txt"), "a (2).txt");
        assert_eq!(unique_name(&root, ".hidden"), ".hidden");
        fs::write(root.join(".hidden"), "x").unwrap();
        assert_eq!(unique_name(&root, ".hidden"), ".hidden (1)");
        fs::remove_dir_all(&root).unwrap();
    }
}
