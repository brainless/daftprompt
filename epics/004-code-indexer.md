# Epic 004: Code Indexer — Rust Source Code Search with Tree-sitter

## Introduction

This epic adds source code indexing and search to sugacode. Using tree-sitter, we parse Rust source files, extract symbols (functions, structs, traits, impl methods, etc.), and store them in the existing SQLite database with both FTS5 (full-text) and sqlite-vec (vector embedding) indices. Code search is exposed via a **separate** search API and UI mode, independent from the existing commit search.

### Work Context

**Problem:** Users can search commit messages but cannot find specific functions, structs, or code constructs within the codebase. Searching for "render pipeline" in commits tells you *when* it changed, not *where* it lives.

**Solution:** Build a code indexer that:
- Walks tracked files in the repository (respecting `.gitignore` via gitoxide)
- Parses `.rs` files with tree-sitter, extracting symbols at per-definition granularity
- Groups standalone comments and `use` declarations at per-file granularity
- Stores each symbol as a row in the existing `items` table (`source_type = 'code'`)
- Generates embeddings for semantic search alongside keyword (FTS5) search
- Tracks indexed files with mtime + content hash for efficient incremental re-indexing
- Exposes a **separate** code search API (`search_code_hybrid`) and UI mode (`Cmd+Shift+K`)

**Technology Stack:**
- **tree-sitter** (v0.25) — Incremental parsing engine
- **tree-sitter-rust** (v0.24) — Rust grammar for tree-sitter
- **xxhash-rust** (`xxhash-rust = { version = "0.8", features = ["xxh3"] }`) — Stable, fast non-cryptographic hash for `content_hash` (see Design Decisions)
- Existing **sugacode-indexer** stack (rusqlite, sqlite-vec, model2vec-rs)

**Design Principles:**
- Reuse the existing `items` / `items_fts` schema — no new search tables for keyword search
- **Per-`source_type` partitioned `vec0` tables** — `vec_items` for commits, new `vec_code` for code. KNN isolation is free (no over-fetch-and-filter), and each source type can evolve independently (e.g. different embedding dimensions or quantization later). This diverges from Epic 003's single-`vec_items` design; see **Search Isolation** below for the migration note.
- Per-symbol rows for definitions (fn, struct, enum, trait, impl methods, type alias, const, module)
- Per-file rows for comments and use/import declarations
- Methods inside `impl` blocks **and default methods inside `trait` blocks** are indexed individually (the impl/trait block itself is not a separate item)
- **Full canonical namespaces** in `identifier` — include module path and enclosing type so `src/foo.rs::module::TypeName::method` is unambiguous; see **Symbol Extraction**
- Code search is a **separate** API and UI from commit search
- Rust first; TypeScript and other languages in follow-up epics

---

## Design Decisions

These decisions resolve ambiguity surfaced during epic review. When implementing, keep the **rejected alternatives as comments in the code** so we can revisit during tuning:

1. **Content hash: xxhash (xxh3), not `DefaultHasher`.** `std::collections::hash_map::DefaultHasher` has no stability guarantee across Rust versions — a toolchain bump would silently invalidate every hash and trigger a full reindex. xxh3 is stable, fast, and purpose-built. *Code comment:* note that `simple_hash` (if any) in `db.rs` must not be reused here unless it is also documented as stable.

2. **Vector KNN: partitioned `vec0` tables per `source_type`, not over-fetch-and-filter.** The original Epic 003 design used a single `vec_items` table and filtered by `source_type` after KNN. For code search this is fragile (if code is a minority of rows, `k = limit * 3` may return too few code rows, and no adaptive fallback was specified). Partitioned tables make isolation exact and free. *Code comment:* keep the over-fetch SQL pattern commented in `db.rs` as a fallback if partitioning ever proves costly.

