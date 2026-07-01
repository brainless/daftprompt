pub mod code;
pub mod db;
pub mod embed;

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
            repo_path: repo_path.to_path_buf(),
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
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}
