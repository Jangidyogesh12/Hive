use std::error::Error;
use std::fmt::{Display, Formatter, Result};

#[derive(Debug)]

pub enum DbError {
    Io(std::io::Error),
    FileOpenError,
    SeekError,
    WriteError,
    ReadError,
}

impl Display for DbError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            DbError::Io(err) => write!(f, "I/O error {}", err),
            DbError::FileOpenError => write!(f, "Failed to open database file"),
            DbError::SeekError => write!(f, "Failed to seek in database file"),
            DbError::WriteError => write!(f, "Failed to write database file"),
            DbError::ReadError => write!(f, "Failed to read database file"),
        }
    }
}

impl Error for DbError {}