3. **Body excerpt in embeddings: keep for now, flag for tuning.** Embedding the 10-line body excerpt skews semantic search for large functions (a 500-line function's vector is dominated by its first 10 lines). We keep the body excerpt in the embedded text for v1 to maximize signal on small/medium functions, but the implementation must isolate the text-composition step into one function (`compose_symbol_text`) so the excerpt length / inclusion can be tuned without touching parsing. *Code comment:* list the alternatives (signature+doc-only; rolling-window mean of chunked embeddings; body-FTS5-only) so we can A/B them.

4. **`comments` and `imports` items: FTS5-only, no vector insert.** Concatenated standalone comments and `use` declarations produce low-signal embeddings (TODOs, license headers, stale notes). They go into `items` + `items_fts` for keyword search only; the `vec_code` insert is skipped for these rows. *Code comment:* note that if semantic search over comments becomes useful, we can enable vectors here without schema changes.

5. **Per-file transactions, not one transaction for the whole `index_code()` run.** A single bad file (parse error, non-UTF-8, OOM on a 10k-line file) must not roll back the entire run. Each file's delete+insert+embed+vec-insert is its own transaction. *Code comment:* note that batch-level atomicity is rejected because embeddings are the dominant cost and a partial batch on retry is acceptable.

6. **Validate tree-sitter queries once at startup.** `Query::new` failures would otherwise fail mid-run. The Rust query is compiled once in `Indexer::new` (or lazily cached) and reused for every file; a compile error is a startup error, not a per-file error.

7. **Background auto-index on GUI startup.** Epic 003 already auto-indexes commits on startup; doing both commits and code synchronously for a large repo (the test matrix includes gitoxide/wgpu) would stall the GUI for tens of seconds. Code indexing runs on a background thread; the UI shows a "Indexing code…" indicator and enables `Cmd+Shift+K` results when ready. *Code comment:* note the simpler inline path as a fallback if threading proves messy.

8. **Trait default methods are indexed individually.** Symmetric with `impl` methods. Default method bodies inside `trait_item` are captured the same way as impl methods, producing `file_path::TraitName::method`. Non-default trait method *signatures* are folded into the trait item's text (they have no body to excerpt).

---

## Architecture

### Data Model

Code items reuse the existing `items` table with `source_type = 'code'`:

| Column | Value | Example |
|--------|-------|---------|
| `source_type` | `'code'` | — |
| `identifier` | `{file_path}::{module_path}::{symbol_name}` (and `::{TypeName}::{method}` for methods) | `src/ui/card.rs::render_card` (top-level fn) or `src/ui/mod.rs::ui::Card::render` (method) |
| `text` | doc comment + signature + body excerpt | `/// Renders a card...\nfn render_card(&self, card: &CardData) {\n    let pos = ...` |
| `author` | `NULL` | — |
| `metadata` | JSON blob | `{"file_path":"src/ui/card.rs","line_start":42,"line_end":58,"symbol_kind":"function","language":"rust","content_hash":"a1b2c3"}` |

**Identifier canonical form** (see Design Decisions):
- `file_path` is normalized to forward slashes, repo-relative, no leading `./`
- Module path segments are joined with `::` and derived from the tree-sitter `mod_item` ancestors of the symbol
- Impl methods: `{file_path}::{ImplType}::{method}`
- Trait default methods: `{file_path}::{TraitName}::{method}`
- Per-file pseudo-symbols: `{file_path}::__comments`, `{file_path}::__imports`

For per-file items (comments, imports): **FTS5-only, no vector insert** (see Design Decisions):

| Column | Value |
|--------|-------|
| `identifier` | `{file_path}::__comments` or `{file_path}::__imports` |
| `text` | All comments concatenated, or all `use` declarations |
| `metadata` | `{"file_path":"...","symbol_kind":"comments","language":"rust","content_hash":"..."}` |

### File Tracking Table

New table `code_files` tracks which files have been indexed and their change status:

```sql
CREATE TABLE IF NOT EXISTS code_files (
    file_path TEXT PRIMARY KEY,   -- canonical form: forward slashes, repo-relative, no leading ./
    mtime INTEGER NOT NULL,
    content_hash TEXT NOT NULL,   -- xxh3 hex of file bytes (stable across Rust versions)
    indexed_at TEXT NOT NULL
);
```

**Incremental indexing flow:**
1. List tracked `.rs` files from gitoxide (respects `.gitignore`)
2. For each file, stat the mtime
3. If mtime matches `code_files.mtime`, skip (fast path)
4. If mtime differs, hash the file content with **xxh3**
5. If hash matches `code_files.content_hash`, update mtime only (touch/git-checkout case)
6. If hash differs, delete old items for this file, re-parse, re-index

**Hash stability note:** xxh3 output is stable across Rust toolchain versions, unlike `std::collections::hash_map::DefaultHasher`. Do not reuse `simple_hash` (if present in `db.rs`) unless it is also documented as stable — keep a code comment recording this.

### Symbol Extraction

**Per-symbol items** (one row per definition). `identifier` carries the full canonical namespace (see Data Model) so collisions between nested modules and between top-level and method names are impossible:

| Symbol Kind | Tree-sitter Node | Captured Fields |
|-------------|-----------------|-----------------|
| `function` | `function_item` | name, parameters, return type, doc comment, body excerpt; module path from `mod_item` ancestors |
| `struct` | `struct_item` | name, fields or tuple, doc comment |
| `enum` | `enum_item` | name, variants, doc comment |
| `trait` | `trait_item` | name, non-default method signatures (folded into trait text), doc comment |
| `impl_method` | `function_item` inside `impl_item` | method name, impl type, parameters, doc comment, body excerpt |
| `trait_method` | `function_item` inside `trait_item` **with a body** | method name, trait name, parameters, doc comment, body excerpt (default methods only — signature-only methods stay folded into the trait item) |
| `type_alias` | `type_item` | name, aliased type, doc comment |
| `const` | `const_item`, `static_item` | name, type, value, doc comment |
| `module` | `mod_item` | name, doc comment |
| `macro` | `macro_definition` | name, rules excerpt, doc comment |

**Namespace construction:** while walking matches, track the chain of enclosing `mod_item` names (and `impl_item`/`trait_item` type names) to build `{file_path}::{mod1}::{mod2}::{TypeName}::{method}`. This replaces the v1 `file_path::symbol_name` scheme that flattened nested modules.

**Per-file items** (one row per file) — **FTS5-only, no vector insert**:

| Symbol Kind | Content |
|-------------|---------|
| `comments` | All standalone line/block comments (not doc comments — those attach to symbols) |
| `imports` | All `use` declarations |

### Tree-sitter Queries for Rust

Queries are stored as `.scm` files (or inline strings) and organized per-language for future extensibility:

```scheme
;; rust.scm — Symbol extraction queries

(function_item
  name: (identifier) @name) @definition.function

(struct_item
  name: (type_identifier) @name) @definition.struct

(enum_item
  name: (type_identifier) @name) @definition.enum

(trait_item
  name: (type_identifier) @name) @definition.trait

(impl_item
  type: (type_identifier) @impl_type
  body: (declaration_list
    (function_item
      name: (identifier) @name) @definition.impl_method))

;; Trait default methods (methods with bodies inside trait_item).
;; Signature-only trait methods are NOT captured here — they stay folded
;; into the trait item's text.
(trait_item
  name: (type_identifier) @trait_type
  body: (declaration_list
    (function_item
      body: (block_expression)
      name: (identifier) @name) @definition.trait_method))

(type_item
  name: (type_identifier) @name) @definition.type_alias

(const_item
  name: (identifier) @name) @definition.const

(static_item
  name: (identifier) @name) @definition.static

(mod_item
  name: (identifier) @name) @definition.module

(macro_definition
  name: (identifier) @name) @definition.macro

(line_comment) @comment
(block_comment) @comment

(use_declaration) @import
```

### Search Isolation

Code search and commit search are **separate** at both the API and UI level. Isolation is achieved by **partitioning vectors into per-`source_type` `vec0` tables**, not by over-fetch-and-filter.

#### Schema: partitioned vec0 tables

Epic 003 uses a single `vec_items` virtual table for commit vectors. This epic introduces a sibling `vec_code` table for code vectors. KNN over `vec_code` only ever returns code rows — no filtering, no over-fetching, no probabilistic `k` multiplier.

```sql
-- Existing (Epic 003) — commit vectors only:
CREATE VIRTUAL TABLE IF NOT EXISTS vec_items USING vec0(
    item_id INTEGER PRIMARY KEY,
    embedding FLOAT[{dim}]
);

-- New (Epic 004) — code vectors only:
CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
    item_id INTEGER PRIMARY KEY,
    embedding FLOAT[{dim}]
);
```

`{dim}` is read from the loaded model at init time (same as Epic 003). `item_id` references `items.id`; deletes in `items` must cascade to `vec_code` (same trigger/cleanup pattern Epic 003 uses for `vec_items`).

**Migration note for existing DBs:** `vec_code` is created lazily on first code index if absent. Existing `vec_items` rows remain commit-only by convention — no data migration. If a DB was previously polluted with code rows in `vec_items` (only possible from an in-progress Epic 004 build), a `--reindex-code` cleans `vec_code`; stray commit rows are untouched.

*Code comment (in `db.rs`):* keep the rejected over-fetch-and-filter SQL pattern commented out, with a note explaining why partitioning was chosen (exact isolation, free KNN scoping, independent evolution per source type) and when to revisit (if per-source-type tables proliferate beyond 2–3, consider a single partitioned table with a `source_type` column indexed via vec0 metadata if/when sqlite-vec supports it).

#### API level

New methods on `Indexer` that read only from `vec_code` + code-filtered `items_fts`:

- `search_code_text(query, limit)` — FTS5 only, filtered to code items
- `search_code_similar(query, limit)` — Vector KNN over `vec_code` only
- `search_code_hybrid(query, limit)` — RRF-combined

**FTS5 filtering** (FTS5 is shared across source types, so a filter JOIN is still required):
```sql
SELECT items.id, items_fts.rank
FROM items_fts
JOIN items ON items.id = items_fts.rowid
WHERE items_fts MATCH ? AND items.source_type = 'code'
ORDER BY items_fts.rank LIMIT ?
```

**Vector KNN** (no filtering needed — `vec_code` is code-only by construction):
```sql
SELECT item_id, distance
FROM vec_code
WHERE embedding MATCH ? AND k = ?
ORDER BY distance
```

#### UI level

Separate search mode activated by `Cmd+Shift+K` (vs `Cmd+K` for commits). The search box shows a different placeholder ("Search code... (⌘⇧K)") and results render in a `CodeSearchResults` container with code-specific card styling.

---

## Tasks

### Task 1: Add tree-sitter Dependencies

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 0.5 hours

**Description:** Add tree-sitter and tree-sitter-rust as dependencies to the `sugacode-indexer` crate, plus `xxhash-rust` for stable content hashing.

**Details:**
- Add to `crates/sugacode-indexer/Cargo.toml`:
  ```toml
  tree-sitter = "0.25"
  tree-sitter-rust = "0.24"
  xxhash-rust = { version = "0.8", features = ["xxh3"] }
  ```
- Verify `cargo check --workspace` passes
- Write a minimal smoke test: parse a trivial Rust snippet, assert the tree has a `source_file` root node
- Write a second smoke test: `xxh3::xxh3_64(b"hello")` is stable across runs (sanity check, not a cross-version guarantee)

**Acceptance Criteria:**
- [ ] Dependencies resolve and compile
- [ ] Workspace compiles with `cargo check --workspace`
- [ ] Smoke test parses Rust code successfully
- [ ] xxh3 hash function returns a stable, non-`DefaultHasher` value

---

### Task 2: File Discovery — List Tracked Rust Files

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 1 hour

**Description:** Create a function that uses gitoxide to list all tracked `.rs` files in the repository, respecting `.gitignore`.

**Details:**
- New file: `crates/sugacode-indexer/src/code.rs`
- Function signature:
  ```rust
  pub fn list_tracked_rust_files(repo_path: &Path) -> anyhow::Result<Vec<PathBuf>>
  ```
- Implementation using `gix`:
  1. `gix::discover(repo_path)` to open the repo
  2. Walk the HEAD tree, collecting entries with `.rs` extension
  3. Return paths relative to repo root, resolved to absolute paths
- Handle edge cases:
  - Empty repo (no commits) → return empty vec
  - Repo with no `.rs` files → return empty vec
  - Submodules → skip (or follow, document the choice)

**Acceptance Criteria:**
- [ ] Returns all `.rs` files tracked by git
- [ ] Excludes files in `.gitignore` (e.g. `target/` directory)
- [ ] Returns absolute paths
- [ ] Handles empty repos gracefully

---

### Task 3: File Tracking Table (`code_files`)

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 0.5 hours

**Description:** Add the `code_files` table to the schema and implement tracking operations.

**Details:**
- Add to `schema.sql`:
  ```sql
  CREATE TABLE IF NOT EXISTS code_files (
      file_path TEXT PRIMARY KEY,
      mtime INTEGER NOT NULL,
      content_hash TEXT NOT NULL,
      indexed_at TEXT NOT NULL
  );
  ```
- New functions in `db.rs`:
  ```rust
  pub fn code_file_get(db: &Connection, file_path: &str) -> anyhow::Result<Option<(i64, String)>>;
  // Returns (mtime, content_hash) or None

  pub fn code_file_upsert(db: &Connection, file_path: &str, mtime: i64, content_hash: &str) -> anyhow::Result<()>;

  pub fn code_file_delete(db: &Connection, file_path: &str) -> anyhow::Result<()>;

  pub fn code_files_all(db: &Connection) -> anyhow::Result<Vec<String>>;
  // Returns all tracked file paths (for cleanup of deleted files)
  ```
- Content hash: **xxh3** via `xxhash-rust`. Do not use `std::collections::hash_map::DefaultHasher` (no stability guarantee across Rust versions — would silently trigger full reindexes on toolchain bumps). Keep a code comment recording why `DefaultHasher`/`simple_hash` were rejected.

**Acceptance Criteria:**
- [ ] `code_files` table created on schema init
- [ ] Upsert and get operations work correctly
- [ ] Deleted files can be identified (files in `code_files` but not on disk)

---

### Task 4: Tree-sitter Symbol Extraction for Rust

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 3 hours

**Description:** Implement the core parsing logic that extracts symbols from a Rust file using tree-sitter queries.

**Details:**
- New file: `crates/sugacode-indexer/src/code.rs` (extend from Task 2)
- Core types:
  ```rust
  pub struct CodeSymbol {
      pub identifier: String,        // {file_path}::{module_path}::{symbol_name} (and ::{TypeName}::{method} for methods)
      pub text: String,              // doc comment + signature + body excerpt
      pub symbol_kind: SymbolKind,
      pub file_path: String,         // canonical: forward slashes, repo-relative, no leading ./
      pub line_start: usize,
      pub line_end: usize,
      pub embed: bool,               // false for Comments/Imports (FTS5-only — see Design Decisions)
  }

  pub enum SymbolKind {
      Function,
      Struct,
      Enum,
      Trait,
      ImplMethod,
      TraitMethod,   // default methods with bodies inside trait_item
      TypeAlias,
      Const,
      Static,
      Module,
      Macro,
      Comments,
      Imports,
  }
  ```
- Parsing function:
  ```rust
  pub fn extract_symbols(file_path: &Path, source: &str) -> anyhow::Result<Vec<CodeSymbol>>
  ```
- Implementation:
  1. Create `Parser`, set language to `tree_sitter_rust::LANGUAGE`
  2. Parse source into a `Tree`
  3. Create `Query` from the Rust query patterns (see Architecture section) — **compiled once and reused across files**, not per-file (see Design Decisions)
  4. Execute query with `QueryCursor`, iterate over matches
  5. For each match, extract:
     - Symbol name from `@name` capture
     - Full node text for signature
     - Doc comment: walk preceding siblings for `line_comment` starting with `///` or `//!`, or `block_comment` starting with `/**`
     - Body excerpt: first 10 lines of the function/impl body (or full body if shorter)
  6. **Build the canonical namespace** while walking: track enclosing `mod_item` names plus the `impl_type`/`trait_type` of any enclosing `impl_item`/`trait_item`, joined with `::`. For impl methods: `{file_path}::{ImplType}::{method}`. For trait default methods: `{file_path}::{TraitName}::{method}`. For free items: `{file_path}::{mod_path}::{name}` (empty `mod_path` collapses). See Data Model for the canonical form.
  7. **Trait default methods**: emitted as `TraitMethod` rows with their own `identifier` and body excerpt. Trait methods *without* bodies (signatures only) are folded into the parent `Trait` symbol's `text` and not emitted separately.
  8. Collect standalone comments (not doc comments) into a single `Comments` symbol per file, `embed = false`
  9. Collect all `use` declarations into a single `Imports` symbol per file, `embed = false`
- **Text composition is isolated into one function:**
  ```rust
  fn compose_symbol_text(doc_comment: &str, signature: &str, body_excerpt: &str) -> String
  ```
  This is the single tuning knob for what gets embedded. Keep alternatives as code comments so we can A/B them later:
  - signature + doc-comment only (drop body excerpt from embeddings — helps large fns)
  - rolling-window mean of chunked embeddings for long bodies
  - body-terms-via-FTS5-only (vectors over signature/doc only)
  Current v1 choice: include the 10-line excerpt in the embedded text.
- Body excerpt extraction: use the node's byte range to slice source, take first N lines

**Acceptance Criteria:**
- [ ] Parses a Rust file and extracts all functions, structs, enums, traits, impl methods, **trait default methods**, type aliases, consts, modules, macros
- [ ] Doc comments are correctly associated with their symbols
- [ ] Impl methods include the impl type in their identifier
- [ ] Trait default methods include the trait name in their identifier; signature-only trait methods are folded into the trait item
- [ ] Nested module path is reflected in `identifier` (e.g. `src/lib.rs::outer::inner::fn_name`)
- [ ] Body excerpts are truncated to ~10 lines with "..." indicator
- [ ] Standalone comments grouped into one per-file item with `embed = false`
- [ ] Use declarations grouped into one per-file item with `embed = false`
- [ ] `file_path` is canonicalized (forward slashes, repo-relative, no leading `./`)
- [ ] `compose_symbol_text` is a standalone function with the alternative strategies listed in a comment
- [ ] Handles files with parse errors gracefully (extract what it can, log warnings)

---

### Task 5: Code Indexing — `index_code()` on Indexer

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 2 hours

**Description:** Wire together file discovery, symbol extraction, and the existing DB/embedding pipeline into `Indexer::index_code()`.

**Details:**
- New method on `Indexer`:
  ```rust
  pub fn index_code(&mut self) -> anyhow::Result<CodeIndexReport>

  pub struct CodeIndexReport {
      pub files_scanned: usize,
      pub files_changed: usize,
      pub files_deleted: usize,
      pub symbols_indexed: usize,
  }
  ```
- Implementation flow:
  1. `list_tracked_rust_files(&self.repo_path)` → current files
  2. `code_files_all(&self.db)` → previously indexed files
  3. Compute deleted files (in DB but not on disk) → `delete_code_file(path)` for each
  4. For each current file:
     a. Stat mtime
     b. `code_file_get(path)` → compare mtime
     c. If mtime matches → skip
     d. If mtime differs → read file, compute content hash with **xxh3**
     e. If hash matches → `code_file_upsert(path, new_mtime, hash)` (touch case)
     f. If hash differs → full re-index of this file, **in its own transaction** (see Design Decisions):
        - `delete_code_file_items(db, file_path)` — delete all items where `source_type='code'` AND `identifier` starts with the canonical `{file_path}::` (exact-prefix match on the normalized form; see helper below)
        - `extract_symbols(path, source)` → `Vec<CodeSymbol>`
        - Build `ItemRow` vec from symbols
        - `insert_items(db, "code", &items)` → item_ids
        - For symbols with `embed == true`: `encode_batch(texts)` → embeddings, then `insert_vectors(db, "code", &item_ids, &embeddings)` writing into **`vec_code`** (not `vec_items`)
        - Skip the vec insert for `Comments`/`Imports` (`embed == false`) — they are FTS5-only
        - `code_file_upsert(path, mtime, hash)`
  5. Return `CodeIndexReport`
- **One transaction per file**, not one for the whole run. A parse error, non-UTF-8 file, or OOM on a 10k-line file must roll back only that file, not the entire batch. Keep a code comment noting why batch-level atomicity was rejected (embeddings are the dominant cost; partial progress on retry is acceptable).
- Helper function to delete all items for a file:
  ```rust
  fn delete_code_file_items(db: &Connection, file_path: &str) -> anyhow::Result<()>
  // file_path is already canonical (forward slashes, repo-relative, no leading ./).
  // Delete items where source_type='code' AND identifier = '{file_path}::' || suffix
  // for any suffix — implemented as identifier LIKE '{file_path}::%' ESCAPE '\'.
  // IMPORTANT: file_path must not contain LIKE wildcards; escape '%' and '_'.
  // (Alternative: a separate code_items table keyed by file_path — heavier migration,
  //  revisit if identifier-prefix matching ever proves slow. Keep as code comment.)
  // Also delete matching rows from vec_code (same identifier set).
  ```
- `insert_vectors` is parameterized by the target vec table (`vec_items` for commits, `vec_code` for code) — see Search Isolation.

**Acceptance Criteria:**
- [ ] First run indexes all tracked `.rs` files
- [ ] Second run with no changes reports 0 files changed (fast, <1s for small repos)
- [ ] Modifying a file re-indexes only that file
- [ ] Deleting a file removes its items from the index
- [ ] `touch`-ing a file (mtime change, no content change) does not re-index
- [ ] `CodeIndexReport` accurately reflects what happened

---

### Task 6: Code Search API — `search_code_hybrid()`

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 1.5 hours

**Description:** Add code-specific search methods to `Indexer` that filter results to `source_type = 'code'`.

**Details:**
- New methods on `Indexer`:
  ```rust
  pub fn search_code_text(&self, query: &str, limit: usize) -> anyhow::Result<Vec<CodeSearchResult>>
  pub fn search_code_similar(&self, query: &str, limit: usize) -> anyhow::Result<Vec<CodeSearchResult>>
  pub fn search_code_hybrid(&self, query: &str, limit: usize) -> anyhow::Result<Vec<CodeSearchResult>>
  ```
- New result type (richer than `SearchResult` for code-specific metadata):
  ```rust
  pub struct CodeSearchResult {
      pub identifier: String,        // {file_path}::{module_path}::{symbol_name} (or ::{TypeName}::{method})
      pub symbol_kind: SymbolKind,
      pub file_path: String,
      pub line_start: usize,
      pub line_end: usize,
      pub text: String,              // doc comment + signature + body excerpt
      pub score: f32,
      pub match_type: MatchType,
  }
  ```
- FTS5 filtering — new DB function (FTS5 is shared across source types, so the filter JOIN is still required):
  ```rust
  pub fn search_fts_filtered(db: &Connection, query: &str, source_type: &str, limit: usize)
      -> anyhow::Result<Vec<(i64, f64)>>
  ```
  SQL:
  ```sql
  SELECT items.id, items_fts.rank
  FROM items_fts
  JOIN items ON items.id = items_fts.rowid
  WHERE items_fts MATCH ? AND items.source_type = ?
  ORDER BY items_fts.rank LIMIT ?
  ```
- **Vector KNN over the partitioned `vec_code` table** — no over-fetch, no application-level filtering (see Search Isolation):
  ```rust
  pub fn search_vec_code(db: &Connection, embedding: &[f32], limit: usize)
      -> anyhow::Result<Vec<(i64, f64)>>
  ```
  SQL:
  ```sql
  SELECT item_id, distance
  FROM vec_code
  WHERE embedding MATCH ? AND k = ?
  ORDER BY distance
  ```
  *Code comment:* keep the rejected over-fetch-and-filter pattern (`k = limit * 3` against `vec_items`, then filter by `source_type` in Rust) commented out, with a note on why partitioning replaced it (exact isolation, no probabilistic `k`) and when to revisit (if many `source_type`s make per-type tables proliferate).
- RRF combination: same algorithm as existing `search_hybrid` but combining the FTS5-filtered set with the `vec_code` KNN set
- Parse `metadata` JSON to populate `CodeSearchResult` fields

**Acceptance Criteria:**
- [ ] `search_code_text` returns only code items (no commits)
- [ ] `search_code_similar` reads only from `vec_code` and returns only code items
- [ ] `search_code_hybrid` combines both with RRF, only code items
- [ ] Results include file path, line numbers, symbol kind
- [ ] Empty index returns empty results (no errors)
- [ ] Queries with no matches return empty results
- [ ] Commit search (`search_hybrid` etc.) is unaffected — still reads from `vec_items`

---

### Task 7: CLI Integration — `--index-code` and `--search-code`

**Priority:** Medium
**Status:** ✅ Done
**Estimated Time:** 1 hour

**Description:** Add CLI flags for code indexing and searching, separate from the existing commit flags.

**Details:**
- Extend `Args` in `src/main.rs`:
  ```rust
  #[arg(long)]
  index_code: bool,

  #[arg(long)]
  reindex_code: bool,

  #[arg(long)]
  search_code: Option<String>,
  ```
- CLI flow:
  ```
  cargo run -- --repo . --index-code
  # Indexes all tracked .rs files, prints CodeIndexReport

  cargo run -- --repo . --search-code "render pipeline"
  # Prints code search results and exits (no GUI)

  cargo run -- --repo . --index-code --search-code "AppState"
  # Indexes first, then searches, prints results and exits
  ```
- Output format for `--search-code`:
  ```
  [0.032] fn     src/renderer.rs:42       render_pipeline — Sets up the wgpu render pipeline...
  [0.028] struct src/state.rs:10          AppState — Main application state...
  [0.015] fn     src/ui/card.rs:88        render_card — Renders a single document card...
  ```
- `--reindex-code` drops all code items (and `vec_code` rows) and re-indexes from scratch
- GUI startup also auto-indexes code (unless `--no-index`), alongside commit auto-indexing — **on a background thread** so the event loop is not blocked (see Design Decisions). The UI shows an "Indexing code…" indicator and enables `Cmd+Shift+K` results when the background index completes. *Code comment:* keep the simpler inline path noted as a fallback if threading proves messy.

**Acceptance Criteria:**
- [ ] `--index-code` indexes all tracked `.rs` files and prints report
- [ ] `--index-code` twice is idempotent (second run fast, 0 changes)
- [ ] `--search-code` prints results and exits without GUI
- [ ] `--reindex-code` drops and rebuilds code index
- [ ] GUI startup auto-indexes code alongside commits

---

### Task 8: UI Integration — Code Search Mode (`Cmd+Shift+K`)

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 2.5 hours

**Description:** Add a separate code search mode to the UI, activated by `Cmd+Shift+K`, with its own search results container and card styling.

**Details:**
- **State changes** (`src/state.rs`):
  ```rust
  pub struct AppState {
      // ... existing fields ...
      pub code_search_active: bool,
      pub code_search_query: String,
      pub code_search_results: Vec<CodeSearchResult>,
  }
  ```
- **Input handling** (`src/input.rs`):
  - `Cmd+Shift+K` toggles code search mode (sets `code_search_active = true`)
  - `Cmd+K` toggles commit search mode (existing behavior)
  - `Escape` closes whichever search is active
  - Text input routes to `code_search_query` when `code_search_active`
- **Search box** (`src/ui/search.rs`):
  - When `code_search_active`: placeholder shows "Search code... (⌘⇧K)", different accent color (e.g. green instead of blue)
  - Debounced search (~80ms) calls `indexer.search_code_hybrid(&query, 20)`
  - Results render in a `CodeSearchResults` container
- **Container** (`src/ui/container.rs`):
  - New `ContainerType::CodeSearchResults`
  - `Container::new_code_search_results(id, position, width, height, results)`
  - Card layout similar to commit search results but with code-specific styling:
    - Symbol kind icon/badge (fn, struct, trait, etc.)
    - File path + line number in header
    - Symbol name as title
    - Signature/doc excerpt as body
  - Card colors: slightly different tint from commit cards (e.g. green-ish accent vs blue)
- **Mutual exclusivity**: Only one search mode active at a time. Opening code search closes commit search and vice versa.
- **Indexing-in-progress state**: if code indexing is still running in the background when `Cmd+Shift+K` is opened, the search box shows "Indexing code… results will appear shortly" and queries are debounced but not executed until indexing finishes. *Code comment:* note the alternative of running queries against a partial index (already supported by the schema) — acceptable, just needs a UI hint that results may be incomplete.

**Acceptance Criteria:**
- [ ] `Cmd+Shift+K` opens code search mode
- [ ] Search box shows code-specific placeholder and accent color
- [ ] Typing runs hybrid code search, results appear as cards
- [ ] Cards show symbol kind, file path, line number, symbol name, text excerpt
- [ ] `Escape` closes code search
- [ ] Opening commit search closes code search (mutual exclusivity)
- [ ] No regressions to existing commit search

---

### Task 9: Testing and Validation

**Priority:** Medium
**Status:** ✅ Done
**Estimated Time:** 1.5 hours

**Description:** Test the full code indexing and search pipeline end-to-end.

**Details:**
- CLI test matrix:
  - `cargo run -- --repo . --index-code` — index sugacode's own Rust files
  - `cargo run -- --repo . --index-code` (second run) — should be fast, 0 changes
  - `cargo run -- --repo . --search-code "render"` — should find render functions
  - `cargo run -- --repo . --search-code "AppState"` — should find the struct
  - `cargo run -- --repo . --search-code "index commits"` — semantic search, should find indexer functions
  - `cargo run -- --repo ~/Projects/gitoxide --index-code` — large repo stress test
  - `cargo run -- --repo ~/Projects/wgpu --index-code` — another large repo
- Incremental indexing tests:
  - Modify a single file → only that file re-indexed
  - `touch` a file (no content change) → mtime updated, no re-index
  - Delete a file → items removed from index
  - Add a new `.rs` file → picked up on next index
- GUI test matrix:
  - Open sugacode repo, `Cmd+Shift+K`, type "render" → code cards appear
  - Type "fix state" → semantic results include state management functions
  - Clear query → result cards removed
  - `Cmd+K` while code search is open → switches to commit search
- Performance targets:
  - Initial index of 100-file Rust project: < 30 seconds (dominated by embeddings)
  - Incremental re-index (0 changes): < 1 second
  - Code search queries: < 100ms
- Edge cases:
  - File with syntax errors → extract what tree-sitter can parse, log warnings
  - Empty file → no symbols, no crash
  - File with only comments → one `comments` item, FTS5-indexed only (no `vec_code` row)
  - Very large file (>10k lines) → parses without OOM; one-file transaction means other files survive a failure here
  - Non-UTF-8 file → skip with warning
  - Trait with default methods → each default method is its own `TraitMethod` row with `file_path::TraitName::method` identifier; signature-only methods fold into the trait item
  - Nested modules → `identifier` reflects full module path (`src/lib.rs::outer::inner::fn`)
  - `touch` (mtime change, no content change) → no re-index; xxh3 hash unchanged

**Acceptance Criteria:**
- [ ] End-to-end CLI index + search works on sugacode repo
- [ ] End-to-end CLI works on large repos (gitoxide, wgpu)
- [ ] Incremental indexing correctly detects changes, skips unchanged files
- [ ] `touch` without content change does not trigger re-index
- [ ] GUI `Cmd+Shift+K` code search renders result cards
- [ ] Code search results are visually distinct from commit search results
- [ ] No panics on edge cases (syntax errors, empty files, non-UTF-8)
- [ ] Performance targets met

---

## Implementation Order

1. **Task 1:** Add tree-sitter dependencies (0.5h)
2. **Task 2:** File discovery — list tracked Rust files (1h)
3. **Task 3:** File tracking table (0.5h)
4. **Task 4:** Tree-sitter symbol extraction for Rust (3h)
5. **Task 5:** Code indexing — `index_code()` on Indexer (2h)
6. **Task 6:** Code search API — `search_code_hybrid()` (1.5h)
7. **Task 7:** CLI integration (1h)
8. **Task 8:** UI integration — `Cmd+Shift+K` code search (2.5h)
9. **Task 9:** Testing and validation (1.5h)

**Total Estimated Time:** ~13.5 hours

---

## File Changes Summary

| File | Action | Description |
|------|--------|-------------|
| `crates/sugacode-indexer/Cargo.toml` | Modify | Add `tree-sitter`, `tree-sitter-rust`, `xxhash-rust` |
| `crates/sugacode-indexer/src/schema.sql` | Modify | Add `code_files` table; add partitioned `vec_code` virtual table (sibling of `vec_items`) |
| `crates/sugacode-indexer/src/db.rs` | Modify | Add `code_files` CRUD, `search_fts_filtered`, `search_vec_code`, `delete_code_file_items`; parameterize `insert_vectors` by vec-table target. Keep rejected over-fetch-and-filter SQL + `DefaultHasher`/`simple_hash` notes as comments. |
| `crates/sugacode-indexer/src/code.rs` | **New** | File discovery, tree-sitter parsing, symbol extraction, `compose_symbol_text` (with tuning alternatives in comments), canonical namespace construction |
| `crates/sugacode-indexer/src/lib.rs` | Modify | Add `index_code()`, `search_code_*()`, `CodeSearchResult`, `CodeIndexReport`, `SymbolKind` (incl. `TraitMethod`); compile tree-sitter `Query` once at construction; thread `embed: bool` through the pipeline |
| `src/main.rs` | Modify | Add `--index-code`, `--reindex-code`, `--search-code` CLI args; spawn background code index on GUI startup |
| `src/state.rs` | Modify | Add `code_search_active`, `code_search_query`, `code_search_results`, `code_indexing_in_progress` |
| `src/input.rs` | Modify | Add `Cmd+Shift+K` shortcut, route input to code search |
| `src/ui/search.rs` | Modify | Add code search mode with different placeholder/color; show "Indexing code…" state |
| `src/ui/container.rs` | Modify | Add `CodeSearchResults` container type and constructor |

---

## Success Criteria

The feature is complete when:
1. `cargo run -- --repo . --index-code` indexes all tracked `.rs` files into the existing `items`/`items_fts` tables and the new `vec_code` table
2. `cargo run -- --repo . --search-code "query"` returns relevant code symbols and exits
3. **In the GUI, `Cmd+Shift+K` opens code search and renders result cards on the canvas**
4. Code search results show symbol kind, file path, line number, and text excerpt
5. Code search is **separate** from commit search (different API, different UI mode, partitioned `vec_code` table)
6. Incremental indexing skips unchanged files (mtime + **xxh3** content hash, stable across Rust versions)
7. `--reindex-code` drops and rebuilds cleanly (clears `items` code rows + `vec_code` rows)
8. GUI startup auto-indexes code on a **background thread** alongside commits (unless `--no-index`)
9. Tree-sitter queries are structured for easy addition of new languages
10. Code compiles without warnings across the workspace
11. `Comments`/`Imports` items are FTS5-only (no `vec_code` rows)
12. Trait default methods are indexed individually; nested-module paths are reflected in `identifier`

---

## Future Enhancements (Post-Epic)

- **TypeScript support** — Add `tree-sitter-typescript`, write TS-specific queries, extend `extract_symbols` with language dispatch
- **More languages** — Python, Go, etc. via the same tree-sitter pattern
- **Language auto-detection** — Dispatch parser based on file extension
- **Go-to-definition** — Click a code search result to open the file at the exact line
- **Reference tracking** — Index function calls and type references, not just definitions
- **Live re-indexing** — File watcher (e.g. `notify` crate) to re-index on save
- **Symbol graph** — Visualize call relationships between indexed symbols on the canvas
- **Binary quantization** — Use `bit[N]` vectors for smaller index on large codebases
- **Scope-aware search** — Filter by module, crate, or directory
