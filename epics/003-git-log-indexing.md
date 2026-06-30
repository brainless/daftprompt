# Epic 003: Git Log Indexing with sqlite-vec and model2vec-rs

## Introduction

This epic adds semantic and full-text search over git commit history. It introduces a separate `sugacode-indexer` crate that reads commit data, generates embeddings with model2vec-rs, and stores everything in a SQLite database with FTS5 (full-text) and sqlite-vec (vector similarity) indices. The schema is designed to accommodate future data sources (source code via tree-sitter, GitHub/Jira issues, markdown files) without migration.

### Work Context

**Problem:** Users need to search commit history semantically — "find the commit that fixed the renderer crash" — not just by exact keyword match. The existing search box (Cmd+K) does substring matching only.

**Solution:** Build an indexing pipeline that:
- Reads commits from all local branches using gitoxide
- Embeds commit text (title + body) and author with model2vec-rs (`potion-code-16M`)
- Stores vectors in sqlite-vec `vec0` virtual tables and text in FTS5
- Provides hybrid search combining keyword (FTS5) and semantic (vector KNN) results
- Persists the index in the OS cache directory, **one SQLite DB file per repository** (named after a slug of the repo path), so KNN never sees other repos' vectors
- Supports incremental indexing (only new commits) and full re-index via CLI flag
- **Auto-incrementally indexes on GUI startup** (skippable with `--no-index`) so search results stay fresh without manual `--index`
- **Integrates with the existing Cmd+K search box** in the UI — replaces placeholder substring matching with hybrid results rendered as cards on the canvas (commits)
- Keeps the `--search <query>` CLI flag for fast, testable iteration without launching the GUI

**Technology Stack:**
- **model2vec-rs** (v0.2.1) — Static text embedding inference (Rust port)
- **sqlite-vec** (v0.0.1-alpha.7) — Vector similarity search SQLite extension
- **rusqlite** (v0.31, bundled) — SQLite driver
- **zerocopy** (v0.7) — Zero-copy vector byte passing to SQLite
- **dirs** (v5) — OS cache directory resolution
- Existing **gix** — Git repository access

**Design Principles:**
- Single source-of-truth `items` table — all data sources write into it with a different `source_type`
- **External-content FTS5** (`content='items', content_rowid='id'`) kept in sync via triggers — FTS5 is a pure index, `items` owns the text, deletes/updates propagate automatically
- **Per-repo DB file** in the OS cache dir (filename derived from a slug of the repo path) — simplest correct KNN isolation, free locking/isolation, easy per-repo wipe
- One embedding per item — text and author combined or author via FTS5 column
- Model-agnostic — embedding dimension read from model at load time, not hardcoded
- Incremental by default — `--reindex` flag for full rebuild
- Auto-index on GUI startup (incremental is fast); `--no-index` skips it

---

## Architecture

### Crate Structure

```
sugacode/
├── Cargo.toml                         (workspace root)
├── src/
│   ├── main.rs                        (binary — CLI + UI)
│   ├── git_log.rs                     (modified: multi-branch, full SHA, body)
│   └── ...
└── crates/
    └── sugacode-indexer/
        ├── Cargo.toml
        └── src/
            ├── lib.rs                 (Indexer, CommitData, SearchResult)
            ├── db.rs                  (SQLite schema, FTS5, vec0, queries)
            └── embed.rs              (model2vec-rs wrapper)
```

### SQLite Schema

**Per-repo DB file.** Each repository gets its own SQLite file in the OS cache dir:
```
~/Library/Caches/sugacode/{repo_slug}.db     (macOS)
~/.cache/sugacode/{repo_slug}.db             (Linux)
%LOCALAPPDATA%\sugacode\{repo_slug}.db       (Windows)
```
`repo_slug` = lowercase, non-alphanumeric chars folded to `_`, with a short hash of the absolute path to disambiguate (e.g. `projects_sugacode_a1b2c3.db`). Because each repo has its own DB, the `repositories` table is **not** needed; the repo path is stored as a single metadata row for `indexed_at`. KNN is naturally scoped to the one repo.

