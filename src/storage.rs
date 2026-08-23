//! The on-disk format and the in-memory table store.
//!
//! A Ledgerstone database file is an append-only log of operation records,
//! one JSON object per line, length-prefixed with a 4-byte little-endian
//! `u32`. Opening a database means reading the log front to back and
//! replaying every record into memory, the same way a write-ahead log is
//! replayed after a restart. There are no pages and no B-tree, on purpose:
//! the format is small enough to read start to finish.
//!
//! Record kinds:
//!   CreateTable  { table, columns }
//!   Insert       { table, row_id, values }
//!   Update       { table, row_id, values }
//!   Delete       { table, row_id }
//!
//! Each table keeps its live rows in a `HashMap<row_id, Vec<Value>>`, which
//! is the "in-memory index" lookups go through; the log on disk is never
//! scanned again once a database is open. `flush()` fsyncs the file so a
//! completed write survives a crash.

use crate::error::{LsError, LsResult};
use crate::types::{ColumnType, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// Largest single WAL record we will allocate for on replay. A corrupt or
/// hostile log claiming a huge length is rejected before the allocation,
/// so opening a bad database file cannot exhaust memory.
const MAX_RECORD_SIZE: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredColumn {
    pub name: String,
    pub ty: ColumnType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
enum Record {
    CreateTable {
        table: String,
        columns: Vec<StoredColumn>,
    },
    Insert {
        table: String,
        row_id: u64,
        values: Vec<Value>,
    },
    Update {
        table: String,
        row_id: u64,
        values: Vec<Value>,
    },
    Delete {
        table: String,
        row_id: u64,
    },
}

pub struct Table {
    pub columns: Vec<StoredColumn>,
    /// The in-memory index: row id to row values. Row ids are never reused.
    pub rows: HashMap<u64, Vec<Value>>,
    next_row_id: u64,
}

impl Table {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name))
    }
}

pub struct Database {
    path: PathBuf,
    file: File,
    pub tables: HashMap<String, Table>,
}

impl Database {
    /// Opens a database file, creating it if absent, and replays the log.
    pub fn open<P: AsRef<Path>>(path: P) -> LsResult<Self> {
        let path = path.as_ref().to_path_buf();
        let mut tables: HashMap<String, Table> = HashMap::new();

        if path.exists() {
            let f = File::open(&path)
                .map_err(|e| LsError::Storage(format!("cannot open {}: {e}", path.display())))?;
            let mut reader = BufReader::new(f);
            let mut len_buf = [0u8; 4];
            loop {
                match reader.read_exact(&mut len_buf) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(LsError::Storage(format!("read error: {e}"))),
                }
                let len = u32::from_le_bytes(len_buf) as usize;
                if len > MAX_RECORD_SIZE {
                    return Err(LsError::Storage(format!(
                        "record length {len} exceeds the {MAX_RECORD_SIZE} byte limit, refusing to allocate"
                    )));
                }
                let mut buf = vec![0u8; len];
                reader
                    .read_exact(&mut buf)
                    .map_err(|e| LsError::Storage(format!("truncated record: {e}")))?;
                let record: Record = serde_json::from_slice(&buf)
                    .map_err(|e| LsError::Storage(format!("corrupt record: {e}")))?;
                apply_record(&mut tables, record);
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| LsError::Storage(format!("cannot open {}: {e}", path.display())))?;

        Ok(Database { path, file, tables })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn create_table(&mut self, table: &str, columns: Vec<StoredColumn>) -> LsResult<()> {
        if self.tables.contains_key(table) {
            return Err(LsError::Duplicate(format!("table '{table}' already exists")));
        }
        self.append(&Record::CreateTable {
            table: table.to_string(),
            columns: columns.clone(),
        })?;
        self.tables.insert(
            table.to_string(),
            Table {
                columns,
                rows: HashMap::new(),
                next_row_id: 1,
            },
        );
        Ok(())
    }

    pub fn table(&self, name: &str) -> LsResult<&Table> {
        self.tables
            .get(name)
            .ok_or_else(|| LsError::NotFound(format!("table '{name}' does not exist")))
    }

    pub fn insert_row(&mut self, table: &str, values: Vec<Value>) -> LsResult<u64> {
        let row_id = {
            let t = self
                .tables
                .get_mut(table)
                .ok_or_else(|| LsError::NotFound(format!("table '{table}' does not exist")))?;
            let id = t.next_row_id;
            t.next_row_id += 1;
            id
        };
        self.append(&Record::Insert {
            table: table.to_string(),
            row_id,
            values: values.clone(),
        })?;
        let t = self.tables.get_mut(table).expect("checked above");
        t.rows.insert(row_id, values);
        Ok(row_id)
    }

    pub fn update_row(&mut self, table: &str, row_id: u64, values: Vec<Value>) -> LsResult<()> {
        self.append(&Record::Update {
            table: table.to_string(),
            row_id,
            values: values.clone(),
        })?;
        let t = self
            .tables
            .get_mut(table)
            .ok_or_else(|| LsError::NotFound(format!("table '{table}' does not exist")))?;
        t.rows.insert(row_id, values);
        Ok(())
    }

    pub fn delete_row(&mut self, table: &str, row_id: u64) -> LsResult<()> {
        self.append(&Record::Delete {
            table: table.to_string(),
            row_id,
        })?;
        let t = self
            .tables
            .get_mut(table)
            .ok_or_else(|| LsError::NotFound(format!("table '{table}' does not exist")))?;
        t.rows.remove(&row_id);
        Ok(())
    }

    fn append(&mut self, record: &Record) -> LsResult<()> {
        let bytes = serde_json::to_vec(record)
            .map_err(|e| LsError::Storage(format!("cannot serialize record: {e}")))?;
        let len = (bytes.len() as u32).to_le_bytes();
        self.file
            .write_all(&len)
            .map_err(|e| LsError::Storage(format!("write error: {e}")))?;
        self.file
            .write_all(&bytes)
            .map_err(|e| LsError::Storage(format!("write error: {e}")))?;
        self.file
            .flush()
            .map_err(|e| LsError::Storage(format!("flush error: {e}")))?;
        self.file
            .sync_data()
            .map_err(|e| LsError::Storage(format!("sync error: {e}")))?;
        Ok(())
    }
}

fn apply_record(tables: &mut HashMap<String, Table>, record: Record) {
    match record {
        Record::CreateTable { table, columns } => {
            tables.insert(
                table,
                Table {
                    columns,
                    rows: HashMap::new(),
                    next_row_id: 1,
                },
            );
        }
        Record::Insert { table, row_id, values } => {
            if let Some(t) = tables.get_mut(&table) {
                t.rows.insert(row_id, values);
                if row_id >= t.next_row_id {
                    t.next_row_id = row_id + 1;
                }
            }
        }
        Record::Update { table, row_id, values } => {
            if let Some(t) = tables.get_mut(&table) {
                t.rows.insert(row_id, values);
            }
        }
        Record::Delete { table, row_id } => {
            if let Some(t) = tables.get_mut(&table) {
                t.rows.remove(&row_id);
            }
        }
    }
}
