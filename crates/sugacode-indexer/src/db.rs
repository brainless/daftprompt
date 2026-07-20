use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;
use zerocopy::AsBytes;

pub fn register_sqlite_vec() {
    #[allow(clippy::missing_transmute_annotations)]
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
    db.execute(
        &format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(\
                item_id INTEGER PRIMARY KEY, embedding float[{dim}] distance_metric=cosine)"
        ),
        [],
    )?;
    // Document embeddings — separate vec0 table for source isolation.
    // Rejected alternative: share vec_items across all source types and
    // post-filter by source_type. That wastes KNN slots on non-document
    // results and makes the effective result count unpredictable. A
    // dedicated vec_documents table keeps the cosine-distance index small
    // and focused, consistent with the vec_code isolation in Epic 004.
    db.execute(
        &format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_documents USING vec0(\
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

pub fn insert_items(
    db: &Connection,
    source_type: &str,
    items: &[ItemRow],
) -> anyhow::Result<Vec<i64>> {
    let mut stmt = db.prepare(
        "INSERT OR REPLACE INTO items(source_type, identifier, text, author, metadata) VALUES (?, ?, ?, ?, ?)",
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

pub fn insert_vectors(
    db: &Connection,
    item_ids: &[i64],
    embeddings: &[Vec<f32>],
) -> anyhow::Result<()> {
    let mut stmt = db.prepare("INSERT INTO vec_items(item_id, embedding) VALUES (?, ?)")?;
    for (id, emb) in item_ids.iter().zip(embeddings.iter()) {
        stmt.execute(rusqlite::params![id, emb.as_bytes()])?;
    }
    Ok(())
}

pub fn insert_vectors_into(
    db: &Connection,
    table: &str,
    item_ids: &[i64],
    embeddings: &[Vec<f32>],
) -> anyhow::Result<()> {
    let sql = format!("INSERT INTO {}(item_id, embedding) VALUES (?, ?)", table);
    let mut stmt = db.prepare(&sql)?;
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

pub fn search_vec(
    db: &Connection,
    query_embedding: &[f32],
    limit: usize,
) -> anyhow::Result<Vec<(i64, f64)>> {
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

pub fn search_fts_filtered(
    db: &Connection,
    query: &str,
    source_type: &str,
    limit: usize,
) -> anyhow::Result<Vec<(i64, f64)>> {
    let mut stmt = db.prepare(
        "SELECT items.id, items_fts.rank \
         FROM items_fts \
         JOIN items ON items.id = items_fts.rowid \
         WHERE items_fts MATCH ? AND items.source_type = ? \
         ORDER BY items_fts.rank LIMIT ?",
    )?;
    let rows = stmt.query_map(rusqlite::params![query, source_type, limit as i64], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn search_vec_code(
    db: &Connection,
    query_embedding: &[f32],
    limit: usize,
) -> anyhow::Result<Vec<(i64, f64)>> {
    // Partitioning approach: vec_code is a separate vec0 table scoped to code items,
    // so no post-filter is needed — every row is already a code item.
    //
    // Rejected alternative — over-fetch from vec_items and filter by source_type:
    //   SELECT item_id, distance FROM vec_items WHERE embedding MATCH ? AND k = ? ORDER BY distance
    // Then look up each item's source_type in a loop and discard non-"code" rows.
    // This was rejected because: (a) vec_items contains commit embeddings that dominate
    // the index, so for a query about code most of the k results would be wasted;
    // (b) filtering after the fact means the effective result count is unpredictable;
    // (c) a dedicated vec_code table keeps the cosine-distance index small and focused.
    let mut stmt = db.prepare(
        "SELECT item_id, distance FROM vec_code WHERE embedding MATCH ? AND k = ? ORDER BY distance",
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
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
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

pub fn code_file_get(db: &Connection, file_path: &str) -> anyhow::Result<Option<(i64, String)>> {
    let mut stmt = db.prepare("SELECT mtime, content_hash FROM code_files WHERE file_path = ?")?;
    let mut rows = stmt.query_map([file_path], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn code_file_upsert(
    db: &Connection,
    file_path: &str,
    mtime: i64,
    content_hash: &str,
) -> anyhow::Result<()> {
    let indexed_at = Utc::now().to_rfc3339();
    db.execute(
        "INSERT INTO code_files(file_path, mtime, content_hash, indexed_at) VALUES (?, ?, ?, ?)
         ON CONFLICT(file_path) DO UPDATE SET mtime = excluded.mtime, content_hash = excluded.content_hash, indexed_at = excluded.indexed_at",
        rusqlite::params![file_path, mtime, content_hash, indexed_at],
    )?;
    Ok(())
}

pub fn code_file_delete(db: &Connection, file_path: &str) -> anyhow::Result<()> {
    db.execute("DELETE FROM code_files WHERE file_path = ?", [file_path])?;
    Ok(())
}

pub fn code_files_all(db: &Connection) -> anyhow::Result<Vec<String>> {
    let mut stmt = db.prepare("SELECT file_path FROM code_files")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut paths = Vec::new();
    for row in rows {
        paths.push(row?);
    }
    Ok(paths)
}

/// Hash content bytes into a stable hex string using xxh3.
///
/// We use xxh3_64 instead of `simple_hash` / `DefaultHasher` because those
/// are not guaranteed to produce the same output across Rust toolchain versions.
pub fn content_hash(data: &[u8]) -> String {
    use xxhash_rust::xxh3::xxh3_64;
    format!("{:016x}", xxh3_64(data))
}

fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn delete_code_file_items(db: &Connection, file_path: &str) -> anyhow::Result<()> {
    let escaped = escape_like_pattern(file_path);
    let pattern = format!("{}::%", escaped);

    let mut stmt = db.prepare(
        "SELECT id FROM items WHERE source_type = 'code' AND identifier LIKE ? ESCAPE '\\'",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([&pattern], |row| row.get::<_, i64>(0))?
        .filter_map(|r| r.ok())
        .collect();

    for chunk in ids.chunks(500) {
        let placeholders: Vec<&str> = chunk.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM vec_code WHERE item_id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = db.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        stmt.execute(params.as_slice())?;
    }

    db.execute(
        "DELETE FROM items WHERE source_type = 'code' AND identifier LIKE ? ESCAPE '\\'",
        [&pattern],
    )?;
    Ok(())
}

pub fn delete_source_vec(
    db: &Connection,
    source_table: &str,
    source_type: &str,
) -> anyhow::Result<()> {
    let mut stmt = db.prepare("SELECT id FROM items WHERE source_type = ?")?;
    let ids: Vec<i64> = stmt
        .query_map([source_type], |row| row.get::<_, i64>(0))?
        .filter_map(|r| r.ok())
        .collect();

    for chunk in ids.chunks(500) {
        let placeholders: Vec<&str> = chunk.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM {} WHERE item_id IN ({})",
            source_table,
            placeholders.join(",")
        );
        let mut stmt = db.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        stmt.execute(params.as_slice())?;
    }
    Ok(())
}

// --- Document file tracking (analogous to code_files) ---

pub fn document_file_get(
    db: &Connection,
    file_path: &str,
) -> anyhow::Result<Option<(i64, String)>> {
    let mut stmt =
        db.prepare("SELECT mtime, content_hash FROM document_files WHERE file_path = ?")?;
    let mut rows = stmt.query_map([file_path], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn document_file_upsert(
    db: &Connection,
    file_path: &str,
    mtime: i64,
    content_hash: &str,
) -> anyhow::Result<()> {
    let indexed_at = Utc::now().to_rfc3339();
    db.execute(
        "INSERT INTO document_files(file_path, mtime, content_hash, indexed_at) VALUES (?, ?, ?, ?)
         ON CONFLICT(file_path) DO UPDATE SET mtime = excluded.mtime, content_hash = excluded.content_hash, indexed_at = excluded.indexed_at",
        rusqlite::params![file_path, mtime, content_hash, indexed_at],
    )?;
    Ok(())
}

pub fn document_file_delete(db: &Connection, file_path: &str) -> anyhow::Result<()> {
    db.execute(
        "DELETE FROM document_files WHERE file_path = ?",
        [file_path],
    )?;
    Ok(())
}

pub fn document_files_all(db: &Connection) -> anyhow::Result<Vec<String>> {
    let mut stmt = db.prepare("SELECT file_path FROM document_files")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut paths = Vec::new();
    for row in rows {
        paths.push(row?);
    }
    Ok(paths)
}

pub fn delete_document_file_items(db: &Connection, file_path: &str) -> anyhow::Result<()> {
    let escaped = escape_like_pattern(file_path);
    let pattern = format!("{}::%", escaped);

    let mut stmt = db.prepare(
        "SELECT id FROM items WHERE source_type = 'document' AND identifier LIKE ? ESCAPE '\\'",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([&pattern], |row| row.get::<_, i64>(0))?
        .filter_map(|r| r.ok())
        .collect();

    for chunk in ids.chunks(500) {
        let placeholders: Vec<&str> = chunk.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM vec_documents WHERE item_id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = db.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        stmt.execute(params.as_slice())?;
    }

    db.execute(
        "DELETE FROM items WHERE source_type = 'document' AND identifier LIKE ? ESCAPE '\\'",
        [&pattern],
    )?;
    Ok(())
}

pub fn search_vec_documents(
    db: &Connection,
    query_embedding: &[f32],
    limit: usize,
) -> anyhow::Result<Vec<(i64, f64)>> {
    // Partitioning approach: vec_documents is a separate vec0 table scoped to
    // document items, so no post-filter is needed — every row is already a
    // document item. Same rationale as vec_code isolation (Epic 004).
    let mut stmt = db.prepare(
        "SELECT item_id, distance FROM vec_documents WHERE embedding MATCH ? AND k = ? ORDER BY distance",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        register_sqlite_vec();
        let db = Connection::open_in_memory().unwrap();
        init_schema(&db, 384).unwrap();
        db
    }

    #[test]
    fn test_code_file_upsert_and_get() {
        let db = test_db();
        code_file_upsert(&db, "src/main.rs", 1000, "abc123").unwrap();
        let result = code_file_get(&db, "src/main.rs").unwrap();
        assert_eq!(result, Some((1000, "abc123".to_string())));
    }

    #[test]
    fn test_code_file_get_missing() {
        let db = test_db();
        let result = code_file_get(&db, "nope.rs").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_code_file_upsert_updates() {
        let db = test_db();
        code_file_upsert(&db, "src/main.rs", 1000, "hash_old").unwrap();
        code_file_upsert(&db, "src/main.rs", 2000, "hash_new").unwrap();
        let result = code_file_get(&db, "src/main.rs").unwrap();
        assert_eq!(result, Some((2000, "hash_new".to_string())));
    }

    #[test]
    fn test_code_file_delete() {
        let db = test_db();
        code_file_upsert(&db, "src/main.rs", 1000, "abc").unwrap();
        code_file_delete(&db, "src/main.rs").unwrap();
        let result = code_file_get(&db, "src/main.rs").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_code_files_all() {
        let db = test_db();
        code_file_upsert(&db, "src/a.rs", 1, "h1").unwrap();
        code_file_upsert(&db, "src/b.rs", 2, "h2").unwrap();
        code_file_upsert(&db, "src/c.rs", 3, "h3").unwrap();
        let mut paths = code_files_all(&db).unwrap();
        paths.sort();
        assert_eq!(paths, vec!["src/a.rs", "src/b.rs", "src/c.rs"]);
    }

    #[test]
    fn test_content_hash_stable() {
        let data = b"hello world";
        let h1 = content_hash(data);
        let h2 = content_hash(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    // --- Document file tracking tests ---

    #[test]
    fn test_document_file_upsert_and_get() {
        let db = test_db();
        document_file_upsert(&db, "docs/readme.md", 1000, "abc123").unwrap();
        let result = document_file_get(&db, "docs/readme.md").unwrap();
        assert_eq!(result, Some((1000, "abc123".to_string())));
    }

    #[test]
    fn test_document_file_get_missing() {
        let db = test_db();
        let result = document_file_get(&db, "nope.md").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_document_file_upsert_updates() {
        let db = test_db();
        document_file_upsert(&db, "docs/readme.md", 1000, "hash_old").unwrap();
        document_file_upsert(&db, "docs/readme.md", 2000, "hash_new").unwrap();
        let result = document_file_get(&db, "docs/readme.md").unwrap();
        assert_eq!(result, Some((2000, "hash_new".to_string())));
    }

    #[test]
    fn test_document_file_delete() {
        let db = test_db();
        document_file_upsert(&db, "docs/readme.md", 1000, "abc").unwrap();
        document_file_delete(&db, "docs/readme.md").unwrap();
        let result = document_file_get(&db, "docs/readme.md").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_document_files_all() {
        let db = test_db();
        document_file_upsert(&db, "docs/a.md", 1, "h1").unwrap();
        document_file_upsert(&db, "docs/b.txt", 2, "h2").unwrap();
        document_file_upsert(&db, "docs/c.rst", 3, "h3").unwrap();
        let mut paths = document_files_all(&db).unwrap();
        paths.sort();
        assert_eq!(paths, vec!["docs/a.md", "docs/b.txt", "docs/c.rst"]);
    }
}