```sql
-- Single-row metadata for this repo's index
CREATE TABLE IF NOT EXISTS repo_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- key='repo_path'         -> absolute path
-- key='indexed_at'        -> ISO8601 timestamp of last successful index
-- key='embedding_dimension' -> model dimension (for sanity-checking on reopen)

CREATE TABLE IF NOT EXISTS items (
    id INTEGER PRIMARY KEY,
    source_type TEXT NOT NULL,          -- 'commit', 'code', 'issue', 'markdown'
    identifier TEXT NOT NULL,           -- commit SHA, file path, issue ID, etc.
    text TEXT NOT NULL,                 -- searchable text
    author TEXT,                        -- nullable
    metadata TEXT,                      -- JSON blob for source-specific data
    UNIQUE(source_type, identifier)     -- unique within this repo
);

-- External-content FTS5: text lives in `items`, FTS5 is a pure index.
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
    text, author,
    content='items', content_rowid='id'
);
-- DML on `items` is kept in sync by AFTER INSERT/UPDATE/DELETE triggers
-- (see schema.sql). Re-indexing a source is: DELETE FROM items WHERE source_type=?
-- which cascades to FTS5 via triggers; vec_items is cleaned explicitly.

CREATE VIRTUAL TABLE IF NOT EXISTS vec_items USING vec0(
    item_id INTEGER PRIMARY KEY,
    embedding float[{dim}] distance_metric=cosine
);
-- vec0 does not support triggers; deletion path deletes from vec_items
-- explicitly before deleting from items (matched by item_id list).
```

For commits: `text` = `"{title}\n\n{body}"`, `author` = `"{author_name}"`, `metadata` = `{"short_hash":"abc1234","time":"2024-01-15","branches":["main"]}`.

### Indexing Flow

1. Resolve repo path → compute `repo_slug` → open (or create) `{cache}/sugacode/{repo_slug}.db`
2. Query existing SHAs from `items WHERE source_type='commit'`
3. Filter to new SHAs only (incremental) or DELETE all commit rows (re-index) — FTS5 cleanup is automatic via triggers; `vec_items` rows are deleted explicitly by `item_id`
4. Transaction: INSERT items (triggers populate FTS5) → batch-encode → INSERT vec_items
5. `REPLACE INTO repo_meta(key,value) VALUES ('indexed_at', ?)`

### Hybrid Search (Reciprocal Rank Fusion)

FTS5 keyword results and vec0 KNN results are combined using RRF. Each list is consumed **ordered best-first** and converted to 1-based ordinal ranks (1 = top hit):

```
combined_score = w_fts / (k + fts_position) + w_vec / (k + vec_position)
```

Use the *position* in the result list, **not** FTS5's `rank` column (which is a BM25 score, possibly negative) and **not** raw vec0 distance. `k=60`, `w_fts=1.0`, `w_vec=1.0` by default.

---

## Tasks

### Task 1: Spike — Verify sqlite-vec Load on Bundled rusqlite

**Priority:** High (blocker)
**Status:** ⬜ Not Started
**Estimated Time:** 0.5 hours

**Description:** `sqlite-vec` is an alpha crate (0.0.1-alpha.7) and the `sqlite3_auto_extension` registration incantation has changed across releases. Verify the exact load incantation against the pinned versions in a throwaway binary **before** writing any DB code, since everything downstream depends on it.

**Details:**
- Scratch binary (not committed outside this spike): `examples/vec_spike.rs`:
  ```rust
  use rusqlite::Connection;

  fn main() -> anyhow::Result<()> {
      // The exact init symbol and auto_extension call MUST be confirmed against
      // the pinned sqlite-vec version. Document the working incantation in
      // `crates/sugacode-indexer/src/db.rs` as a code comment.
      unsafe {
          rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
              sqlite_vec::sqlite3_vec_init as *const ()
          )));
      }
      let db = Connection::open_in_memory()?;
      db.execute_batch(
          "CREATE VIRTUAL TABLE v USING vec0(embedding float[4] distance_metric=cosine);",
      )?;
      // Insert a few float[4] vectors and run a KNN MATCH to confirm end-to-end.
      // Confirm it works against *bundled* SQLite (rusqlite "bundled" feature),
      // not the system libsqlite3.
      Ok(())
  }
  ```
- Pin rusqlite 0.31 with `features = ["bundled"]` and sqlite-vec 0.0.1-alpha.7.
- Record the verified load incantation (with version pin) as a comment at the top of `db.rs` so it isn't cargo-culted later.

**Acceptance Criteria:**
- [ ] KNN `MATCH ... AND k=? ORDER BY distance` query returns expected nearest neighbours against in-memory vectors
- [ ] Works with rusqlite's **bundled** SQLite (no system libsqlite3 dependency)
- [ ] Verified load incantation documented as a comment in `db.rs`

