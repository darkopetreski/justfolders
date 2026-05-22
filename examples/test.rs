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
    let db = Database::new().unwrap_or_else(|e| {
        eprintln!("failed to init database: {e}");
        std::process::exit(1);
    });

    if let Ok(row) = db.fetch("/tmp/test") {
        println!("{}", row.hash);
    } else {
        println!("record not found");
    }

    let insert_result = db.insert(&FileRow::new_for_insert(
        123,
        "/tmp/test".into(),
        "123".into(),
    ));
    match insert_result {
        Ok(_) => println!("inserted"),
        Err(e) => {
            println!("Error, cannot insert: {}", e)
        }
    }
}

const DB_PATH: &str = "/tmp/checksum.db";
const CHECKSUM_TABLE: &str = "checksum";

pub struct FileRow {
    id: i64,
    inode: i64,
    path: String,
    hash: String,
}

impl FileRow {
    pub fn new_for_insert(inode: i64, path: String, hash: String) -> Self {
        Self {
            id: 0,
            inode,
            path,
            hash,
        }
    }
}

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
     * Fetch a row by path
     */
    pub fn fetch(&self, path: &str) -> Result<FileRow, DatabaseError> {
        let sql = format!(
            "SELECT id, inode, path, hash FROM {} WHERE path = ?1",
            CHECKSUM_TABLE
        );
        let row = self.conn.query_row(&sql, [path], |row| {
            Ok(FileRow {
                id: row.get(0)?,
                inode: row.get(1)?,
                path: row.get(2)?,
                hash: row.get(3)?,
            })
        })?;

        Ok(row)
    }

    /**
     * Insert new item
     */
    pub fn insert(&self, file_row: &FileRow) -> Result<(), DatabaseError> {
        //let sql = "INSERT INTO files (inode, path, hash) VALUES (?1, ?2, ?3)";

        let sql = format!(
            "INSERT INTO {} (inode, path, hash) VALUES (?1, ?2, ?3)",
            CHECKSUM_TABLE
        );

        self.conn
            .execute(&sql, (file_row.inode, &file_row.path, &file_row.hash))?;
        Ok(())
    }

    //pub fn update(&self, inode: i64, path: &str, hash: &str) {}

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
            hash TEXT NOT NULL
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
