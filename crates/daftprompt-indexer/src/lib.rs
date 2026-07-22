pub mod code;
pub mod db;
pub mod documents;
pub mod embed;

pub use code::SymbolKind;

use std::path::{Path, PathBuf};
use std::sync::Once;

use rusqlite::Connection;

use db::ItemRow;
use embed::{Embedder, DEFAULT_MODEL};

static REGISTER_VEC: Once = Once::new();

pub struct IndexerConfig {
    pub cache_dir: Option<PathBuf>,
    pub model_name: String,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            cache_dir: None,
            model_name: DEFAULT_MODEL.to_string(),
        }
    }
}

pub struct CodeIndexReport {
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub symbols_indexed: usize,
}

pub struct CommitData {
    pub sha: String,
    pub short_hash: String,
    pub author_name: String,
    pub time: String,
    pub message_title: String,
    pub message_body: String,
}

pub struct SearchResult {
    pub identifier: String,
    pub short_hash: String,
    pub text: String,
    pub author: Option<String>,
    pub score: f32,
    pub match_type: MatchType,
}

pub struct CodeSearchResult {
    pub identifier: String,
    pub symbol_kind: code::SymbolKind,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub text: String,
    pub score: f32,
    pub match_type: MatchType,
}

pub struct DocumentSearchResult {
    pub identifier: String,
    pub file_path: String,
    pub text: String,
    pub score: f32,
    pub match_type: MatchType,
}

pub struct DocumentIndexReport {
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub chunks_indexed: usize,
}

pub struct AllSourceSearchResult {
    pub combined: Vec<UnifiedSearchHit>,
    pub git_log: Vec<SearchResult>,
    pub code: Vec<CodeSearchResult>,
    pub documents: Vec<DocumentSearchResult>,
}

