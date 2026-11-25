//! Error type for all engine operations.
//!
//! Errors are converted into a stable machine-readable code plus a human
//! message at the JNI boundary, so the Kotlin side never needs to parse
//! Rust debug output.

use std::fmt;
use std::io;

/// Machine-readable error codes. Keep in sync with `FsErrors` in Kotlin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    NotFound,
    NotADirectory,
    AlreadyExists,
    PermissionDenied,
    NotEmpty,
    InvalidName,
    InvalidArgument,
    CrossDevice,
    NoSpace,
    ReadOnly,
    Busy,
    Cancelled,
    Unsupported,
    Io,
    CorruptArchive,
    InvalidPath,
    TooLarge,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::NotFound => "not_found",
            ErrorCode::NotADirectory => "not_a_directory",
            ErrorCode::AlreadyExists => "already_exists",
            ErrorCode::PermissionDenied => "permission_denied",
            ErrorCode::NotEmpty => "not_empty",
            ErrorCode::InvalidName => "invalid_name",
            ErrorCode::InvalidArgument => "invalid_argument",
            ErrorCode::CrossDevice => "cross_device",
            ErrorCode::NoSpace => "no_space",
            ErrorCode::ReadOnly => "read_only",
            ErrorCode::Busy => "busy",
            ErrorCode::Cancelled => "cancelled",
            ErrorCode::Unsupported => "unsupported",
            ErrorCode::Io => "io",
            ErrorCode::CorruptArchive => "corrupt_archive",
            ErrorCode::InvalidPath => "invalid_path",
            ErrorCode::TooLarge => "too_large",
        }
    }
}

#[derive(Debug)]
pub struct FsError {
    pub code: ErrorCode,
    pub message: String,
    /// Path involved, when meaningful. Used for UI messages only.
    pub path: Option<String>,
}

impl FsError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        FsError { code, message: message.into(), path: None }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn invalid_arg(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "Operation cancelled")
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, message)
    }

    pub fn corrupt(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::CorruptArchive, message)
    }

    /// Classify an `io::Error` into a stable code.
    pub fn from_io(err: io::Error, path: Option<&std::path::Path>) -> Self {
        let code = match err.kind() {
            io::ErrorKind::NotFound => ErrorCode::NotFound,
            io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            io::ErrorKind::AlreadyExists => ErrorCode::AlreadyExists,
            io::ErrorKind::InvalidInput => ErrorCode::InvalidName,
            io::ErrorKind::InvalidData => ErrorCode::Io,
            io::ErrorKind::WriteZero => ErrorCode::Io,
            io::ErrorKind::Interrupted => ErrorCode::Io,
            io::ErrorKind::UnexpectedEof => ErrorCode::Io,
            io::ErrorKind::Unsupported => ErrorCode::Unsupported,
            io::ErrorKind::OutOfMemory => ErrorCode::NoSpace,
            io::ErrorKind::DirectoryNotEmpty => ErrorCode::NotEmpty,
            io::ErrorKind::StorageFull => ErrorCode::NoSpace,
            io::ErrorKind::ReadOnlyFilesystem => ErrorCode::ReadOnly,
            io::ErrorKind::IsADirectory => ErrorCode::NotADirectory,
            io::ErrorKind::NotADirectory => ErrorCode::NotADirectory,
            io::ErrorKind::CrossesDevices => ErrorCode::CrossDevice,
            _ => ErrorCode::Io,
        };
        let code = match err.raw_os_error() {
            Some(libc_like) => match libc_like {
                1 => ErrorCode::PermissionDenied,      // EPERM
                2 => ErrorCode::NotFound,              // ENOENT
                13 => ErrorCode::PermissionDenied,     // EACCES
                17 => ErrorCode::AlreadyExists,        // EEXIST
                18 => ErrorCode::CrossDevice,          // EXDEV
                20 => ErrorCode::NotADirectory,        // ENOTDIR
                21 => ErrorCode::NotADirectory,        // EISDIR
                28 => ErrorCode::NoSpace,              // ENOSPC
                30 => ErrorCode::ReadOnly,             // EROFS
                36 => ErrorCode::InvalidName,          // ENAMETOOLONG
                39 => ErrorCode::NotEmpty,             // ENOTEMPTY
                122 => ErrorCode::NoSpace,             // EDQUOT
                _ => code,
            },
            None => code,
        };
        let mut e = FsError {
            code,
            message: err.to_string(),
            path: path.map(|p| p.to_string_lossy().into_owned()),
        };
        if e.message.is_empty() {
            e.message = format!("{:?}", err.kind());
        }
        e
    }

    pub fn from_io_path(err: io::Error, path: &std::path::Path) -> Self {
        Self::from_io(err, Some(path))
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(p) => write!(f, "{} ({}): {}", self.code.as_str(), p, self.message),
            None => write!(f, "{}: {}", self.code.as_str(), self.message),
        }
    }
}

impl std::error::Error for FsError {}

pub type Result<T> = std::result::Result<T, FsError>;
