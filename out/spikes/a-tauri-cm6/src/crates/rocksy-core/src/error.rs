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

/// `{ kind, message }`, serialized exactly as the contract's `ApiError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
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
