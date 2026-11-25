//! Minimal, correct ZIP reader and writer.
//!
//! Reading: central-directory based, so only the central directory is parsed for
//! browsing — entries are decompressed on demand, which is what makes
//! "archive browsing without unnecessary extraction" cheap even for multi-GB
//! zips. Writing: streamed to disk, deflate via `miniz_oxide`, plus ZIP64
//! records whenever counts/offsets overflow 32 bit.
//!
//! Not supported (never silently accepted): encrypted entries, multi-disk
//! archives, exotic compression methods other than stored/deflate.

use crate::archive::crc32::Crc32;
use crate::archive::inflate_stream::{DeflateDecoder, DeflateEncoder};
use crate::cancel::CancelToken;
use crate::error::{ErrorCode, FsError, Result};
use crate::path::safe_archive_relative;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_EOCD: u32 = 0x0605_4b50;
const SIG_EOCD64: u32 = 0x0606_4b50;
const SIG_EOCD64_LOC: u32 = 0x0706_4b50;

const FLAG_ENCRYPTED: u16 = 1 << 0;
const FLAG_UTF8: u16 = 1 << 11;

const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub compressed_size: u64,
    pub crc32: u32,
    pub modified: i64,
    pub method: u16,
    pub is_encrypted: bool,
    pub local_header_offset: u64,
    /// Virtual path inside the archive, already sanitized.
    pub path: String,
}

#[derive(Debug, Default, Clone)]
pub struct ZipListing {
    pub entries: Vec<ZipEntry>,
    /// Comment from the end-of-central-directory record.
    pub comment: String,
    pub total_uncompressed: u64,
}

fn zip_err(msg: impl Into<String>) -> FsError {
    FsError::corrupt(msg)
}

fn read_u16(r: &mut impl Read) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

/// Convert MS-DOS date/time to a Unix timestamp (local time interpreted as UTC;
/// ZIP has no timezone, this matches what most tools display).
fn dos_datetime_to_unix(date: u16, time: u16) -> i64 {
    let day = (date & 0x1F) as i64;
    let month = ((date >> 5) & 0x0F) as i64;
    let year = ((date >> 9) & 0x7F) as i64 + 1980;
    let sec = ((time & 0x1F) * 2) as i64;
    let min = ((time >> 5) & 0x3F) as i64;
    let hour = ((time >> 11) & 0x1F) as i64;
    if month < 1 || month > 12 || day < 1 || day > 31 {
        return 0;
    }
    // Days-from-civil algorithm (Howard Hinnant), valid for these ranges.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + hour * 3600 + min * 60 + sec
}

