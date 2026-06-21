use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::errors::DbError;

pub struct LabelStore {
    writer: BufWriter<File>,
    pub label_to_id: HashMap<String, u32>,
    pub id_to_label: HashMap<u32, String>,
    pub next_id: u32,
}

impl LabelStore {
    /// Opens the label store file at `path`, creating it if it does not exist.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let mut reader = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;
        let writer_file = OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        let mut id_to_label = HashMap::new();
        let mut label_to_id = HashMap::new();
        let mut next_id = 1;

        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| DbError::SeekError)?;

        loop {
            let mut id_buf = [0u8; 4];
            match reader.read_exact(&mut id_buf) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(_) => return Err(DbError::ReadError),
            }

            let id = u32::from_le_bytes(id_buf);

            let mut len_buf = [0u8; 4];
            reader
                .read_exact(&mut len_buf)
                .map_err(|_| DbError::ReadError)?;
            let len = u32::from_le_bytes(len_buf) as usize;

            let mut label_buf = vec![0u8; len];
            reader
                .read_exact(&mut label_buf)
                .map_err(|_| DbError::ReadError)?;

            let label = String::from_utf8(label_buf).map_err(|_| DbError::ReadError)?;
            next_id = next_id.max(id + 1);
            label_to_id.insert(label.clone(), id);
            id_to_label.insert(id, label);
        }

        Ok(Self {
            writer: BufWriter::new(writer_file),
            id_to_label,
            label_to_id,
            next_id,
        })
    }

    /// Resolves a label string to its numeric ID, creating a new entry if
    /// the label has not been seen before.
    pub fn get_or_create(&mut self, label: &str) -> Result<u32, DbError> {
        if let Some(&id) = self.label_to_id.get(label) {
            return Ok(id);
        }

        let id = self.next_id;
        self.next_id += 1;

        self.writer
            .write_all(&id.to_le_bytes())
            .map_err(|_| DbError::WriteError)?;

        self.writer
            .write_all(&(label.len() as u32).to_le_bytes())
            .map_err(|_| DbError::WriteError)?;

        self.writer
            .write_all(label.as_bytes())
            .map_err(|_| DbError::WriteError)?;

        self.label_to_id.insert(label.to_string(), id);
        self.id_to_label.insert(id, label.to_string());

        return Ok(id);
    }

    /// Looks up a label string by its numeric ID.
    /// Returns `None` if the ID is not registered.
    pub fn get_by_id(&self, id: u32) -> Option<&str> {
        return self.id_to_label.get(&id).map(|s| s.as_str());
    }

    /// Looks up a numeric label ID by label string.
    pub fn get_id(&self, label: &str) -> Option<u32> {
        self.label_to_id.get(label).copied()
    }

    pub fn flush(&mut self) -> Result<(), DbError> {
        self.writer.flush().map_err(|_| DbError::WriteError)
    }

    pub fn sync(&mut self) -> Result<(), DbError> {
        self.flush()?;
        self.writer.get_ref().sync_all().map_err(DbError::Io)
    }
}