pub enum UnifiedSearchHit {
    GitLog(SearchResult),
    Code(CodeSearchResult),
    Document(DocumentSearchResult),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchType {
    Fts,
    Vector,
    Hybrid,
}

pub struct Indexer {
    db: Connection,
    embedder: Option<Embedder>,
    repo_path: PathBuf,
}

struct ItemDetail {
    identifier: String,
    text: String,
    author: Option<String>,
    metadata: Option<String>,
}

fn lookup_item(db: &Connection, id: i64) -> anyhow::Result<Option<ItemDetail>> {
    let mut stmt =
        db.prepare("SELECT identifier, text, author, metadata FROM items WHERE id = ?")?;
    let mut rows = stmt.query_map([id], |row| {
        Ok(ItemDetail {
            identifier: row.get(0)?,
            text: row.get(1)?,
            author: row.get(2)?,
            metadata: row.get(3)?,
        })
    })?;
    match rows.next() {
        Some(Ok(d)) => Ok(Some(d)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn extract_short_hash(metadata: &Option<String>) -> String {
    metadata
        .as_deref()
        .and_then(|m| {
            serde_json::from_str::<serde_json::Value>(m)
                .ok()
                .and_then(|v| v.get("short_hash")?.as_str().map(String::from))
        })
        .unwrap_or_default()
}

fn parse_symbol_kind(s: &str) -> code::SymbolKind {
    match s {
        "function" => code::SymbolKind::Function,
        "struct" => code::SymbolKind::Struct,
        "enum" => code::SymbolKind::Enum,
        "trait" => code::SymbolKind::Trait,
        "implmethod" => code::SymbolKind::ImplMethod,
        "traitmethod" => code::SymbolKind::TraitMethod,
        "typealias" => code::SymbolKind::TypeAlias,
        "const" => code::SymbolKind::Const,
        "static" => code::SymbolKind::Static,
        "module" => code::SymbolKind::Module,
        "macro" => code::SymbolKind::Macro,
        "comments" => code::SymbolKind::Comments,
        "imports" => code::SymbolKind::Imports,
        _ => code::SymbolKind::Function,
    }
}

fn parse_code_search_result(
    detail: &ItemDetail,
    score: f32,
    match_type: MatchType,
) -> CodeSearchResult {
    let (file_path, line_start, line_end, symbol_kind) = match &detail.metadata {
        Some(m) => {
            let v: serde_json::Value = serde_json::from_str(m).unwrap_or_default();
            let file_path = v
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let line_start = v.get("line_start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let line_end = v.get("line_end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let kind_str = v
                .get("symbol_kind")
                .and_then(|v| v.as_str())
                .unwrap_or("function");
            let symbol_kind = parse_symbol_kind(kind_str);
            (file_path, line_start, line_end, symbol_kind)
        }
        None => (String::new(), 0, 0, code::SymbolKind::Function),
    };

    CodeSearchResult {
        identifier: detail.identifier.clone(),
        symbol_kind,
        file_path,
        line_start,
        line_end,
        text: detail.text.clone(),
        score,
        match_type,
    }
}

fn parse_document_search_result(
    detail: &ItemDetail,
    score: f32,
    match_type: MatchType,
) -> DocumentSearchResult {
    let file_path = match &detail.metadata {
        Some(m) => {
            let v: serde_json::Value = serde_json::from_str(m).unwrap_or_default();
            v.get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        None => String::new(),
    };

    DocumentSearchResult {
        identifier: detail.identifier.clone(),
        file_path,
        text: detail.text.clone(),
        score,
        match_type,
    }
}

impl Indexer {
    pub fn new(repo_path: &Path, config: &IndexerConfig) -> anyhow::Result<Self> {
        REGISTER_VEC.call_once(|| {
            db::register_sqlite_vec();
        });

        let db_path = if let Some(ref cache_dir) = config.cache_dir {
            // Custom cache dir: derive a DB filename from the repo path, same
            // slug logic as db_path_for_repo but rooted at cache_dir.
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
            let hash = {
                let mut h: u64 = 0;
                for b in abs_str.bytes() {
                    h = h.wrapping_mul(31).wrapping_add(b as u64);
                }
                format!("{:06x}", h & 0xFFFFFF)
            };
            let slug = format!("{}_{}", stem, hash);
            std::fs::create_dir_all(cache_dir)?;
            cache_dir.join(format!("{}.db", slug))
        } else {
            db::db_path_for_repo(repo_path)?
        };
        let db = Connection::open(&db_path)?;

        // Create tables first so repo_meta reads work on a fresh DB.
        db::init_schema(&db, 256)?;

        let embedder = match Embedder::new(&config.model_name) {
            Ok(e) => Some(e),
            Err(e) => {
                log::warn!("Failed to load embedding model: {e}. Falling back to FTS5-only.");
                None
            }
        };

        let dim = if let Some(ref emb) = embedder {
            emb.dimension
        } else {
            match db::repo_meta_get(&db, "embedding_dimension")? {
                Some(v) => v.parse::<usize>().unwrap_or(256),
                None => 256,
            }
        };

        // Re-init with correct dimension if it differs from default.
        if dim != 256 {
            db::init_schema(&db, dim)?;
        }

        db::repo_meta_set(&db, "embedding_dimension", &dim.to_string())?;
        db::repo_meta_set(
            &db,
            "repo_path",
            &std::fs::canonicalize(repo_path)
                .unwrap_or_else(|_| repo_path.to_path_buf())
                .to_string_lossy(),
        )?;

        Ok(Self {
            db,
            embedder,
            repo_path: std::fs::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf()),
        })
    }

    pub fn index_commits(&mut self, commits: &[CommitData]) -> anyhow::Result<usize> {
        let existing = db::existing_identifiers(&self.db, "commit")?;

        let new_commits: Vec<&CommitData> = commits
            .iter()
            .filter(|c| !existing.contains(&c.sha))
            .collect();

        if new_commits.is_empty() {
            return Ok(0);
        }

        let tx = self.db.transaction()?;

        let items: Vec<ItemRow> = new_commits
            .iter()
            .map(|c| {
                let text = if c.message_body.is_empty() {
                    c.message_title.clone()
                } else {
                    format!("{}\n\n{}", c.message_title, c.message_body)
                };
                let metadata = serde_json::json!({
                    "short_hash": c.short_hash,
                    "time": c.time,
                })
                .to_string();
                ItemRow {
                    identifier: c.sha.clone(),
                    text,
                    author: Some(c.author_name.clone()),
                    metadata: Some(metadata),
                }
            })
            .collect();

        let item_ids = db::insert_items(&tx, "commit", &items)?;

        if let Some(ref embedder) = self.embedder {
            let texts: Vec<String> = items.iter().map(|i| i.text.clone()).collect();
            let embeddings = embedder.encode_batch(&texts);
            db::insert_vectors(&tx, &item_ids, &embeddings)?;
        }

        tx.commit()?;

        let now = chrono_now();
        db::repo_meta_set(&self.db, "indexed_at", &now)?;

        Ok(new_commits.len())
    }

    pub fn reindex_commits(&mut self, commits: &[CommitData]) -> anyhow::Result<usize> {
        db::delete_source(&self.db, "commit")?;
        self.index_commits(commits)
    }

    pub fn index_code(&mut self) -> anyhow::Result<CodeIndexReport> {
        let current_files = code::list_tracked_rust_files(&self.repo_path)?;
        let indexed_files = db::code_files_all(&self.db)?;

        let current_set: std::collections::HashSet<String> = current_files
            .iter()
            .filter_map(|p| {
                let rel = p.strip_prefix(&self.repo_path).ok()?;
                Some(code::canonicalize_file_path(&self.repo_path, rel))
            })
            .collect();

        let mut files_deleted = 0usize;
        for indexed_path in &indexed_files {
            if !current_set.contains(indexed_path) {
                db::delete_code_file_items(&self.db, indexed_path)?;
                db::code_file_delete(&self.db, indexed_path)?;
                files_deleted += 1;
            }
        }

        let mut files_changed = 0usize;
        let mut symbols_indexed = 0usize;

        for file_path in &current_files {
            let canonical = {
                let rel = file_path.strip_prefix(&self.repo_path).unwrap_or(file_path);
                code::canonicalize_file_path(&self.repo_path, rel)
            };

            let metadata = match std::fs::metadata(file_path) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("Failed to stat {}: {}", file_path.display(), e);
                    continue;
                }
            };
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let source = match std::fs::read_to_string(file_path) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("Failed to read {}: {}", file_path.display(), e);
                    continue;
                }
            };

            let hash = db::content_hash(source.as_bytes());

            // Content hashes are authoritative: filesystem mtimes may have
            // only second precision, so an edit can share its prior mtime.
            if let Some((_stored_mtime, stored_hash)) = db::code_file_get(&self.db, &canonical)? {
                if stored_hash == hash {
                    db::code_file_upsert(&self.db, &canonical, mtime, &hash)?;
                    continue;
                }
            }

            let tx = self.db.transaction()?;

            db::delete_code_file_items(&tx, &canonical)?;

            let symbols = match code::extract_symbols_in_repo(&self.repo_path, file_path, &source) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("Failed to parse {}: {}", file_path.display(), e);
                    tx.rollback()?;
                    continue;
                }
            };

            let symbol_count = symbols.len();

            let items: Vec<db::ItemRow> = symbols
                .iter()
                .map(|s| {
                    let metadata = serde_json::json!({
                        "file_path": s.file_path,
                        "line_start": s.line_start,
                        "line_end": s.line_end,
                        "symbol_kind": format!("{:?}", s.symbol_kind).to_lowercase(),
                        "language": "rust",
                        "content_hash": hash,
                    })
                    .to_string();
                    db::ItemRow {
                        identifier: s.identifier.clone(),
                        text: s.text.clone(),
                        author: None,
                        metadata: Some(metadata),
                    }
                })
                .collect();

            let item_ids = db::insert_items(&tx, "code", &items)?;

            if let Some(ref embedder) = self.embedder {
                let embed_pairs: Vec<(usize, &str)> = symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.embed)
                    .map(|(i, s)| (i, s.text.as_str()))
                    .collect();

                if !embed_pairs.is_empty() {
                    let texts: Vec<String> =
                        embed_pairs.iter().map(|(_, t)| t.to_string()).collect();
                    let embeddings = embedder.encode_batch(&texts);
                    let ids: Vec<i64> = embed_pairs.iter().map(|(i, _)| item_ids[*i]).collect();
                    db::insert_vectors_into(&tx, "vec_code", &ids, &embeddings)?;
                }
            }

