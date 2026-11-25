//! Favorites, bookmarks and recents.
//!
//! Persisted as one small text file (one record per line, tab separated) in the
//! app's private storage. No database dependency, trivially recoverable if
//! corrupt, and small enough to rewrite atomically on every change.

use crate::error::{FsError, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Default)]
pub struct Prefs {
    pub favorites: Vec<String>,
    pub recent: Vec<String>,
    pub bookmarks: Vec<Bookmark>,
}

const MAX_RECENT: usize = 30;
const MAX_FAVORITES: usize = 100;

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Prefs {
    /// Load from disk. A missing file is an empty prefs set, not an error.
    pub fn load(path: &Path) -> Result<Prefs> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Prefs::default()),
            Err(e) => return Err(FsError::from_io_path(e, path)),
        };
        let mut p = Prefs::default();
        for line in text.lines() {
            let mut parts = line.split('\t');
            match parts.next() {
                Some("f") => {
                    if let Some(v) = parts.next() {
                        p.favorites.push(unescape(v));
                    }
                }
                Some("r") => {
                    if let Some(v) = parts.next() {
                        p.recent.push(unescape(v));
                    }
                }
                Some("b") => {
                    let label = parts.next().map(unescape).unwrap_or_default();
                    let path = parts.next().map(unescape).unwrap_or_default();
                    if !path.is_empty() {
                        p.bookmarks.push(Bookmark { label, path });
                    }
                }
                _ => {}
            }
        }
        Ok(p)
    }

    /// Serializable form (also used by tests).
    pub fn serialize(&self) -> String {
        let mut s = String::with_capacity(256 + self.favorites.len() * 64);
        for f in &self.favorites {
            s.push_str("f\t");
            s.push_str(&escape(f));
            s.push('\n');
        }
        for r in &self.recent {
            s.push_str("r\t");
            s.push_str(&escape(r));
            s.push('\n');
        }
        for b in &self.bookmarks {
            s.push_str("b\t");
            s.push_str(&escape(&b.label));
            s.push('\t');
            s.push_str(&escape(&b.path));
            s.push('\n');
        }
        s
    }

    /// Atomically replace the file on disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        fs::write(&tmp, self.serialize()).map_err(|e| FsError::from_io_path(e, &tmp))?;
        fs::rename(&tmp, path).map_err(|e| FsError::from_io_path(e, path))?;
        Ok(())
    }

    pub fn add_favorite(&mut self, path: &str) {
        if self.favorites.iter().any(|p| p == path) {
            return;
        }
        self.favorites.push(path.to_string());
        if self.favorites.len() > MAX_FAVORITES {
            self.favorites.remove(0);
        }
    }

    pub fn remove_favorite(&mut self, path: &str) {
        self.favorites.retain(|p| p != path);
    }

    pub fn is_favorite(&self, path: &str) -> bool {
        self.favorites.iter().any(|p| p == path)
    }

    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn clear_recent(&mut self) {
        self.recent.clear();
    }

    pub fn add_bookmark(&mut self, label: &str, path: &str) {
        self.bookmarks.retain(|b| b.path != path);
        self.bookmarks.push(Bookmark { label: label.to_string(), path: path.to_string() });
    }

    pub fn remove_bookmark(&mut self, path: &str) {
        self.bookmarks.retain(|b| b.path != path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_special_chars() {
        let mut p = Prefs::default();
        p.add_favorite("/storage/emulated/0/DCIM");
        p.add_favorite("/storage/emulated/0/My\\dir\twith\ttabs");
        p.add_recent("/storage/emulated/0/Download/a\tb");
        p.add_bookmark("Work\nStuff", "/storage/emulated/0/work");
        let text = p.serialize();
        let dir = std::env::temp_dir().join(format!("fuss-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("prefs.txt");
        p.save(&f).unwrap();
        let loaded = Prefs::load(&f).unwrap();
        assert_eq!(loaded.favorites, p.favorites);
        assert_eq!(loaded.recent, p.recent);
        assert_eq!(loaded.bookmarks, p.bookmarks);
        assert!(text.contains("\\\\dir"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn favorites_are_unique_and_capped() {
        let mut p = Prefs::default();
        p.add_favorite("/a");
        p.add_favorite("/a");
        assert_eq!(p.favorites.len(), 1);
        for i in 0..200 {
            p.add_favorite(&format!("/f{i}"));
        }
        assert_eq!(p.favorites.len(), MAX_FAVORITES);
        assert!(p.is_favorite(&format!("/f{}", 199)));
    }

    #[test]
    fn recents_move_to_front() {
        let mut p = Prefs::default();
        p.add_recent("/a");
        p.add_recent("/b");
        p.add_recent("/a");
        assert_eq!(p.recent, vec!["/a", "/b"]);
        for i in 0..40 {
            p.add_recent(&format!("/r{i}"));
        }
        assert_eq!(p.recent.len(), MAX_RECENT);
        assert_eq!(p.recent[0], "/r39");
    }

    #[test]
    fn missing_file_is_empty() {
        let p = Prefs::load(Path::new("/nonexistent/fuss/prefs.txt")).unwrap();
        assert!(p.favorites.is_empty());
    }
}
