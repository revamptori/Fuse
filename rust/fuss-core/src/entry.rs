//! Directory entry representation and the sort/filter model.
//!
//! Entries are produced in chunks by the lister so the Kotlin side can render
//! incrementally instead of waiting for a whole directory. A chunk is encoded
//! into a single byte buffer (see `codec`), which keeps JNI traffic to one call
//! per ~200 entries rather than one per entry.

use crate::ext::{classify_name, mime_type, FileKind};
use std::fs::Metadata;
use std::path::Path;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Type,
    Modified,
}

impl SortKey {
    pub fn from_u8(v: u8) -> SortKey {
        match v {
            1 => SortKey::Size,
            2 => SortKey::Type,
            3 => SortKey::Modified,
            _ => SortKey::Name,
        }
    }
}

/// Natural, case-insensitive name comparison: `file2` sorts before `file10`,
/// leading dots and digits are handled predictably.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ai = a.char_indices().peekable();
    let mut bi = b.char_indices().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.len().cmp(&b.len()),
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some((ia, ca)), Some((ib, cb))) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    // Compare whole digit runs numerically, without allocation.
                    let na = digit_run(&a[ia..]);
                    let nb = digit_run(&b[ib..]);
                    let la = na.len();
                    let lb = nb.len();
                    let sa = na.trim_start_matches('0');
                    let sb = nb.trim_start_matches('0');
                    let ord = sa.len().cmp(&sb.len()).then_with(|| sa.cmp(sb));
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                    if la != lb {
                        // Same numeric value, fewer leading zeros first.
                        return la.cmp(&lb);
                    }
                    for _ in 0..la {
                        ai.next();
                        bi.next();
                    }
                } else {
                    let la = ca.to_lowercase().next().unwrap_or(ca);
                    let lb = cb.to_lowercase().next().unwrap_or(cb);
                    if la != lb {
                        return la.cmp(&lb);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn digit_run(s: &str) -> &str {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    &s[..end]
}

/// One entry in a directory listing. `name` is the file name only; the full
/// path is reconstructed by the UI from the current directory, which keeps each
/// record small (important for directories with tens of thousands of files).
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: i64,
    pub mode: u32,
    pub kind: FileKind,
    /// Empty when the engine could not stat the child (e.g. dangling link).
    pub stat_ok: bool,
}

impl Entry {
    pub fn dir_placeholder(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: 0,
            mode: 0o755,
            kind: FileKind::Directory,
            stat_ok: false,
        }
    }

    pub fn from_parts(name: String, meta: Option<&Metadata>) -> Entry {
        match meta {
            Some(m) => {
                let is_dir = m.is_dir();
                let kind = classify_name(&name, is_dir);
                // Directory sizes are expensive to compute; report 0 and let the
                // properties screen / size analysis compute on demand.
                let size = if is_dir { 0 } else { m.len() };
                Entry {
                    name,
                    is_dir,
                    is_symlink: m.file_type().is_symlink(),
                    size,
                    modified: mtime_secs(m),
                    mode: mode_of(m),
                    kind,
                    stat_ok: true,
                }
            }
            None => Entry {
                name: name.clone(),
                is_dir: false,
                is_symlink: false,
                size: 0,
                modified: 0,
                mode: 0,
                kind: classify_name(&name, false),
                stat_ok: false,
            },
        }
    }

    pub fn full_path(&self, parent: &Path) -> std::path::PathBuf {
        parent.join(&self.name)
    }

    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }

    pub fn mime(&self) -> &'static str {
        mime_type(&self.name, self.is_dir)
    }
}

#[cfg(unix)]
pub fn mtime_secs(m: &Metadata) -> i64 {
    m.mtime()
}

#[cfg(not(unix))]
pub fn mtime_secs(m: &Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(unix)]
pub fn mode_of(m: &Metadata) -> u32 {
    m.mode()
}

#[cfg(not(unix))]
pub fn mode_of(_m: &Metadata) -> u32 {
    0
}

/// Sort a slice of entries. Directories always group before files, matching
/// what every Android file manager does and what users expect.
pub fn sort_entries(entries: &mut [Entry], key: SortKey, ascending: bool) {
    use std::cmp::Ordering;
    entries.sort_by(|a, b| {
        let dir_ord = b.is_dir.cmp(&a.is_dir);
        if dir_ord != Ordering::Equal {
            return dir_ord;
        }
        let ord = match key {
            SortKey::Name => natural_cmp(&a.name, &b.name),
            SortKey::Size => a.size.cmp(&b.size).then_with(|| natural_cmp(&a.name, &b.name)),
            SortKey::Type => (a.kind.as_u8(), &a.name)
                .cmp(&(b.kind.as_u8(), &b.name)),
            SortKey::Modified => a.modified.cmp(&b.modified).then_with(|| natural_cmp(&a.name, &b.name)),
        };
        if ascending {
            ord
        } else {
            ord.reverse()
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10.txt", "file2.txt", "File1.txt", "file001.txt", "apple"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["apple", "File1.txt", "file001.txt", "file2.txt", "file10.txt"]);
    }

    #[test]
    fn natural_case_insensitive_but_stable() {
        assert_eq!(natural_cmp("ABC", "abc"), std::cmp::Ordering::Equal);
        assert_eq!(natural_cmp("a2", "a10"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("img_9", "img_10"), std::cmp::Ordering::Less);
    }

    fn e(name: &str, is_dir: bool) -> Entry {
        Entry { name: name.into(), is_dir, is_symlink: false, size: 0, modified: 0, mode: 0, kind: if is_dir { FileKind::Directory } else { FileKind::Other }, stat_ok: true }
    }

    #[test]
    fn dirs_first_and_descending() {
        let mut v = vec![e("b.txt", false), e("zdir", true), e("a.txt", false), e("adir", true)];
        sort_entries(&mut v, SortKey::Name, true);
        assert_eq!(v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["adir", "zdir", "a.txt", "b.txt"]);
        sort_entries(&mut v, SortKey::Name, false);
        assert_eq!(v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["zdir", "adir", "b.txt", "a.txt"]);
    }
}
