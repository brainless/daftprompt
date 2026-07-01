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
- Existing **sugacode-indexer** stack (rusqlite, sqlite-vec, model2vec-rs)

**Design Principles:**
- Reuse the existing `items` / `items_fts` / `vec_items` schema — no new search tables
- Per-symbol rows for definitions (fn, struct, enum, trait, impl methods, type alias, const, module)
- Per-file rows for comments and use/import declarations
- Methods inside `impl` blocks are indexed individually (the impl block itself is not a separate item)
- Code search is a **separate** API and UI from commit search
- Rust first; TypeScript and other languages in follow-up epics

---

## Architecture

### Data Model

Code items reuse the existing `items` table with `source_type = 'code'`:

| Column | Value | Example |
|--------|-------|---------|
| `source_type` | `'code'` | — |
| `identifier` | `{file_path}::{symbol_name}` | `src/ui/card.rs::render_card` |
| `text` | doc comment + signature + body excerpt | `/// Renders a card...\nfn render_card(&self, card: &CardData) {\n    let pos = ...` |
| `author` | `NULL` | — |
| `metadata` | JSON blob | `{"file_path":"src/ui/card.rs","line_start":42,"line_end":58,"symbol_kind":"function","language":"rust","content_hash":"a1b2c3"}` |

For per-file items (comments, imports):

| Column | Value |
|--------|-------|
| `identifier` | `{file_path}::__comments` or `{file_path}::__imports` |
| `text` | All comments concatenated, or all `use` declarations |
| `metadata` | `{"file_path":"...","symbol_kind":"comments","language":"rust","content_hash":"..."}` |

### File Tracking Table

New table `code_files` tracks which files have been indexed and their change status:

```sql
CREATE TABLE IF NOT EXISTS code_files (
    file_path TEXT PRIMARY KEY,
    mtime INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    indexed_at TEXT NOT NULL
);
```

**Incremental indexing flow:**
1. List tracked `.rs` files from gitoxide (respects `.gitignore`)
2. For each file, stat the mtime
3. If mtime matches `code_files.mtime`, skip (fast path)
4. If mtime differs, hash the file content
5. If hash matches `code_files.content_hash`, update mtime only (touch/git-checkout case)
6. If hash differs, delete old items for this file, re-parse, re-index

### Symbol Extraction

**Per-symbol items** (one row per definition):

| Symbol Kind | Tree-sitter Node | Captured Fields |
|-------------|-----------------|-----------------|
| `function` | `function_item` | name, parameters, return type, doc comment, body excerpt |
| `struct` | `struct_item` | name, fields or tuple, doc comment |
| `enum` | `enum_item` | name, variants, doc comment |
| `trait` | `trait_item` | name, methods (signatures only), doc comment |
| `impl_method` | `function_item` inside `impl_item` | method name, impl type, parameters, doc comment, body excerpt |
| `type_alias` | `type_item` | name, aliased type, doc comment |
| `const` | `const_item`, `static_item` | name, type, value, doc comment |
| `module` | `mod_item` | name, doc comment |
| `macro` | `macro_definition` | name, rules excerpt, doc comment |

**Per-file items** (one row per file):

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

Code search and commit search are **separate** at both the API and UI level:

**API level:** New methods on `Indexer` that filter to `source_type = 'code'`:
- `search_code_text(query, limit)` — FTS5 only, filtered to code items
- `search_code_similar(query, limit)` — Vector KNN only, filtered to code items
- `search_code_hybrid(query, limit)` — RRF-combined, filtered to code items

FTS5 filtering: JOIN back to `items` table and filter by `source_type`:
```sql
SELECT items.id, items_fts.rank
FROM items_fts
JOIN items ON items.id = items_fts.rowid
WHERE items_fts MATCH ? AND items.source_type = 'code'
ORDER BY items_fts.rank LIMIT ?
```

Vector KNN filtering: over-fetch from `vec0` (e.g. `k = limit * 3`), then filter in application code by `source_type`:
```sql
SELECT vec_items.item_id, vec_items.distance
FROM vec_items
WHERE vec_items.embedding MATCH ? AND k = ?
ORDER BY vec_items.distance
```
Then in Rust: lookup each `item_id` in `items`, discard non-code rows, take top `limit`.

**UI level:** Separate search mode activated by `Cmd+Shift+K` (vs `Cmd+K` for commits). The search box shows a different placeholder ("Search code... (⌘⇧K)") and results render in a `CodeSearchResults` container with code-specific card styling.

