//! Typed error shared by every public function (`ApiError` in the contract).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NotFound,
    NotUtf8,
    Conflict,
    Io,
    Invalid,
}

/// Stable reason of an error, for the callers that react to it (the
/// message is English text for a human and may be reworded at any time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The file is larger than what `read_file` accepts.
    TooLarge,
    /// A network location the page may not reach.
    NetworkPath,
    /// A path, a vault or a folder the page has no right to.
    NotAllowed,
    /// `save_file` on a file that can only be read (extension, `.obsidian`,
    /// `.canvas`).
    ReadOnly,
    /// The file to save no longer exists on disk.
    Gone,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::TooLarge => "too_large",
            ErrorCode::NetworkPath => "network_path",
            ErrorCode::NotAllowed => "not_allowed",
            ErrorCode::ReadOnly => "read_only",
            ErrorCode::Gone => "gone",
        }
    }
}

/// `{ kind, message, code? }`, serialized exactly as the contract's `ApiError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub kind: ErrorKind,
    pub message: String,
    /// Present for the errors a caller is expected to tell apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
}

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), code: None }
    }
    /// The same error with a stable reason attached.
    pub fn with_code(mut self, code: ErrorCode) -> Self {
        self.code = Some(code);
        self
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }
    pub fn not_utf8(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotUtf8, message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, message)
    }
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, message)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }

    /// Maps an I/O error, prefixing the message with what was being done.
    pub fn from_io(err: &io::Error, context: impl fmt::Display) -> Self {
        let kind = if err.kind() == io::ErrorKind::NotFound {
            ErrorKind::NotFound
        } else {
            ErrorKind::Io
        };
        Self::new(kind, format!("{context}: {err}"))
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            ErrorKind::NotFound => "not_found",
            ErrorKind::NotUtf8 => "not_utf8",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Io => "io",
            ErrorKind::Invalid => "invalid",
        };
        write!(f, "{kind}: {}", self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<io::Error> for ApiError {
    fn from(err: io::Error) -> Self {
        Self::from_io(&err, "I/O error")
    }
}
