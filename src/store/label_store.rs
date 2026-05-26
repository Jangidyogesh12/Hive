use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
};

use crate::errors::DbError;

pub struct LabelStore {
    pub file: File,
    pub label_to_id: HashMap<String, u32>,
    pub id_to_label: HashMap<u32, String>,
    pub next_id: u32,
}

impl LabelStore {
    /// Opens the label store file at `path`, creating it if it does not exist.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .append(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        Ok(Self {
            file,
            id_to_label: HashMap::new(),
            label_to_id: HashMap::new(),
            next_id: 1,
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

        self.file
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        self.file
            .write_all(&id.to_le_bytes())
            .map_err(|_| DbError::WriteError)?;

        self.file
            .write_all(&(label.len() as u32).to_le_bytes())
            .map_err(|_| DbError::WriteError)?;

        self.file
            .write_all(label.as_bytes())
            .map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        self.label_to_id.insert(label.to_string(), id);
        self.id_to_label.insert(id, label.to_string());

        return Ok(id);
    }

    /// Looks up a label string by its numeric ID.
    /// Returns `None` if the ID is not registered.
    pub fn get_by_id(&self, id: u32) -> Option<&str> {
        return self.id_to_label.get(&id).map(|s| s.as_str());
    }
}