            db::code_file_upsert(&tx, &canonical, mtime, &hash)?;

            tx.commit()?;
            files_changed += 1;
            symbols_indexed += symbol_count;
        }

        Ok(CodeIndexReport {
            files_scanned: current_files.len(),
            files_changed,
            files_deleted,
            symbols_indexed,
        })
    }

    pub fn reindex_code(&mut self) -> anyhow::Result<CodeIndexReport> {
        // Delete vec_code rows FIRST (before items), since delete_source_vec
        // looks up item IDs from the items table.
        db::delete_source_vec(&self.db, "vec_code", "code")?;
        db::delete_source(&self.db, "code")?;
        // Also clean up code_files tracking table
        let tracked = db::code_files_all(&self.db)?;
        for path in tracked {
            db::code_file_delete(&self.db, &path)?;
        }
        self.index_code()
    }

    pub fn search_text(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        let hits = db::search_fts(&self.db, query, limit)?;
        let mut results = Vec::new();
        for (id, score) in hits {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(SearchResult {
                    identifier: detail.identifier,
                    short_hash: extract_short_hash(&detail.metadata),
                    text: detail.text,
                    author: detail.author,
                    score: score as f32,
                    match_type: MatchType::Fts,
                });
            }
        }
        Ok(results)
    }

    pub fn search_similar(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        let embedder = match &self.embedder {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        let query_embedding = embedder.encode_single(query);
        let hits = db::search_vec(&self.db, &query_embedding, limit)?;

        let mut results = Vec::new();
        for (id, score) in hits {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(SearchResult {
                    identifier: detail.identifier,
                    short_hash: extract_short_hash(&detail.metadata),
                    text: detail.text,
                    author: detail.author,
                    score: score as f32,
                    match_type: MatchType::Vector,
                });
            }
        }
        Ok(results)
    }

    pub fn search_hybrid(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        let fts_hits = db::search_fts(&self.db, query, limit)?;

        let vec_hits = if let Some(ref embedder) = self.embedder {
            let query_embedding = embedder.encode_single(query);
            db::search_vec(&self.db, &query_embedding, limit)?
        } else {
            Vec::new()
        };

        if vec_hits.is_empty() {
            let mut results = Vec::new();
            for (id, score) in &fts_hits {
                if let Some(detail) = lookup_item(&self.db, *id)? {
                    results.push(SearchResult {
                        identifier: detail.identifier,
                        short_hash: extract_short_hash(&detail.metadata),
                        text: detail.text,
                        author: detail.author,
                        score: *score as f32,
                        match_type: MatchType::Fts,
                    });
                }
            }
            return Ok(results);
        }

        let k: f64 = 60.0;
        let w_fts: f64 = 1.0;
        let w_vec: f64 = 1.0;

        let mut scores: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();

        for (pos, (id, _)) in fts_hits.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_fts / (k + rank);
        }

        for (pos, (id, _)) in vec_hits.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_vec / (k + rank);
        }

        let mut ranked: Vec<(i64, f64)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);

        let mut results = Vec::new();
        for (id, score) in ranked {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(SearchResult {
                    identifier: detail.identifier,
                    short_hash: extract_short_hash(&detail.metadata),
                    text: detail.text,
                    author: detail.author,
                    score: score as f32,
                    match_type: MatchType::Hybrid,
                });
            }
        }
        Ok(results)
    }

    pub fn search_code_text(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<CodeSearchResult>> {
        let hits = db::search_fts_filtered(&self.db, query, "code", limit)?;
        let mut results = Vec::new();
        for (id, score) in hits {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(parse_code_search_result(
                    &detail,
                    score as f32,
                    MatchType::Fts,
                ));
            }
        }
        Ok(results)
    }

    pub fn search_code_similar(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<CodeSearchResult>> {
        let embedder = match &self.embedder {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        let query_embedding = embedder.encode_single(query);
        let hits = db::search_vec_code(&self.db, &query_embedding, limit)?;

        let mut results = Vec::new();
        for (id, score) in hits {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(parse_code_search_result(
                    &detail,
                    score as f32,
                    MatchType::Vector,
                ));
            }
        }
        Ok(results)
    }

    pub fn search_code_hybrid(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<CodeSearchResult>> {
        let fts_hits = db::search_fts_filtered(&self.db, query, "code", limit)?;

        let vec_hits = if let Some(ref embedder) = self.embedder {
            let query_embedding = embedder.encode_single(query);
            db::search_vec_code(&self.db, &query_embedding, limit)?
        } else {
            Vec::new()
        };

        if vec_hits.is_empty() {
            let mut results = Vec::new();
            for (id, score) in &fts_hits {
                if let Some(detail) = lookup_item(&self.db, *id)? {
                    results.push(parse_code_search_result(
                        &detail,
                        *score as f32,
                        MatchType::Fts,
                    ));
                }
            }
            return Ok(results);
        }

        let k: f64 = 60.0;
        let w_fts: f64 = 1.0;
        let w_vec: f64 = 1.0;

        let mut scores: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();

        for (pos, (id, _)) in fts_hits.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_fts / (k + rank);
        }

        for (pos, (id, _)) in vec_hits.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_vec / (k + rank);
        }

        let mut ranked: Vec<(i64, f64)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);

        let mut results = Vec::new();
        for (id, score) in ranked {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(parse_code_search_result(
                    &detail,
                    score as f32,
                    MatchType::Hybrid,
                ));
            }
        }
        Ok(results)
    }

    pub fn index_documents(&mut self) -> anyhow::Result<DocumentIndexReport> {
        let current_files = documents::list_tracked_document_files(&self.repo_path)?;
        let indexed_files = db::document_files_all(&self.db)?;

        let current_set: std::collections::HashSet<String> = current_files
            .iter()
            .filter_map(|p| {
                let rel = p.strip_prefix(&self.repo_path).ok()?;
                Some(documents::canonicalize_file_path(&self.repo_path, rel))
            })
            .collect();

        let mut files_deleted = 0usize;
        for indexed_path in &indexed_files {
            if !current_set.contains(indexed_path) {
                db::delete_document_file_items(&self.db, indexed_path)?;
                db::document_file_delete(&self.db, indexed_path)?;
                files_deleted += 1;
            }
        }

        let mut files_changed = 0usize;
        let mut chunks_indexed = 0usize;

        for file_path in &current_files {
            let canonical = {
                let rel = file_path.strip_prefix(&self.repo_path).unwrap_or(file_path);
                documents::canonicalize_file_path(&self.repo_path, rel)
            };

            let metadata = match std::fs::metadata(file_path) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("Failed to stat {}: {}", file_path.display(), e);
                    continue;
                }
            };
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            if let Some((stored_mtime, _stored_hash)) = db::document_file_get(&self.db, &canonical)?
            {
                if stored_mtime == mtime {
                    continue;
                }
            }

            let source = match std::fs::read_to_string(file_path) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!(
                        "Failed to read (invalid UTF-8?): {}: {}",
                        file_path.display(),
                        e
                    );
                    continue;
                }
            };

            let hash = db::content_hash(source.as_bytes());

            if let Some((_stored_mtime, stored_hash)) = db::document_file_get(&self.db, &canonical)?
            {
                if stored_hash == hash {
                    db::document_file_upsert(&self.db, &canonical, mtime, &hash)?;
                    continue;
                }
            }

            let extension = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            let chunks = documents::extract_document_chunks(&canonical, &source, &extension);

            let tx = self.db.transaction()?;

            // Delete old items for this file
            db::delete_document_file_items(&tx, &canonical)?;

            let items: Vec<db::ItemRow> = chunks
                .iter()
                .map(|chunk| {
                    let identifier = if chunks.len() == 1 {
                        canonical.clone()
                    } else {
                        format!("{}::{}", canonical, chunk.ordinal)
                    };
                    let metadata = serde_json::json!({
                        "file_path": canonical,
                        "extension": extension,
                        "mtime": mtime,
                        "content_hash": hash,
                        "chunk_ordinal": chunk.ordinal,
                        "line_start": chunk.line_start,
                        "line_end": chunk.line_end,
                        "heading_path": chunk.heading_path,
                    })
                    .to_string();
                    db::ItemRow {
                        identifier,
                        text: chunk.text.clone(),
                        author: None,
                        metadata: Some(metadata),
                    }
                })
                .collect();

            let item_ids = db::insert_items(&tx, "document", &items)?;

            if let Some(ref embedder) = self.embedder {
                let texts: Vec<String> = items.iter().map(|i| i.text.clone()).collect();
                let embeddings = embedder.encode_batch(&texts);
                db::insert_vectors_into(&tx, "vec_documents", &item_ids, &embeddings)?;
            }

            db::document_file_upsert(&tx, &canonical, mtime, &hash)?;

            tx.commit()?;
            files_changed += 1;
            chunks_indexed += chunks.len();
        }

        Ok(DocumentIndexReport {
            files_scanned: current_files.len(),
            files_changed,
            files_deleted,
            chunks_indexed,
        })
    }

    pub fn reindex_documents(&mut self) -> anyhow::Result<DocumentIndexReport> {
        db::delete_source_vec(&self.db, "vec_documents", "document")?;
        db::delete_source(&self.db, "document")?;
        let tracked = db::document_files_all(&self.db)?;
        for path in tracked {
            db::document_file_delete(&self.db, &path)?;
        }
        self.index_documents()
    }

    pub fn search_document_text(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<DocumentSearchResult>> {
        let hits = db::search_fts_filtered(&self.db, query, "document", limit)?;
        let mut results = Vec::new();
        for (id, score) in hits {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(parse_document_search_result(
                    &detail,
                    score as f32,
                    MatchType::Fts,
                ));
            }
        }
        Ok(results)
    }

    pub fn search_document_similar(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<DocumentSearchResult>> {
        let embedder = match &self.embedder {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        let query_embedding = embedder.encode_single(query);
        let hits = db::search_vec_documents(&self.db, &query_embedding, limit)?;

        let mut results = Vec::new();
        for (id, score) in hits {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(parse_document_search_result(
                    &detail,
                    score as f32,
                    MatchType::Vector,
                ));
            }
        }
        Ok(results)
    }

    pub fn search_document_hybrid(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<DocumentSearchResult>> {
        let fts_hits = db::search_fts_filtered(&self.db, query, "document", limit)?;

        let vec_hits = if let Some(ref embedder) = self.embedder {
            let query_embedding = embedder.encode_single(query);
            db::search_vec_documents(&self.db, &query_embedding, limit)?
        } else {
            Vec::new()
        };

        if vec_hits.is_empty() {
            let mut results = Vec::new();
            for (id, score) in &fts_hits {
                if let Some(detail) = lookup_item(&self.db, *id)? {
                    results.push(parse_document_search_result(
                        &detail,
                        *score as f32,
                        MatchType::Fts,
                    ));
                }
            }
            return Ok(results);
        }

        let k: f64 = 60.0;
        let w_fts: f64 = 1.0;
        let w_vec: f64 = 1.0;

        let mut scores: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();

        for (pos, (id, _)) in fts_hits.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_fts / (k + rank);
        }

        for (pos, (id, _)) in vec_hits.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_vec / (k + rank);
        }

        let mut ranked: Vec<(i64, f64)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);

        let mut results = Vec::new();
        for (id, score) in ranked {
            if let Some(detail) = lookup_item(&self.db, id)? {
                results.push(parse_document_search_result(
                    &detail,
                    score as f32,
                    MatchType::Hybrid,
                ));
            }
        }
        Ok(results)
    }

    /// Search all three sources (git log, code, documents) with a single query.
    ///
    /// Returns a combined cross-source ranked result list plus source-specific
    /// groups derived from the same candidate set. The ranking gathers FTS and
    /// vector candidates from all three source-specific indexes, ranks them
    /// together, and applies RRF over the global rankings.
    pub fn search_all_hybrid(
        &self,
        query: &str,
        limit_per_source: usize,
    ) -> anyhow::Result<AllSourceSearchResult> {
        // Gather FTS candidates from all three sources
        let fts_commits = db::search_fts_filtered(&self.db, query, "commit", limit_per_source)?;
        let fts_code = db::search_fts_filtered(&self.db, query, "code", limit_per_source)?;
        let fts_documents = db::search_fts_filtered(&self.db, query, "document", limit_per_source)?;

        // Gather vector candidates from all three vec tables
        let (vec_commits, vec_code, vec_documents) = if let Some(ref embedder) = self.embedder {
            let query_embedding = embedder.encode_single(query);
            (
                db::search_vec(&self.db, &query_embedding, limit_per_source)?,
                db::search_vec_code(&self.db, &query_embedding, limit_per_source)?,
                db::search_vec_documents(&self.db, &query_embedding, limit_per_source)?,
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };

        // Build source_type lookup for all candidate IDs
        let all_fts = [&fts_commits[..], &fts_code[..], &fts_documents[..]].concat();
        let all_vec = [&vec_commits[..], &vec_code[..], &vec_documents[..]].concat();

        // RRF with global rankings across all sources
        let k: f64 = 60.0;
        let w_fts: f64 = 1.0;
        let w_vec: f64 = 1.0;

        // Track which source each ID belongs to
        let mut id_source: std::collections::HashMap<i64, &str> = std::collections::HashMap::new();
        for (id, _) in &fts_commits {
            id_source.insert(*id, "commit");
        }
        for (id, _) in &fts_code {
            id_source.insert(*id, "code");
        }
        for (id, _) in &fts_documents {
            id_source.insert(*id, "document");
        }
        // Vector results may add more IDs
        for (id, _) in &vec_commits {
            id_source.entry(*id).or_insert("commit");
        }
        for (id, _) in &vec_code {
            id_source.entry(*id).or_insert("code");
        }
        for (id, _) in &vec_documents {
            id_source.entry(*id).or_insert("document");
        }

        let mut scores: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();

        for (pos, (id, _)) in all_fts.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_fts / (k + rank);
        }

        for (pos, (id, _)) in all_vec.iter().enumerate() {
            let rank = (pos + 1) as f64;
            let entry = scores.entry(*id).or_insert(0.0);
            *entry += w_vec / (k + rank);
        }

        let mut ranked: Vec<(i64, f64)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit_per_source * 3);

        let mut combined = Vec::new();
        let mut git_log_results = Vec::new();
        let mut code_results = Vec::new();
        let mut document_results = Vec::new();

        for (id, score) in ranked {
            let detail = match lookup_item(&self.db, id)? {
                Some(d) => d,
                None => continue,
            };
            let source = id_source.get(&id).copied().unwrap_or("commit");

            match source {
                "commit" => {
                    let result = SearchResult {
                        identifier: detail.identifier.clone(),
                        short_hash: extract_short_hash(&detail.metadata),
                        text: detail.text.clone(),
                        author: detail.author.clone(),
                        score: score as f32,
                        match_type: MatchType::Hybrid,
                    };
                    combined.push(UnifiedSearchHit::GitLog(SearchResult {
                        identifier: result.identifier.clone(),
                        short_hash: result.short_hash.clone(),
                        text: result.text.clone(),
                        author: result.author.clone(),
                        score: result.score,
                        match_type: result.match_type,
                    }));
                    git_log_results.push(result);
                }
                "code" => {
                    let result = parse_code_search_result(&detail, score as f32, MatchType::Hybrid);
                    combined.push(UnifiedSearchHit::Code(CodeSearchResult {
                        identifier: result.identifier.clone(),
                        symbol_kind: result.symbol_kind.clone(),
                        file_path: result.file_path.clone(),
                        line_start: result.line_start,
                        line_end: result.line_end,
                        text: result.text.clone(),
                        score: result.score,
                        match_type: result.match_type,
                    }));
                    code_results.push(result);
                }
                _ => {
                    let result =
                        parse_document_search_result(&detail, score as f32, MatchType::Hybrid);
                    combined.push(UnifiedSearchHit::Document(DocumentSearchResult {
                        identifier: result.identifier.clone(),
                        file_path: result.file_path.clone(),
                        text: result.text.clone(),
                        score: result.score,
                        match_type: result.match_type,
                    }));
                    document_results.push(result);
                }
            }
        }

        Ok(AllSourceSearchResult {
            combined,
            git_log: git_log_results,
            code: code_results,
            documents: document_results,
        })
    }
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Fixture: a small Rust source file with a function, struct, and comment.
    const FIXTURE_A: &str = r#"
