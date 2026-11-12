use rusqlite::{Connection, OptionalExtension, Result, params};
use std::path::PathBuf;

use crate::Meme;

#[derive(Debug)]
pub struct MemeRow {
    id: i64,
    pub name: String,
    pub path: String
}

fn db_path() -> PathBuf {
    let dir = dirs::data_dir().unwrap().join("flexmeme");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("memes.db")
}


pub fn open() -> Result<Connection>{
    let conn = Connection::open(db_path())?;

    conn.execute("CREATE TABLE IF NOT EXISTS memes (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        path TEXT NOT NULL UNIQUE,
        source TEXT NOT NULL DEFAULT 'upload',
        added_at DATE NOT NULL DEFAULT (strftime('%s','now'))
    );", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS uploads (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        path TEXT NOT NULL UNIQUE,
        source TEXT NOT NULL DEFAULT 'local',
        size INTEGER,
        mtime INTEGER,
        tags TEXT,
        hash TEXT,
        indexed INTEGER NOT NULL DEFAULT 0,
        added_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
    );", [])?;
    Ok(conn)
}

pub fn find(conn: &Connection) -> Result<Vec<MemeRow>> {
    println!("db: {:?}", db_path());
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM memes", [], |r| r.get(0))?;
    println!("rows: {count}");
    let mut stmt = conn.prepare("SELECT id, name, path FROM memes")?;
    let data = stmt.query_map([], |row| {
        Ok(MemeRow {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
        })
    })?;

    data.collect()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str)-> Result<()>{
    conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value],)?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str)->Result<Option<String>>{
    conn.query_row("SELECT value FROM settings where key=?1", params![key], |r|r.get(0)).optional()
}

pub fn upsert_meme(conn: &Connection, name: &str, path: &str, size: i64, mtime: i64) -> Result<()>{
    conn.execute(
        "INSERT INTO memes (name, path, size, mtime) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(path, hash) DO UPDATE SET
                size = excluded.size,
                mtime = excluded.mtime,
                indexed=0",
            params![name, path, size, mtime],
    )?;
    Ok(())
}