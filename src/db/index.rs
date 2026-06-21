use crate::errors::DbError;
use crate::store::edge::record::EdgeRecord;
use crate::store::edge::store::EdgeStore;
use crate::store::node::record::NodeRecord;
use crate::store::node::store::NodeStore;
use crate::store::property::record::PropertyRecord;
use crate::store::property::store::PropertyStore;
use crate::store::string_store::StringStore;
use crate::types::{DELETED, EdgeId, NIL_ID, NodeId};
use crate::value::{BOOLEAN, FLOAT, INTEGER, LONG_STRING, NULL, STRING, Value};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

const INDEX_MAGIC: [u8; 8] = *b"HIVEIDX1";
const INDEX_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IndexValue {
    Null,
    Integer(i64),
    FloatBits(u64),
    Boolean(bool),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PropertyIndexKey {
    pub key_hash: u64,
    pub value: IndexValue,
}

#[derive(Debug, Default)]
pub struct IndexStore {
    label_index: HashMap<u32, Vec<NodeId>>,
    property_index: HashMap<PropertyIndexKey, Vec<NodeId>>,
    edge_type_index: HashMap<u32, Vec<EdgeId>>,
}

impl IndexStore {
    pub fn load_or_rebuild(
        index_path: &Path,
        node_store: &mut NodeStore,
        edge_store: &mut EdgeStore,
        property_store: &mut PropertyStore,
        string_store: &mut StringStore,
    ) -> Result<Self, DbError> {
        if index_path.exists() {
            if let Ok(indexes) = Self::load(index_path) {
                if indexes.validate_posting_lists() {
                    return Ok(indexes);
                }
            }
        }

        let indexes = Self::rebuild(node_store, edge_store, property_store, string_store)?;
        indexes.save(index_path)?;
        Ok(indexes)
    }

    pub fn load(index_path: &Path) -> Result<Self, DbError> {
        let mut file = OpenOptions::new()
            .read(true)
            .open(index_path)
            .map_err(|_| DbError::FileOpenError)?;

        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)
            .map_err(|_| DbError::ReadError)?;
        if magic != INDEX_MAGIC {
            return Err(DbError::InvalidHeader);
        }

        let version = read_u32(&mut file)?;
        if version != INDEX_VERSION {
            return Err(DbError::UnsupportedVersion);
        }

        let mut indexes = Self::default();

        let label_bucket_count = read_u64(&mut file)?;
        for _ in 0..label_bucket_count {
            let label_id = read_u32(&mut file)?;
            let posting_len = read_u64(&mut file)?;
            let mut posting = Vec::with_capacity(posting_len as usize);
            for _ in 0..posting_len {
                posting.push(read_u64(&mut file)?);
            }
            indexes.label_index.insert(label_id, posting);
        }

        let property_bucket_count = read_u64(&mut file)?;
        for _ in 0..property_bucket_count {
            let key_hash = read_u64(&mut file)?;
            let value = read_index_value(&mut file)?;
            let posting_len = read_u64(&mut file)?;
            let mut posting = Vec::with_capacity(posting_len as usize);
            for _ in 0..posting_len {
                posting.push(read_u64(&mut file)?);
            }
            indexes
                .property_index
                .insert(PropertyIndexKey { key_hash, value }, posting);
        }

        let edge_bucket_count = read_u64(&mut file)?;
        for _ in 0..edge_bucket_count {
            let edge_type_id = read_u32(&mut file)?;
            let posting_len = read_u64(&mut file)?;
            let mut posting = Vec::with_capacity(posting_len as usize);
            for _ in 0..posting_len {
                posting.push(read_u64(&mut file)?);
            }
            indexes.edge_type_index.insert(edge_type_id, posting);
        }

        if !indexes.validate_posting_lists() {
            return Err(DbError::InvalidHeader);
        }

        Ok(indexes)
    }

    pub fn save(&self, index_path: &Path) -> Result<(), DbError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(index_path)
            .map_err(|_| DbError::FileOpenError)?;
        let mut file = BufWriter::new(file);

        file.write_all(&INDEX_MAGIC)
            .map_err(|_| DbError::WriteError)?;
        write_u32(&mut file, INDEX_VERSION)?;

        write_u64(&mut file, self.label_index.len() as u64)?;
        for (label_id, node_ids) in &self.label_index {
            write_u32(&mut file, *label_id)?;
            write_u64(&mut file, node_ids.len() as u64)?;
            for node_id in node_ids {
                write_u64(&mut file, *node_id)?;
            }
        } 

        write_u64(&mut file, self.property_index.len() as u64)?;
        for (key, node_ids) in &self.property_index {
            write_u64(&mut file, key.key_hash)?;
            write_index_value(&mut file, &key.value)?;
            write_u64(&mut file, node_ids.len() as u64)?;
            for node_id in node_ids {
                write_u64(&mut file, *node_id)?;
            }
        }

        write_u64(&mut file, self.edge_type_index.len() as u64)?;
        for (edge_type_id, edge_ids) in &self.edge_type_index {
            write_u32(&mut file, *edge_type_id)?;
            write_u64(&mut file, edge_ids.len() as u64)?;
            for edge_id in edge_ids {
                write_u64(&mut file, *edge_id)?;
            }
        }

        file.flush().map_err(|_| DbError::WriteError)?;
        file.get_ref().sync_all().map_err(DbError::Io)?;
        Ok(())
    }

    pub fn rebuild(
        node_store: &mut NodeStore,
        edge_store: &mut EdgeStore,
        property_store: &mut PropertyStore,
        string_store: &mut StringStore,
    ) -> Result<Self, DbError> {
        let mut indexes = Self::default();

        let node_count = node_store.count()?;
        for node_id in 0..node_count {
            let node = node_store.read(node_id)?;
            if Self::is_deleted(node.flags) {
                continue;
            }
            indexes.index_node_record(node_id, &node, property_store, string_store)?;
        }

        let edge_count = edge_store.count()?;
        for edge_id in 0..edge_count {
            let edge = edge_store.read(edge_id)?;
            if Self::is_deleted(edge.flags) {
                continue;
            }
            indexes.insert_edge(edge_id, &edge);
        }

        Ok(indexes)
    }

    pub fn lookup_nodes_by_label_id(&self, label_id: u32) -> Vec<NodeId> {
        self.label_index.get(&label_id).cloned().unwrap_or_default()
    }

    pub fn lookup_nodes_by_property(&self, key_hash: u64, value: &Value) -> Vec<NodeId> {
        let key = PropertyIndexKey {
            key_hash,
            value: Self::normalize_value(value),
        };

        self.property_index.get(&key).cloned().unwrap_or_default()
    }

    pub fn lookup_edges_by_type_id(&self, edge_type_id: u32) -> Vec<EdgeId> {
        self.edge_type_index
            .get(&edge_type_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn insert_node(
        &mut self,
        node_id: NodeId,
        node: &NodeRecord,
        property_store: &mut PropertyStore,
        string_store: &mut StringStore,
    ) -> Result<(), DbError> {
        self.index_node_record(node_id, node, property_store, string_store)
    }

    pub fn remove_node(
        &mut self,
        node_id: NodeId,
        node: &NodeRecord,
        property_store: &mut PropertyStore,
        string_store: &mut StringStore,
    ) -> Result<(), DbError> {
        if let Some(ids) = self.label_index.get_mut(&node.label_id) {
            remove_id_from_posting(ids, node_id);
        }
        if self
            .label_index
            .get(&node.label_id)
            .is_some_and(|ids| ids.is_empty())
        {
            self.label_index.remove(&node.label_id);
        }

        let mut keys = Vec::new();
        Self::walk_node_properties(node.first_property, property_store, string_store, |prop| {
            keys.push(PropertyIndexKey {
                key_hash: prop.key_hash,
                value: prop.value,
            });

            Ok(())
        })?;

        for key in keys {
            if let Some(ids) = self.property_index.get_mut(&key) {
                remove_id_from_posting(ids, node_id);
            }

            if self
                .property_index
                .get(&key)
                .is_some_and(|ids| ids.is_empty())
            {
                self.property_index.remove(&key);
            }
        }

        Ok(())
    }

    pub fn upsert_node_property(
        &mut self,
        node_id: NodeId,
        key_hash: u64,
        old_value: Option<&Value>,
        new_value: &Value,
    ) {
        if let Some(old) = old_value {
            let old_key = PropertyIndexKey {
                key_hash,
                value: Self::normalize_value(old),
            };
            if let Some(ids) = self.property_index.get_mut(&old_key) {
                remove_id_from_posting(ids, node_id);
            }
            if self
                .property_index
                .get(&old_key)
                .is_some_and(|ids| ids.is_empty())
            {
                self.property_index.remove(&old_key);
            }
        }

        let new_key = PropertyIndexKey {
            key_hash,
            value: Self::normalize_value(new_value),
        };
        append_unique(self.property_index.entry(new_key).or_default(), node_id);
    }

    pub fn insert_edge(&mut self, edge_id: EdgeId, edge: &EdgeRecord) {
        append_unique(
            self.edge_type_index.entry(edge.label_id).or_default(),
            edge_id,
        );
    }

    pub fn remove_edge(&mut self, edge_id: EdgeId, edge: &EdgeRecord) {
        if let Some(ids) = self.edge_type_index.get_mut(&edge.label_id) {
            remove_id_from_posting(ids, edge_id);
        }
        if self
            .edge_type_index
            .get(&edge.label_id)
            .is_some_and(|ids| ids.is_empty())
        {
            self.edge_type_index.remove(&edge.label_id);
        }
    }

    fn index_node_record(
        &mut self,
        node_id: NodeId,
        node: &NodeRecord,
        property_store: &mut PropertyStore,
        string_store: &mut StringStore,
    ) -> Result<(), DbError> {
        append_unique(self.label_index.entry(node.label_id).or_default(), node_id);

        Self::walk_node_properties(node.first_property, property_store, string_store, |prop| {
            let key = PropertyIndexKey {
                key_hash: prop.key_hash,
                value: prop.value,
            };
            append_unique(self.property_index.entry(key).or_default(), node_id);
            Ok(())
        })
    }

    fn walk_node_properties<F>(
        first_property: u64,
        property_store: &mut PropertyStore,
        string_store: &mut StringStore,
        mut visit: F,
    ) -> Result<(), DbError>
    where
        F: FnMut(NormalizedProperty) -> Result<(), DbError>,
    {
        let mut prop_id = first_property;
        while prop_id != NIL_ID {
            let prop = property_store.read(prop_id)?;
            let value = Self::read_property_value(&prop, string_store)?;
            visit(NormalizedProperty {
                key_hash: prop.key_hash,
                value,
            })?;
            prop_id = prop.next_property;
        }
        Ok(())
    }

    fn read_property_value(
        prop: &PropertyRecord,
        string_store: &mut StringStore,
    ) -> Result<IndexValue, DbError> {
        let value = match prop.value_type {
            NULL | INTEGER | FLOAT | BOOLEAN | STRING => {
                Value::from_bytes(prop.value_type, prop.value_inline)
            }
            LONG_STRING => {
                let offset = u64::from_le_bytes(prop.value_inline[..8].try_into().unwrap());
                Value::String(string_store.read(offset)?)
            }
            _ => return Err(DbError::ReadError),
        };

        Ok(Self::normalize_value(&value))
    }

    fn normalize_value(value: &Value) -> IndexValue {
        match value {
            Value::Null => IndexValue::Null,
            Value::Integer(n) => IndexValue::Integer(*n),
            Value::Float(f) => IndexValue::FloatBits(f.to_bits()),
            Value::Boolean(b) => IndexValue::Boolean(*b),
            Value::String(s) => IndexValue::String(s.clone()),
        }
    }

    fn is_deleted(flags: u32) -> bool {
        (flags & DELETED) != 0
    }

    fn validate_posting_lists(&self) -> bool {
        self.label_index.values().all(|ids| has_no_duplicates(ids))
            && self
                .property_index
                .values()
                .all(|ids| has_no_duplicates(ids))
            && self
                .edge_type_index
                .values()
                .all(|ids| has_no_duplicates(ids))
    }
}

struct NormalizedProperty {
    key_hash: u64,
    value: IndexValue,
}

fn append_unique(ids: &mut Vec<u64>, id: u64) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

fn remove_id_from_posting(ids: &mut Vec<u64>, id: u64) {
    ids.retain(|existing| *existing != id);
}

fn has_no_duplicates(ids: &[u64]) -> bool {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.iter().all(|id| seen.insert(*id))
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, DbError> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, DbError> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(u64::from_le_bytes(buf))
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> Result<(), DbError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

fn write_u64<W: Write>(writer: &mut W, value: u64) -> Result<(), DbError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

fn read_index_value<R: Read>(reader: &mut R) -> Result<IndexValue, DbError> {
    let mut tag = [0u8; 1];
    reader
        .read_exact(&mut tag)
        .map_err(|_| DbError::ReadError)?;

    match tag[0] {
        0 => Ok(IndexValue::Null),
        1 => {
            let mut buf = [0u8; 8];
            reader
                .read_exact(&mut buf)
                .map_err(|_| DbError::ReadError)?;
            Ok(IndexValue::Integer(i64::from_le_bytes(buf)))
        }
        2 => {
            let mut buf = [0u8; 8];
            reader
                .read_exact(&mut buf)
                .map_err(|_| DbError::ReadError)?;
            Ok(IndexValue::FloatBits(u64::from_le_bytes(buf)))
        }
        3 => {
            let mut buf = [0u8; 1];
            reader
                .read_exact(&mut buf)
                .map_err(|_| DbError::ReadError)?;
            Ok(IndexValue::Boolean(buf[0] != 0))
        }
        4 => {
            let len = read_u64(reader)?;
            let mut buf = vec![0u8; len as usize];
            reader
                .read_exact(&mut buf)
                .map_err(|_| DbError::ReadError)?;
            let value = String::from_utf8(buf).map_err(|_| DbError::ReadError)?;
            Ok(IndexValue::String(value))
        }
        _ => Err(DbError::InvalidHeader),
    }
}

fn write_index_value<W: Write>(writer: &mut W, value: &IndexValue) -> Result<(), DbError> {
    match value {
        IndexValue::Null => writer.write_all(&[0]).map_err(|_| DbError::WriteError),
        IndexValue::Integer(n) => {
            writer.write_all(&[1]).map_err(|_| DbError::WriteError)?;
            writer
                .write_all(&n.to_le_bytes())
                .map_err(|_| DbError::WriteError)
        }
        IndexValue::FloatBits(bits) => {
            writer.write_all(&[2]).map_err(|_| DbError::WriteError)?;
            writer
                .write_all(&bits.to_le_bytes())
                .map_err(|_| DbError::WriteError)
        }
        IndexValue::Boolean(b) => {
            writer.write_all(&[3]).map_err(|_| DbError::WriteError)?;
            writer
                .write_all(&[*b as u8])
                .map_err(|_| DbError::WriteError)
        }
        IndexValue::String(s) => {
            writer.write_all(&[4]).map_err(|_| DbError::WriteError)?;
            write_u64(writer, s.len() as u64)?;
            writer
                .write_all(s.as_bytes())
                .map_err(|_| DbError::WriteError)
        }
    }
}