/// Validates the cart contents before checkout.
fn validate_cart(items: &[String]) -> bool {
    !items.is_empty()
}

struct Cart {
    items: Vec<String>,
}
"#;

    /// Fixture: a second file with different symbols.
    const FIXTURE_B: &str = r#"
use std::collections::HashMap;

// Maximum number of items allowed per cart.
const MAX_ITEMS: usize = 100;

fn calculate_total(prices: &HashMap<String, f64>) -> f64 {
    prices.values().sum()
}
"#;

    /// Run a git command in the given directory, panicking on failure.
    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .args(args)
            .status()
            .expect("failed to run git");
        assert!(status.success(), "git {:?} failed", args);
    }

    /// Create a temp dir with a Git repo, write fixture files, commit them,
    /// and return (repo_dir, cache_dir) — both TempDir so they clean up.
    fn setup_repo() -> (tempfile::TempDir, tempfile::TempDir) {
        let repo_dir = tempfile::tempdir().expect("repo tempdir");
        let cache_dir = tempfile::tempdir().expect("cache tempdir");

        // init repo
        git(repo_dir.path(), &["init"]);
        git(repo_dir.path(), &["config", "user.email", "test@test.com"]);
        git(repo_dir.path(), &["config", "user.name", "Test"]);

        // Write fixture files
        fs::create_dir_all(repo_dir.path().join("src")).unwrap();
        fs::write(repo_dir.path().join("src/cart.rs"), FIXTURE_A).unwrap();
        fs::write(repo_dir.path().join("src/pricing.rs"), FIXTURE_B).unwrap();

        // commit
        git(repo_dir.path(), &["add", "."]);
        git(repo_dir.path(), &["commit", "-m", "initial"]);

        (repo_dir, cache_dir)
    }

    /// Build an Indexer pointing at the repo with FTS5-only (empty model name).
    fn make_indexer(repo_dir: &std::path::Path, cache_dir: &std::path::Path) -> Indexer {
        let config = IndexerConfig {
            cache_dir: Some(cache_dir.to_path_buf()),
            model_name: String::new(),
        };
        Indexer::new(repo_dir, &config).expect("Indexer::new")
    }

    #[test]
    fn first_index_all_tracked_files() {
        let (repo_dir, cache_dir) = setup_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let report = indexer.index_code().expect("index_code");

        assert_eq!(report.files_scanned, 2, "should scan 2 tracked .rs files");
        assert_eq!(report.files_changed, 2, "both files are new");
        assert_eq!(report.files_deleted, 0);
        assert!(report.symbols_indexed > 0, "should index some symbols");

        // Verify items exist in DB via FTS search
        let results = indexer
            .search_code_text("validate_cart", 10)
            .expect("search_code_text");
        assert!(
            !results.is_empty(),
            "should find validate_cart via FTS after first index"
        );
    }

    #[test]
    fn second_unchanged_run_reports_zero_changes() {
        let (repo_dir, cache_dir) = setup_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let first = indexer.index_code().expect("first index");
        assert_eq!(first.files_changed, 2);

        let second = indexer.index_code().expect("second index");
        assert_eq!(second.files_scanned, 2);
        assert_eq!(second.files_changed, 0, "no files changed");
        assert_eq!(second.files_deleted, 0);
        assert_eq!(second.symbols_indexed, 0);
    }

    #[test]
    fn touch_without_content_change_does_not_reindex() {
        let (repo_dir, cache_dir) = setup_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        indexer.index_code().expect("first index");

        // Touch a file: update mtime but keep content identical
        let path = repo_dir.path().join("src/cart.rs");
        let content = fs::read_to_string(&path).unwrap();
        // Write same content back — filesystem mtime will update
        fs::write(&path, &content).unwrap();

        // Re-add to git index so it's still tracked
        git(repo_dir.path(), &["add", "src/cart.rs"]);

        let report = indexer.index_code().expect("second index after touch");

        // The mtime changed, so the file passes the mtime check, but the
        // content hash is the same so the indexer skips re-inserting items.
        assert_eq!(
            report.files_changed, 0,
            "touch-only should not count as changed"
        );
        assert_eq!(report.symbols_indexed, 0);

        // code_files tracking row should still exist with the same hash
        let canonical =
            code::canonicalize_file_path(repo_dir.path(), std::path::Path::new("src/cart.rs"));
        let db_path = {
            // open the DB directly to inspect code_files
            let db_file = {
                let abs = fs::canonicalize(repo_dir.path()).unwrap();
                let abs_str = abs.to_string_lossy();
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
                let mut h: u64 = 0;
                for b in abs_str.bytes() {
                    h = h.wrapping_mul(31).wrapping_add(b as u64);
                }
                let hash = format!("{:06x}", h & 0xFFFFFF);
                let slug = format!("{}_{}", stem, hash);
                cache_dir.path().join(format!("{}.db", slug))
            };
            let conn = rusqlite::Connection::open_with_flags(
                &db_file,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let row: Option<(i64, String)> = conn
                .query_row(
                    "SELECT mtime, content_hash FROM code_files WHERE file_path = ?",
                    [&canonical],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            row
        };
        assert!(db_path.is_some(), "code_files row should exist");
        let (_, hash) = db_path.unwrap();
        assert_eq!(hash, db::content_hash(content.as_bytes()));
    }

    #[test]
    fn content_edit_replaces_evidence_records() {
        let (repo_dir, cache_dir) = setup_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        indexer.index_code().expect("first index");

        // Verify original function is searchable
        let results = indexer
            .search_code_text("validate_cart", 10)
            .expect("search");
        assert!(!results.is_empty(), "should find validate_cart before edit");

        // Edit the file: replace validate_cart with check_inventory
        let new_content = r#"
/// Checks inventory levels before shipping.
fn check_inventory(sku: &str) -> bool {
    sku.starts_with("INV")
}

struct Warehouse {
    location: String,
}
"#;
        let path = repo_dir.path().join("src/cart.rs");
        fs::write(&path, new_content).unwrap();
        git(repo_dir.path(), &["add", "src/cart.rs"]);
        git(repo_dir.path(), &["commit", "-m", "edit cart"]);

        let report = indexer.index_code().expect("index after edit");
        assert_eq!(report.files_changed, 1, "one file was edited");
        assert!(report.symbols_indexed > 0);

        // Old function should no longer be found
        let old_results = indexer
            .search_code_text("validate_cart", 10)
            .expect("search old");
        assert!(
            old_results.is_empty(),
            "old function name should be gone after edit"
        );

        // New function should be found
        let new_results = indexer
            .search_code_text("check_inventory", 10)
            .expect("search new");
        assert!(
            !new_results.is_empty(),
            "new function should be findable after edit"
        );
    }

    #[test]
    fn file_deletion_removes_records() {
        let (repo_dir, cache_dir) = setup_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        indexer.index_code().expect("first index");

        // Delete a tracked file
        fs::remove_file(repo_dir.path().join("src/pricing.rs")).unwrap();
        git(repo_dir.path(), &["add", "-A"]);
        git(repo_dir.path(), &["commit", "-m", "remove pricing"]);

        let report = indexer.index_code().expect("index after deletion");
        assert_eq!(report.files_deleted, 1, "one file was deleted");
        assert_eq!(report.files_scanned, 1, "only cart.rs remains");

        // pricing.rs symbols should no longer be searchable
        let results = indexer
            .search_code_text("calculate_total", 10)
            .expect("search deleted");
        assert!(
            results.is_empty(),
            "deleted file's symbols should not appear"
        );
    }

    /// Checkout feature fixture: covers product capability, supporting type,
    /// nested module, impl method, standalone comment, doc comments, and imports.
    /// Mirrors the CHECKOUT_FIXTURE in code.rs extraction tests.
    const CHECKOUT_FIXTURE: &str = r#"use std::collections::HashMap;

/// Process a checkout session for the given cart.
///
/// Validates the cart contents, applies any active discounts,
/// and delegates to the payment provider for charging.
fn create_checkout_session(cart: &HashMap<String, i32>) -> Result<String, String> {
    if cart.is_empty() {
        return Err("cart is empty".into());
    }
    let total: i32 = cart.values().sum();
    if total <= 0 {
        return Err("total must be positive".into());
    }
    Ok(format!("session_{}", total))
}

mod payments {
    pub fn helper() -> bool {
        true
    }
}

struct PaymentGateway;

impl PaymentGateway {
    fn process_payment(&self) -> bool {
        true
    }
}

const MAX_RETRIES: u32 = 3;

// TODO: temporary limitation — only USD currency is supported right now
use std::sync::Arc;
"#;

    /// Create a temp Git repo with the checkout fixture committed as src/checkout.rs.
    /// Returns (repo_dir, cache_dir) — both TempDir so they clean up.
    fn setup_checkout_repo() -> (tempfile::TempDir, tempfile::TempDir) {
        let repo_dir = tempfile::tempdir().expect("repo tempdir");
        let cache_dir = tempfile::tempdir().expect("cache tempdir");

        git(repo_dir.path(), &["init"]);
        git(repo_dir.path(), &["config", "user.email", "test@test.com"]);
        git(repo_dir.path(), &["config", "user.name", "Test"]);

        fs::create_dir_all(repo_dir.path().join("src")).unwrap();
        fs::write(repo_dir.path().join("src/checkout.rs"), CHECKOUT_FIXTURE).unwrap();

        git(repo_dir.path(), &["add", "."]);
        git(repo_dir.path(), &["commit", "-m", "initial"]);

        (repo_dir, cache_dir)
    }

    // ── Epic 008 Task 3: Deterministic evidence-retrieval tests ───────────

    #[test]
    fn search_checkout_validation_returns_evidence() {
        let (repo_dir, cache_dir) = setup_checkout_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        indexer.index_code().expect("index_code");

        // FTS5 default tokenizer (unicode61) doesn't stem, so "validation"
        // won't match "validates". Using OR ensures either term matches.
        let results = indexer
            .search_code_text("checkout OR validation", 10)
            .expect("search_code_text");

        assert!(
            !results.is_empty(),
            "'checkout OR validation' should return at least one result"
        );

        let hit = results
            .iter()
            .find(|r| r.identifier.contains("create_checkout_session"))
            .expect("expected create_checkout_session in results");

        assert_eq!(hit.file_path, "src/checkout.rs");
        assert!(hit.line_start > 0, "line_start should be non-zero");
        assert!(hit.line_end >= hit.line_start, "line_end >= line_start");
        assert!(
            matches!(hit.symbol_kind, code::SymbolKind::Function),
            "expected Function, got {:?}",
            hit.symbol_kind
        );
        assert!(
            hit.text.contains("checkout") || hit.text.contains("valid"),
            "text should contain relevant content; got:\n{}",
            hit.text
        );
    }

    #[test]
    fn search_payment_gateway_returns_evidence() {
        let (repo_dir, cache_dir) = setup_checkout_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        indexer.index_code().expect("index_code");

        // FTS5 unicode61 tokenizes "PaymentGateway" as a single token, so
        // the phrase "payment gateway" won't match. Using OR catches the
        // struct via "payment" (in its doc comment and identifier) and the
        // impl method via "payment" (in its parent type name).
        let results = indexer
            .search_code_text("payment OR gateway", 10)
            .expect("search_code_text");

        assert!(
            !results.is_empty(),
            "'payment OR gateway' should return at least one result"
        );

        let hit = results
            .iter()
            .find(|r| {
                r.identifier.contains("PaymentGateway")
                    || matches!(r.symbol_kind, code::SymbolKind::Struct)
                    || matches!(r.symbol_kind, code::SymbolKind::ImplMethod)
            })
            .expect("expected PaymentGateway struct or process_payment method in results");

        assert_eq!(hit.file_path, "src/checkout.rs");
        assert!(hit.line_start > 0);
        assert!(hit.line_end >= hit.line_start);
        assert!(!hit.text.is_empty(), "result text should not be empty");
    }

    #[test]
    fn search_temporary_limitation_returns_comment_evidence() {
        let (repo_dir, cache_dir) = setup_checkout_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        indexer.index_code().expect("index_code");

        // "temporary limitation" is a contiguous phrase in the standalone
        // comment, so the phrase query works directly.
        let results = indexer
            .search_code_text("temporary limitation", 10)
            .expect("search_code_text");

        assert!(
            !results.is_empty(),
            "'temporary limitation' should return at least one result"
        );

        let hit = results
            .iter()
            .find(|r| r.text.contains("temporary limitation") || r.text.contains("USD"))
            .expect("expected the standalone comment with 'temporary limitation' in results");

        assert_eq!(hit.file_path, "src/checkout.rs");
        assert!(hit.line_start > 0);
        assert!(hit.line_end >= hit.line_start);
        assert!(
            matches!(hit.symbol_kind, code::SymbolKind::Comments),
            "expected Comments kind, got {:?}",
            hit.symbol_kind
        );
        assert!(
            hit.text.contains("USD currency"),
            "comment text should mention USD currency; got:\n{}",
            hit.text
        );
    }

    #[test]
    fn untracked_rs_file_is_excluded() {
        let (repo_dir, cache_dir) = setup_repo();

        // Write an untracked .rs file (not git-added)
        fs::write(
            repo_dir.path().join("src/secret.rs"),
            "fn leaked_secret() -> &'static str { \"password\" }\n",
        )
        .unwrap();

        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        let report = indexer.index_code().expect("index_code");

        // Only the 2 committed files should be scanned
        assert_eq!(report.files_scanned, 2, "untracked file must be excluded");

        // The untracked function should not be searchable
        let results = indexer
            .search_code_text("leaked_secret", 10)
            .expect("search");
        assert!(
            results.is_empty(),
            "untracked file's symbols must not be indexed"
        );
    }
}
