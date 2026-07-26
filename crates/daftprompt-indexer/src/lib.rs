pub mod code;
pub mod db;
pub mod documents;
pub mod embed;

pub use code::{CodeLanguage, SymbolKind};

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
    /// Per-dialect extractors built once at construction (Epic 009 DD #1:
    /// queries are compiled at construction, not per file). Rust, TS, TSX,
    /// JavaScript, and JSX are all present so the dispatch arm in
    /// [`Indexer::index_code`] cannot fall through to a previous dialect.
    rust_extractor: code::CodeExtractor,
    typescript_extractor: code::CodeExtractor,
    tsx_extractor: code::CodeExtractor,
    javascript_extractor: code::CodeExtractor,
    jsx_extractor: code::CodeExtractor,
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

/// Parse a stored symbol-kind key.
///
/// Epic 009 Design Decision #3: unknown values no longer collapse to
/// `Function`; they surface as `SymbolKind::Unknown(original)` so callers
/// can preserve diagnostic context.
fn parse_symbol_kind(s: &str) -> code::SymbolKind {
    code::SymbolKind::from_str_key(s)
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
            // Build per-dialect extractors once at indexer construction
            // (Epic 009 Design Decision #1: queries are compiled at
            // construction, not per file).
            rust_extractor: code::CodeExtractor::rust(),
            typescript_extractor: code::CodeExtractor::for_language(code::CodeLanguage::TypeScript),
            tsx_extractor: code::CodeExtractor::for_language(code::CodeLanguage::Tsx),
            // Epic 010 Task 1: build JavaScript and JSX extractors here so
            // `index_code` cannot fall through to Rust / TypeScript for
            // `.js` or `.jsx` files. Both share the same grammar (the JS
            // scanner accepts JSX text natively) but keep distinct
            // metadata via the enum variant.
            javascript_extractor: code::CodeExtractor::for_language(code::CodeLanguage::JavaScript),
            jsx_extractor: code::CodeExtractor::for_language(code::CodeLanguage::Jsx),
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
        // Epic 009 Design Decision #2: one tracked-file traversal that
        // accepts the supported extension set. Epic 010 Task 1 adds
        // `.js` and `.jsx` here so deletion reconciliation and the
        // generated/vendor filter run across all five supported dialects
        // in a single pass.
        //
        // Rejected alternative: keep `list_tracked_rust_files` and add a
        // separate `list_tracked_ts_files` later. That would double the
        // HEAD-tree walk and complicate deletion-set reconciliation.
        let current_files = code::list_tracked_code_files(
            &self.repo_path,
            &[".rs", ".ts", ".tsx", ".js", ".jsx"],
        )?;
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

            // Epic 009 Design Decision #1: production indexing must route
            // by extension and refuse unsupported dialects rather than
            // silently falling back to Rust parsing.
            let extension = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let language = match code::language_for_extension(extension) {
                Some(lang) => lang,
                None => {
                    log::warn!(
                        "Skipping unsupported code file extension '{}': {}",
                        extension,
                        file_path.display()
                    );
                    continue;
                }
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

            // Dispatch by language: each dialect has its own pre-built
            // extractor so the compiled tree-sitter query is reused
            // (Epic 009 Design Decision #1). Five arms, all five
            // supported languages wired — `.js` cannot fall through to
            // Rust and `.jsx` cannot fall through to TypeScript.
            let extractor: &code::CodeExtractor = match language {
                code::CodeLanguage::Rust => &self.rust_extractor,
                code::CodeLanguage::TypeScript => &self.typescript_extractor,
                code::CodeLanguage::Tsx => &self.tsx_extractor,
                code::CodeLanguage::JavaScript => &self.javascript_extractor,
                code::CodeLanguage::Jsx => &self.jsx_extractor,
            };
            let symbols = match code::extract_symbols_with_extractor(
                extractor,
                &self.repo_path,
                file_path,
                &source,
            ) {
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
                    // Epic 009 Design Decision #3: use the explicit
                    // storage key from `SymbolKind::as_str` instead of
                    // `format!("{:?}", ...).to_lowercase()`. The debug
                    // formatting produces unhelpful strings for
                    // `Unknown(...)` and is otherwise brittle to future
                    // variant renames.
                    let metadata = serde_json::json!({
                        "file_path": s.file_path,
                        "line_start": s.line_start,
                        "line_end": s.line_end,
                        "symbol_kind": s.symbol_kind.as_str(),
                        "language": language.as_str(),
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
    use filetime::{set_file_mtime, FileTime};
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

    /// Create a temp Git repo containing the supplied repo-relative fixtures.
    /// Returns (repo_dir, cache_dir) — both TempDir so they clean up.
    fn setup_repo_with_files(files: &[(&str, &str)]) -> (tempfile::TempDir, tempfile::TempDir) {
        let repo_dir = tempfile::tempdir().expect("repo tempdir");
        let cache_dir = tempfile::tempdir().expect("cache tempdir");

        git(repo_dir.path(), &["init"]);
        git(repo_dir.path(), &["config", "user.email", "test@test.com"]);
        git(repo_dir.path(), &["config", "user.name", "Test"]);

        for (relative_path, contents) in files {
            let path = repo_dir.path().join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, contents).unwrap();
        }

        git(repo_dir.path(), &["add", "."]);
        git(repo_dir.path(), &["commit", "-m", "initial"]);

        (repo_dir, cache_dir)
    }

    /// Rust-fixture compatibility wrapper used by the Epic 008 tests.
    fn setup_repo() -> (tempfile::TempDir, tempfile::TempDir) {
        setup_repo_with_files(&[("src/cart.rs", FIXTURE_A), ("src/pricing.rs", FIXTURE_B)])
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

        let path = repo_dir.path().join("src/cart.rs");
        let content = fs::read_to_string(&path).unwrap();
        let canonical =
            code::canonicalize_file_path(repo_dir.path(), std::path::Path::new("src/cart.rs"));
        let (old_mtime, old_hash) = db::code_file_get(&indexer.db, &canonical)
            .expect("read initial code_files row")
            .expect("initial code_files row should exist");

        // Set a known later timestamp rather than relying on filesystem clock
        // precision; content remains identical, so this must not re-index items.
        let updated_mtime = old_mtime + 2;
        set_file_mtime(&path, FileTime::from_unix_time(updated_mtime, 0)).expect("set later mtime");

        let report = indexer.index_code().expect("second index after touch");

        assert_eq!(
            report.files_changed, 0,
            "touch-only should not count as changed"
        );
        assert_eq!(report.symbols_indexed, 0);

        let (stored_mtime, stored_hash) = db::code_file_get(&indexer.db, &canonical)
            .expect("read updated code_files row")
            .expect("updated code_files row should exist");
        assert_eq!(stored_mtime, updated_mtime, "tracking mtime should update");
        assert_eq!(stored_hash, old_hash, "content hash should not change");
        assert_eq!(stored_hash, db::content_hash(content.as_bytes()));
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
    fn setup_checkout_repo() -> (tempfile::TempDir, tempfile::TempDir) {
        setup_repo_with_files(&[("src/checkout.rs", CHECKOUT_FIXTURE)])
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

    const TYPESCRIPT_FIXTURE: &str = r#"/** Validates a typed cart before checkout. */
export function validateTypedCart(items: string[]): boolean {
    return items.length > 0;
}

export interface TypedCart {
    items: string[];
}
"#;

    const TYPESCRIPT_SECOND_FIXTURE: &str = r#"export const calculateTypedTotal = (prices: number[]): number => {
    return prices.reduce((sum, price) => sum + price, 0);
};
"#;

    const TSX_FIXTURE: &str = r#"import React from "react";

interface CheckoutButtonProps {
    label: string;
}

/** Renders the typed checkout control. */
export function TypedCheckoutButton({ label }: CheckoutButtonProps) {
    return <button>{label}</button>;
}
"#;

    struct LifecycleCase<'a> {
        files: &'a [(&'a str, &'a str)],
        primary_path: &'a str,
        deleted_path: &'a str,
        initial_query: &'a str,
        initial_identifier: &'a str,
        deleted_query: &'a str,
        edited_contents: &'a str,
        edited_query: &'a str,
        edited_identifier: &'a str,
        untracked_path: &'a str,
        untracked_contents: &'a str,
        untracked_query: &'a str,
        language: &'a str,
        kind: code::SymbolKind,
    }

    fn assert_code_hit(
        indexer: &Indexer,
        query: &str,
        identifier: &str,
        path: &str,
        language: &str,
        kind: &code::SymbolKind,
    ) {
        let results = indexer.search_code_text(query, 20).expect("search_code_text");
        let hit = results
            .iter()
            .find(|result| result.identifier == identifier)
            .unwrap_or_else(|| panic!("expected {identifier} for query {query:?}"));
        assert_eq!(hit.file_path, path);
        assert!(hit.line_start > 0, "line_start should be one-based");
        assert!(hit.line_end >= hit.line_start);
        assert_eq!(&hit.symbol_kind, kind);
        assert!(hit.text.contains(query), "text should contain {query:?}:\n{}", hit.text);

        let metadata: String = indexer
            .db
            .query_row(
                "SELECT metadata FROM items WHERE source_type = 'code' AND identifier = ?1",
                [identifier],
                |row| row.get(0),
            )
            .expect("code item metadata");
        let metadata: serde_json::Value = serde_json::from_str(&metadata).expect("valid metadata");
        assert_eq!(metadata["language"], language);
        assert_eq!(metadata["file_path"], path);
        assert_eq!(metadata["line_start"], hit.line_start);
        assert_eq!(metadata["line_end"], hit.line_end);
        assert_eq!(metadata["symbol_kind"], kind.as_str());
    }

    fn run_language_lifecycle(case: &LifecycleCase<'_>) {
        let (repo_dir, cache_dir) = setup_repo_with_files(case.files);
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let first = indexer.index_code().expect("first index");
        assert_eq!(first.files_scanned, case.files.len());
        assert_eq!(first.files_changed, case.files.len());
        assert_eq!(first.files_deleted, 0);
        assert!(first.symbols_indexed > 0);
        assert_code_hit(
            &indexer,
            case.initial_query,
            case.initial_identifier,
            case.primary_path,
            case.language,
            &case.kind,
        );

        let unchanged = indexer.index_code().expect("unchanged index");
        assert_eq!(unchanged.files_scanned, case.files.len());
        assert_eq!(unchanged.files_changed, 0);
        assert_eq!(unchanged.files_deleted, 0);
        assert_eq!(unchanged.symbols_indexed, 0);

        let primary = repo_dir.path().join(case.primary_path);
        let canonical = code::canonicalize_file_path(repo_dir.path(), primary.as_path());
        let (old_mtime, old_hash) = db::code_file_get(&indexer.db, &canonical)
            .expect("tracking lookup")
            .expect("tracked primary fixture");
        set_file_mtime(&primary, FileTime::from_unix_time(old_mtime + 2, 0)).expect("touch fixture");
        let touched = indexer.index_code().expect("touch index");
        assert_eq!(touched.files_changed, 0);
        assert_eq!(touched.symbols_indexed, 0);
        let (new_mtime, new_hash) = db::code_file_get(&indexer.db, &canonical)
            .expect("tracking lookup")
            .expect("tracked primary fixture");
        assert_eq!(new_mtime, old_mtime + 2);
        assert_eq!(new_hash, old_hash);

        fs::write(&primary, case.edited_contents).expect("edit fixture");
        git(repo_dir.path(), &["add", case.primary_path]);
        git(repo_dir.path(), &["commit", "-m", "edit fixture"]);
        let edited = indexer.index_code().expect("edit index");
        assert_eq!(edited.files_changed, 1);
        assert!(edited.symbols_indexed > 0);
        assert!(indexer
            .search_code_text(case.initial_query, 20)
            .expect("search old evidence")
            .iter()
            .all(|result| result.identifier != case.initial_identifier));
        assert_code_hit(
            &indexer,
            case.edited_query,
            case.edited_identifier,
            case.primary_path,
            case.language,
            &case.kind,
        );

        fs::remove_file(repo_dir.path().join(case.deleted_path)).expect("delete fixture");
        git(repo_dir.path(), &["add", "-A"]);
        git(repo_dir.path(), &["commit", "-m", "delete fixture"]);
        let deleted = indexer.index_code().expect("deletion index");
        assert_eq!(deleted.files_deleted, 1);
        assert!(indexer
            .search_code_text(case.deleted_query, 20)
            .expect("search deleted evidence")
            .is_empty());

        let deleted_canonical = code::canonicalize_file_path(
            repo_dir.path(),
            std::path::Path::new(case.deleted_path),
        );
        assert!(
            db::code_file_get(&indexer.db, &deleted_canonical)
                .expect("lookup deleted code_files row")
                .is_none(),
            "code_files row for {deleted_canonical} must be removed after tracked deletion"
        );
        let tracked = db::code_files_all(&indexer.db).expect("list tracked code files");
        assert!(
            !tracked.iter().any(|path| path == &deleted_canonical),
            "code_files_all must not contain {deleted_canonical}; got {tracked:?}"
        );

        let untracked = repo_dir.path().join(case.untracked_path);
        fs::create_dir_all(untracked.parent().expect("untracked parent")).unwrap();
        fs::write(untracked, case.untracked_contents).expect("write untracked fixture");
        let excluded = indexer.index_code().expect("untracked index");
        assert_eq!(excluded.files_scanned, 1);
        assert_eq!(excluded.files_changed, 0);
        assert!(indexer
            .search_code_text(case.untracked_query, 20)
            .expect("search untracked evidence")
            .is_empty());
    }

    #[test]
    fn typescript_indexing_lifecycle_contract() {
        let files = [
            ("src/cart.ts", TYPESCRIPT_FIXTURE),
            ("src/pricing.ts", TYPESCRIPT_SECOND_FIXTURE),
        ];
        run_language_lifecycle(&LifecycleCase {
            files: &files,
            primary_path: "src/cart.ts",
            deleted_path: "src/pricing.ts",
            initial_query: "validateTypedCart",
            initial_identifier: "src/cart.ts::validateTypedCart",
            deleted_query: "calculateTypedTotal",
            edited_contents: "export function checkTypedInventory(sku: string): boolean {\n    return sku.startsWith(\"INV\");\n}\n",
            edited_query: "checkTypedInventory",
            edited_identifier: "src/cart.ts::checkTypedInventory",
            untracked_path: "src/secret.ts",
            untracked_contents: "export function leakedTypedSecret() { return 'password'; }\n",
            untracked_query: "leakedTypedSecret",
            language: "typescript",
            kind: code::SymbolKind::Function,
        });
    }

    #[test]
    fn tsx_indexing_lifecycle_contract() {
        let files = [
            ("src/components/CheckoutButton.tsx", TSX_FIXTURE),
            ("src/components/Secondary.tsx", "export function SecondaryControl() { return <span>secondary</span>; }\n"),
        ];
        run_language_lifecycle(&LifecycleCase {
            files: &files,
            primary_path: "src/components/CheckoutButton.tsx",
            deleted_path: "src/components/Secondary.tsx",
            initial_query: "TypedCheckoutButton",
            initial_identifier: "src/components/CheckoutButton.tsx::TypedCheckoutButton",
            deleted_query: "SecondaryControl",
            edited_contents: "export function ConfirmCheckoutButton() {\n    return <button>confirm</button>;\n}\n",
            edited_query: "ConfirmCheckoutButton",
            edited_identifier: "src/components/CheckoutButton.tsx::ConfirmCheckoutButton",
            untracked_path: "src/components/Secret.tsx",
            untracked_contents: "export function LeakedTsxSecret() { return <div>password</div>; }\n",
            untracked_query: "LeakedTsxSecret",
            language: "tsx",
            kind: code::SymbolKind::Function,
        });
    }

    const JS_LIFECYCLE_PRIMARY: &str = r#"/**
 * Validate the cart contents before checkout.
 */
function validateCart(items) {
    if (!items || items.length === 0) {
        return false;
    }
    return true;
}

const DEFAULT_TIMEOUT = 3000;
"#;

    const JS_LIFECYCLE_SECONDARY: &str = r#"const calculateTotal = (prices) => {
    return prices.reduce((sum, price) => sum + price, 0);
};
"#;

    const JSX_LIFECYCLE_PRIMARY: &str = r#"import React from "react";

/**
 * Renders the checkout button.
 *
 * The button stays disabled while checkout validation runs.
 */
export function CheckoutButton({ disabled, label }) {
    return (
        <button disabled={disabled} onClick={() => alert(label)}>
            {label}
        </button>
    );
}
"#;

    const JSX_LIFECYCLE_SECONDARY: &str = r#"export function SecondaryControl() {
    return <span>secondary</span>;
}
"#;

    #[test]
    fn javascript_indexing_lifecycle_contract() {
        let files = [
            ("src/cart.js", JS_LIFECYCLE_PRIMARY),
            ("src/pricing.js", JS_LIFECYCLE_SECONDARY),
        ];
        run_language_lifecycle(&LifecycleCase {
            files: &files,
            primary_path: "src/cart.js",
            deleted_path: "src/pricing.js",
            initial_query: "validateCart",
            initial_identifier: "src/cart.js::validateCart",
            deleted_query: "calculateTotal",
            edited_contents: "function checkCart(sku) {\n    return sku.startsWith(\"INV\");\n}\n",
            edited_query: "checkCart",
            edited_identifier: "src/cart.js::checkCart",
            untracked_path: "src/secret.js",
            untracked_contents: "function leakedJsSecret() { return \"password\"; }\n",
            untracked_query: "leakedJsSecret",
            language: "javascript",
            kind: code::SymbolKind::Function,
        });
    }

    #[test]
    fn jsx_indexing_lifecycle_contract() {
        let files = [
            ("src/components/CheckoutButton.jsx", JSX_LIFECYCLE_PRIMARY),
            ("src/components/Secondary.jsx", JSX_LIFECYCLE_SECONDARY),
        ];
        run_language_lifecycle(&LifecycleCase {
            files: &files,
            primary_path: "src/components/CheckoutButton.jsx",
            deleted_path: "src/components/Secondary.jsx",
            initial_query: "CheckoutButton",
            initial_identifier: "src/components/CheckoutButton.jsx::CheckoutButton",
            deleted_query: "SecondaryControl",
            edited_contents: "export function ConfirmCheckoutButton() {\n    return <button>confirm</button>;\n}\n",
            edited_query: "ConfirmCheckoutButton",
            edited_identifier: "src/components/CheckoutButton.jsx::ConfirmCheckoutButton",
            untracked_path: "src/components/Secret.jsx",
            untracked_contents: "export function LeakedJsxSecret() { return <div>password</div>; }\n",
            untracked_query: "LeakedJsxSecret",
            language: "jsx",
            kind: code::SymbolKind::Function,
        });
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

    // ── Epic 010 Task 1: JavaScript/JSX indexer wiring ───────────────────────

    const JS_FIXTURE: &str = r#"/**
 * Validate the cart contents for checkout.
 */
function validateCart(items) {
    if (!items || items.length === 0) {
        return false;
    }
    return true;
}

class PaymentGateway {
    configure(provider) {
        this.provider = provider;
    }
}

const DEFAULT_TIMEOUT = 3000;
"#;

    const JSX_FIXTURE: &str = r#"import React from "react";

/**
 * Renders the checkout button.
 */
export function CheckoutButton({ disabled, label }) {
    return (
        <button disabled={disabled} onClick={() => alert(label)}>
            {label}
        </button>
    );
}
"#;

    #[test]
    fn javascript_indexer_wires_metadata_language_javascript() {
        // Epic 010 Task 1 acceptance #6: `CodeLanguage::as_str()` and
        // indexed metadata report exactly `javascript` for `.js` and
        // `jsx` for `.jsx`. This test runs the full discovery + dispatch
        // path (`list_tracked_code_files` -> `language_for_extension`
        // -> JavaScript extractor -> metadata write).
        let (repo_dir, cache_dir) = setup_repo_with_files(&[("src/cart.js", JS_FIXTURE)]);
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let report = indexer.index_code().expect("index_code");
        assert_eq!(report.files_scanned, 1, ".js file must be discovered");
        assert_eq!(report.files_changed, 1);
        assert!(
            report.symbols_indexed > 0,
            "must produce at least one symbol from the JS fixture"
        );

        // The JSDoc named `validateCart` is the canonical evidence; assert
        // the metadata language string for the produced code item.
        let metadata: String = indexer
            .db
            .query_row(
                "SELECT metadata FROM items WHERE source_type = 'code' AND identifier = ?1",
                ["src/cart.js::validateCart"],
                |row| row.get(0),
            )
            .expect("code item metadata for validateCart");
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata).expect("valid metadata");
        assert_eq!(
            metadata["language"], "javascript",
            "indexed metadata language must be exactly 'javascript'; got {metadata}"
        );
        assert_eq!(metadata["file_path"], "src/cart.js");
    }

    #[test]
    fn jsx_indexer_wires_metadata_language_jsx() {
        let (repo_dir, cache_dir) =
            setup_repo_with_files(&[("src/components/CheckoutButton.jsx", JSX_FIXTURE)]);
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let report = indexer.index_code().expect("index_code");
        assert_eq!(report.files_scanned, 1, ".jsx file must be discovered");
        assert!(
            report.symbols_indexed > 0,
            "must produce at least one symbol from the JSX fixture"
        );

        let metadata: String = indexer
            .db
            .query_row(
                "SELECT metadata FROM items WHERE source_type = 'code' AND identifier = ?1",
                ["src/components/CheckoutButton.jsx::CheckoutButton"],
                |row| row.get(0),
            )
            .expect("code item metadata for CheckoutButton");
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata).expect("valid metadata");
        assert_eq!(
            metadata["language"], "jsx",
            "indexed metadata language must be exactly 'jsx'; got {metadata}"
        );
        assert_eq!(
            metadata["file_path"],
            "src/components/CheckoutButton.jsx"
        );
    }

    #[test]
    fn indexer_dispatches_js_and_jsx_to_their_respective_extractors() {
        // Acceptance #3: ".js and .jsx dispatch cannot fall through to
        // Rust or TypeScript". This test mixes a JS file and a JSX file
        // in one repo and verifies the metadata language column matches
        // the file extension on each produced code row.
        let files = [
            ("src/cart.js", JS_FIXTURE),
            ("src/components/CheckoutButton.jsx", JSX_FIXTURE),
        ];
        let (repo_dir, cache_dir) = setup_repo_with_files(&files);
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let report = indexer.index_code().expect("index_code");
        assert_eq!(report.files_scanned, 2);

        // Pull every code row and verify language matches the file
        // extension. This catches any dispatcher regression where a JS
        // file is incorrectly routed to the Rust or TypeScript path.
        let mut stmt = indexer
            .db
            .prepare("SELECT metadata FROM items WHERE source_type = 'code'")
            .expect("prepare code rows");
        let metadatas: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .expect("iterate code rows")
            .map(|r| r.unwrap_or_default())
            .collect();
        assert!(
            !metadatas.is_empty(),
            "should produce at least one code row per file"
        );
        for raw in &metadatas {
            let v: serde_json::Value = serde_json::from_str(raw).expect("valid metadata");
            let lang = v["language"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let path = v["file_path"].as_str().unwrap_or_default();
            if path.ends_with(".js") {
                assert_eq!(lang, "javascript", "language mismatch for {path}: {v}");
            } else if path.ends_with(".jsx") {
                assert_eq!(lang, "jsx", "language mismatch for {path}: {v}");
            } else {
                panic!("unexpected code file extension: {path}");
            }
        }
    }

    #[test]
    fn indexer_excludes_tracked_generated_vendor_js() {
        // Acceptance #2: committed fixtures for every configured
        // generated/vendor directory and `*.min.js` are excluded by the
        // shared discovery filter. End-to-end check via `index_code`:
        // only the real `.js` file should produce any code rows.
        let real_js = "function real() { return 1; }\n";
        let files = [
            ("src/real.js", real_js),
            ("node_modules/vendor/leaked.js", "function leakedNode() { return 1; }\n"),
            ("vendor/leaked2.js", "function leakedVendor() { return 1; }\n"),
            ("dist/bundle.min.js", "!function(){return 1}();\n"),
        ];
        let (repo_dir, cache_dir) = setup_repo_with_files(&files);
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let report = indexer.index_code().expect("index_code");
        assert_eq!(
            report.files_scanned, 1,
            "shared discovery filter must drop tracked generated/vendor paths"
        );
        assert_eq!(report.files_changed, 1);

        let results = indexer
            .search_code_text("leaked", 10)
            .expect("search");
        assert!(
            results.is_empty(),
            "no `leaked*` symbol should be indexed from vendor / node_modules paths; got {:?}",
            results
                .iter()
                .map(|r| (&r.identifier, &r.file_path))
                .collect::<Vec<_>>()
        );
    }

    // ── Epic 009 Task 5: Deterministic TypeScript/TSX retrieval tests ─────

    /// Checkout-shaped TypeScript fixture: covers a public function with a
    /// JSDoc that mentions both "checkout" and "validation" tokens, a typed
    /// configuration interface, a gateway class with `configured` / `payment`
    /// JSDoc and methods, plus a standalone "temporary limitation" comment.
    /// Mirrors the Epic 008 Rust `CHECKOUT_FIXTURE` shape so the four queries
    /// map 1:1 onto the Epic's expected evidence table.
    const TS_CHECKOUT_FIXTURE: &str = r#"import { PaymentProvider } from "./payment";

/**
 * Configuration for the checkout service.
 */
export interface CheckoutConfig {
    timeoutMs: number;
    provider: "stripe" | "manual";
}

/**
 * Process a checkout session for the given cart.
 *
 * Performs checkout validation, applies any active discounts,
 * and delegates to the configured payment provider for charging.
 */
export function createCheckoutSession(
    cart: { sku: string; qty: number }[],
    config: CheckoutConfig
): Promise<string> {
    if (cart.length === 0) {
        throw new Error("cart is empty");
    }
    return Promise.resolve("session_" + cart.length);
}

/**
 * The configured payment provider gateway.
 */
export class PaymentGateway {
    constructor(private readonly provider: PaymentProvider) {}

    /** Charge the configured amount and return true on success. */
    charge(amount: number): boolean {
        return this.provider.charge(amount);
    }
}

// TODO: temporary limitation — only USD currency is supported right now
export function unused() { return 1; }
"#;

    /// Checkout-shaped TSX fixture: covers a function component with typed
    /// props (including a `disabled?: boolean` prop), JSX that uses the
    /// disabled attribute, and an attached JSDoc mentioning both "checkout"
    /// and "disabled" so an FTS5 OR query hits it deterministically.
    const TSX_CHECKOUT_FIXTURE: &str = r#"import React from "react";

interface CheckoutButtonProps {
    disabled?: boolean;
    label: string;
}

/**
 * Renders the checkout button.
 *
 * The button is disabled while the checkout validation is in progress.
 */
export function CheckoutButton({ disabled, label }: CheckoutButtonProps) {
    return (
        <button disabled={disabled} onClick={() => alert(label)}>
            {label}
        </button>
    );
}

/** Helper that ignores disabled state but is bound to a clear name. */
export const CheckoutLabel = ({ label }: { label: string }) => <span>{label}</span>;
"#;

    /// Helper: build an indexer over a repo seeded with a checkout-shaped TS
    /// fixture plus a checkout-shaped TSX fixture (and nothing else).
    fn setup_ts_checkout_repo() -> (tempfile::TempDir, tempfile::TempDir) {
        setup_repo_with_files(&[
            ("src/checkout/session.ts", TS_CHECKOUT_FIXTURE),
            ("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE),
        ])
    }

    /// Pull the matching `CodeSearchResult` for a specific identifier from a
    /// `search_code_text` result set. Panics with full context if missing so a
    /// failure points at the query, identifier, and stored languages observed.
    fn expect_code_hit<'a>(
        results: &'a [CodeSearchResult],
        query: &str,
        identifier: &str,
    ) -> &'a CodeSearchResult {
        results
            .iter()
            .find(|r| r.identifier == identifier)
            .unwrap_or_else(|| {
                panic!(
                    "expected {identifier} for query {query:?}; got {:?}",
                    results
                        .iter()
                        .map(|r| (&r.identifier, format!("{:?}", r.symbol_kind), &r.file_path))
                        .collect::<Vec<_>>()
                )
            })
    }

    #[test]
    fn search_checkout_validation_returns_ts_evidence() {
        let (repo_dir, cache_dir) = setup_ts_checkout_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        indexer.index_code().expect("index_code");

        // FTS5 unicode61 doesn't stem, so "validation" matches "validation"
        // (exact token in JSDoc) and "checkout" matches the function name
        // and signature. The Epic 008 Rust retrieval test uses the same OR
        // shape, so this keeps the contract parallel across languages.
        let results = indexer
            .search_code_text("checkout OR validation", 20)
            .expect("search_code_text");
        assert!(
            !results.is_empty(),
            "'checkout OR validation' should return at least one result"
        );

        let hit = expect_code_hit(
            &results,
            "checkout OR validation",
            "src/checkout/session.ts::createCheckoutSession",
        );

        assert_eq!(hit.file_path, "src/checkout/session.ts");
        assert!(hit.line_start > 0, "line_start should be one-based");
        assert!(hit.line_end >= hit.line_start);
        assert!(
            matches!(hit.symbol_kind, code::SymbolKind::Function),
            "expected Function, got {:?}",
            hit.symbol_kind
        );
        // JSDoc + signature should both be present in the composed text.
        assert!(
            hit.text.contains("checkout validation"),
            "JSDoc validation excerpt should appear in composed text:\n{}",
            hit.text
        );
        assert!(
            hit.text.contains("createCheckoutSession"),
            "signature should appear in composed text:\n{}",
            hit.text
        );
    }

    #[test]
    fn search_payment_provider_configured_returns_ts_evidence() {
        let (repo_dir, cache_dir) = setup_ts_checkout_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        indexer.index_code().expect("index_code");

        // "PaymentGateway" is camelCase so FTS5 sees it as a single token.
        // The fixture JSDoc phrases "The configured payment provider gateway."
        // and the class body excerpt both contain "configured" and "payment"
        // as separate tokens (the hyphenated form is tokenized by unicode61),
        // so OR semantics reliably hit either the class or its `charge`
        // method. The Epic accepts "Typed configuration constant/interface
        // OR gateway method" — both the Class record and the Method record
        // surface under this query.
        let results = indexer
            .search_code_text("payment OR configured", 20)
            .expect("search_code_text");
        assert!(
            !results.is_empty(),
            "'payment OR configured' should return at least one result"
        );

        // At least one identifier must reference the PaymentGateway class
        // hierarchy: either the class record or the namespace-prefixed
        // method record produced by tree-sitter extraction.
        let class_hit = results
            .iter()
            .find(|r| r.identifier == "src/checkout/session.ts::PaymentGateway")
            .expect("expected PaymentGateway class record in results");
        let method_hit = results
            .iter()
            .find(|r| r.identifier == "src/checkout/session.ts::PaymentGateway::charge")
            .expect("expected PaymentGateway::charge method record in results");

        // The class record must surface Class kind with the JSDoc explaining
        // what it owns.
        assert_eq!(class_hit.file_path, "src/checkout/session.ts");
        assert!(
            matches!(class_hit.symbol_kind, code::SymbolKind::Class),
            "expected Class, got {:?}",
            class_hit.symbol_kind
        );
        assert!(
            class_hit.text.contains("configured payment"),
            "class JSDoc should mention configured payment:\n{}",
            class_hit.text
        );

        // The method record must surface Method kind with the namespaced
        // identifier, and its JSDoc must carry the "configured" token so the
        // OR query can find it deterministically.
        assert_eq!(method_hit.file_path, "src/checkout/session.ts");
        assert!(
            matches!(method_hit.symbol_kind, code::SymbolKind::Method),
            "expected Method, got {:?}",
            method_hit.symbol_kind
        );
        assert!(
            method_hit.text.contains("configured"),
            "method JSDoc should mention configured:\n{}",
            method_hit.text
        );
    }

    #[test]
    fn search_temporary_limitation_returns_ts_comment_evidence() {
        let (repo_dir, cache_dir) = setup_ts_checkout_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        indexer.index_code().expect("index_code");

        // "temporary limitation" is a contiguous phrase in the standalone
        // comment, so the phrase query works directly without OR — matching
        // the Epic 008 Rust retrieval test's style for the same scenario.
        let results = indexer
            .search_code_text("temporary limitation", 20)
            .expect("search_code_text");
        assert!(
            !results.is_empty(),
            "'temporary limitation' should return at least one result"
        );

        let hit = expect_code_hit(
            &results,
            "temporary limitation",
            "src/checkout/session.ts::__comments__",
        );

        assert_eq!(hit.file_path, "src/checkout/session.ts");
        assert!(hit.line_start > 0);
        assert!(hit.line_end >= hit.line_start);
        assert!(
            matches!(hit.symbol_kind, code::SymbolKind::Comments),
            "expected Comments, got {:?}",
            hit.symbol_kind
        );
        assert!(
            hit.text.contains("USD currency"),
            "comment text should mention USD currency:\n{}",
            hit.text
        );
    }

    #[test]
    fn search_checkout_button_disabled_returns_tsx_evidence() {
        let (repo_dir, cache_dir) = setup_ts_checkout_repo();
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        indexer.index_code().expect("index_code");

        // The TSX JSDoc mentions "checkout" and "disabled", and the JSX body
        // uses `disabled={disabled}` as an attribute. Both tokens appear in
        // the composed text so the OR query deterministically surfaces the
        // component record (and its props interface).
        let results = indexer
            .search_code_text("checkout OR disabled", 20)
            .expect("search_code_text");
        assert!(
            !results.is_empty(),
            "'checkout OR disabled' should return at least one result"
        );

        let hit = expect_code_hit(
            &results,
            "checkout OR disabled",
            "src/components/CheckoutButton.tsx::CheckoutButton",
        );

        assert_eq!(hit.file_path, "src/components/CheckoutButton.tsx");
        assert!(hit.line_start > 0, "line_start should be one-based");
        assert!(hit.line_end >= hit.line_start);
        assert!(
            matches!(hit.symbol_kind, code::SymbolKind::Function),
            "expected Function (component), got {:?}",
            hit.symbol_kind
        );
        // JSDoc + JSX body should both appear in the composed text.
        assert!(
            hit.text.contains("disabled"),
            "JSX `disabled` attribute or JSDoc must appear:\n{}",
            hit.text
        );
        assert!(
            hit.text.contains("checkout"),
            "JSDoc or signature must mention checkout:\n{}",
            hit.text
        );
    }

    #[test]
    fn search_returns_results_across_rust_typescript_and_tsx() {
        // Mixed repo: one .rs file, one .ts file, one .tsx file. A single
        // index_code() run should produce evidence for all three languages,
        // and a single FTS5 query that names a shared token ("checkout" or
        // "validate") must surface records from every language in one
        // result set.
        let files = [
            ("src/checkout.rs", CHECKOUT_FIXTURE),
            ("src/checkout/session.ts", TS_CHECKOUT_FIXTURE),
            ("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE),
        ];
        let (repo_dir, cache_dir) = setup_repo_with_files(&files);
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());

        let report = indexer.index_code().expect("index_code");
        assert_eq!(report.files_scanned, 3, "should scan all three tracked files");
        assert_eq!(report.files_changed, 3, "all three files are new on first run");
        assert_eq!(report.files_deleted, 0);

        // "checkout" appears as a token in the Rust create_checkout_session
        // JSDoc, the TypeScript createCheckoutSession JSDoc/signature, and
        // the TSX CheckoutButton JSDoc/signature.
        let results = indexer
            .search_code_text("checkout", 30)
            .expect("search_code_text");

        // No exact result order asserted — the Epic explicitly disallows
        // it. We only assert each language shows up at least once.
        let languages: std::collections::HashSet<&str> =
            results.iter().map(|r| r.file_path.rsplit('.').next().unwrap_or("")).collect();
        // file_path ends with .rs, .ts, or .tsx — derive language from suffix.
        let has_rust = results.iter().any(|r| r.file_path.ends_with(".rs"));
        let has_ts = results.iter().any(|r| {
            r.file_path.ends_with(".ts") && !r.file_path.ends_with(".tsx")
        });
        let has_tsx = results.iter().any(|r| r.file_path.ends_with(".tsx"));

        assert!(has_rust, "expected at least one Rust result for 'checkout'; got file_paths: {:?}", languages);
        assert!(has_ts, "expected at least one TypeScript result for 'checkout'; got file_paths: {:?}", languages);
        assert!(has_tsx, "expected at least one TSX result for 'checkout'; got file_paths: {:?}", languages);

        // Spot-check that the expected identifiers across all three languages
        // appear. We don't require them to be the only hits; the contract is
        // a single search covers all three.
        let identifiers: Vec<&str> = results.iter().map(|r| r.identifier.as_str()).collect();
        assert!(
            identifiers.iter().any(|id| id.contains("create_checkout_session")),
            "expected create_checkout_session (Rust) in identifiers: {:?}",
            identifiers
        );
        assert!(
            identifiers.iter().any(|id| id.contains("createCheckoutSession")),
            "expected createCheckoutSession (TS) in identifiers: {:?}",
            identifiers
        );
        assert!(
            identifiers.iter().any(|id| id.ends_with("::CheckoutButton")),
            "expected CheckoutButton (TSX) in identifiers: {:?}",
            identifiers
        );

        // The mixed-language result set must also preserve language metadata
        // for each hit. Pull each expected language's record and verify the
        // stored metadata language string matches.
        let rust_hit = results
            .iter()
            .find(|r| r.identifier.contains("create_checkout_session"))
            .expect("rust hit");
        let ts_hit = results
            .iter()
            .find(|r| r.identifier.contains("createCheckoutSession"))
            .expect("ts hit");
        let tsx_hit = results
            .iter()
            .find(|r| r.identifier.ends_with("::CheckoutButton"))
            .expect("tsx hit");

        assert_eq!(rust_hit.file_path, "src/checkout.rs");
        assert_eq!(ts_hit.file_path, "src/checkout/session.ts");
        assert_eq!(tsx_hit.file_path, "src/components/CheckoutButton.tsx");

        // Symbol kind + one-based line range + non-empty explanatory text
        // must be preserved for each language. The Epic explicitly lists
        // these four invariants under "Hits preserve language, path, line
        // range, kind, and text."
        for hit in &[rust_hit, ts_hit, tsx_hit] {
            assert!(
                hit.line_start > 0,
                "line_start should be one-based for {}",
                hit.identifier
            );
            assert!(
                hit.line_end >= hit.line_start,
                "line_end >= line_start for {} ({} >= {})",
                hit.identifier,
                hit.line_end,
                hit.line_start
            );
            assert!(
                !hit.text.is_empty(),
                "hit text must not be empty for {}",
                hit.identifier
            );
        }
        assert!(
            matches!(rust_hit.symbol_kind, code::SymbolKind::Function),
            "rust hit should be Function, got {:?}",
            rust_hit.symbol_kind
        );
        assert!(
            matches!(ts_hit.symbol_kind, code::SymbolKind::Function),
            "ts hit should be Function, got {:?}",
            ts_hit.symbol_kind
        );
        assert!(
            matches!(tsx_hit.symbol_kind, code::SymbolKind::Function),
            "tsx hit should be Function (component), got {:?}",
            tsx_hit.symbol_kind
        );

        // Confirm language metadata round-trips through the items table.
        // This validates that the language string stored alongside each hit
        // matches the actual file extension of that hit — a separate
        // assertion from `file_path`, which guards against accidental
        // metadata drift between dialects.
        for (expected_language, hit) in [
            ("rust", rust_hit),
            ("typescript", ts_hit),
            ("tsx", tsx_hit),
        ] {
            let metadata: String = indexer
                .db
                .query_row(
                    "SELECT metadata FROM items WHERE source_type = 'code' AND identifier = ?1",
                    [&hit.identifier],
                    |row| row.get(0),
                )
                .expect("code item metadata");
            let metadata: serde_json::Value =
                serde_json::from_str(&metadata).expect("valid metadata");
            assert_eq!(
                metadata["language"], expected_language,
                "language metadata mismatch for identifier {:?}",
                hit.identifier
            );
        }
    }

    // ── Epic 010 Task 3: JSX metadata language contract ──────────────────

    /// Comprehensive JSX fixture for Task 3. Mirrors the code.rs fixture
    /// shape (function, arrow, class components, default props,
    /// destructured props, fragments, expression-container callbacks,
    /// `items.map(item => ...)`) so the indexed metadata language string
    /// can be asserted against the canonical `jsx` value rather than
    /// `javascript`.
    const JSX_TASK3_FIXTURE: &str = r#"import React from "react";

/**
 * Function component with default props and a product-visible disabled
 * condition. The button stays disabled while checkout validation runs.
 */
export function CheckoutButton({ disabled = false, label = "Pay" }) {
    return (
        <button
            disabled={disabled}
            onClick={() => submitCheckout(label)}
        >
            {disabled ? "Loading..." : label}
        </button>
    );
}

/** Arrow component with destructured props and a click handler call. */
export const CheckoutLabel = ({ amount, provider }) => (
    <span onClick={() => recordClick(amount)}>
        Charging {amount} via {provider.name}
    </span>
);

/** Class component with adjacent render + handler methods. */
export class CheckoutForm extends React.Component {
    render() {
        return (
            <ul>
                {this.props.items.map((item) => (
                    <ItemRow key={item.id} item={item} />
                ))}
            </ul>
        );
    }

    handleClick() {
        return "clicked";
    }

    anotherMethod() {
        return "another only";
    }
}

/** Helper function — must remain independently searchable. */
function helper() { return true; }

const ItemRow = ({ item }) => <li>{item.name}</li>;
"#;

    /// Default-export fixtures for Task 3 metadata assertions. Split
    /// into named and anonymous fixtures because valid JS modules allow
    /// exactly one `export default`.
    const JSX_TASK3_NAMED_DEFAULT_FIXTURE: &str = r#"import React from "react";

/** Named default function export. */
export default function NamedDefault(props) {
    return <form>{props.children}</form>;
}
"#;

    const JSX_TASK3_ANONYMOUS_DEFAULT_FIXTURE: &str = r#"import React from "react";

/** Anonymous default class export with adjacent methods. */
export default class {
    render() {
        return <div>only render body</div>;
    }

    handleClick() {
        return "clicked";
    }
}
"#;

    const JSX_TASK3_ANONYMOUS_ARROW_DEFAULT_FIXTURE: &str = r#"import React from "react";

/** Anonymous default arrow export. */
export default (props) => <button>{props.label}</button>;
"#;

    #[test]
    fn jsx_task3_metadata_language_is_jsx_not_javascript() {
        let files = [
            ("src/components/CheckoutButton.jsx", JSX_TASK3_FIXTURE),
            (
                "src/components/NamedDefault.jsx",
                JSX_TASK3_NAMED_DEFAULT_FIXTURE,
            ),
            (
                "src/components/AnonymousDefault.jsx",
                JSX_TASK3_ANONYMOUS_DEFAULT_FIXTURE,
            ),
            (
                "src/components/AnonymousArrowDefault.jsx",
                JSX_TASK3_ANONYMOUS_ARROW_DEFAULT_FIXTURE,
            ),
        ];
        let (repo_dir, cache_dir) = setup_repo_with_files(&files);
        let mut indexer = make_indexer(repo_dir.path(), cache_dir.path());
        let report = indexer.index_code().expect("index_code");
        assert_eq!(report.files_scanned, 4);
        assert_eq!(report.files_changed, 4);

        for (path, expected_identifier) in [
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::CheckoutButton",
            ),
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::CheckoutLabel",
            ),
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::CheckoutForm",
            ),
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::CheckoutForm::render",
            ),
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::CheckoutForm::handleClick",
            ),
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::CheckoutForm::anotherMethod",
            ),
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::helper",
            ),
            (
                "src/components/CheckoutButton.jsx",
                "src/components/CheckoutButton.jsx::ItemRow",
            ),
            (
                "src/components/NamedDefault.jsx",
                "src/components/NamedDefault.jsx::NamedDefault",
            ),
            (
                "src/components/AnonymousDefault.jsx",
                "src/components/AnonymousDefault.jsx::__default_export",
            ),
            (
                "src/components/AnonymousDefault.jsx",
                "src/components/AnonymousDefault.jsx::__default_export::render",
            ),
            (
                "src/components/AnonymousDefault.jsx",
                "src/components/AnonymousDefault.jsx::__default_export::handleClick",
            ),
            (
                "src/components/AnonymousArrowDefault.jsx",
                "src/components/AnonymousArrowDefault.jsx::__default_export",
            ),
        ] {
            let metadata: String = indexer
                .db
                .query_row(
                    "SELECT metadata FROM items WHERE source_type = 'code' AND identifier = ?1",
                    [expected_identifier],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| {
                    panic!("code item metadata for {expected_identifier}")
                });
            let metadata: serde_json::Value =
                serde_json::from_str(&metadata).expect("valid metadata");
            assert_eq!(
                metadata["language"], "jsx",
                "language metadata mismatch for {expected_identifier}: {metadata}"
            );
            assert_eq!(
                metadata["file_path"], path,
                "file_path mismatch for {expected_identifier}"
            );
        }

        let js_hits: i64 = indexer
            .db
            .query_row(
                "SELECT COUNT(*) FROM items WHERE source_type = 'code' AND json_extract(metadata, '$.language') = 'javascript'",
                [],
                |row| row.get(0),
            )
            .expect("javascript count");
        assert_eq!(
            js_hits, 0,
            "no code row in a .jsx file should carry language=javascript"
        );
    }
}
