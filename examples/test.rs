// cargo run --example test

use std::path::Path;

use rusqlite::{Connection, Result};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

fn main() {
    let _db = Database::new();
}


const DB_PATH: &str = "/tmp/checksum.db";
const CHECKSUM_TABLE: &str = "checksum";

struct Database {
    conn: Connection,
}

impl Database {
    /**
     * Create new instance
     */
    pub fn new() -> Result<Self, DatabaseError> {
        let db_path = Path::new(DB_PATH);
        let conn = Connection::open(db_path)?;
        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    /**
     * Insert new item
     */
    pub fn insert(inode: u64, path: &Path, hash: [u8; 16]) {

    }

    pub fn update(inode: u64, path: &Path, hash: [u8; 16]) {

    }

    /**
     * Initialize tables if needed
     */
    fn init_tables(&self) -> Result<(), DatabaseError> {
        if self.table_exists(CHECKSUM_TABLE) {
            return Ok(());
        }
        println!("Initialize database now");
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
            id INTEGER PRIMARY KEY,
            inode INTEGER NOT NULL,
            path TEXT NOT NULL UNIQUE,
            hash BLOB NOT NULL
        )",
            CHECKSUM_TABLE
        );

        self.conn.execute(&sql, [])?;
        Ok(())
    }

    /**
     * Check if the table exists
     */
    fn table_exists(&self, name: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
                [name],
                |_| Ok(()),
            )
            .is_ok()
    }
}
 