/// Locate and parse the end-of-central-directory record. Returns
/// (entry count, central dir offset, central dir size, comment).
fn find_eocd(f: &mut (impl Read + Seek), file_len: u64) -> Result<(u64, u64, u64, String)> {
    let tail = file_len.min(66_000) as usize + 22;
    let start = file_len.saturating_sub(tail as u64);
    f.seek(SeekFrom::Start(start)).map_err(|e| FsError::from_io(e, None))?;
    let mut buf = vec![0u8; (file_len - start) as usize];
    f.read_exact(&mut buf).map_err(|e| FsError::from_io(e, None))?;
    let mut pos = buf.len().saturating_sub(22);
    loop {
        if buf.len() >= pos + 4 && u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) == SIG_EOCD {
            let mut cur = &buf[pos + 4..];
            let disk = read_u16(&mut cur).map_err(|e| FsError::from_io(e, None))?;
            let cd_disk = read_u16(&mut cur).map_err(|e| FsError::from_io(e, None))?;
            let _entries_disk = read_u16(&mut cur).map_err(|e| FsError::from_io(e, None))?;
            let entries = read_u16(&mut cur).map_err(|e| FsError::from_io(e, None))?;
            let cd_size = read_u32(&mut cur).map_err(|e| FsError::from_io(e, None))?;
            let cd_offset = read_u32(&mut cur).map_err(|e| FsError::from_io(e, None))?;
            let comment_len = read_u16(&mut cur).map_err(|e| FsError::from_io(e, None))?;
            let comment = String::from_utf8_lossy(&cur[..(comment_len as usize).min(cur.len())]).into_owned();
            let mut entries = entries as u64;
            let mut cd_size = cd_size as u64;
            let mut cd_offset = cd_offset as u64;
            // ZIP64: look for the locator immediately before the EOCD.
            if entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
                if pos >= 20 {
                    let loc_start = pos - 20;
                    if u32::from_le_bytes([buf[loc_start], buf[loc_start + 1], buf[loc_start + 2], buf[loc_start + 3]]) == SIG_EOCD64_LOC {
                        let eocd64_offset = u64::from_le_bytes([
                            buf[loc_start + 8], buf[loc_start + 9], buf[loc_start + 10], buf[loc_start + 11],
                            buf[loc_start + 12], buf[loc_start + 13], buf[loc_start + 14], buf[loc_start + 15],
                        ]);
                        let mut b = [0u8; 56];
                        f.seek(SeekFrom::Start(eocd64_offset)).map_err(|e| FsError::from_io(e, None))?;
                        f.read_exact(&mut b).map_err(|e| FsError::from_io(e, None))?;
                        if u32::from_le_bytes([b[0], b[1], b[2], b[3]]) != SIG_EOCD64 {
                            return Err(zip_err("Corrupt ZIP64 end of central directory record"));
                        }
                        let mut cur = &b[32..];
                        entries = read_u64(&mut cur).map_err(|e| FsError::from_io(e, None))?;
                        cd_size = read_u64(&mut cur).map_err(|e| FsError::from_io(e, None))?;
                        cd_offset = read_u64(&mut cur).map_err(|e| FsError::from_io(e, None))?;
                    }
                }
                if entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
                    return Err(zip_err("ZIP64 records missing"));
                }
            }
            if disk != 0 || cd_disk != 0 {
                return Err(FsError::unsupported("Multi-disk ZIP archives are not supported"));
            }
            return Ok((entries, cd_offset, cd_size, comment));
        }
        if pos == 0 {
            break;
        }
        pos -= 1;
    }
    Err(zip_err("Not a ZIP archive (no end-of-central-directory record)"))
}

/// Read the central directory. This is the only pass over the archive metadata;
/// entry data is not touched.
pub fn list_zip(path: &Path, cancel: &CancelToken) -> Result<ZipListing> {
    let mut file = File::open(path).map_err(|e| FsError::from_io_path(e, path))?;
    let len = file.metadata().map_err(|e| FsError::from_io_path(e, path))?.len();
    let (count, cd_offset, cd_size, comment) = find_eocd(&mut file, len)?;
    if cd_size > len {
        return Err(zip_err("Corrupt central directory size"));
    }
    file.seek(SeekFrom::Start(cd_offset)).map_err(|e| FsError::from_io_path(e, path))?;
    let mut rd = BufReader::with_capacity(256 * 1024, file);
    let mut listing = ZipListing { comment, ..Default::default() };
    listing.entries.reserve(count.min(200_000) as usize);
    for _ in 0..count {
        cancel.check()?;
        let sig = match read_u32(&mut rd) {
            Ok(s) => s,
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(FsError::from_io_path(e, path)),
        };
        if sig != SIG_CENTRAL {
            return Err(zip_err("Bad central directory entry signature"));
        }
        let _version_made = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let _version_needed = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let flags = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let method = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let mod_time = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let mod_date = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let crc = read_u32(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let mut comp = read_u32(&mut rd).map_err(|e| FsError::from_io_path(e, path))? as u64;
        let mut size = read_u32(&mut rd).map_err(|e| FsError::from_io_path(e, path))? as u64;
        let name_len = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))? as usize;
        let extra_len = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))? as usize;
        let comment_len = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))? as usize;
        let _disk_start = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let _int_attr = read_u16(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let _ext_attr = read_u32(&mut rd).map_err(|e| FsError::from_io_path(e, path))?;
        let mut offset = read_u32(&mut rd).map_err(|e| FsError::from_io_path(e, path))? as u64;
        let mut name_buf = vec![0u8; name_len];
        rd.read_exact(&mut name_buf).map_err(|e| FsError::from_io_path(e, path))?;
        let mut extra = vec![0u8; extra_len];
        rd.read_exact(&mut extra).map_err(|e| FsError::from_io_path(e, path))?;
        if comment_len > 0 {
            let mut c = vec![0u8; comment_len];
            rd.read_exact(&mut c).map_err(|e| FsError::from_io_path(e, path))?;
        }
        // ZIP64 extended information extra field (0x0001).
        if extra_len >= 4 {
            let mut i = 0usize;
            while i + 4 <= extra.len() {
                let tag = u16::from_le_bytes([extra[i], extra[i + 1]]);
                let sz = u16::from_le_bytes([extra[i + 2], extra[i + 3]]) as usize;
                if i + 4 + sz > extra.len() {
                    break;
                }
                if tag == 0x0001 {
                    let mut cur = &extra[i + 4..i + 4 + sz];
                    let mut take = |v: &mut u64| {
                        if *v == 0xFFFF_FFFF {
                            if let Ok(x) = read_u64(&mut cur) {
                                *v = x;
                            }
                        }
                    };
                    if size == 0xFFFF_FFFF {
                        if let Ok(x) = read_u64(&mut cur) {
                            size = x;
                        }
                    }
                    if comp == 0xFFFF_FFFF {
                        if let Ok(x) = read_u64(&mut cur) {
                            comp = x;
                        }
                    }
                    take(&mut offset);
                }
                i += 4 + sz;
            }
        }
        let name = if flags & FLAG_UTF8 != 0 {
            String::from_utf8_lossy(&name_buf).into_owned()
        } else {
            // Many Android-created zips use UTF-8 without setting the flag.
            match std::str::from_utf8(&name_buf) {
                Ok(s) => s.to_string(),
                Err(_) => String::from_utf8_lossy(&name_buf).into_owned(),
            }
        };
        let is_dir = name.ends_with('/') || name.ends_with('\\');
        let clean = match safe_archive_relative(&name) {
            Some(p) => p.to_string_lossy().into_owned(),
            None if is_dir => name.clone(),
            None => continue,
        };
        listing.total_uncompressed += size;
        listing.entries.push(ZipEntry {
            path: clean,
            name,
            is_dir,
            size,
            compressed_size: comp,
            crc32: crc,
            modified: dos_datetime_to_unix(mod_date, mod_time),
            method,
            is_encrypted: flags & FLAG_ENCRYPTED != 0,
            local_header_offset: offset,
        });
    }
    cancel.check()?;
    Ok(listing)
}

