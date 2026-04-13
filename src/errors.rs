pub enum DbError {
    Io(std::io::Error),
    FileOpenError,
    SeekError,
    WriteError,
    ReadError,
}
