//! Storage analysis: directory sizes, large files, empty folders, type stats,
//! and duplicate detection.
//!
//! All of it walks trees iteratively with an explicit stack (no recursion, so a
//! pathological directory depth cannot blow the native stack) and coordinates
//! through `CancelToken` so the analysis screen can stop instantly.

use crate::cancel::CancelToken;
use crate::error::Result;
use crate::ext::FileKind;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct DirSize {
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
}

/// Recursively total a directory. Symlinks are not followed; unreadable
/// entries are counted and skipped.
pub fn dir_size(root: &Path, cancel: &CancelToken) -> Result<DirSize> {
    let mut out = DirSize { path: root.to_string_lossy().into_owned(), ..Default::default() };
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        cancel.check()?;
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            let p = item.path();
            let meta = match fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() && !meta.file_type().is_symlink() {
                out.dirs += 1;
                stack.push(p);
            } else {
                out.files += 1;
                out.bytes += meta.len();
                cancel.add_processed(meta.len());
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct FileHit {
    pub path: String,
    pub size: u64,
    pub modified: i64,
}

/// Find the N largest files under `root`.
pub fn largest_files(root: &Path, limit: usize, cancel: &CancelToken) -> Result<Vec<FileHit>> {
    // Bounded min-heap of the best `limit` candidates.
    let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(u64, String, i64)>> =
        std::collections::BinaryHeap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        cancel.check()?;
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            let p = item.path();
            let meta = match fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() && !meta.file_type().is_symlink() {
                stack.push(p);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let size = meta.len();
            let modified = crate::entry::mtime_secs(&meta);
            let rec = (size, p.to_string_lossy().into_owned(), modified);
            if heap.len() < limit {
                heap.push(std::cmp::Reverse(rec));
            } else if let Some(std::cmp::Reverse(min)) = heap.peek() {
                if size > min.0 {
                    heap.pop();
                    heap.push(std::cmp::Reverse(rec));
                }
            }
        }
    }
    let mut v: Vec<FileHit> = heap
        .into_iter()
        .map(|std::cmp::Reverse((size, path, modified))| FileHit { path, size, modified })
        .collect();
    v.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(v)
}

/// Find empty directories (no files, no subdirectories) under `root`.
pub fn empty_dirs(root: &Path, limit: usize, cancel: &CancelToken) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        cancel.check()?;
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        let mut count = 0usize;
        let mut subdirs = Vec::new();
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            count += 1;
            let p = item.path();
            if let Ok(m) = fs::symlink_metadata(&p) {
                if m.is_dir() && !m.file_type().is_symlink() {
                    subdirs.push(p);
                }
            }
        }
        if count == 0 && Some(dir.as_path()) != Some(root) {
            out.push(dir.to_string_lossy().into_owned());
            if out.len() >= limit {
                break;
            }
            continue;
        }
        stack.extend(subdirs);
    }
    Ok(out)
}

#[derive(Debug, Default, Clone)]
pub struct TypeStat {
    pub kind: FileKind,
    pub count: u64,
    pub bytes: u64,
}

/// File-type statistics for a tree: count and bytes per `FileKind`.
pub fn type_stats(root: &Path, cancel: &CancelToken) -> Result<Vec<TypeStat>> {
    let mut by_kind: HashMap<u8, (FileKind, u64, u64)> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        cancel.check()?;
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            let p = item.path();
            let meta = match fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() && !meta.file_type().is_symlink() {
                stack.push(p);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let name = item.file_name().to_string_lossy().into_owned();
            let kind = crate::ext::classify_name(&name, false);
            let e = by_kind.entry(kind.as_u8()).or_insert((kind, 0, 0));
            e.1 += 1;
            e.2 += meta.len();
        }
    }
    let mut out: Vec<TypeStat> = by_kind
        .into_values()
        .map(|(kind, count, bytes)| TypeStat { kind, count, bytes })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub size: u64,
    pub hash: u64,
    pub paths: Vec<String>,
    pub wasted: u64,
}

/// Two-stage duplicate detection.
///
/// Stage 1 groups by size (cheap). Stage 2 hashes the first and last 64 KiB of
/// same-size candidates with a fast non-cryptographic hash, only reading the
/// whole file when the head/tail digest matches. Boundaries are deliberately
/// coarse: the UI labels results "likely duplicates" and lets the user decide,
/// and we never touch files above `max_file_bytes` or below `min_file_bytes`.
pub fn find_duplicates(
    root: &Path,
    min_file_bytes: u64,
    max_file_bytes: u64,
    cancel: &CancelToken,
) -> Result<Vec<DuplicateGroup>> {
    const SAMPLE: usize = 64 * 1024;
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        cancel.check()?;
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            let p = item.path();
            let meta = match fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() && !meta.file_type().is_symlink() {
                stack.push(p);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let size = meta.len();
            if size < min_file_bytes || size > max_file_bytes {
                continue;
            }
            by_size.entry(size).or_default().push(p);
        }
    }
    let mut groups: Vec<DuplicateGroup> = Vec::new();
    for (size, paths) in by_size {
        if paths.len() < 2 {
            continue;
        }
        cancel.check()?;
        // Stage 2: sample hash.
        let mut by_sample: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        for p in paths {
            match sample_hash(&p, size as usize, SAMPLE) {
                Ok(h) => by_sample.entry(h).or_default().push(p),
                Err(_) => continue,
            }
        }
        for (_h, candidates) in by_sample {
            if candidates.len() < 2 {
                continue;
            }
            // Stage 3: full hash only for candidates whose samples match.
            let mut full: HashMap<u64, Vec<String>> = HashMap::new();
            for p in &candidates {
                cancel.check()?;
                match full_hash(p, cancel) {
                    Ok(h) => full.entry(h).or_default().push(p.to_string_lossy().into_owned()),
                    Err(_) => continue,
                }
            }
            for (h, mut paths) in full {
                if paths.len() < 2 {
                    continue;
                }
                paths.sort();
                groups.push(DuplicateGroup {
                    size,
                    hash: h,
                    wasted: size * (paths.len() as u64 - 1),
                    paths,
                });
            }
        }
    }
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    Ok(groups)
}

