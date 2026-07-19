use crate::errors::DbError;
use crate::storage::page::format::{META_PAGE_ID, PageType};
use crate::storage::page::layout;
use crate::storage::page::record::{EdgeRecord, NodeRecord};
use crate::storage::pager::Pager;
use crate::types::{EdgeId, NodeId, pack_record_id, unpack_record_id};
use crate::wal::Wal;
use crate::wal::recovery::{self, RecoveryOutcome};
use std::{fs, path::Path};

pub struct HiveDb {
    pub(crate) pager: Pager,
    #[allow(dead_code)]
    pub(crate) wal: Wal,
}

impl HiveDb {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        fs::create_dir_all(path).map_err(|_| DbError::FileOpenError)?;

        let wal_path = path.join("wal.hive");
        let mut pager = Pager::open(path, 128, 128)?;
        let wal = Wal::open(&wal_path)?;

        let recovery_outcome = recovery::recover(path, &mut pager)?;

        match recovery_outcome {
            RecoveryOutcome::Clean => {}
            RecoveryOutcome::Recovered {
                committed_tx_count,
                pages_redone,
            } => {
                eprintln!(
                    "Recovery: {} transactions replayed, {} pages redone",
                    committed_tx_count, pages_redone
                );
            }
        }

        Ok(Self { pager, wal })
    }

    /// Creates a new node and returns its packed NodeId.
    pub fn create_node(&mut self) -> Result<NodeId, DbError> {
        let node_id_counter = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.node_count + 1
        };

        let page_id = self.find_or_alloc_data_node_page()?;

        let mut page_buf = self.pager.get_page_mut(page_id)?;
        let record = NodeRecord::new(node_id_counter);
        let mut record_buf = vec![0u8; record.encoded_size()];
        record.to_bytes(&mut record_buf)?;
        let slot = layout::insert_record(&mut page_buf, &record_buf)?;

        self.update_meta_node_count(node_id_counter)?;

        Ok(pack_record_id(page_id, slot.0))
    }

    /// Reads a node by its packed NodeId.
    pub fn get_node(&mut self, node_id: NodeId) -> Result<NodeRecord, DbError> {
        let (page_id, slot_id) = unpack_record_id(node_id);

        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }

        let page_buf = self.pager.get_page(page_id)?;
        let record_bytes =
            layout::read_record_bytes(page_buf, slot_id).ok_or(DbError::ReadError)?;

        NodeRecord::from_bytes(record_bytes)
    }

    /// Creates an edge from src to dst and returns its packed EdgeId.
    pub fn create_edge(&mut self, src_id: NodeId, dst_id: NodeId) -> Result<EdgeId, DbError> {
        let edge_id_counter = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.edge_count + 1
        };

        let page_id = self.find_or_alloc_data_edge_page()?;

        let mut page_buf = self.pager.get_page_mut(page_id)?;
        let mut edge = EdgeRecord::new(edge_id_counter);
        edge.src = src_id;
        edge.dst = dst_id;

        let mut record_buf = vec![0u8; edge.encoded_size()];
        edge.to_bytes(&mut record_buf)?;
        let slot = layout::insert_record(&mut page_buf, &record_buf)?;

        self.update_meta_edge_count(edge_id_counter)?;

        Ok(pack_record_id(page_id, slot.0))
    }

    /// Reads an edge by its packed EdgeId.
    pub fn get_edge(&mut self, edge_id: EdgeId) -> Result<EdgeRecord, DbError> {
        let (page_id, slot_id) = unpack_record_id(edge_id);

        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }

        let page_buf = self.pager.get_page(page_id)?;
        let record_bytes =
            layout::read_record_bytes(page_buf, slot_id).ok_or(DbError::ReadError)?;

        EdgeRecord::from_bytes(record_bytes)
    }

    /// Finds an existing DataNode page with free space, or allocates a new one.
    fn find_or_alloc_data_node_page(&mut self) -> Result<u32, DbError> {
        let root_page = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_data_page
        };

        if root_page != 0 {
            let page_buf = self.pager.get_page(root_page)?;
            if layout::get_free_space(page_buf) > 0 {
                return Ok(root_page);
            }
        }

        let new_page = self.pager.allocate_page()?;
        let mut page_buf = self.pager.get_page_mut(new_page)?;
        layout::init_regular_page(&mut page_buf, PageType::DataNode);

        self.update_meta_root_data_page(new_page)?;

        Ok(new_page)
    }

    /// Finds an existing DataEdge page with free space, or allocates a new one.
    fn find_or_alloc_data_edge_page(&mut self) -> Result<u32, DbError> {
        let root_page = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_edge_page
        };

        if root_page != 0 {
            let page_buf = self.pager.get_page(root_page)?;
            if layout::get_free_space(page_buf) > 0 {
                return Ok(root_page);
            }
        }

        let new_page = self.pager.allocate_page()?;
        let mut page_buf = self.pager.get_page_mut(new_page)?;
        layout::init_regular_page(&mut page_buf, PageType::DataEdge);

        self.update_meta_root_edge_page(new_page)?;

        Ok(new_page)
    }

    /// Updates the node count in the meta header.
    fn update_meta_node_count(&mut self, count: u64) -> Result<(), DbError> {
        let mut meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(&meta_page);
        meta.node_count = count;
        layout::write_meta_header(&mut meta_page, &meta);
        Ok(())
    }

    /// Updates the edge count in the meta header.
    fn update_meta_edge_count(&mut self, count: u64) -> Result<(), DbError> {
        let mut meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(&meta_page);
        meta.edge_count = count;
        layout::write_meta_header(&mut meta_page, &meta);
        Ok(())
    }

    /// Updates the root_data_page pointer in the meta header.
    fn update_meta_root_data_page(&mut self, page_id: u32) -> Result<(), DbError> {
        let mut meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(&meta_page);
        meta.root_data_page = page_id;
        layout::write_meta_header(&mut meta_page, &meta);
        Ok(())
    }

    /// Updates the root_edge_page pointer in the meta header.
    fn update_meta_root_edge_page(&mut self, page_id: u32) -> Result<(), DbError> {
        let mut meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(&meta_page);
        meta.root_edge_page = page_id;
        layout::write_meta_header(&mut meta_page, &meta);
        Ok(())
    }

    pub fn close(mut self) {
        let _ = self.pager.sync_all();
    }
}
