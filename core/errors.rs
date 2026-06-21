use std::error::Error;
use std::fmt::{Display, Formatter, Result};

#[derive(Debug)]
/// Error type returned by Hive storage and query APIs.
pub enum DbError {
    /// Wrapped low-level I/O error.
    Io(std::io::Error),
    /// Opening the target file path failed.
    FileOpenError,
    /// Seeking to a file offset failed.
    SeekError,
    /// Writing bytes to the file failed.
    WriteError,
    /// Reading bytes from the file failed.
    ReadError,
    /// File magic bytes do not match the expected Hive signature.
    InvalidHeader,
    /// File format version is not supported by this library.
    UnsupportedVersion,
    /// Query parsing, planning, or execution failed.
    QueryError(String),
}

impl Display for DbError {
    /// Formats the error as a human-readable string.
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            DbError::Io(err) => write!(f, "I/O error {}", err),
            DbError::FileOpenError => write!(f, "Failed to open database file"),
            DbError::SeekError => write!(f, "Failed to seek in database file"),
            DbError::WriteError => write!(f, "Failed to write database file"),
            DbError::ReadError => write!(f, "Failed to read database file"),
            DbError::InvalidHeader => write!(f, "Invalid database file header"),
            DbError::UnsupportedVersion => write!(f, "Unsupported database version"),
            DbError::QueryError(msg) => write!(f, "Query error: {}", msg),
        }
    }
}

impl From<std::io::Error> for DbError {
    /// Converts a standard I/O error into a `DbError::Io` variant.
    fn from(err: std::io::Error) -> Self {
        DbError::Io(err)
    }
}

impl Error for DbError {}
