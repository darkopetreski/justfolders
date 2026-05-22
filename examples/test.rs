// cargo run --example test

use std::path::Path;

use rusqlite::{Connection, Result};
use thiserror::Error;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

        #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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

    // let insert_result = db.insert(&FileRow::new_for_insert(
    //     123,
    //     "/tmp/test".into(),
    //     "123".into(),
    // ));
    // match insert_result {
    //     Ok(_) => println!("inserted"),
    //     Err(e) => {
    //         println!("Error, cannot insert: {}", e)
    //     }
    // }

    let _ = ingest_files(&db);
}


const BATCH_SIZE: usize = 5000;

fn ingest_files(db: &Database) -> Result<(), DatabaseError> {
    let child = Command::new("find")
        .arg("/")
        .arg("-type")
        .arg("f")
        .arg("-printf")
        .arg("%i %p\n")
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.unwrap();
    let reader = BufReader::new(stdout);

    let mut tx = db.conn.unchecked_transaction()?;
    let mut count = 0;

    for line in reader.lines() {
        let line = line?;

        let mut parts = line.splitn(2, ' ');
        let inode: i64 = match parts.next().and_then(|x| x.parse().ok()) {
            Some(v) => v,
            None => continue,
        };

        let path = match parts.next() {
            Some(p) => p,
            None => continue,
        };

        tx.execute(
            "INSERT INTO checksum (inode, path, hash) VALUES (?1, ?2, ?3)",
            (inode, path, ""),
        )?;

        count += 1;

        // 🔥 batch flush
        if count % BATCH_SIZE == 0 {
            tx.commit()?;
            tx = db.conn.unchecked_transaction()?;
            println!("Committed {} rows", count);
        }
    }

    tx.commit()?; // final flush
    Ok(())
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
    pub conn: Connection,
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
    pub fn insert(&self, file_row: &FileRow) -> Result<FileRow, DatabaseError> {
        let sql = format!(
            "INSERT INTO {} (inode, path, hash) VALUES (?1, ?2, ?3)",
            CHECKSUM_TABLE
        );

        self.conn
            .execute(&sql, (file_row.inode, &file_row.path, &file_row.hash))?;

        Ok(FileRow {
            id: self.conn.last_insert_rowid(),
            inode: file_row.inode,
            path: file_row.path.clone(),
            hash: file_row.hash.clone(),
        })
    }

    /**
     * Update by id
     */
    pub fn update(&self, file_row: &FileRow) -> Result<(), DatabaseError> {
        let sql = format!(
            "UPDATE {} SET inode = ?1, path = ?2, hash = ?3 WHERE id = ?4",
            CHECKSUM_TABLE
        );

        self.conn.execute(
            &sql,
            (file_row.inode, &file_row.path, &file_row.hash, file_row.id),
        )?;

        Ok(())
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
            hash TEXT NOT NULL
        )",
            CHECKSUM_TABLE
        );

        self.conn.execute(&sql, ())?;
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
