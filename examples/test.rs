// cargo run --example test

use std::{ path::Path};

use rusqlite::{Connection, Result};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

// #[derive(Debug)]
// struct FileChecksum {
//     inode: i32,
//     path: String,
//     checksum: Option<Vec<u8>>,
// }

fn main() {

    let _db = Database::new();

    // let db_path = Path::new("/tmp/checksum.db");

    // let conn = Connection::open(db_path);
    // match conn {
    //     Ok(c) => println!("has connection"),
    //     Err(e) => println!("error"),
    // }
    // //create_db_in_needed(&conn.unwrap());

    // println!("test");
}

// fn create_db_in_needed(conn: &Connection) {

//     if fs::exists(path)
//}

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
        db.init_tables();
        Ok(db)
    }

    /**
     * Initialize tables if needed
     */
    fn init_tables(&self) {
        if self.table_exists(CHECKSUM_TABLE) {
            return;
        }

        println!("init database now ......")
    }

    /**
     * Check if the database is initialized
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