fn sample_hash(path: &Path, size: usize, sample: usize) -> io::Result<u64> {
    use std::io::{Seek, SeekFrom};
    let mut f = fs::File::open(path)?;
    let mut hasher = DefaultHasher::new();
    size.hash(&mut hasher);
    let mut buf = vec![0u8; sample.min(size)];
    let n = f.read(&mut buf)?;
    buf[..n].hash(&mut hasher);
    if size > sample * 2 {
        f.seek(SeekFrom::End(-(sample as i64)))?;
        let n = f.read(&mut buf)?;
        buf[..n].hash(&mut hasher);
    }
    Ok(hasher.finish())
}

fn full_hash(path: &Path, cancel: &CancelToken) -> io::Result<u64> {
    let mut f = fs::File::open(path)?;
    let mut hasher = DefaultHasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        buf[..n].hash(&mut hasher);
    }
    Ok(hasher.finish())
}

/// Top-level directory sizes for the storage overview (one level below root).
pub fn child_dir_sizes(root: &Path, cancel: &CancelToken) -> Result<Vec<DirSize>> {
    let mut out = Vec::new();
    let rd = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) => return Err(crate::error::FsError::from_io_path(e, root)),
    };
    for item in rd {
        cancel.check()?;
        let item = match item {
            Ok(i) => i,
            Err(_) => continue,
        };
        let p = item.path();
        if p.is_dir() {
            out.push(dir_size(&p, cancel)?);
        }
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fuss-an-{}-{}-{}", tag, std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn sizes_and_largest() {
        let d = tmpdir("size");
        fs::create_dir_all(d.join("x/y")).unwrap();
        fs::write(d.join("small.txt"), vec![0u8; 10]).unwrap();
        fs::write(d.join("x/mid.bin"), vec![0u8; 5000]).unwrap();
        fs::write(d.join("x/y/big.bin"), vec![0u8; 1_000_000]).unwrap();
        let t = CancelToken::new();
        let s = dir_size(&d, &t).unwrap();
        assert_eq!(s.files, 3);
        assert_eq!(s.dirs, 2);
        assert_eq!(s.bytes, 1_005_010);
        let top = largest_files(&d, 2, &t).unwrap();
        assert_eq!(top.len(), 2);
        assert!(top[0].path.ends_with("big.bin"));
        assert!(top[1].path.ends_with("mid.bin"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn empties_and_types() {
        let d = tmpdir("empty");
        fs::create_dir_all(d.join("empty1")).unwrap();
        fs::create_dir_all(d.join("notempty")).unwrap();
        fs::create_dir(d.join("notempty/inner-empty")).unwrap();
        fs::write(d.join("notempty/a.jpg"), vec![0u8; 100]).unwrap();
        fs::write(d.join("b.mp3"), vec![0u8; 50]).unwrap();
        let t = CancelToken::new();
        let e = empty_dirs(&d, 100, &t).unwrap();
        assert_eq!(e.len(), 2, "{e:?}");
        let stats = type_stats(&d, &t).unwrap();
        let img = stats.iter().find(|s| s.kind == FileKind::Image).unwrap();
        assert_eq!(img.count, 1);
        let aud = stats.iter().find(|s| s.kind == FileKind::Audio).unwrap();
        assert_eq!(aud.bytes, 50);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn duplicates() {
        let d = tmpdir("dup");
        fs::create_dir_all(d.join("sub")).unwrap();
        let content = b"the same bytes".repeat(1000);
        fs::write(d.join("one.bin"), &content).unwrap();
        fs::write(d.join("sub/two.bin"), &content).unwrap();
        fs::write(d.join("three.bin"), b"different".repeat(1000)).unwrap();
        // Same size, different content: must NOT be grouped.
        fs::write(d.join("four.bin"), vec![9u8; content.len()]).unwrap();
        let t = CancelToken::new();
        let g = find_duplicates(&d, 1, u64::MAX, &t).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].paths.len(), 2);
        assert_eq!(g[0].wasted, content.len() as u64);
        fs::remove_dir_all(&d).unwrap();
    }
}