---

### Task 2: Workspace Setup and Dependencies

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 1 hour

**Description:** Convert the project to a Cargo workspace and create the `sugacode-indexer` crate with all dependencies.

**Details:**
- Modify root `Cargo.toml` to declare workspace:
  ```toml
  [workspace]
  members = [".", "crates/sugacode-indexer"]
  ```
- Create `crates/sugacode-indexer/Cargo.toml`:
  ```toml
  [package]
  name = "sugacode-indexer"
  version = "0.1.0"
  edition = "2021"

  [dependencies]
  model2vec-rs = "0.2.1"
  sqlite-vec = "0.0.1-alpha.7"
  rusqlite = { version = "0.31", features = ["bundled"] }
  zerocopy = "0.7"
  anyhow = "1"
  dirs = "5"
  serde_json = "1"
  ```
- Add `sugacode-indexer` as a path dependency in root `Cargo.toml`:
  ```toml
  sugacode-indexer = { path = "crates/sugacode-indexer" }
  ```
- Create skeleton files: `crates/sugacode-indexer/src/lib.rs`, `db.rs`, `embed.rs`, `schema.sql`
- Carry the verified sqlite-vec load incantation from Task 1 into `db.rs`.
- Verify `cargo check --workspace` passes

**Acceptance Criteria:**
- [ ] Workspace compiles with `cargo check --workspace`
- [ ] `sugacode-indexer` crate visible in workspace
- [ ] All new dependencies resolve
- [ ] Existing app still runs with `cargo run`

---

### Task 3: Update `git_log.rs` — Multi-Branch, Full SHA, Message Body

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Extend the existing git log reader to support all local branches, full SHA extraction, and commit message body. Existing `read_log` is updated (not left stale) so it also produces the new fields.

**Details:**
- Replace `CommitInfo` (currently only 4 fields in `src/git_log.rs`) with:
  ```rust
  pub struct CommitInfo {
      pub sha: String,                 // full 40-char
      pub short_hash: String,
      pub author_name: String,
      pub time: String,
      pub message_title: String,
      pub message_body: String,        // everything after the title line
  }
  ```
- In the existing `read_log` loop, extract:
  ```rust
  let sha = commit.id().to_string();
  let message = commit_ref.message();
  let body = message.body
      .map(|b| b.to_str_lossy().to_string())
      .unwrap_or_default();
  ```
- Update **existing** `read_log` to populate `sha` and `message_body` (its callers — the Epic 002 git log column UI — must be updated; default `message_body` to `""` for display there).
- Add new function:
  ```rust
  pub fn read_log_all_branches(repo_path: &Path) -> Result<Vec<CommitInfo>, GitLogError>
  ```
  Implementation:
  1. `repo.references()?.local_branches()?.peeled()?` → collect all branch head OIDs
  2. `repo.rev_walk(branch_heads)` — gitoxide deduplicates by SHA automatically
  3. Same commit extraction logic as `read_log` (factor out a shared helper to avoid drift)
  4. Fall back to HEAD-only walk if no local branches found

**Acceptance Criteria:**
- [ ] `CommitInfo` has `sha` and `message_body` fields
- [ ] Both `read_log` and `read_log_all_branches` populate the new fields
- [ ] Epic 002 git log column UI still compiles and renders (uses existing fields; ignores body)
- [ ] `read_log_all_branches` walks all `refs/heads/*` and deduplicates
- [ ] Tested with repos that have multiple branches diverging from main
- [ ] Graceful fallback when repo has only one branch

---

### Task 4: `embed.rs` — model2vec-rs Wrapper

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Create a wrapper around model2vec-rs that loads the model and provides batch encoding.

**Details:**
- New file: `crates/sugacode-indexer/src/embed.rs`
- Core struct:
  ```rust
  use model2vec_rs::model::StaticModel;

  pub struct Embedder {
      model: StaticModel,
      pub dimension: usize,
  }

  impl Embedder {
      pub fn new(model_name: &str) -> Result<Self> {
          let model = StaticModel::from_pretrained(model_name, None, None, None)?;
          // Encode a probe to discover dimension
          let probe = model.encode_single("probe");
          let dimension = probe.len();
          Ok(Self { model, dimension })
      }

      pub fn encode_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
          self.model.encode(texts) // default max_length=512, batch_size=1024
      }

      pub fn encode_single(&self, text: &str) -> Vec<f32> {
          self.model.encode_single(text)
      }
  }
  ```
