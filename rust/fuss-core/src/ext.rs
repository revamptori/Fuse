//! File extensions and coarse file kinds.
//!
//! Kept in Rust because sorting by type, icon choice, preview routing and
//! storage statistics all need it, and doing it once here avoids a JNI round
//! trip per entry. The mapping is deliberately small: a handful of well-known
//! extensions, everything else falls back to `Other`.

/// Coarse classification used for icons, previews and statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum FileKind {
    Directory,
    Image,
    Video,
    Audio,
    Text,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Archive,
    Apk,
    Code,
    Font,
    #[default]
    Other,
}

impl FileKind {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => FileKind::Directory,
            1 => FileKind::Image,
            2 => FileKind::Video,
            3 => FileKind::Audio,
            4 => FileKind::Text,
            5 => FileKind::Pdf,
            6 => FileKind::Document,
            7 => FileKind::Spreadsheet,
            8 => FileKind::Presentation,
            9 => FileKind::Archive,
            10 => FileKind::Apk,
            11 => FileKind::Code,
            12 => FileKind::Font,
            _ => FileKind::Other,
        }
    }
}

/// Lowercased extension of a file name, without the dot. `None` for dotfiles
/// without a second dot (`.bashrc`) and names without a dot.
pub fn extension(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 >= name.len() {
        return None;
    }
    // "archive.tar.gz" -> "gz"; that's what MIME detection does too.
    Some(name[dot + 1..].to_ascii_lowercase())
}

/// Classify a file name (does not touch the filesystem).
pub fn classify_name(name: &str, is_dir: bool) -> FileKind {
    if is_dir {
        return FileKind::Directory;
    }
    let ext = match extension(name) {
        Some(e) => e,
        None => return FileKind::Other,
    };
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "heif" | "avif" | "tif"
        | "tiff" | "svg" | "ico" | "raw" | "dng" | "cr2" | "nef" | "arw" | "orf" => FileKind::Image,
        "mp4" | "m4v" | "mkv" | "webm" | "avi" | "mov" | "3gp" | "wmv" | "flv" | "mpg" | "mpeg"
        | "ts" | "m2ts" => FileKind::Video,
        "mp3" | "m4a" | "aac" | "ogg" | "oga" | "opus" | "flac" | "wav" | "wma" | "amr" | "mid"
        | "midi" | "aiff" | "aif" | "ape" | "mka" => FileKind::Audio,
        "txt" | "log" | "md" | "markdown" | "nfo" | "srt" | "vtt" | "ass" | "rtf" | "ini"
        | "conf" | "cfg" | "properties" => FileKind::Text,
        "pdf" => FileKind::Pdf,
        "doc" | "docx" | "odt" | "pages" | "epub" | "mobi" | "azw3" => FileKind::Document,
        "xls" | "xlsx" | "ods" | "csv" | "tsv" | "numbers" => FileKind::Spreadsheet,
        "ppt" | "pptx" | "odp" | "key" => FileKind::Presentation,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "tbz2" | "xz" | "txz" | "zst"
        | "lz4" | "lzma" | "jar" | "war" | "iso" | "cab" | "zipx" => FileKind::Archive,
        "apk" | "apks" | "xapk" | "aab" => FileKind::Apk,
        "rs" | "kt" | "kts" | "java" | "c" | "h" | "cc" | "cpp" | "hpp" | "py" | "js" | "tsx"
        | "jsx" | "go" | "rb" | "php" | "swift" | "sh" | "bash" | "zsh" | "fish" | "lua" | "pl"
        | "sql" | "json" | "yaml" | "yml" | "xml" | "html" | "htm" | "css" | "scss" | "toml"
        | "gradle" | "makefile" | "cmake" | "asm" | "s" | "dart" | "ex" | "exs" | "erl" | "hs"
        | "ml" | "scala" | "clj" | "vim" => FileKind::Code,
        "ttf" | "otf" | "woff" | "woff2" => FileKind::Font,
        _ => FileKind::Other,
    }
}

/// MIME type for common extensions. Empty string means "unknown".
pub fn mime_type(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "inode/directory";
    }
    let ext = match extension(name) {
        Some(e) => e,
        None => return "",
    };
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "heic" | "heif" => "image/heic",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "3gp" => "video/3gpp",
        "ts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "amr" => "audio/amr",
        "mid" | "midi" => "audio/midi",
        "txt" | "log" | "nfo" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "odt" => "application/vnd.oasis.opendocument.text",
        "epub" => "application/epub+zip",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        "rar" => "application/vnd.rar",
        "7z" => "application/x-7z-compressed",
        "tar" => "application/x-tar",
        "gz" | "tgz" => "application/gzip",
        "bz2" => "application/x-bzip2",
        "xz" => "application/x-xz",
        "zst" => "application/zstd",
        "apk" | "apks" | "xapk" => "application/vnd.android.package-archive",
        "aab" => "application/x-authorware-bin",
        "rtf" => "application/rtf",
        "rs" => "text/x-rust",
        "kt" | "kts" => "text/x-kotlin",
        "java" => "text/x-java",
        "py" => "text/x-python",
        "js" => "text/javascript",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert_eq!(extension("a.TXT"), Some("txt".into()));
        assert_eq!(extension("archive.tar.gz"), Some("gz".into()));
        assert_eq!(extension("noext"), None);
        assert_eq!(extension(".bashrc"), None);
        assert_eq!(extension("trailing."), None);
        assert_eq!(extension(".hidden.png"), Some("png".into()));
    }

    #[test]
    fn classification() {
        assert_eq!(classify_name("x", true), FileKind::Directory);
        assert_eq!(classify_name("a.JPG", false), FileKind::Image);
        assert_eq!(classify_name("a.mp4", false), FileKind::Video);
        assert_eq!(classify_name("a.flac", false), FileKind::Audio);
        assert_eq!(classify_name("a.pdf", false), FileKind::Pdf);
        assert_eq!(classify_name("a.zip", false), FileKind::Archive);
        assert_eq!(classify_name("a.apk", false), FileKind::Apk);
        assert_eq!(classify_name("a.unknownext", false), FileKind::Other);
        assert_eq!(mime_type("a.jpg", false), "image/jpeg");
        assert_eq!(mime_type("dir", true), "inode/directory");
        assert_eq!(mime_type("a.unknownext", false), "");
    }
}
