
use rusqlite::{params, Connection, Result};
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