- Model name defaults to `"minishlab/potion-code-16M"` but is configurable
- Dimension discovered at runtime from first encode — no hardcoded values
- First call to `from_pretrained` downloads model to HuggingFace cache (`~/.cache/huggingface/`)
- On download failure, return a typed error so the GUI can degrade gracefully (search falls back to FTS5-only, see Task 7)

**Acceptance Criteria:**
- [ ] `Embedder::new("minishlab/potion-code-16M")` succeeds
- [ ] `dimension` matches model output (not hardcoded)
- [ ] `encode_batch` returns correct number of vectors
- [ ] Each vector has `dimension` elements
- [ ] Download failure surfaces as a typed error (not a panic)
- [ ] Subsequent loads use cached model (no re-download)

---

### Task 5: `db.rs` — SQLite Schema and Core Operations

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 3 hours

**Description:** Implement SQLite schema creation (per-repo DB file, external-content FTS5 with triggers, vec0), plus insert/query operations for FTS5 and vec0.

**Details:**
- New files: `crates/sugacode-indexer/src/db.rs`, `crates/sugacode-indexer/src/schema.sql`
- **DB path resolution:** one file per repo in the OS cache dir:
  ```rust
  pub fn db_path_for_repo(repo_path: &Path) -> Result<PathBuf>;
  // {cache}/sugacode/{repo_slug}.db where repo_slug =
  //   stem + 6-hex digest of absolute path, non-alphanumerics folded to '_'.
  `
- **Schema initialization** (`schema.sql`, loaded via `include_str!`):
  ```rust
  pub fn init_schema(db: &Connection, dim: usize) -> Result<()> {
      db.execute_batch(include_str!("schema.sql"))?;
      // vec0 needs the dimension at CREATE time — run it separately:
      db.execute(&format!(
          "CREATE VIRTUAL TABLE IF NOT EXISTS vec_items USING vec0(\
              item_id INTEGER PRIMARY KEY, embedding float[{dim}] distance_metric=cosine)",
          dim = dim,
      ), [])?;
      Ok(())
  }
  ```
  `schema.sql` creates `repo_meta`, `items`, `items_fts` (external-content), and **AFTER INSERT/UPDATE/DELETE triggers** on `items` that keep `items_fts` in sync (standard FTS5 external-content pattern: `INSERT INTO items_fts(rowid, text, author) VALUES (new.id, new.text, new.author);` on insert, etc.). When reopening a DB created with a different `dim`, drop & recreate `vec_items` (vectors are rebuilt by `--reindex`; dimension mismatches mean the model changed).
- **Operations (no `repo_id` — the whole DB is one repo):**
  ```rust
  pub fn existing_identifiers(db: &Connection, source_type: &str) -> Result<HashSet<String>>;
  pub fn insert_items(db: &Connection, source_type: &str, items: &[ItemRow]) -> Result<Vec<i64>>;
  // FTS5 rows are added by triggers — do NOT insert into items_fts directly.
  pub fn insert_vectors(db: &Connection, item_ids: &[i64], embeddings: &[Vec<f32>]) -> Result<()>;
  // INSERT INTO vec_items(item_id, embedding) VALUES (?, ?) via zerocopy::AsBytes on &[f32]
  pub fn search_fts(db: &Connection, query: &str, limit: usize) -> Result<Vec<(i64, f64)>>;
  pub fn search_vec(db: &Connection, query_embedding: &[f32], limit: usize) -> Result<Vec<(i64, f64)>>;
  pub fn delete_source(db: &Connection, source_type: &str) -> Result<()>;
  // Collect item ids for the source, DELETE FROM vec_items WHERE item_id IN (...),
  // then DELETE FROM items WHERE source_type=? (triggers clean FTS5).
  pub fn repo_meta_get(db: &Connection, key: &str) -> Result<Option<String>>;
  pub fn repo_meta_set(db: &Connection, key: &str, value: &str) -> Result<()>;
  ```