/// Reader that decompresses exactly one ZIP entry.
pub struct ZipEntryReader<'a> {
    inner: Box<dyn Read + 'a>,
    remaining: u64,
    crc: Crc32,
    expected_crc: u32,
    verify: bool,
}

impl Read for ZipEntryReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            if self.verify && self.crc.finish() != self.expected_crc {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "CRC mismatch"));
            }
            return Ok(0);
        }
        let want = buf.len().min(self.remaining as usize);
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Truncated archive entry"));
        }
        if self.verify {
            self.crc.update(&buf[..n]);
        }
        self.remaining -= n as u64;
        Ok(n)
    }
}

/// Open one entry's decompressed stream. The archive file must stay alive for
/// the returned reader's lifetime (`file` is borrowed).
pub fn open_entry<'a>(
    file: &'a mut File,
    entry: &ZipEntry,
    verify_crc: bool,
) -> Result<ZipEntryReader<'a>> {
    if entry.is_encrypted {
        return Err(FsError::unsupported("Encrypted ZIP entries are not supported"));
    }
    file.seek(SeekFrom::Start(entry.local_header_offset)).map_err(|e| FsError::from_io(e, None))?;
    let mut hdr = [0u8; 30];
    file.read_exact(&mut hdr).map_err(|e| FsError::from_io(e, None))?;
    if u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) != SIG_LOCAL {
        return Err(zip_err("Bad local file header"));
    }
    let name_len = u16::from_le_bytes([hdr[26], hdr[27]]) as u64;
    let extra_len = u16::from_le_bytes([hdr[28], hdr[29]]) as u64;
    let data_offset = entry.local_header_offset + 30 + name_len + extra_len;
    file.seek(SeekFrom::Start(data_offset)).map_err(|e| FsError::from_io(e, None))?;
    let limited = (&mut *file).take(entry.compressed_size);
    let inner: Box<dyn Read + 'a> = match entry.method {
        METHOD_STORE => Box::new(limited),
        METHOD_DEFLATE => Box::new(DeflateDecoder::new(limited)),
        other => {
            return Err(FsError::unsupported(format!("Unsupported ZIP compression method {other}")));
        }
    };
    Ok(ZipEntryReader {
        inner,
        remaining: entry.size,
        crc: Crc32::new(),
        expected_crc: entry.crc32,
        verify: verify_crc,
    })
}

