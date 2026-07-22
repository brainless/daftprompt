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

        let db_path = db::db_path_for_repo(repo_path)?;
        let db = Connection::open(&db_path)?;

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

        db::init_schema(&db, dim)?;

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

            if let Some((stored_mtime, _stored_hash)) = db::code_file_get(&self.db, &canonical)? {
                if stored_mtime == mtime {
                    continue;
                }
            }

            let source = match std::fs::read_to_string(file_path) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("Failed to read {}: {}", file_path.display(), e);
                    continue;
                }
            };

            let hash = db::content_hash(source.as_bytes());

            if let Some((_stored_mtime, stored_hash)) = db::code_file_get(&self.db, &canonical)? {
                if stored_hash == hash {
                    db::code_file_upsert(&self.db, &canonical, mtime, &hash)?;
                    continue;
                }
            }

            let tx = self.db.transaction()?;

            db::delete_code_file_items(&tx, &canonical)?;

            let symbols = match code::extract_symbols(file_path, &source) {
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

            if let Some((stored_mtime, _stored_hash)) =
                db::document_file_get(&self.db, &canonical)?
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

            if let Some((_stored_mtime, stored_hash)) =
                db::document_file_get(&self.db, &canonical)?
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

            let chunks =
                documents::extract_document_chunks(&canonical, &source, &extension);

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
        let fts_documents =
            db::search_fts_filtered(&self.db, query, "document", limit_per_source)?;

        // Gather vector candidates from all three vec tables
        let (vec_commits, vec_code, vec_documents) =
            if let Some(ref embedder) = self.embedder {
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