**Acceptance Criteria:**
- [ ] Per-repo DB file created at `{cache}/sugacode/{repo_slug}.db`; two different repos get two different files
- [ ] Schema creates `repo_meta`, `items`, `items_fts` (external-content), `vec_items`, and the sync triggers
- [ ] INSERT into `items` is reflected in `items_fts` automatically (verified by FTS MATCH)
- [ ] DELETE from `items` removes the FTS5 row via trigger
- [ ] `existing_identifiers` returns SHAs already in DB
- [ ] Vectors inserted via zerocopy and queryable via KNN
- [ ] FTS5 keyword search returns ranked results (no JOIN-by-repo_id needed)
- [ ] Vector KNN search returns nearest neighbors (no JOIN-by-repo_id needed — whole DB is one repo)
- [ ] `delete_source` clears all data for a source type from `items`, FTS5, and `vec_items`

---

### Task 6: `lib.rs` — Indexer Public API

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Wire together `db.rs` and `embed.rs` into the `Indexer` struct with the public API.

**Details:**
- New file: `crates/sugacode-indexer/src/lib.rs`
- Core types:
  ```rust
  pub mod db;
  pub mod embed;

  pub struct IndexerConfig {
      pub cache_dir: Option<PathBuf>, // default: dirs::cache_dir().join("sugacode")
      pub model_name: String,         // default: "minishlab/potion-code-16M"
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
      pub identifier: String,         // commit SHA
      pub short_hash: String,
      pub text: String,                // title (+ body) snippet
      pub author: Option<String>,
      pub score: f32,
      pub match_type: MatchType,
  }

  pub enum MatchType { Fts, Vector, Hybrid }
  ```
- `Indexer` implementation:
  ```rust
  pub struct Indexer {
      db: Connection,
      embedder: Option<Embedder>,    // None if model load failed → FTS5-only fallback
      repo_path: PathBuf,
  }

  impl Indexer {
      pub fn new(repo_path: &Path, config: &IndexerConfig) -> Result<Self>;
      // 1. Resolve {cache}/sugacode/{repo_slug}.db
      // 2. Register sqlite-vec auto extension (once, idempotent) using the
      //    incantation verified in Task 1
      // 3. Open Connection, init schema (load model first to learn dim,
      //    or reopen and reuse existing dim from repo_meta)
      // 4. Try to load Embedder; on failure store None (search_similar/search_hybrid
      //    degrade to FTS5-only)

      pub fn index_commits(&mut self, commits: &[CommitData]) -> Result<usize>;
      // 1. existing_identifiers("commit") → new SHAs only
      // 2. Begin transaction
      // 3. insert_items (text="{title}\n\n{body}", author="{name}",
      //    metadata=json!({"short_hash","time"})) — FTS5 populated by triggers
      // 4. encode_batch (skip if embedder is None)
      // 5. insert_vectors (skip if embedder is None)
      // 6. Commit; repo_meta_set("indexed_at", now)
      // Returns number of newly indexed commits

      pub fn reindex_commits(&mut self, commits: &[CommitData]) -> Result<usize>;
      // delete_source("commit") then index_commits

      pub fn search_text(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;

      pub fn search_similar(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
      // Requires embedder Some; else returns Err or empty — see Task 7 fallback policy

      pub fn search_hybrid(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
      // FTS5 + vec0 combined via Reciprocal Rank Fusion.
      // If embedder is None: returns FTS5-only results (degraded, not an error).
  }
  ```
- sqlite-vec registration (uses the exact call verified in Task 1; guarded so it runs at most once per process):
  ```rust
  // SAFETY: documented in Task 1 spike.
  unsafe {
      rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
          sqlite_vec::sqlite3_vec_init as *const ()
      )));
  }
  ```

**Acceptance Criteria:**
- [ ] `Indexer::new` creates the per-repo DB file and schema if absent
- [ ] Re-opening an existing DB reuses its schema/dim and preserves data
- [ ] `index_commits` only inserts new SHAs (incremental); idempotent across runs
- [ ] `index_commits` returns count of newly indexed commits
- [ ] If the model fails to load, `search_hybrid` still returns FTS5-only results (no panic, no hard error)
- [ ] `search_text`, `search_similar`, `search_hybrid` return `SearchResult` with SHA + text + author + score + match type
- [ ] RRF combination ordering is sensible (FTS and vector hits merge correctly)

---

### Task 7: CLI Integration — `--index`, `--reindex`, `--search`, `--no-index`

**Priority:** Medium
**Status:** ⬜ Not Started
**Estimated Time:** 1.5 hours

