//! Archive support: ZIP (read/write), TAR (read/write), GZIP (read/write).
//!
//! 7z is intentionally not implemented: pure-Rust 7z support would pull in a
//! large dependency tree for a format that Android users rarely receive, and
//! the core promise of this app is a small, auditable engine. The UI reports 7z
//! as unsupported instead of pretending. RAR likewise.

pub mod crc32;
pub mod inflate_stream;
pub mod tar;
pub mod zip;

use crate::cancel::CancelToken;
use crate::error::{FsError, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    Gzip,
    Unsupported,
}

/// Detect the archive format from the file name and magic bytes.
pub fn detect(path: &Path) -> ArchiveFormat {
    let name = crate::path::file_name_of(path).to_ascii_lowercase();
    let by_name = if name.ends_with(".zip") || name.ends_with(".zipx") || name.ends_with(".apks") || name.ends_with(".xapk") || name.ends_with(".jar") || name.ends_with(".epub") {
        ArchiveFormat::Zip
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        ArchiveFormat::TarGz
    } else if name.ends_with(".tar") {
        ArchiveFormat::Tar
    } else if name.ends_with(".gz") {
        ArchiveFormat::Gzip
    } else {
        // bz2/xz/7z/rar/zst are known archive extensions the engine cannot open;
        // magic sniffing below may still find a supported container inside.
        ArchiveFormat::Unsupported
    };
    if by_name != ArchiveFormat::Unsupported {
        return by_name;
    }
    // Magic sniffing for extension-less files.
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut magic = [0u8; 6];
        if f.read_exact(&mut magic).is_ok() {
            if magic[0..4] == [0x50, 0x4b, 0x03, 0x04] || magic[0..4] == [0x50, 0x4b, 0x05, 0x06] {
                return ArchiveFormat::Zip;
            }
            if magic[0..2] == [0x1F, 0x8B] {
                return ArchiveFormat::Gzip;
            }
            if &magic[0..5] == b"ustar" || &magic[0..3] == b"BZh" || magic == [0xFD, b'7', b'z', b'X', b'Z', 0] {
                return ArchiveFormat::Unsupported;
            }
        }
    }
    ArchiveFormat::Unsupported
}

