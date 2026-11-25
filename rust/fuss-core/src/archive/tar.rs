//! TAR reader and writer (ustar + GNU/PAX long names), plus GZIP via miniz_oxide.
//!
//! TAR has no central directory, so listing scans the headers. That is still
//! cheap: each entry costs one 512-byte header read and a seek over the data.
//! For `.tar.gz` the whole stream must be inflated to enumerate, which is
//! inherently sequential; we stream it and check cancellation between entries.

use crate::archive::inflate_stream::GzipDecoder;
use crate::cancel::CancelToken;
use crate::error::{ErrorCode, FsError, Result};
use crate::path::safe_archive_relative;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const BLOCK: usize = 512;
const MAX_NAME: usize = 100;
const MAX_META_FILE: u64 = 1 << 20; // PAX/GNU long name/attr files

#[derive(Debug, Clone)]
pub struct TarEntry {
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: i64,
    pub mode: u32,
    pub link_target: Option<String>,
    /// Byte offset of the entry's data inside the (uncompressed) stream.
    pub data_offset: u64,
}

#[derive(Debug, Default, Clone)]
pub struct TarListing {
    pub entries: Vec<TarEntry>,
    pub total_bytes: u64,
    /// True if any header looked like gzip; caller should decompress first.
    pub seems_gzip: bool,
    pub truncated: bool,
}

fn tar_err(msg: impl Into<String>) -> FsError {
    FsError::corrupt(msg)
}

/// Parse an octal (or GNU base-256) numeric field.
fn parse_num(field: &[u8]) -> u64 {
    if field.is_empty() {
        return 0;
    }
    if field[0] & 0x80 != 0 {
        // GNU base-256 encoding, big-endian, two's complement positive.
        let mut v: u64 = 0;
        for &b in &field[1..] {
            v = (v << 8) | b as u64;
        }
        return v;
    }
    let mut v: u64 = 0;
    let mut seen = false;
    for &b in field {
        match b {
            b'0'..=b'7' => {
                v = v * 8 + (b - b'0') as u64;
                seen = true;
            }
            b' ' | 0 => {
                if seen {
                    break;
                }
            }
            _ => break,
        }
    }
    v
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn checksum_ok(hdr: &[u8; BLOCK]) -> bool {
    let stored = parse_num(&hdr[148..156]);
    let mut sum: u64 = 0;
    for (i, &b) in hdr.iter().enumerate() {
        sum += if (148..156).contains(&i) { b' ' as u64 } else { b as u64 };
    }
    stored == sum
}

/// Read a header at `pos`, following PAX/GNU extensions. Returns the effective
/// entry (if any), the data offset, and the position right after the data.
fn read_header<R: Read + Seek>(
    r: &mut R,
    pos: u64,
    cancel: &CancelToken,
) -> Result<Option<(TarEntry, u64)>> {
    let mut long_name: Option<String> = None;
    let mut pax_path: Option<String> = None;
    let mut cur = pos;
    loop {
        cancel.check()?;
        r.seek(SeekFrom::Start(cur)).map_err(|e| FsError::from_io(e, None))?;
        let mut hdr = [0u8; BLOCK];
        match r.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(FsError::from_io(e, None)),
        }
        if hdr.iter().all(|&b| b == 0) {
            return Ok(None);
        }
        if !checksum_ok(&hdr) {
            return Err(tar_err("Bad TAR header checksum"));
        }
        let typeflag = hdr[156];
        let size = parse_num(&hdr[124..136]);
        let data_start = cur + BLOCK as u64;
        match typeflag {
            b'L' => {
                // GNU long name
                let n = size.min(MAX_META_FILE) as usize;
                let mut buf = vec![0u8; n];
                r.read_exact(&mut buf).map_err(|e| FsError::from_io(e, None))?;
                long_name = Some(cstr(&buf));
                cur = data_start + size.div_ceil(BLOCK as u64) * BLOCK as u64;
                continue;
            }
            b'x' => {
                // PAX extended header for the next entry
                let n = size.min(MAX_META_FILE) as usize;
                let mut buf = vec![0u8; n];
                r.read_exact(&mut buf).map_err(|e| FsError::from_io(e, None))?;
                if let Some(p) = parse_pax_path(&buf) {
                    pax_path = Some(p);
                }
                cur = data_start + size.div_ceil(BLOCK as u64) * BLOCK as u64;
                continue;
            }
            b'g' => {
                // Global PAX header: ignore contents, skip data.
                cur = data_start + size.div_ceil(BLOCK as u64) * BLOCK as u64;
                continue;
            }
            b'K' => {
                // GNU long link target: skip (we do not create links on extract).
                cur = data_start + size.div_ceil(BLOCK as u64) * BLOCK as u64;
                continue;
            }
            _ => {}
        }
        let raw_name = cstr(&hdr[0..MAX_NAME]);
        let prefix = cstr(&hdr[345..500]);
        let mut name = if pax_path.is_some() {
            pax_path.take().unwrap()
        } else if let Some(n) = long_name.take() {
            n
        } else if !prefix.is_empty() {
            format!("{prefix}/{raw_name}")
        } else {
            raw_name
        };
        name = name.trim_end_matches('/').to_string();
        if name.is_empty() {
            cur = data_start + size.div_ceil(BLOCK as u64) * BLOCK as u64;
            continue;
        }
        let is_dir = typeflag == b'5' || name.ends_with('/');
        let is_symlink = typeflag == b'2';
        let link_target = if is_symlink {
            let t = cstr(&hdr[157..257]);
            if t.is_empty() { None } else { Some(t) }
        } else {
            None
        };
        let mode = parse_num(&hdr[100..108]) as u32;
        let mtime = parse_num(&hdr[136..148]) as i64;
        let clean = match safe_archive_relative(&name) {
            Some(p) => p.to_string_lossy().into_owned(),
            None => {
                // Entry tries to escape; skip it but continue scanning.
                cur = data_start + size.div_ceil(BLOCK as u64) * BLOCK as u64;
                continue;
            }
        };
        let entry = TarEntry {
            path: clean,
            is_dir,
            is_symlink,
            size: if is_dir { 0 } else { size },
            modified: mtime,
            mode: if mode == 0 { 0o644 } else { mode },
            link_target,
            data_offset: data_start,
        };
        return Ok(Some((entry, data_start + size.div_ceil(BLOCK as u64) * BLOCK as u64)));
    }
}