**Description:** Wire the indexer into the main binary's CLI with index and search commands, kept as a testing/diagnostic surface alongside the GUI search path (Task 8).

**Details:**
- Extend `Args` in `src/main.rs`:
  ```rust
  #[derive(Parser)]
  struct Args {
      #[arg(short, long, default_value = ".")]
      repo: PathBuf,

      #[arg(short, long)]
      count: Option<usize>,

      /// (Re-)index the repository into the search database
      #[arg(long)]
      index: bool,

      /// Drop and re-build the index for this repo
      #[arg(long)]
      reindex: bool,

      /// Skip auto-indexing on GUI startup (still uses cached index if present)
      #[arg(long)]
      no_index: bool,

      /// Search indexed data and print results (does not launch GUI)
      #[arg(long)]
      search: Option<String>,
  }
  ```
- Main flow. If `--search` or `--index`/`--reindex` are passed, run the headless path and exit (no GUI). Otherwise proceed to GUI launch with an auto-index step (unless `--no-index`):
  ```rust
  let indexer_config = IndexerConfig::default();

  // Headless: explicit index/reindex then optionally search and exit.
  if args.index || args.reindex {
      let mut commits = git_log::read_log_all_branches(&args.repo)?;
      if let Some(count) = args.count {
          commits.truncate(count);
      }
      let commit_data: Vec<CommitData> = commits.iter().map(Into::into).collect();
      let mut indexer = Indexer::new(&args.repo, &indexer_config)?;
      let n = if args.reindex {
          indexer.reindex_commits(&commit_data)?
      } else {
          indexer.index_commits(&commit_data)?
      };
      println!("Indexed {} commits", n);
  }

  if let Some(query) = &args.search {
      let indexer = Indexer::new(&args.repo, &indexer_config)?;
      let results = indexer.search_hybrid(query, 10)?;
      for r in &results {
          let id = r.short_hash.as_str();
          let title = r.text.lines().next().unwrap_or("");
          println!("[{:.3}] {:<7} {} — {}", r.score, id, r.author.as_deref().unwrap_or(""), title);
      }
      return Ok(()); // no GUI
  }

  // GUI path: auto-incremental-index unless disabled (see Task 8 for full wiring).
  ```
- Add `impl From<&CommitInfo> for CommitData` (simple field mapping).
- Note: `--search` returns early and never launches the GUI; the GUI search path is Task 8.

**Acceptance Criteria:**
- [ ] `cargo run -- --repo . --index` indexes all local branches
- [ ] `cargo run -- --repo . --reindex` drops and rebuilds
- [ ] `cargo run -- --repo . --search "fix bug"` prints results and exits without GUI
- [ ] `--count` limits commits indexed (useful for large repos)
- [ ] Running `--index` twice is idempotent (no duplicates, second run reports 0)
- [ ] `--no-index` prevents startup auto-indexing in the GUI path
- [ ] Each repo gets its own DB file under `{cache}/sugacode/{repo_slug}.db` (e.g. `~/Library/Caches/sugacode/projects_sugacode_a1b2c3.db` on macOS)

---

### Task 8: UI Integration — Cmd+K Hybrid Search with Result Cards

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 3 hours

**Description:** Replace the placeholder substring search behind Cmd+K with hybrid search via the indexer. Match results are commit cards rendered on the canvas; the existing non-matching-dim behaviour is retained (otherwise cards brighten normally).

**Details:**
- **Startup auto-index (GUI path).** In `main.rs`, before the event loop (and unless `--no-index`):
  ```rust
  let mut indexer = Indexer::new(&args.repo, &IndexerConfig::default())?;
  let commits = git_log::read_log_all_branches(&args.repo)?;
  let n = indexer.index_commits(&commits.iter().map(Into::into).collect::<Vec<_>>())?;
  log::info!("Indexed {n} new commits for {repo}");
  ```
  Run incrementally (fast). If there are no new commits (common case), it should complete in well under a second (Task 5 acceptance: incremental re-index of 0 commits < 1s).