/// Detect TAR by reading the ustar magic at offset 257.
pub fn is_tar(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut magic = [0u8; 5];
        if f.seek(SeekFrom::Start(257)).is_ok() && f.read_exact(&mut magic).is_ok() {
            return &magic == b"ustar";
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct ArchiveEntryInfo {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
    pub compressed_size: u64,
}

#[derive(Debug, Default, Clone)]
pub struct ArchiveContents {
    pub format: &'static str,
    pub entries: Vec<ArchiveEntryInfo>,
    pub total_bytes: u64,
    pub total_compressed: u64,
}

fn format_name(f: ArchiveFormat) -> &'static str {
    match f {
        ArchiveFormat::Zip => "zip",
        ArchiveFormat::Tar => "tar",
        ArchiveFormat::TarGz => "tar.gz",
        ArchiveFormat::Gzip => "gzip",
        ArchiveFormat::Unsupported => "unknown",
    }
}

/// List an archive's contents without extracting anything (except a temporary
/// gunzip for tar.gz).
pub fn list(path: &Path, cancel: &CancelToken) -> Result<ArchiveContents> {
    let fmt = detect(path);
    match fmt {
        ArchiveFormat::Zip => {
            let l = zip::list_zip(path, cancel)?;
            Ok(ArchiveContents {
                format: "zip",
                total_bytes: l.total_uncompressed,
                total_compressed: l.entries.iter().map(|e| e.compressed_size).sum(),
                entries: l
                    .entries
                    .into_iter()
                    .map(|e| ArchiveEntryInfo { path: e.path, is_dir: e.is_dir, size: e.size, modified: e.modified, compressed_size: e.compressed_size })
                    .collect(),
            })
        }
        ArchiveFormat::Tar | ArchiveFormat::TarGz => {
            let l = tar::list_tar(path, cancel)?;
            Ok(ArchiveContents {
                format: format_name(fmt),
                total_bytes: l.total_bytes,
                total_compressed: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
                entries: l
                    .entries
                    .into_iter()
                    .map(|e| ArchiveEntryInfo { path: e.path, is_dir: e.is_dir, size: e.size, modified: e.modified, compressed_size: 0 })
                    .collect(),
            })
        }
        ArchiveFormat::Gzip => {
            // A bare gzip file: single stream whose name is the file minus ".gz".
            let name = crate::path::file_name_of(path);
            let out_name = name.strip_suffix(".gz").or_else(|| name.strip_suffix(".GZ")).unwrap_or(&name).to_string();
            Ok(ArchiveContents {
                format: "gzip",
                total_bytes: 0,
                total_compressed: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
                entries: vec![ArchiveEntryInfo { path: out_name, is_dir: false, size: 0, modified: 0, compressed_size: 0 }],
            })
        }
        ArchiveFormat::Unsupported => Err(FsError::unsupported("Unsupported archive format")),
    }
}

#[derive(Debug, Default, Clone)]
pub struct ExtractSummary {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: Vec<String>,
    pub skipped: u64,
}

/// Extract an archive into `dest_dir` (created if missing).
pub fn extract(path: &Path, dest_dir: &Path, cancel: &CancelToken) -> Result<ExtractSummary> {
    std::fs::create_dir_all(dest_dir).map_err(|e| FsError::from_io_path(e, dest_dir))?;
    let fmt = detect(path);
    match fmt {
        ArchiveFormat::Zip => {
            let r = zip::extract_zip(path, dest_dir, cancel)?;
            Ok(ExtractSummary { files: r.files, dirs: r.dirs, bytes: r.bytes, errors: r.errors, skipped: r.skipped_encrypted })
        }
        ArchiveFormat::Tar => {
            let r = tar::extract_tar(path, dest_dir, cancel)?;
            Ok(r)
        }
        ArchiveFormat::TarGz => {
            let r = tar::extract_tar(path, dest_dir, cancel)?;
            Ok(r)
        }
        ArchiveFormat::Gzip => {
            // Strip .gz and inflate to a single file.
            let name = crate::path::file_name_of(path);
            let out_name = name.strip_suffix(".gz").or_else(|| name.strip_suffix(".GZ")).map(|s| s.to_string()).unwrap_or_else(|| format!("{name}.out"));
            let out = dest_dir.join(&out_name);
            let n = tar::gunzip_to(path, &out, cancel)?;
            Ok(ExtractSummary { files: 1, dirs: 0, bytes: n, errors: Vec::new(), skipped: 0 })
        }
        ArchiveFormat::Unsupported => Err(FsError::unsupported("Unsupported archive format")),
    }
}

/// Create a ZIP archive from `sources` (files and/or directories).
pub fn create_zip(sources: &[PathBuf], archive: &Path, cancel: &CancelToken) -> Result<u64> {
    let file = std::fs::File::create(archive).map_err(|e| FsError::from_io_path(e, archive))?;
    let mut w = zip::ZipWriter::new(file);
    for src in sources {
        cancel.check()?;
        let meta = std::fs::symlink_metadata(src).map_err(|e| FsError::from_io_path(e, src))?;
        let name = crate::path::file_name_of(src);
        if meta.is_dir() {
            zip::add_tree(&mut w, src, cancel, true)?;
        } else if meta.is_file() {
            w.add_file(&name, src, cancel)?;
        }
    }
    let out = w.finish()?;
    out.sync_all().map_err(|e| FsError::from_io_path(e, archive))?;
    Ok(std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0))
}

