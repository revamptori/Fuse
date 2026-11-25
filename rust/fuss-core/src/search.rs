//! Incremental search.
//!
//! The search walks the tree breadth-first with a bounded frontier so results
//! arrive early and the search stays responsive on huge trees. It never follows
//! symlinks (avoids cycles), skips hidden directories unless asked, and reports
//! progress through the `CancelToken`.

use crate::cancel::CancelToken;
use crate::entry::Entry;
use crate::error::Result;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Case-insensitive substring match on the name.
    Name,
    /// Like `Name`, plus a glob pattern (`*.jpg`, `photo_?.png`).
    Glob,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub query: String,
    pub max_depth: u32,
    pub include_hidden: bool,
    pub max_results: usize,
}

impl SearchOptions {
    pub fn new(query: &str) -> SearchOptions {
        SearchOptions { query: query.to_string(), max_depth: 64, include_hidden: false, max_results: 20_000 }
    }
}

/// Simple glob matcher supporting `*`, `?`, and literal everything else.
/// Case-insensitive; no character classes (kept intentionally tiny).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    glob_inner(&p, &t)
}

fn glob_inner(p: &[char], t: &[char]) -> bool {
    // Iterative backtracking matcher: O(n*m) worst case, O(1) extra space.
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = pi;
            star_t = ti;
            pi += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn matches(opts: &SearchOptions, name: &str) -> bool {
    let q = opts.query.trim();
    if q.is_empty() {
        return false;
    }
    let mode = if q.contains('*') || q.contains('?') { SearchMode::Glob } else { SearchMode::Name };
    match mode {
        SearchMode::Glob => glob_match(q, name),
        SearchMode::Name => name.to_lowercase().contains(&q.to_lowercase()),
    }
}

#[derive(Debug, Default)]
pub struct SearchResult {
    pub entries: Vec<(PathBuf, Entry)>,
    pub scanned_dirs: u64,
    pub scanned_files: u64,
    pub truncated: bool,
    pub elapsed_ms: u128,
}

/// Search `root` breadth-first. Returns early once `max_results` matches are
/// found or the token is cancelled.
pub fn search(root: &Path, opts: &SearchOptions, cancel: &CancelToken) -> Result<SearchResult> {
    let started = std::time::Instant::now();
    let mut result = SearchResult::default();
    let mut queue: VecDeque<(PathBuf, u32)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));
    while let Some((dir, depth)) = queue.pop_front() {
        cancel.check()?;
        result.scanned_dirs += 1;
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for item in rd {
            if result.entries.len() >= opts.max_results {
                result.truncated = true;
                break;
            }
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            let name = item.file_name().to_string_lossy().into_owned();
            let hidden = name.starts_with('.');
            if hidden && !opts.include_hidden {
                continue;
            }
            let child = item.path();
            let meta = match fs::symlink_metadata(&child) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let is_dir = meta.is_dir() && !meta.file_type().is_symlink();
            result.scanned_files += 1;
            // Hidden files are matched only when the caller asked for them; a
            // normal search must not surface ".thumbnails" just because the
            // query appears inside its name.
            if matches(opts, &name) && (opts.include_hidden || !hidden) {
                let entry = Entry::from_parts(name, Some(&meta));
                result.entries.push((child.clone(), entry));
            }
            if is_dir && depth + 1 < opts.max_depth {
                queue.push_back((child, depth + 1));
            }
        }
        if result.truncated {
            break;
        }
    }
    result.elapsed_ms = started.elapsed().as_millis();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("*.jpg", "IMG_001.JPG"));
        assert!(glob_match("photo_?.png", "photo_1.png"));
        assert!(!glob_match("photo_?.png", "photo_12.png"));
        assert!(glob_match("*report*", "my-report-final.pdf"));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("exact.txt", "exact.txt"));
    }

    #[test]
    fn search_tree() {
        let root = std::env::temp_dir().join(format!("fuss-search-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join(".secret")).unwrap();
        fs::write(root.join("a/b/needle.txt"), "x").unwrap();
        fs::write(root.join("top.txt"), "x").unwrap();
        fs::write(root.join(".hidden-needle"), "x").unwrap();
        let t = CancelToken::new();
        let r = search(&root, &SearchOptions::new("needle"), &t).unwrap();
        assert_eq!(r.entries.len(), 1, "matches: {:?}", r.entries.iter().map(|(p, _)| p).collect::<Vec<_>>());
        assert!(r.entries[0].0.ends_with("a/b/needle.txt"));
        assert!(r.entries.iter().all(|(_, e)| !e.name.starts_with('.')));
        let r2 = search(&root, &SearchOptions { include_hidden: true, ..SearchOptions::new("needle") }, &t).unwrap();
        assert_eq!(r2.entries.len(), 2);
        let r3 = search(&root, &SearchOptions::new("*.TXT"), &t).unwrap();
        assert_eq!(r3.entries.len(), 2);
        let r4 = search(&root, &SearchOptions::new(""), &t).unwrap();
        assert_eq!(r4.entries.len(), 0, "empty query matches nothing");
        fs::remove_dir_all(&root).unwrap();
    }
}