- **State ownership.** Store `Option<Arc<Indexer>>` (or a lightweight handle) in `AppState` so the search box can call `search_hybrid` directly. The `Arc<Mutex<Indexer>>` is overkill for MVP — rusqlite `Connection` is `Send` but not `Sync`; simplest is to owned it on the main thread and run search synchronously (queries are <100ms).
- **Search box wiring** (`src/ui/search.rs` + `src/state.rs`):
  - On each query change (debounced ~80ms to avoid hammering the indexer), call `indexer.search_hybrid(&query, 20)`.
  - If `indexer` is `None` (DB unavailable / repo not a git repo), fall back to the existing placeholder substring behaviour so the app still works in non-git folders.
  - If `search_hybrid` returns FTS5-only results (model failed to load), use them as-is; do not error to the user.
  - Keep the existing match-count display, now showing the result count from the indexer.
- **Rendering results on canvas.**
  - Add a new `ContainerType::SearchResults` (or extend the existing `GitLogColumn` to accept a `Vec<SearchResult>` directly) positioned at a fixed spot near the search box / canvas center.
  - Each result renders as a commit card identical in style to Epic 002's git-log cards (short hash in cyan, author, date, message title). Cards are transient — cleared when the query is emptied or Escape pressed.
  - Selecting a result (click / Enter) highlights the corresponding commit card in the `GitLogColumn` container if present, scrolling it into view. For MVP, "scroll into view" can be a clamped `scroll_offset` set to the card's y-offset; full smooth scroll is post-epic.
- **Existing dim/brighten behaviour.** Non-matching cards dim to 50% opacity as today; matching result cards stay bright. Since result cards are a separate transient container, the impedance is: dim *all* git-log cards while SearchResults is showing, and show the matches in the SearchResults container. This keeps the existing "dim non-matches" UX in spirit.

**Acceptance Criteria:**
- [ ] With a git repo open, Cmd+K box runs hybrid search debounced (~80ms)
- [ ] Result cards render on canvas as commit cards (hash/author/date/title) in a transient container
- [ ] Clearing the query or pressing Escape removes the result container
- [ ] In a non-git folder, Cmd+K falls back to the existing substring behaviour (no panic)
- [ ] If the embedding model failed to load, Cmd+K still returns keyword matches (FTS5-only)
- [ ] Selecting a result highlights the matching commit in the GitLogColumn and brings it into view (clamped jump is acceptable)
- [ ] No regressions to zoom/pan/drawer

---

### Task 9: Testing and Validation

**Priority:** Medium
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Test the full pipeline end-to-end with real repositories, both CLI and GUI.

**Details:**
- CLI test matrix:
  - `cargo run -- --repo . --index --count 50` — small index
  - `cargo run -- --repo . --search "renderer"` — keyword search
  - `cargo run -- --repo . --search "fix crash"` — semantic search (note difference vs keyword)
  - `cargo run -- --repo . --reindex` — full rebuild
  - `cargo run -- --repo ~/Projects/gitoxide --index` — large repo
  - Verify incremental: run `--index` twice, second run should be fast (0 new commits)
- GUI test matrix:
  - Open sugacode repo, Cmd+K, type "renderer" → cards appear on canvas
  - Type a natural-language query ("fix crash") → results include semantically relevant commits even if they don't contain "crash"
  - Clear query → result cards removed
  - `cargo run -- --no-index` in a fresh cache dir falls back to substring search without crashing
  - Open a non-git folder → Cmd+K still works (substring fallback)
- Verify DB per-repo isolation:
  ```bash
  ls ~/Library/Caches/sugacode/        # should show {repo_slug}.db, one file per indexed repo
  sqlite3 ~/Library/Caches/sugacode/<sugacode_slug>.db "SELECT count(*) FROM items WHERE source_type='commit'"
  ```
- Verify FTS5 sync via triggers:
  ```bash
  sqlite3 <db> "DELETE FROM items; SELECT count(*) FROM items_fts;"   # FTS should be empty too
  ```
- Performance targets:
  - Indexing 1000 commits: < 30 seconds (dominated by embedding time)
  - Search queries: < 100ms
  - Incremental re-index of 0 new commits: < 1 second
  - GUI search debounce keeps typing responsive (<16ms per keystroke beyond the indexer call)
- Edge cases:
  - Empty repository (no commits)
  - Repository with only one commit
  - Commits with empty message body
  - Non-UTF-8 commit messages (handled by `to_str_lossy`)
  - Repo path with spaces / unicode (slug derivation must be stable)
  - Model download failure (offline first run) → FTS5-only degradation, no panic