/// Create a TAR archive from sources. `gzip = true` writes a proper gzip
/// container: the tar stream is spilled to a temp file, then deflated and
/// finalized with CRC32 + ISIZE, exactly as RFC 1952 requires.
pub fn create_tar(sources: &[PathBuf], archive: &Path, gzip: bool, cancel: &CancelToken) -> Result<u64> {
    if gzip {
        let tmp = std::env::temp_dir().join(format!(
            ".fuss-tar-{}-{}.tar",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()
        ));
        let build = (|| -> Result<()> {
            let mut w = tar::TarWriter::new(std::fs::File::create(&tmp).map_err(|e| FsError::from_io_path(e, &tmp))?);
            write_tar_tree(&mut w, sources, cancel)?;
            w.finish()?;
            Ok(())
        })();
        if let Err(e) = build {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        let res = compress_gzip_file(&tmp, archive, cancel);
        let _ = std::fs::remove_file(&tmp);
        return res;
    }
    let file = std::fs::File::create(archive).map_err(|e| FsError::from_io_path(e, archive))?;
    let mut w = tar::TarWriter::new(file);
    write_tar_tree(&mut w, sources, cancel)?;
    let f = w.finish()?;
    f.sync_all().map_err(|e| FsError::from_io_path(e, archive))?;
    Ok(std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0))
}

/// gzip-compress `src` into `dest` (RFC 1952: header, deflate, CRC32, ISIZE).
pub fn compress_gzip_file(src: &Path, dest: &Path, cancel: &CancelToken) -> Result<u64> {
    use crate::archive::crc32::Crc32;
    use crate::archive::inflate_stream::DeflateEncoder;
    let mut input = std::fs::File::open(src).map_err(|e| FsError::from_io_path(e, src))?;
    let out = std::fs::File::create(dest).map_err(|e| FsError::from_io_path(e, dest))?;
    let mut out = std::io::BufWriter::with_capacity(128 * 1024, out);
    let mut header = [0u8; 10];
    header[0] = 0x1F;
    header[1] = 0x8B;
    header[2] = 8; // deflate
    header[3] = 0; // flags
    // bytes 4..8: mtime left at 0 for reproducible output
    header[8] = 0;
    header[9] = 0xFF; // OS: unknown
    out.write_all(&header).map_err(|e| FsError::from_io_path(e, dest))?;
    let mut crc = Crc32::new();
    let mut total = 0u64;
    {
        let mut enc = DeflateEncoder::new(&mut out, 6);
        let mut buf = vec![0u8; 128 * 1024];
        loop {
            cancel.check()?;
            use std::io::Read;
            let n = input.read(&mut buf).map_err(|e| FsError::from_io_path(e, src))?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            total += n as u64;
            enc.write_all(&buf[..n]).map_err(|e| FsError::from_io_path(e, dest))?;
            cancel.add_processed(n as u64);
        }
        enc.finish().map_err(|e| FsError::from_io_path(e, dest))?;
    }
    out.write_all(&crc.finish().to_le_bytes()).map_err(|e| FsError::from_io_path(e, dest))?;
    out.write_all(&(total as u32).to_le_bytes()).map_err(|e| FsError::from_io_path(e, dest))?;
    out.flush().map_err(|e| FsError::from_io_path(e, dest))?;
    let f = out.into_inner().map_err(|e| FsError::from_io(e.into_error(), None))?;
    f.sync_all().map_err(|e| FsError::from_io_path(e, dest))?;
    Ok(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0))
}