/// Extract a single entry to `dest` (which must not exist or will be replaced).
/// Returns bytes written. Path traversal is the caller's concern; `dest` is
/// always constructed from the sanitized `entry.path`.
pub fn extract_entry(
    archive: &Path,
    entry: &ZipEntry,
    dest: &Path,
    cancel: &CancelToken,
) -> Result<u64> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| FsError::from_io_path(e, parent))?;
    }
    let mut file = File::open(archive).map_err(|e| FsError::from_io_path(e, archive))?;
    let mut reader = open_entry(&mut file, entry, true)?;
    let out = File::create(dest).map_err(|e| FsError::from_io_path(e, dest))?;
    let mut out = BufWriter::with_capacity(128 * 1024, out);
    let mut buf = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        cancel.check()?;
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(FsError::from_io_path(e, dest)),
        };
        out.write_all(&buf[..n]).map_err(|e| FsError::from_io_path(e, dest))?;
        total += n as u64;
        cancel.add_processed(total);
    }
    out.flush().map_err(|e| FsError::from_io_path(e, dest))?;
    set_mtime_android(dest, entry.modified);
    Ok(total)
}

fn set_mtime_android(path: &Path, unix_secs: i64) {
    if unix_secs <= 0 {
        return;
    }
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix_secs as u64);
    crate::fsops::set_file_times(path, t);
}

/// Extract a whole archive into `dest_dir`, creating subdirectories and
/// refusing to escape `dest_dir`. Per-entry failures are collected.
#[derive(Debug, Default)]
pub struct ExtractResult {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: Vec<String>,
    pub first_error_code: Option<String>,
    pub skipped_encrypted: u64,
}

pub fn extract_zip(
    archive: &Path,
    dest_dir: &Path,
    cancel: &CancelToken,
) -> Result<ExtractResult> {
    let listing = list_zip(archive, cancel)?;
    let mut out = ExtractResult::default();
    for entry in &listing.entries {
        cancel.check()?;
        if entry.is_encrypted {
            out.skipped_encrypted += 1;
            continue;
        }
        let rel = match safe_archive_relative(&entry.path) {
            Some(p) => p,
            None => continue,
        };
        let dest = dest_dir.join(&rel);
        if entry.is_dir {
            if let Err(e) = std::fs::create_dir_all(&dest) {
                out.push_err(format!("{}: {e}", entry.path));
            } else {
                out.dirs += 1;
            }
            continue;
        }
        match extract_entry(archive, entry, &dest, cancel) {
            Ok(n) => {
                out.files += 1;
                out.bytes += n;
            }
            Err(e) if e.code == ErrorCode::Cancelled => return Err(e),
            Err(e) => out.push_err(format!("{}: {}", entry.path, e.message)),
        }
    }
    Ok(out)
}

