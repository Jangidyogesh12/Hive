use std::error::Error;
use std::fmt::{Display, Formatter, Result};

#[derive(Debug)]
pub enum DbError {
    Io(std::io::Error), // Wrapped low-level I/O error.
    FileOpenError,      // Opening the target file path failed.
    SeekError,          // Seeking to a file offset failed.
    WriteError,         // Writing bytes to the file failed.
    ReadError,          // Reading bytes from the file failed.
    InvalidHeader,      // File magic bytes do not match expected signature.
    UnsupportedVersion, // File format version is not supported by this library.
    QueryError(String), //
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