fn write_tar_tree<W: Write>(w: &mut tar::TarWriter<W>, sources: &[PathBuf], cancel: &CancelToken) -> Result<()> {
    for src in sources {
        cancel.check()?;
        let meta = std::fs::symlink_metadata(src).map_err(|e| FsError::from_io_path(e, src))?;
        let name = crate::path::file_name_of(src);
        if name.contains('\n') {
            return Err(FsError::invalid_arg("File names with newlines cannot be archived"));
        }
        if meta.is_dir() {
            tar::add_tree(w, src, cancel)?;
        } else if meta.is_file() {
            w.add_file(&name, src, cancel)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fuss-arch-{}-{}-{}", tag, std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn zip_create_and_extract() {
        let d = tmp("zip");
        std::fs::create_dir_all(d.join("in/sub")).unwrap();
        std::fs::write(d.join("in/a.txt"), b"aaa").unwrap();
        std::fs::write(d.join("in/sub/b.txt"), b"bbb").unwrap();
        let zip = d.join("out.zip");
        let cancel = CancelToken::new();
        create_zip(&[d.join("in")], &zip, &cancel).unwrap();
        let contents = list(&zip, &cancel).unwrap();
        assert_eq!(contents.format, "zip");
        assert!(contents.entries.iter().any(|e| e.path == "in/a.txt"));
        let out = d.join("out");
        let r = extract(&zip, &out, &cancel).unwrap();
        assert_eq!(r.files, 2);
        assert_eq!(std::fs::read_to_string(out.join("in/sub/b.txt")).unwrap(), "bbb");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn tar_create_and_extract() {
        let d = tmp("tar");
        std::fs::create_dir_all(d.join("in")).unwrap();
        std::fs::write(d.join("in/a.txt"), b"aaa").unwrap();
        let tar = d.join("out.tar");
        let cancel = CancelToken::new();
        create_tar(&[d.join("in")], &tar, false, &cancel).unwrap();
        let contents = list(&tar, &cancel).unwrap();
        assert_eq!(contents.format, "tar");
        assert!(contents.entries.iter().any(|e| e.path == "in/a.txt"));
        let out = d.join("out");
        extract(&tar, &out, &cancel).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("in/a.txt")).unwrap(), "aaa");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn tar_gz_create_and_extract() {
        let d = tmp("targz");
        std::fs::create_dir_all(d.join("in")).unwrap();
        std::fs::write(d.join("in/a.txt"), b"hello gzip").unwrap();
        let tgz = d.join("out.tar.gz");
        let cancel = CancelToken::new();
        create_tar(&[d.join("in")], &tgz, true, &cancel).unwrap();
        let out = d.join("out");
        let r = extract(&tgz, &out, &cancel).unwrap();
        assert_eq!(r.files, 1);
        assert_eq!(std::fs::read_to_string(out.join("in/a.txt")).unwrap(), "hello gzip");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn gzip_single_file_roundtrip() {
        let d = tmp("gzip");
        std::fs::write(d.join("notes.txt"), b"compress me please").unwrap();
        let gz = d.join("notes.txt.gz");
        compress_gzip_file(&d.join("notes.txt"), &gz, &CancelToken::new()).unwrap();
        let cancel = CancelToken::new();
        // Decompressed content must match.
        let restored = d.join("restored");
        tar::gunzip_to(&gz, &restored, &cancel).unwrap();
        assert_eq!(std::fs::read_to_string(&restored).unwrap(), "compress me please");
        let contents = list(&gz, &cancel).unwrap();
        assert_eq!(contents.format, "gzip");
        assert_eq!(contents.entries[0].path, "notes.txt");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn detect_formats() {
        assert_eq!(detect(Path::new("/a/b.zip")), ArchiveFormat::Zip);
        assert_eq!(detect(Path::new("/a/b.tar.gz")), ArchiveFormat::TarGz);
        assert_eq!(detect(Path::new("/a/b.tgz")), ArchiveFormat::TarGz);
        assert_eq!(detect(Path::new("/a/b.tar")), ArchiveFormat::Tar);
        assert_eq!(detect(Path::new("/a/b.gz")), ArchiveFormat::Gzip);
        assert_eq!(detect(Path::new("/a/b.7z")), ArchiveFormat::Unsupported);
        assert_eq!(detect(Path::new("/a/b.rar")), ArchiveFormat::Unsupported);
    }
}