impl ExtractResult {
    fn push_err(&mut self, msg: String) {
        if self.errors.len() < 100 {
            self.errors.push(msg);
        }
        if self.first_error_code.is_none() {
            self.first_error_code = Some("io".into());
        }
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

struct CentralRec {
    name: String,
    crc: u32,
    size: u64,
    compressed: u64,
    offset: u64,
    method: u16,
    mod_time: u16,
    mod_date: u16,
    external_attr: u32,
    is_dir: bool,
}

/// Streaming ZIP writer.
///
/// Every file is compressed to a temporary spill file first; its real sizes and
/// CRC then go into both the local header and the central directory. This costs
/// one extra temp file per entry, but yields archives that every ZIP reader
/// accepts (data descriptors are poorly supported on some Android tools) and
/// exact 64-bit offsets, so ZIP64 output is correct for >4 GiB archives.
///
/// Memory use is constant: one 128 KiB copy buffer plus one spill file on disk.
pub struct ZipWriter<W: Write> {
    out: W,
    /// Total uncompressed bytes planned so far for one entry (named `.fuss-zip-*`),
    /// removed when the writer is dropped if not consumed.
    offset: u64,
    records: Vec<CentralRec>,
    finished: bool,
}

impl<W: Write> ZipWriter<W> {
    pub fn new(w: W) -> ZipWriter<W> {
        ZipWriter { out: w, offset: 0, records: Vec::new(), finished: false }
    }

    fn write_all(&mut self, b: &[u8]) -> Result<()> {
        self.out.write_all(b).map_err(|e| FsError::from_io(e, None))?;
        self.offset += b.len() as u64;
        Ok(())
    }

    fn dos_now() -> (u16, u16) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        unix_to_dos(secs)
    }

    pub fn add_dir(&mut self, name: &str) -> Result<()> {
        self.add_dir_at(name, Self::dos_now())
    }

    fn add_dir_at(&mut self, name: &str, (time, date): (u16, u16)) -> Result<()> {
        let mut name = name.trim_matches('/').to_string();
        if name.is_empty() {
            return Ok(());
        }
        name.push('/');
        let local_offset = self.offset;
        let name_b = name.as_bytes();
        let mut hdr = Vec::with_capacity(30 + name_b.len());
        hdr.extend_from_slice(&SIG_LOCAL.to_le_bytes());
        hdr.extend_from_slice(&20u16.to_le_bytes());
        hdr.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        hdr.extend_from_slice(&METHOD_STORE.to_le_bytes());
        hdr.extend_from_slice(&time.to_le_bytes());
        hdr.extend_from_slice(&date.to_le_bytes());
        hdr.extend_from_slice(&0u32.to_le_bytes());
        hdr.extend_from_slice(&0u32.to_le_bytes());
        hdr.extend_from_slice(&0u32.to_le_bytes());
        hdr.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        hdr.extend_from_slice(&0u16.to_le_bytes());
        hdr.extend_from_slice(name_b);
        self.write_all(&hdr)?;
        self.records.push(CentralRec {
            name,
            crc: 0,
            size: 0,
            compressed: 0,
            offset: local_offset,
            method: METHOD_STORE,
            mod_time: time,
            mod_date: date,
            external_attr: (0o40755u32) << 16 | 0x10, // dir bit
            is_dir: true,
        });
        Ok(())
    }

    /// Append `src` under `name`. Streams the file through the compressor into a
    /// spill file, then copies the spill into the archive.
    pub fn add_file(&mut self, name: &str, src: &Path, cancel: &CancelToken) -> Result<()> {
        let mut input = File::open(src).map_err(|e| FsError::from_io_path(e, src))?;
        let meta = input.metadata().map_err(|e| FsError::from_io_path(e, src))?;
        let mtime = crate::entry::mtime_secs(&meta);
        let (time, date) = if mtime > 0 { unix_to_dos(mtime) } else { Self::dos_now() };
        let name = name.trim_start_matches('/').to_string();
        if name.is_empty() {
            return Err(FsError::invalid_arg("Empty archive entry name"));
        }
        let spill_path = std::env::temp_dir().join(format!(
            ".fuss-zip-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
            self.records.len()
        ));
        let spill = File::create(&spill_path).map_err(|e| FsError::from_io_path(e, &spill_path))?;
        let mut crc = Crc32::new();
        let mut raw_total = 0u64;
        let compressed_total: u64;
        {
            let mut enc = DeflateEncoder::new(std::io::BufWriter::with_capacity(128 * 1024, spill), 6);
            let mut buf = vec![0u8; 128 * 1024];
            let copy_result = (|| -> Result<()> {
                loop {
                    cancel.check()?;
                    let n = match input.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(FsError::from_io_path(e, src)),
                    };
                    crc.update(&buf[..n]);
                    raw_total += n as u64;
                    enc.write_all(&buf[..n]).map_err(|e| FsError::from_io(e, None))?;
                    cancel.add_processed(n as u64);
                }
                enc.finish().map_err(|e| FsError::from_io(e, None))?;
                Ok(())
            })();
            if let Err(e) = copy_result {
                drop(enc);
                let _ = std::fs::remove_file(&spill_path);
                return Err(e);
            }
            let bw = enc.into_inner();
            let f = bw.into_inner().map_err(|e| FsError::from_io(e.into_error(), None))?;
            compressed_total = f.metadata().map(|m| m.len()).map_err(|e| FsError::from_io_path(e, &spill_path))?;
        }
        let result = (|| -> Result<()> {
            let local_offset = self.offset;
            let name_b = name.as_bytes();
            let needs_zip64_local = raw_total >= u32::MAX as u64 || compressed_total >= u32::MAX as u64 || local_offset >= u32::MAX as u64;
            let mut extra: Vec<u8> = Vec::new();
            if needs_zip64_local {
                let mut payload = Vec::new();
                payload.extend_from_slice(&raw_total.to_le_bytes());
                payload.extend_from_slice(&compressed_total.to_le_bytes());
                extra.extend_from_slice(&0x0001u16.to_le_bytes());
                extra.extend_from_slice(&(payload.len() as u16).to_le_bytes());
                extra.extend_from_slice(&payload);
            }
            let mut hdr = Vec::with_capacity(30 + name_b.len() + extra.len());
            hdr.extend_from_slice(&SIG_LOCAL.to_le_bytes());
            hdr.extend_from_slice(&45u16.to_le_bytes()); // version needed: 4.5 (zip64 capable)
            hdr.extend_from_slice(&FLAG_UTF8.to_le_bytes());
            hdr.extend_from_slice(&METHOD_DEFLATE.to_le_bytes());
            hdr.extend_from_slice(&time.to_le_bytes());
            hdr.extend_from_slice(&date.to_le_bytes());
            hdr.extend_from_slice(&crc.finish().to_le_bytes());
            hdr.extend_from_slice(&(if needs_zip64_local { u32::MAX } else { compressed_total as u32 }).to_le_bytes());
            hdr.extend_from_slice(&(if needs_zip64_local { u32::MAX } else { raw_total as u32 }).to_le_bytes());
            hdr.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
            hdr.extend_from_slice(&(extra.len() as u16).to_le_bytes());
            hdr.extend_from_slice(name_b);
            hdr.extend_from_slice(&extra);
            self.write_all(&hdr)?;
            let mut spill = File::open(&spill_path).map_err(|e| FsError::from_io_path(e, &spill_path))?;
            let mut buf = vec![0u8; 128 * 1024];
            loop {
                cancel.check()?;
                let n = spill.read(&mut buf).map_err(|e| FsError::from_io_path(e, &spill_path))?;
                if n == 0 {
                    break;
                }
                self.write_all(&buf[..n])?;
            }
            self.records.push(CentralRec {
                name,
                crc: crc.finish(),
                size: raw_total,
                compressed: compressed_total,
                offset: local_offset,
                method: METHOD_DEFLATE,
                mod_time: time,
                mod_date: date,
                external_attr: (0o100644u32) << 16,
                is_dir: false,
            });
            Ok(())
        })();
        let _ = std::fs::remove_file(&spill_path);
        result
    }