**Acceptance Criteria:**
- [ ] End-to-end CLI index + search works on sugacode repo
- [ ] End-to-end CLI works on a large repo (gitoxide)
- [ ] End-to-end GUI Cmd+K hybrid search renders result cards on canvas
- [ ] GUI falls back to substring search in non-git folders and on `--no-index`
- [ ] Model-download failure degrades to FTS5-only search (no panic, no hard error)
- [ ] Per-repo DB files are isolated (KNN in repo A never returns repo B's vectors)
- [ ] Incremental indexing correctly skips existing commits
- [ ] `--reindex` produces identical results to fresh index
- [ ] Search returns meaningful results for natural-language queries
- [ ] No panics or crashes on edge cases

---

## Implementation Order

1. **Task 1:** Spike — verify sqlite-vec load on bundled rusqlite (0.5h, blocker)
2. **Task 2:** Workspace setup and dependencies (1h)
3. **Task 3:** Update `git_log.rs` — multi-branch, SHA, body (2h)
4. **Task 4:** `embed.rs` — model2vec-rs wrapper (2h)
5. **Task 5:** `db.rs` — SQLite schema and operations (3h)
6. **Task 6:** `lib.rs` — Indexer public API (2h)
7. **Task 7:** CLI integration (1.5h)
8. **Task 8:** UI integration — Cmd+K hybrid search with result cards (3h)
9. **Task 9:** Testing and validation (2h)

**Total Estimated Time:** ~17 hours (up from 13.5h, mostly the new UI task)

---

## File Changes Summary

| File | Action | Description |
|------|--------|-------------|
| `Cargo.toml` | Modify | Add workspace, add `sugacode-indexer` dep |
| `examples/vec_spike.rs` | **New** (spike, may be deleted after Task 5) | Verify sqlite-vec load incantation |
| `src/git_log.rs` | Modify | Add `sha`, `message_body`, update `read_log`, add `read_log_all_branches` |
| `src/main.rs` | Modify | Add `--index`/`--reindex`/`--search`/`--no-index` CLI args, startup auto-index |
| `src/state.rs` | Modify | Store `Option<Indexer>` in `AppState`, transient SearchResults container |
| `src/ui/search.rs` | Modify | Replace substring matcher with debounced `search_hybrid` + fallback |
| `src/ui/container.rs` | Modify | `SearchResults` container type (or parametrize `GitLogColumn`) |
| `crates/sugacode-indexer/Cargo.toml` | **New** | Indexer crate manifest |
| `crates/sugacode-indexer/src/lib.rs` | **New** | `Indexer`, `CommitData`, `SearchResult`, `IndexerConfig` |
| `crates/sugacode-indexer/src/db.rs` | **New** | Per-repo DB path, schema init, FTS5 + vec0 ops, triggers |
| `crates/sugacode-indexer/src/schema.sql` | **New** | `items`, external-content `items_fts`, sync triggers, `repo_meta` |
| `crates/sugacode-indexer/src/embed.rs` | **New** | model2vec-rs wrapper |

---

## Success Criteria

The feature is complete when:
1. `cargo run -- --repo . --index` indexes all local branches into a per-repo SQLite DB
2. `cargo run -- --repo . --search "query"` returns relevant commits (and exits, no GUI)
3. **In the GUI, Cmd+K runs hybrid search and renders result cards on the canvas**
4. **GUI startup auto-incrementally indexes unless `--no-index` is passed**
5. In non-git folders, Cmd+K falls back to the existing substring behaviour (no crash)
6. If the embedding model fails to load, search degrades to FTS5-only (no panic)
7. Incremental indexing skips already-indexed commits
8. `--reindex` drops and rebuilds cleanly
9. The `items` schema supports future data sources without migration
10. Model switching requires only changing a string constant
11. Code compiles without warnings across the workspace
12. No panics on edge cases (empty repo, empty messages, network failures during model download, non-git folder open in GUI)

---

## Future Enhancements (Post-Epic)

- **Source code indexing** — tree-sitter parsing → items with `source_type='code'`
- **GitHub/Jira issues** — API fetch → items with `source_type='issue'`
- **Markdown files** — filesystem scan → items with `source_type='markdown'`
- **Binary quantization** — `bit[N]` vectors for 32x smaller index on large repos
- **Multi-model support** — different models per source type
- **Smooth scroll-to-result** in the GitLogColumn (currently a clamped jump)
- **Debounced/auto indexing** of new commits while the GUI is running (file watcher)
- **Per-result match highlighting** of matched substrings in the rendered card