---

## Tasks

### Task 1: Add tree-sitter Dependencies

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 0.5 hours

**Description:** Add tree-sitter and tree-sitter-rust as dependencies to the `sugacode-indexer` crate.

**Details:**
- Add to `crates/sugacode-indexer/Cargo.toml`:
  ```toml
  tree-sitter = "0.25"
  tree-sitter-rust = "0.24"
  ```
- Verify `cargo check --workspace` passes
- Write a minimal smoke test: parse a trivial Rust snippet, assert the tree has a `source_file` root node

**Acceptance Criteria:**
- [ ] Dependencies resolve and compile
- [ ] Workspace compiles with `cargo check --workspace`
- [ ] Smoke test parses Rust code successfully

---

### Task 2: File Discovery — List Tracked Rust Files

**Priority:** High
**Status:** ⬜ Not Started
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
**Status:** ⬜ Not Started
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
- Content hash: use a fast non-cryptographic hash (e.g. `std::collections::hash_map::DefaultHasher` on file bytes, or the existing `simple_hash` function in `db.rs`)

**Acceptance Criteria:**
- [ ] `code_files` table created on schema init
- [ ] Upsert and get operations work correctly
- [ ] Deleted files can be identified (files in `code_files` but not on disk)

---

### Task 4: Tree-sitter Symbol Extraction for Rust

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 3 hours

**Description:** Implement the core parsing logic that extracts symbols from a Rust file using tree-sitter queries.

**Details:**
- New file: `crates/sugacode-indexer/src/code.rs` (extend from Task 2)
- Core types:
  ```rust
  pub struct CodeSymbol {
      pub identifier: String,        // file_path::symbol_name
      pub text: String,              // doc comment + signature + body excerpt
      pub symbol_kind: SymbolKind,
      pub file_path: String,
      pub line_start: usize,
      pub line_end: usize,
  }

  pub enum SymbolKind {
      Function,
      Struct,
      Enum,
      Trait,
      ImplMethod,
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
  3. Create `Query` from the Rust query patterns (see Architecture section)
  4. Execute query with `QueryCursor`, iterate over matches
  5. For each match, extract:
     - Symbol name from `@name` capture
     - Full node text for signature
     - Doc comment: walk preceding siblings for `line_comment` starting with `///` or `//!`, or `block_comment` starting with `/**`
     - Body excerpt: first 10 lines of the function/impl body (or full body if shorter)
  6. For impl methods: capture the `@impl_type` to build identifier like `file.rs::TypeName::method_name`
  7. Collect standalone comments (not doc comments) into a single `Comments` symbol per file
  8. Collect all `use` declarations into a single `Imports` symbol per file
- Text composition for each symbol:
  ```
  {doc_comment}
  {signature_line}
  {body_excerpt}  // first 10 lines, prefixed with "..." if truncated
  ```
- Body excerpt extraction: use the node's byte range to slice source, take first N lines

**Acceptance Criteria:**
- [ ] Parses a Rust file and extracts all functions, structs, enums, traits, impl methods, type aliases, consts, modules, macros
- [ ] Doc comments are correctly associated with their symbols
- [ ] Impl methods include the impl type in their identifier
- [ ] Body excerpts are truncated to ~10 lines with "..." indicator
- [ ] Standalone comments grouped into one per-file item
- [ ] Use declarations grouped into one per-file item
- [ ] Handles files with parse errors gracefully (extract what it can, log warnings)

---

### Task 5: Code Indexing — `index_code()` on Indexer

**Priority:** High
**Status:** ⬜ Not Started
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
     d. If mtime differs → read file, compute content hash
     e. If hash matches → `code_file_upsert(path, new_mtime, hash)` (touch case)
     f. If hash differs → full re-index of this file:
        - `delete_items_for_identifier_prefix(db, "code", file_path)` — delete all items where identifier starts with `{file_path}::`
        - `extract_symbols(path, source)` → `Vec<CodeSymbol>`
        - Build `ItemRow` vec from symbols
        - `insert_items(db, "code", &items)` → item_ids
        - `encode_batch(texts)` → embeddings
        - `insert_vectors(db, &item_ids, &embeddings)`
        - `code_file_upsert(path, mtime, hash)`
  5. Return `CodeIndexReport`
