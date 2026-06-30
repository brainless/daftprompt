use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use zerocopy::AsBytes;

pub fn register_sqlite_vec() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    }
}

pub fn db_path_for_repo(repo_path: &Path) -> anyhow::Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine cache directory"))?
        .join("sugacode");
    std::fs::create_dir_all(&cache_dir)?;

    let abs_path = std::fs::canonicalize(repo_path)?;
    let abs_str = abs_path.to_string_lossy();
    let stem: String = abs_str
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let hash = simple_hash(&abs_str);
    let slug = format!("{}_{}", stem, hash);
    Ok(cache_dir.join(format!("{}.db", slug)))
}

fn simple_hash(s: &str) -> String {
    let mut h: u64 = 0;
    for b in s.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    format!("{:06x}", h & 0xFFFFFF)
}

pub fn init_schema(db: &Connection, dim: usize) -> anyhow::Result<()> {
    db.execute_batch(include_str!("schema.sql"))?;
    db.execute(
        &format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_items USING vec0(\
                item_id INTEGER PRIMARY KEY, embedding float[{dim}] distance_metric=cosine)"
        ),
        [],
    )?;
    Ok(())
}

pub fn existing_identifiers(db: &Connection, source_type: &str) -> anyhow::Result<HashSet<String>> {
    let mut stmt = db.prepare("SELECT identifier FROM items WHERE source_type = ?")?;
    let rows = stmt.query_map([source_type], |row| row.get::<_, String>(0))?;
    let mut set = HashSet::new();
    for row in rows {
        set.insert(row?);
    }
    Ok(set)
}

pub struct ItemRow {
    pub identifier: String,
    pub text: String,
    pub author: Option<String>,
    pub metadata: Option<String>,
}

pub fn insert_items(db: &Connection, source_type: &str, items: &[ItemRow]) -> anyhow::Result<Vec<i64>> {
    let mut stmt = db.prepare(
        "INSERT INTO items(source_type, identifier, text, author, metadata) VALUES (?, ?, ?, ?, ?)",
    )?;
    let mut ids = Vec::new();
    for item in items {
        stmt.execute(rusqlite::params![
            source_type,
            item.identifier,
            item.text,
            item.author,
            item.metadata,
        ])?;
        ids.push(db.last_insert_rowid());
    }
    Ok(ids)
}

pub fn insert_vectors(db: &Connection, item_ids: &[i64], embeddings: &[Vec<f32>]) -> anyhow::Result<()> {
    let mut stmt = db.prepare("INSERT INTO vec_items(item_id, embedding) VALUES (?, ?)")?;
    for (id, emb) in item_ids.iter().zip(embeddings.iter()) {
        stmt.execute(rusqlite::params![id, emb.as_bytes()])?;
    }
    Ok(())
}

pub fn search_fts(db: &Connection, query: &str, limit: usize) -> anyhow::Result<Vec<(i64, f64)>> {
    let mut stmt = db.prepare(
        "SELECT rowid, rank FROM items_fts WHERE items_fts MATCH ? ORDER BY rank LIMIT ?",
    )?;
    let rows = stmt.query_map(rusqlite::params![query, limit as i64], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn search_vec(db: &Connection, query_embedding: &[f32], limit: usize) -> anyhow::Result<Vec<(i64, f64)>> {
    let mut stmt = db.prepare(
        "SELECT item_id, distance FROM vec_items WHERE embedding MATCH ? AND k = ? ORDER BY distance",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![query_embedding.as_bytes(), limit as i64],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
    )?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn delete_source(db: &Connection, source_type: &str) -> anyhow::Result<()> {
    let mut stmt = db.prepare("SELECT id FROM items WHERE source_type = ?")?;
    let ids: Vec<i64> = stmt
        .query_map([source_type], |row| row.get::<_, i64>(0))?
        .filter_map(|r| r.ok())
        .collect();

    for chunk in ids.chunks(500) {
        let placeholders: Vec<&str> = chunk.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM vec_items WHERE item_id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = db.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
        stmt.execute(params.as_slice())?;
    }

    db.execute("DELETE FROM items WHERE source_type = ?", [source_type])?;
    Ok(())
}

pub fn repo_meta_get(db: &Connection, key: &str) -> anyhow::Result<Option<String>> {
    let mut stmt = db.prepare("SELECT value FROM repo_meta WHERE key = ?")?;
    let mut rows = stmt.query_map([key], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn repo_meta_set(db: &Connection, key: &str, value: &str) -> anyhow::Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO repo_meta(key, value) VALUES (?, ?)",
        [key, value],
    )?;
    Ok(())
}