    /// Write the central directory and finish, returning the inner writer.
    pub fn finish(mut self) -> Result<W> {
        if self.finished {
            return Err(FsError::invalid_arg("Archive already finished"));
        }
        self.finished = true;
        let cd_start = self.offset;
        let count = self.records.len() as u64;
        let mut cd: Vec<u8> = Vec::with_capacity(self.records.len() * 80);
        let mut need_zip64 = count >= u16::MAX as u64 || cd_start >= u32::MAX as u64;
        for r in &self.records {
            if r.offset >= u32::MAX as u64 || r.size >= u32::MAX as u64 || r.compressed >= u32::MAX as u64 {
                need_zip64 = true;
            }
        }
        for r in &self.records {
            cd.extend_from_slice(&SIG_CENTRAL.to_le_bytes());
            cd.extend_from_slice(&45u16.to_le_bytes()); // version made by
            cd.extend_from_slice(&45u16.to_le_bytes()); // version needed
            cd.extend_from_slice(&FLAG_UTF8.to_le_bytes());
            cd.extend_from_slice(&r.method.to_le_bytes());
            cd.extend_from_slice(&r.mod_time.to_le_bytes());
            cd.extend_from_slice(&r.mod_date.to_le_bytes());
            cd.extend_from_slice(&r.crc.to_le_bytes());
            let needs_sizes = r.size >= u32::MAX as u64 || r.compressed >= u32::MAX as u64;
            let needs_offset = r.offset >= u32::MAX as u64;
            let mut extra: Vec<u8> = Vec::new();
            if needs_sizes || needs_offset {
                let mut payload: Vec<u8> = Vec::new();
                if needs_sizes {
                    payload.extend_from_slice(&r.size.to_le_bytes());
                    payload.extend_from_slice(&r.compressed.to_le_bytes());
                }
                if needs_offset {
                    payload.extend_from_slice(&r.offset.to_le_bytes());
                }
                extra.extend_from_slice(&0x0001u16.to_le_bytes());
                extra.extend_from_slice(&(payload.len() as u16).to_le_bytes());
                extra.extend_from_slice(&payload);
            }
            cd.extend_from_slice(&(if needs_sizes { u32::MAX } else { r.compressed as u32 }).to_le_bytes());
            cd.extend_from_slice(&(if needs_sizes { u32::MAX } else { r.size as u32 }).to_le_bytes());
            cd.extend_from_slice(&(r.name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&(extra.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // comment len
            cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
            cd.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            cd.extend_from_slice(&r.external_attr.to_le_bytes());
            cd.extend_from_slice(&(if needs_offset { u32::MAX } else { r.offset as u32 }).to_le_bytes());
            cd.extend_from_slice(r.name.as_bytes());
            cd.extend_from_slice(&extra);
        }
        let cd_size = cd.len() as u64;
        self.write_all(&cd)?;
        if need_zip64 {
            let eocd64_offset = self.offset;
            let mut e = Vec::new();
            e.extend_from_slice(&SIG_EOCD64.to_le_bytes());
            e.extend_from_slice(&44u64.to_le_bytes());
            e.extend_from_slice(&45u16.to_le_bytes());
            e.extend_from_slice(&45u16.to_le_bytes());
            e.extend_from_slice(&0u32.to_le_bytes());
            e.extend_from_slice(&0u32.to_le_bytes());
            e.extend_from_slice(&count.to_le_bytes());
            e.extend_from_slice(&count.to_le_bytes());
            e.extend_from_slice(&cd_size.to_le_bytes());
            e.extend_from_slice(&cd_start.to_le_bytes());
            e.extend_from_slice(&SIG_EOCD64_LOC.to_le_bytes());
            e.extend_from_slice(&0u32.to_le_bytes());
            e.extend_from_slice(&eocd64_offset.to_le_bytes());
            e.extend_from_slice(&1u32.to_le_bytes());
            self.write_all(&e)?;
        }
        let mut e = Vec::new();
        e.extend_from_slice(&SIG_EOCD.to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        let c16 = if count >= u16::MAX as u64 { u16::MAX } else { count as u16 };
        e.extend_from_slice(&c16.to_le_bytes());
        e.extend_from_slice(&c16.to_le_bytes());
        e.extend_from_slice(&(if need_zip64 { u32::MAX } else { cd_size as u32 }).to_le_bytes());
        e.extend_from_slice(&(if need_zip64 { u32::MAX } else { cd_start as u32 }).to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        self.write_all(&e)?;
        self.out.flush().map_err(|e| FsError::from_io(e, None))?;
        Ok(self.out)
    }
}

impl<W: Write> std::io::Write for ZipWriter<W> {
    /// Convenience so `ZipWriter` can be used where a `Write` sink is expected
    /// (auto-detects nothing; callers must use `add_file` for entries).
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.out.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

fn unix_to_dos(secs: i64) -> (u16, u16) {
    if secs <= 0 {
        return (0, 0x21);
    }
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    let year = year.clamp(1980, 2107);
    let hour = sod / 3600;
    let min = (sod % 3600) / 60;
    let sec = sod % 60;
    let time = ((hour as u16) << 11) | ((min as u16) << 5) | ((sec as u16) / 2);
    let date = (((year - 1980) as u16) << 9) | ((m as u16) << 5) | (d as u16);
    (time, date)
}

/// Add a directory tree to the writer, streaming each file.
pub fn add_tree<W: Write>(writer: &mut ZipWriter<W>, root: &Path, cancel: &CancelToken, include_root: bool) -> Result<()> {
    let root_name = if include_root { crate::path::file_name_of(root) } else { String::new() };
    if include_root {
        writer.add_dir(&root_name)?;
    }
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), root_name)];
    while let Some((dir, prefix)) = stack.pop() {
        cancel.check()?;
        let rd = std::fs::read_dir(&dir).map_err(|e| FsError::from_io_path(e, &dir))?;
        let mut children: Vec<(PathBuf, bool)> = Vec::new();
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            let p = item.path();
            if let Ok(m) = std::fs::symlink_metadata(&p) {
                children.push((p, m.is_dir() && !m.file_type().is_symlink()));
            }
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        for (p, is_dir) in children {
            let name = crate::path::file_name_of(&p);
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if rel.contains("..") {
                continue;
            }
            if is_dir {
                writer.add_dir(&rel)?;
                stack.push((p, rel));
            } else {
                writer.add_file(&rel, &p, cancel)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn build_sample() -> Vec<u8> {
        let sink = Cursor::new(Vec::new());
        let mut w = ZipWriter::new(sink);
        w.add_dir("folder").unwrap();
        let tmp = std::env::temp_dir().join(format!("fuss-zip-src-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&tmp).unwrap();
        let f1 = tmp.join("hello.txt");
        std::fs::write(&f1, b"Hello, ZIP!\n").unwrap();
        let f2 = tmp.join("data.bin");
        let payload: Vec<u8> = (0..200_000u32).map(|i| (i * 31 % 256) as u8).collect();
        std::fs::write(&f2, &payload).unwrap();
        w.add_file("folder/hello.txt", &f1, &CancelToken::new()).unwrap();
        w.add_file("folder/data.bin", &f2, &CancelToken::new()).unwrap();
        w.add_file("empty.txt", &{
            let e = tmp.join("empty.txt");
            std::fs::write(&e, b"").unwrap();
            e
        }, &CancelToken::new()).unwrap();
        let out = w.finish().unwrap().into_inner();
        std::fs::remove_dir_all(&tmp).unwrap();
        out
    }

    fn write_archive(bytes: &[u8], tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fuss-zip-{}-{}-{}.zip", tag, std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn roundtrip_list_and_extract() {
        let bytes = build_sample();
        let path = write_archive(&bytes, "rt");
        let cancel = CancelToken::new();
        let listing = list_zip(&path, &cancel).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, vec!["folder", "folder/hello.txt", "folder/data.bin", "empty.txt"]);
        let dest = std::env::temp_dir().join(format!("fuss-zip-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let r = extract_zip(&path, &dest, &cancel).unwrap();
        assert_eq!(r.files, 3);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(std::fs::read(dest.join("folder/hello.txt")).unwrap(), b"Hello, ZIP!\n");
        let data = std::fs::read(dest.join("folder/data.bin")).unwrap();
        assert_eq!(data.len(), 200_000);
        assert_eq!(data[12345], (12345u32 * 31 % 256) as u8);
        std::fs::remove_dir_all(&dest).unwrap();
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn detects_corruption_via_crc() {
        let mut bytes = build_sample();
        let path = write_archive(&bytes, "crc");
        let cancel = CancelToken::new();
        let listing = list_zip(&path, &cancel).unwrap();
        // Flip one byte of the stored (uncompressed) "Hello" entry data.
        let entry = listing.entries.iter().find(|e| e.path.ends_with("hello.txt")).unwrap();
        let mut f = File::open(&path).unwrap();
        let mut r = open_entry(&mut f, entry, true).unwrap();
        let mut buf = Vec::new();
        assert!(r.read_to_end(&mut buf).is_ok());
        assert_eq!(buf, b"Hello, ZIP!\n");
        // Corrupt the data byte in place and re-verify.
        let data_offset = {
            let mut hdr = [0u8; 30];
            f.seek(SeekFrom::Start(entry.local_header_offset)).unwrap();
            f.read_exact(&mut hdr).unwrap();
            let nl = u16::from_le_bytes([hdr[26], hdr[27]]) as u64;
            let el = u16::from_le_bytes([hdr[28], hdr[29]]) as u64;
            entry.local_header_offset + 30 + nl + el
        };
        bytes[data_offset as usize] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        let mut f = File::open(&path).unwrap();
        let mut r = open_entry(&mut f, entry, true).unwrap();
        let mut err = None;
        let mut out = Vec::new();
        match r.read_to_end(&mut out) {
            Ok(_) => err = None,
            Err(e) => err = Some(e),
        }
        assert!(err.is_some(), "CRC mismatch must be reported");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_non_zip() {
        let p = write_archive(b"this is not a zip file at all, not even close.......", "nozip");
        assert!(list_zip(&p, &CancelToken::new()).is_err());
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn dos_time_roundtrip() {
        for secs in [0i64, 1_700_000_000, 1_000_000_000, 2_000_000_000] {
            let (t, d) = unix_to_dos(secs);
            let back = dos_datetime_to_unix(d, t);
            assert!((back - secs).abs() <= 2, "{secs} -> {back}");
        }
    }
}