- All DB operations within a single transaction for atomicity
- Helper function to delete all items for a file:
  ```rust
  fn delete_code_file_items(db: &Connection, file_path: &str) -> anyhow::Result<()>
  // DELETE FROM items WHERE source_type='code' AND identifier LIKE '{file_path}::%'
  // (with explicit vec_items cleanup, same pattern as delete_source)
  ```

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
**Status:** ⬜ Not Started
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
      pub identifier: String,        // file_path::symbol_name
      pub symbol_kind: SymbolKind,
      pub file_path: String,
      pub line_start: usize,
      pub line_end: usize,
      pub text: String,              // doc comment + signature + body excerpt
      pub score: f32,
      pub match_type: MatchType,
  }
  ```
- FTS5 filtering — new DB function:
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
- Vector KNN filtering — over-fetch then filter:
  ```rust
  pub fn search_vec_filtered(db: &Connection, embedding: &[f32], source_type: &str, limit: usize)
      -> anyhow::Result<Vec<(i64, f64)>>
  ```
  Fetch `k = limit * 3` from vec0, lookup each in `items`, discard non-matching, take top `limit`.
- RRF combination: same algorithm as existing `search_hybrid` but using the filtered result sets
- Parse `metadata` JSON to populate `CodeSearchResult` fields

**Acceptance Criteria:**
- [ ] `search_code_text` returns only code items (no commits)
- [ ] `search_code_similar` returns only code items
- [ ] `search_code_hybrid` combines both with RRF, only code items
- [ ] Results include file path, line numbers, symbol kind
- [ ] Empty index returns empty results (no errors)
- [ ] Queries with no matches return empty results

---

### Task 7: CLI Integration — `--index-code` and `--search-code`

**Priority:** Medium
**Status:** ⬜ Not Started
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
- `--reindex-code` drops all code items and re-indexes from scratch
- GUI startup also auto-indexes code (unless `--no-index`), alongside commit auto-indexing

**Acceptance Criteria:**
- [ ] `--index-code` indexes all tracked `.rs` files and prints report
- [ ] `--index-code` twice is idempotent (second run fast, 0 changes)
- [ ] `--search-code` prints results and exits without GUI
- [ ] `--reindex-code` drops and rebuilds code index
- [ ] GUI startup auto-indexes code alongside commits

---

### Task 8: UI Integration — Code Search Mode (`Cmd+Shift+K`)

**Priority:** High
**Status:** ⬜ Not Started
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
**Status:** ⬜ Not Started
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
  - File with only comments → one `comments` item
  - Very large file (>10k lines) → parses without OOM
  - Non-UTF-8 file → skip with warning

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
| `crates/sugacode-indexer/Cargo.toml` | Modify | Add `tree-sitter`, `tree-sitter-rust` |
| `crates/sugacode-indexer/src/schema.sql` | Modify | Add `code_files` table |
| `crates/sugacode-indexer/src/db.rs` | Modify | Add `code_files` CRUD, `search_fts_filtered`, `search_vec_filtered`, `delete_code_file_items` |
| `crates/sugacode-indexer/src/code.rs` | **New** | File discovery, tree-sitter parsing, symbol extraction |
| `crates/sugacode-indexer/src/lib.rs` | Modify | Add `index_code()`, `search_code_*()`, `CodeSearchResult`, `CodeIndexReport`, `SymbolKind` |
| `src/main.rs` | Modify | Add `--index-code`, `--reindex-code`, `--search-code` CLI args, auto-index code on GUI startup |
| `src/state.rs` | Modify | Add `code_search_active`, `code_search_query`, `code_search_results` |
| `src/input.rs` | Modify | Add `Cmd+Shift+K` shortcut, route input to code search |
| `src/ui/search.rs` | Modify | Add code search mode with different placeholder/color |
| `src/ui/container.rs` | Modify | Add `CodeSearchResults` container type and constructor |

---

## Success Criteria

The feature is complete when:
1. `cargo run -- --repo . --index-code` indexes all tracked `.rs` files into the existing SQLite DB
2. `cargo run -- --repo . --search-code "query"` returns relevant code symbols and exits
3. **In the GUI, `Cmd+Shift+K` opens code search and renders result cards on the canvas**
4. Code search results show symbol kind, file path, line number, and text excerpt
5. Code search is **separate** from commit search (different API, different UI mode)
6. Incremental indexing skips unchanged files (mtime + content hash)
7. `--reindex-code` drops and rebuilds cleanly
8. GUI startup auto-indexes code alongside commits (unless `--no-index`)
9. Tree-sitter queries are structured for easy addition of new languages
10. Code compiles without warnings across the workspace

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