fn parse_pax_path(buf: &[u8]) -> Option<String> {
    // Records: "<len> key=value\n"
    let mut i = 0usize;
    let mut path = None;
    while i < buf.len() {
        let sp = buf[i..].iter().position(|&b| b == b' ')?;
        let len_str = std::str::from_utf8(&buf[i..i + sp]).ok()?;
        let len: usize = len_str.parse().ok()?;
        if len == 0 || i + len > buf.len() {
            break;
        }
        let record = &buf[i + sp + 1..i + len];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(eq) = record.iter().position(|&b| b == b'=') {
            let key = &record[..eq];
            let value = &record[eq + 1..];
            if key == b"path" {
                path = Some(String::from_utf8_lossy(value).into_owned());
            }
        }
        i += len;
    }
    path
}

fn is_gzip(path: &Path) -> bool {
    if let Ok(mut f) = File::open(path) {
        let mut magic = [0u8; 2];
        if f.read_exact(&mut magic).is_ok() {
            return magic == [0x1F, 0x8B];
        }
    }
    false
}

fn is_bzip2(path: &Path) -> bool {
    if let Ok(mut f) = File::open(path) {
        let mut magic = [0u8; 3];
        if f.read_exact(&mut magic).is_ok() {
            return magic == *b"BZh";
        }
    }
    false
}

fn is_xz(path: &Path) -> bool {
    if let Ok(mut f) = File::open(path) {
        let mut magic = [0u8; 6];
        if f.read_exact(&mut magic).is_ok() {
            return magic == [0xFD, b'7', b'z', b'X', b'Z', 0x00];
        }
    }
    false
}

/// True when the file name suggests a compressed tarball.
pub fn looks_like_compressed_tar(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".tar.gz") || n.ends_with(".tgz") || n.ends_with(".tar.bz2") || n.ends_with(".tbz2") || n.ends_with(".tar.xz") || n.ends_with(".txz")
}

