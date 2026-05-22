// cargo run --example test2

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Malformed line: {0}")]
    Malformed(String),

    #[error("Not found: {0}")]
    NotFound(String),
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

    //test(&db);
}

const BATCH_SIZE: usize = 5000;

fn test(db: &Database) {
    let child = Command::new("find")
        .arg("/")
        .arg("-type")
        .arg("f")
        .arg("-printf")
        .arg("%i %p\n")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.unwrap();
    let reader = BufReader::new(stdout);

    let mut counter = 0;
    for line in reader.lines() {
        let line = line.unwrap();

        let mut parts = line.splitn(2, ' ');
        let _inode: i64 = match parts.next().and_then(|x| x.parse().ok()) {
            Some(v) => v,
            None => continue,
        };

        let path = match parts.next() {
            Some(p) => p,
            None => continue,
        };

        counter += 1;
        if counter == 1000000 {
            break;
        }

        if counter % 10000 == 0 {
            println!("new batch of 1000");
        }
        //println!("{}", path);
        let row_res = db.fetch(path);
        if let Ok(_row) = row_res {
            //println!("Inode number of the found path is {}", _row.inode);
        }
    }
}

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

    let mut count: usize = 0;

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

        db.insert_in_memory(FileRow {
            inode,
            path: path.to_string(),
            hash: String::new(),
        });

        count += 1;

        // 🔥 batch flush
        if count % BATCH_SIZE == 0 {
            db.flush()?;
            println!("Committed {} rows", count);
        }
    }

    db.flush()?; // final flush
    Ok(())
}

const DB_PATH: &str = "/tmp/checksum.txt";
const TMP_PREFIX: &str = "tmp_";
const FIELD_SEP: char = '|';

#[derive(Clone)]
pub struct FileRow {
    inode: i64,
    path: String,
    hash: String,
}

impl FileRow {
    pub fn new_for_insert(inode: i64, path: String, hash: String) -> Self {
        Self { inode, path, hash }
    }
}

struct Database {
    path: PathBuf,
    rows: Mutex<HashMap<String, FileRow>>,
}

impl Database {
    /**
     * Create new instance: load the backing file into memory as a hashmap
     * keyed by path. If the file does not exist yet it starts empty.
     */
    pub fn new() -> Result<Self, DatabaseError> {
        let path = PathBuf::from(DB_PATH);
        let rows = Self::load(&path)?;
        Ok(Self {
            path,
            rows: Mutex::new(rows),
        })
    }

    fn load(path: &Path) -> Result<HashMap<String, FileRow>, DatabaseError> {
        let mut map: HashMap<String, FileRow> = HashMap::new();
        if !path.exists() {
            println!("Initialize database now");
            return Ok(map);
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            let mut parts = line.splitn(3, FIELD_SEP);
            let inode_str = parts
                .next()
                .ok_or_else(|| DatabaseError::Malformed(line.clone()))?;
            let path_str = parts
                .next()
                .ok_or_else(|| DatabaseError::Malformed(line.clone()))?;
            let hash_str = parts.next().unwrap_or("");

            let inode: i64 = inode_str
                .parse()
                .map_err(|_| DatabaseError::Malformed(line.clone()))?;

            map.insert(
                path_str.to_string(),
                FileRow {
                    inode,
                    path: path_str.to_string(),
                    hash: hash_str.to_string(),
                },
            );
        }

        Ok(map)
    }

    /**
     * Fetch a row by path (hashmap lookup).
     */
    pub fn fetch(&self, path: &str) -> Result<FileRow, DatabaseError> {
        let rows = self.rows.lock().unwrap();
        rows.get(path)
            .cloned()
            .ok_or_else(|| DatabaseError::NotFound(path.to_string()))
    }

    /**
     * Insert new item: add it to the in-memory hashmap and persist
     * the whole map back to disk (tmp file + atomic rename).
     */
    pub fn insert(&self, file_row: &FileRow) -> Result<FileRow, DatabaseError> {
        {
            let mut rows = self.rows.lock().unwrap();
            rows.insert(file_row.path.clone(), file_row.clone());
        }
        self.flush()?;
        Ok(file_row.clone())
    }

    /**
     * Update by path: replace the existing entry in the hashmap and
     * persist back to disk via tmp file + rename.
     */
    pub fn update(&self, file_row: &FileRow) -> Result<(), DatabaseError> {
        {
            let mut rows = self.rows.lock().unwrap();
            rows.insert(file_row.path.clone(), file_row.clone());
        }
        self.flush()?;
        Ok(())
    }

    /**
     * Modify the in-memory hashmap without touching the disk. Used by
     * batched ingestion paths which flush explicitly.
     */
    fn insert_in_memory(&self, file_row: FileRow) {
        let mut rows = self.rows.lock().unwrap();
        rows.insert(file_row.path.clone(), file_row);
    }

    /**
     * Dump the whole hashmap to a tmp_ prefixed file next to the real
     * one and then atomically rename it over the real file.
     */
    fn flush(&self) -> Result<(), DatabaseError> {
        let rows = self.rows.lock().unwrap();
        let tmp_path = tmp_path_for(&self.path);

        {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;
            let mut writer = BufWriter::new(file);
            for row in rows.values() {
                writeln!(
                    writer,
                    "{}{sep}{}{sep}{}",
                    row.inode,
                    row.path,
                    row.hash,
                    sep = FIELD_SEP
                )?;
            }
            writer.flush()?;
        }

        fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    parent.join(format!("{}{}", TMP_PREFIX, file_name))
}