/// Decompress a gzip stream into `out` (streaming, cancellable).
pub fn gunzip_to(src: &Path, out: &Path, cancel: &CancelToken) -> Result<u64> {
    let f = File::open(src).map_err(|e| FsError::from_io_path(e, src))?;
    let mut gz = GzipDecoder::new(BufReader::with_capacity(64 * 1024, f));
    let mut w = BufWriter::with_capacity(128 * 1024, File::create(out).map_err(|e| FsError::from_io_path(e, out))?);
    let mut buf = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        cancel.check()?;
        let n = gz.read(&mut buf).map_err(|e| FsError::from_io_path(e, src))?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n]).map_err(|e| FsError::from_io_path(e, out))?;
        total += n as u64;
        cancel.add_processed(n as u64);
    }
    w.flush().map_err(|e| FsError::from_io_path(e, out))?;
    Ok(total)
}

/// List a TAR (optionally gzipped) archive. For `.tar.gz` a temporary
/// uncompressed copy is created next to the archive, then removed.
pub fn list_tar(path: &Path, cancel: &CancelToken) -> Result<TarListing> {
    let compressed = is_gzip(path);
    if is_bzip2(path) || is_xz(path) {
        return Err(FsError::unsupported("Only gzip-compressed TAR archives are supported"));
    }
    let mut tmp: Option<PathBuf> = None;
    let src = if compressed {
        let t = std::env::temp_dir().join(format!(".fuss-tar-{}-{}.tar", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        gunzip_to(path, &t, cancel)?;
        tmp = Some(t.clone());
        t
    } else {
        path.to_path_buf()
    };
    let result = (|| -> Result<TarListing> {
        let f = File::open(&src).map_err(|e| FsError::from_io_path(e, &src))?;
        let mut r = BufReader::with_capacity(128 * 1024, f);
        let mut listing = TarListing::default();
        let mut pos = 0u64;
        loop {
            match read_header(&mut r, pos, cancel) {
                Ok(Some((entry, next))) => {
                    listing.total_bytes += entry.size;
                    listing.entries.push(entry);
                    pos = next;
                }
                Ok(None) => break,
                Err(e) => {
                    if listing.entries.is_empty() {
                        return Err(e);
                    }
                    listing.truncated = true;
                    break;
                }
            }
        }
        Ok(listing)
    })();
    if let Some(t) = tmp {
        let _ = std::fs::remove_file(t);
    }
    result
}

/// Extract an entry from an uncompressed or gzipped TAR.
#[allow(dead_code)]
pub fn extract_entry(
    archive: &Path,
    entry: &TarEntry,
    dest: &Path,
    cancel: &CancelToken,
) -> Result<u64> {
    if entry.is_dir {
        std::fs::create_dir_all(dest).map_err(|e| FsError::from_io_path(e, dest))?;
        return Ok(0);
    }
    if entry.is_symlink {
        return Err(FsError::unsupported("Symlink entries are skipped"));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| FsError::from_io_path(e, parent))?;
    }
    let compressed = is_gzip(archive);
    let mut source_file: Option<File> = None;
    let mut sandbox_dir: Option<PathBuf> = None;
    if compressed {
        let t = std::env::temp_dir().join(format!(".fuss-tar-{}-{}.tar", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        gunzip_to(archive, &t, cancel)?;
        source_file = Some(File::open(&t).map_err(|e| FsError::from_io_path(e, &t))?);
        sandbox_dir = Some(t);
    } else {
        source_file = Some(File::open(archive).map_err(|e| FsError::from_io_path(e, archive))?);
    }
    let mut f = source_file.unwrap();
    f.seek(SeekFrom::Start(entry.data_offset)).map_err(|e| FsError::from_io_path(e, archive))?;
    let mut out = BufWriter::with_capacity(128 * 1024, File::create(dest).map_err(|e| FsError::from_io_path(e, dest))?);
    let mut remaining = entry.size;
    let mut buf = vec![0u8; 128 * 1024];
    while remaining > 0 {
        cancel.check()?;
        let want = buf.len().min(remaining as usize);
        let n = f.read(&mut buf[..want]).map_err(|e| FsError::from_io_path(e, archive))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| FsError::from_io_path(e, dest))?;
        remaining -= n as u64;
        cancel.add_processed(n as u64);
    }
    out.flush().map_err(|e| FsError::from_io_path(e, dest))?;
    if let Some(d) = sandbox_dir {
        let _ = std::fs::remove_file(d);
    }
    if entry.modified > 0 {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(entry.modified as u64);
        crate::fsops::set_file_times(dest, t);
    }
    Ok(entry.size - remaining)
}

/// Extract a whole TAR into `dest_dir`.
pub fn extract_tar(archive: &Path, dest_dir: &Path, cancel: &CancelToken) -> Result<super::ExtractSummary> {
    let listing = list_tar(archive, cancel)?;
    let mut out = super::ExtractSummary::default();
    // Decompress once for the whole extraction when needed.
    let compressed = is_gzip(archive);
    let mut tmp: Option<PathBuf> = None;
    let src = if compressed {
        let t = std::env::temp_dir().join(format!(".fuss-tar-{}-{}.tar", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        gunzip_to(archive, &t, cancel)?;
        tmp = Some(t.clone());
        t
    } else {
        archive.to_path_buf()
    };
    let mut f = File::open(&src).map_err(|e| FsError::from_io_path(e, &src))?;
    let mut buf = vec![0u8; 128 * 1024];
    for entry in &listing.entries {
        cancel.check()?;
        let rel = match safe_archive_relative(&entry.path) {
            Some(p) => p,
            None => continue,
        };
        let dest = dest_dir.join(rel);
        if entry.is_dir {
            if std::fs::create_dir_all(&dest).is_ok() {
                out.dirs += 1;
            }
            continue;
        }
        if entry.is_symlink {
            out.skipped += 1;
            continue;
        }
        if let Some(parent) = dest.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                out.errors.push(format!("{}: cannot create parent", entry.path));
                continue;
            }
        }
        f.seek(SeekFrom::Start(entry.data_offset)).map_err(|e| FsError::from_io_path(e, &src))?;
        let mut out_file = match File::create(&dest) {
            Ok(f) => f,
            Err(e) => {
                out.errors.push(format!("{}: {e}", entry.path));
                continue;
            }
        };
        let mut remaining = entry.size;
        let mut failed = false;
        while remaining > 0 {
            cancel.check()?;
            let want = buf.len().min(remaining as usize);
            let n = match f.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    out.errors.push(format!("{}: {e}", entry.path));
                    failed = true;
                    break;
                }
            };
            if let Err(e) = out_file.write_all(&buf[..n]) {
                out.errors.push(format!("{}: {e}", entry.path));
                failed = true;
                break;
            }
            remaining -= n as u64;
            cancel.add_processed(n as u64);
        }
        if !failed {
            out.files += 1;
            out.bytes += entry.size;
            if entry.modified > 0 {
                let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(entry.modified as u64);
                crate::fsops::set_file_times(&dest, t);
            }
        }
    }
    if let Some(t) = tmp {
        let _ = std::fs::remove_file(t);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn write_num_octal(field: &mut [u8], value: u64) {
    let s = format!("{:0width$o}", value, width = field.len() - 1);
    let bytes = s.as_bytes();
    let start = field.len() - 1 - bytes.len().min(field.len() - 1);
    field[..field.len() - 1].fill(b'0');
    field[start..field.len() - 1].copy_from_slice(&bytes[bytes.len().saturating_sub(field.len() - 1 - start)..]);
    field[field.len() - 1] = 0;
}

fn set_checksum(hdr: &mut [u8; BLOCK]) {
    hdr[148..156].fill(b' ');
    let sum: u64 = hdr.iter().map(|&b| b as u64).sum();
    let s = format!("{:06o}\0 ", sum);
    hdr[148..156].copy_from_slice(s.as_bytes());
}

/// Simple ustar TAR writer (360-byte name limit; longer names use PAX headers
/// when `allow_long_names` is set).
pub struct TarWriter<W: Write> {
    out: BufWriter<W>,
    written: u64,
}

impl<W: Write> TarWriter<W> {
    pub fn new(w: W) -> TarWriter<W> {
        TarWriter { out: BufWriter::with_capacity(128 * 1024, w), written: 0 }
    }

    fn write_block(&mut self, b: &[u8]) -> Result<()> {
        self.out.write_all(b).map_err(|e| FsError::from_io(e, None))?;
        self.written += b.len() as u64;
        Ok(())
    }

    fn header(&mut self, name: &str, size: u64, typeflag: u8, mode: u32, mtime: i64) -> Result<()> {
        let mut hdr = [0u8; BLOCK];
        let name_bytes = name.as_bytes();
        if name_bytes.len() > MAX_NAME {
            return Err(FsError::invalid_arg(format!("TAR name too long: {name}")));
        }
        hdr[..name_bytes.len()].copy_from_slice(name_bytes);
        write_num_octal(&mut hdr[100..108], mode as u64 & 0o7777);
        write_num_octal(&mut hdr[108..116], 0); // uid
        write_num_octal(&mut hdr[116..124], 0); // gid
        write_num_octal(&mut hdr[124..136], size);
        write_num_octal(&mut hdr[136..148], mtime.max(0) as u64);
        hdr[156] = typeflag;
        hdr[257..263].copy_from_slice(b"ustar\0");
        hdr[263..265].copy_from_slice(b"00");
        set_checksum(&mut hdr);
        self.write_block(&hdr)?;
        Ok(())
    }

    fn pad(&mut self, data: u64) -> Result<()> {
        let rem = data % BLOCK as u64;
        if rem != 0 {
            let pad = vec![0u8; (BLOCK as u64 - rem) as usize];
            self.write_block(&pad)?;
        }
        Ok(())
    }

    pub fn add_dir(&mut self, name: &str, mtime: i64) -> Result<()> {
        let name = if name.ends_with('/') { name.to_string() } else { format!("{name}/") };
        self.header(&name, 0, b'5', 0o755, mtime)
    }

    pub fn add_file(&mut self, name: &str, src: &Path, cancel: &CancelToken) -> Result<()> {
        let mut f = File::open(src).map_err(|e| FsError::from_io_path(e, src))?;
        let meta = f.metadata().map_err(|e| FsError::from_io_path(e, src))?;
        let mode = crate::entry::mode_of(&meta);
        let mtime = crate::entry::mtime_secs(&meta);
        // PAX header for long paths keeps the format standards-compliant.
        if name.len() > MAX_NAME {
            let record = format!("path={name}\n");
            let len = record.len() + 1 + record.len().to_string().len();
            // Recompute length until stable (self-referential record size).
            let mut len = record.len() + 3;
            loop {
                let candidate = record.len() + 1 + len.to_string().len();
                if candidate == len {
                    break;
                }
                len = candidate;
            }
            let pax = format!("{len} {record}");
            self.header("./PaxHeaders/fuss", pax.len() as u64, b'x', 0o644, mtime)?;
            self.write_block(pax.as_bytes())?;
            self.pad(pax.len() as u64)?;
            // Use a truncated name in the ustar header; readers prefer PAX.
            let truncated: String = name.chars().take(MAX_NAME - 1).collect();
            self.header(&truncated, meta.len(), b'0', mode, mtime)?;
        } else {
            self.header(name, meta.len(), b'0', mode, mtime)?;
        }
        let mut buf = vec![0u8; 128 * 1024];
        let mut total = 0u64;
        loop {
            cancel.check()?;
            let n = f.read(&mut buf).map_err(|e| FsError::from_io_path(e, src))?;
            if n == 0 {
                break;
            }
            self.write_block(&buf[..n])?;
            total += n as u64;
            cancel.add_processed(n as u64);
        }
        if total < meta.len() {
            // File shrank while reading: the header lied. Error out rather than
            // produce an archive that other tools will misread.
            return Err(FsError::new(ErrorCode::Io, "File changed while archiving"));
        }
        self.pad(total)?;
        Ok(())
    }

    /// Two zero blocks then flush.
    pub fn finish(mut self) -> Result<W> {
        let zeros = [0u8; BLOCK * 2];
        self.write_block(&zeros)?;
        self.out.flush().map_err(|e| FsError::from_io(e, None))?;
        self.out.into_inner().map_err(|e| FsError::from_io(e.into_error(), None))
    }
}

/// Recursively archive `root` (including the root folder itself).
pub fn add_tree(writer: &mut TarWriter<impl Write>, root: &Path, cancel: &CancelToken) -> Result<()> {
    let root_name = crate::path::file_name_of(root);
    writer.add_dir(&root_name, crate::entry::mtime_secs(&std::fs::metadata(root).map_err(|e| FsError::from_io_path(e, root))?))?;
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), root_name)];
    while let Some((dir, prefix)) = stack.pop() {
        cancel.check()?;
        let rd = std::fs::read_dir(&dir).map_err(|e| FsError::from_io_path(e, &dir))?;
        let mut children = Vec::new();
        for item in rd {
            match item {
                Ok(i) => children.push(i.path()),
                Err(_) => continue,
            }
        }
        children.sort();
        for p in children {
            if p.to_string_lossy().contains('\n') {
                continue; // PAX record cannot express newline in path safely here
            }
            let name = format!("{prefix}/{}", crate::path::file_name_of(&p));
            let is_dir = std::fs::symlink_metadata(&p).map(|m| m.is_dir() && !m.file_type().is_symlink()).unwrap_or(false);
            if is_dir {
                let mt = std::fs::metadata(&p).map(|m| crate::entry::mtime_secs(&m)).unwrap_or(0);
                writer.add_dir(&name, mt)?;
                stack.push((p, name));
            } else if std::fs::symlink_metadata(&p).map(|m| m.is_file()).unwrap_or(false) {
                writer.add_file(&name, &p, cancel)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fuss-tar-{}-{}-{}", tag, std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn build_tar(dir: &Path) -> PathBuf {
        let tar_path = dir.join("out.tar");
        let w = TarWriter::new(File::create(&tar_path).unwrap());
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), b"alpha").unwrap();
        std::fs::write(src.join("sub/b.bin"), vec![42u8; 5000]).unwrap();
        let mut w = w;
        add_tree(&mut w, &src, &CancelToken::new()).unwrap();
        w.finish().unwrap();
        tar_path
    }

    #[test]
    fn tar_roundtrip() {
        let d = tmp("rt");
        let tar_path = build_tar(&d);
        let cancel = CancelToken::new();
        let listing = list_tar(&tar_path, &cancel).unwrap();
        let paths: Vec<&str> = listing.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["src", "src/a.txt", "src/sub", "src/sub/b.bin"]);
        let out = d.join("extracted");
        let r = extract_tar(&tar_path, &out, &cancel).unwrap();
        assert_eq!(r.files, 2);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(std::fs::read_to_string(out.join("src/a.txt")).unwrap(), "alpha");
        assert_eq!(std::fs::read(out.join("src/sub/b.bin")).unwrap().len(), 5000);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn long_names_via_pax() {
        let d = tmp("pax");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let long = format!("{}.txt", "x".repeat(150));
        std::fs::write(src.join(&long), b"long").unwrap();
        let tar_path = d.join("long.tar");
        let mut w = TarWriter::new(File::create(&tar_path).unwrap());
        add_tree(&mut w, &src, &CancelToken::new()).unwrap();
        w.finish().unwrap();
        let listing = list_tar(&tar_path, &CancelToken::new()).unwrap();
        assert!(listing.entries.iter().any(|e| e.path.ends_with(&long)), "{:?}", listing.entries.iter().map(|e| &e.path).collect::<Vec<_>>());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn gzip_roundtrip_and_tgz_listing() {
        let d = tmp("gz");
        let tar_path = build_tar(&d);
        let data = std::fs::read(&tar_path).unwrap();
        let gz = miniz_oxide::deflate::compress_to_vec(&data, 6);
        let tgz = d.join("out.tar.gz");
        std::fs::write(&tgz, &gz).unwrap();
        let cancel = CancelToken::new();
        let listing = list_tar(&tgz, &cancel).unwrap();
        assert_eq!(listing.entries.len(), 4);
        let out = d.join("x");
        let r = extract_tar(&tgz, &out, &cancel).unwrap();
        assert_eq!(r.files, 2);
        assert_eq!(std::fs::read_to_string(out.join("src/a.txt")).unwrap(), "alpha");
        // gunzip_to alone
        let pure = d.join("plain");
        let n = gunzip_to(&tgz, &pure, &cancel).unwrap();
        assert_eq!(n, data.len() as u64);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn path_traversal_blocked() {
        // Craft a tar with "../evil.txt".
        let d = tmp("slip");
        let tar_path = d.join("evil.tar");
        let mut w = TarWriter::new(File::create(&tar_path).unwrap());
        let payload = b"evil";
        // Manually write header with name ../evil.txt
        w.header("../evil.txt", payload.len() as u64, b'0', 0o644, 0).unwrap();
        w.write_block(payload).unwrap();
        w.pad(payload.len() as u64).unwrap();
        w.finish().unwrap();
        let listing = list_tar(&tar_path, &CancelToken::new()).unwrap();
        assert!(listing.entries.is_empty() || listing.entries.iter().all(|e| !e.path.starts_with("..")));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
