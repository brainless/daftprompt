# Epic 007: Document Indexing and Unified Search

## Introduction

This epic adds repository document indexing and search, initially for Markdown and plain-text files. After completion, sugacode has three independently searchable sources:

1. Git log (commit history)
2. Codebase (Rust symbols)
3. Documents (Markdown and plain text)

All three sources use the existing hybrid-search approach: SQLite FTS5 for keyword matches and sqlite-vec embeddings for semantic similarity, fused with Reciprocal Rank Fusion (RRF). If the embedding model cannot load, every source continues to provide FTS5-only search.

The existing generic CLI commands, `--index`, `--reindex`, and `--search`, become all-source operations. New source-specific commands preserve targeted indexing/search, including explicit git-log names for what the generic commands previously did.

The GUI changes from a mode-specific search box into one unified search that queries all three sources and maintains one reusable results container for each source.

## Goals

- Index repository Markdown and plain-text documents incrementally.
- Search documents by both keywords and semantic similarity.
- Keep git-log, code, and document data/search results isolated by source type.
- Make `--index`, `--reindex`, and `--search` operate across all three sources.
- Provide source-specific CLI flags, including `--index-git-log` and `--search-git-log`.
- Make the GUI unified search display git-log, codebase, and document results concurrently in separate reusable containers.
- Preserve graceful degradation: unavailable embeddings mean FTS5-only; a non-git directory must not prevent the GUI from launching.

## Non-goals

- Parsing Markdown into a block/heading hierarchy or extracting links/front matter as separate items.
- Indexing binary formats, PDFs, Office files, HTML, notebooks, or source languages beyond the existing Rust support.
- Cross-repository search.
- Changing the current hybrid ranking formula or embedding model.

## Proposed CLI Contract

### Generic all-source commands

| Command | Meaning |
|---|---|
| `--index` | Incrementally index git log, codebase, and documents. |
| `--reindex` | Rebuild all three indexes. |
| `--search <query>` | Hybrid-search all three sources and print one combined, cross-source ranked result list. |

### Source-specific commands

| Source | Incremental | Rebuild | Search |
|---|---|---|---|
| Git log | `--index-git-log` | `--reindex-git-log` | `--search-git-log <query>` |
| Codebase | `--index-code` | `--reindex-code` | `--search-code <query>` |
| Documents | `--index-documents` | `--reindex-documents` | `--search-documents <query>` |

`--index-git-log`, `--reindex-git-log`, and `--search-git-log` replace the old source-specific meaning of `--index`, `--reindex`, and `--search`; those generic flags are intentionally repurposed as all-source operations.

Rules:

- A source-specific index/reindex followed by that source’s search in the same invocation runs in that order.
- A generic index/reindex followed by `--search` runs all indexing first, then all searches.
- Generic and source-specific operations may be combined only when their requested work does not duplicate a source. Clap validation should reject ambiguous combinations such as `--index --index-code`.
- CLI output must label every result as `Git log`, `Codebase`, or `Documents`, while preserving a single combined rank order.

## Architecture

### Document data model

Documents reuse the shared `items` and external-content `items_fts` tables:

| Field | Value |
|---|---|
| `source_type` | `document` |
| `identifier` | normalized repo-relative path, using forward slashes; `::{chunk_ordinal}` suffix for a chunked file |
| `text` | complete UTF-8 document text for a short file; section/chunk text for a long file |
| `author` | `NULL` |
| `metadata` | JSON including `file_path`, `extension`, `mtime`, and `content_hash` |

Documents at or below `DOCUMENT_CHUNK_THRESHOLD_BYTES` (initially 16 KiB, as a named tuning constant) produce one item. Larger documents are chunked now:

- Markdown is split on ATX headings, preserving the heading and its ancestor heading path in each chunk.
- A heading section still over the threshold is split at paragraph boundaries; a paragraph over the threshold is split into bounded text windows.
- Plain text is split at blank-line/paragraph boundaries, then bounded text windows if necessary.
- Every chunk’s metadata includes the file path, chunk ordinal, byte/line range, and heading path when applicable.

This improves semantic recall and result previews for long documents while preserving a stable link back to the source file.

### Vector isolation

Add a dedicated `vec_documents` `vec0` table. Git log remains in `vec_items`, code remains in `vec_code`, and document KNN queries use only `vec_documents`.

This avoids result dilution and over-fetch/filter logic: each vector KNN query is exactly scoped to its source. Retain a code comment near the query documenting the rejected shared-vector-table alternative, consistent with Epic 004’s tuning notes.

### Incremental document tracking

Add a `document_files` table analogous to `code_files`:

```sql
CREATE TABLE IF NOT EXISTS document_files (
    file_path TEXT PRIMARY KEY,
    mtime INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    indexed_at TEXT NOT NULL
);
```

The document indexer must:

1. Discover eligible document files.
2. Normalize each path to repo-relative, forward-slash form.
3. Skip an unchanged `mtime` fast path.
4. On changed `mtime`, compare an xxh3 content hash; update only the stored mtime when content is unchanged.
5. Delete and replace the item/vector when content changed.
6. Remove stale items, vectors, and tracking rows for files no longer discovered.
7. Use one transaction per file, so one unreadable or malformed file cannot roll back the batch.

Use xxh3 for `content_hash`; do not use `DefaultHasher`.

### Discovery and failure behavior

The v1 file set is Git-tracked files only, discovered through gitoxide. It includes `.md`, `.markdown`, `.txt`, `.text`, `.rst`, and `.adoc`, plus extensionless files named `README` or `LICENSE` (case-insensitive). Ignored and untracked files are excluded. The indexer skips invalid UTF-8 files with a warning and continues.

Document indexing is unavailable for a non-git directory in this epic; this is an intentional scope boundary rather than a filesystem-walk fallback.

### Public indexer API

Add document equivalents to the existing code APIs:

- `index_documents() -> DocumentIndexReport`
- `reindex_documents() -> DocumentIndexReport`
- `search_document_text(query, limit) -> Vec<DocumentSearchResult>`
- `search_document_similar(query, limit) -> Vec<DocumentSearchResult>`
- `search_document_hybrid(query, limit) -> Vec<DocumentSearchResult>`

Add a shared application-facing operation, `search_all_hybrid(query, limit_per_source)`, returning both a combined list and source groups. It is the sole query path for generic CLI search and GUI unified search.

For a meaningful combined rank, the implementation must not merge independently RRF-ranked source lists. Instead, it gathers candidates from all three source-specific FTS and vector indexes, ranks FTS candidates together, ranks vector candidates together by cosine distance, and applies the existing RRF formula over those two global rankings. Each result retains its source type and source-specific detail. The API then derives the three source groups from that same ranked candidate set for the GUI. This is straightforward with the existing same-model embeddings and cosine metric, while still keeping KNN queries isolated to `vec_items`, `vec_code`, and `vec_documents`.

Similarly add a shared all-source indexing entry point that reports each source independently. Git-log reads remain in the binary/application layer because they rely on `git_log.rs`; the indexer crate owns code and document discovery/indexing.

## GUI Design

### Unified query

The primary search UI runs the same all-source operation as generic `--search`. A query updates three result sets: git log, codebase, and documents. The former commit-only `Cmd+K` behavior becomes unified search.

Remove the code-only `Cmd+Shift+K` / `Ctrl+Shift+K` shortcut and the separate code-search mode for now. All three sources are available from the unified search surface.

### Result containers

Render up to three distinct containers on the canvas:

| Source | Container type | Suggested title |
|---|---|---|
| Git log | `SearchResults` (rename if clarity warrants) | `Git log results` |
| Codebase | `CodeSearchResults` | `Codebase results` |
| Documents | `DocumentSearchResults` | `Document results` |

Containers have stable source identity. On each query change, update the matching container’s cards/documents in place (or replace its content while retaining the same container instance/id, world position, size, and scroll position). Never append a new results container per keystroke.

An empty query removes all three results containers. A non-empty query may retain a container with a clear empty state, or omit only that source’s container; choose one behavior and apply it consistently. A search failure for one source must warn and leave the other two result sets usable.

Document cards must show at least the repo-relative path and a text preview; Markdown does not require special rendering in this epic.

### Startup indexing

Do not introduce or expand GUI startup indexing in this epic. Preserve the existing GUI behavior exactly. If the existing behavior indexes on launch, route it through the same shared all-source indexing operations used by the CLI; if it does not, keep indexing CLI-triggered. `--no-index` retains its existing meaning.

## Tasks

### Task 1: Define CLI migration and shared operation types

**Priority:** High  
**Status:** ⬜ Not Started

- Add generic all-source and explicit source-specific Clap arguments.
- Add conflict/dependency validation and update `--help` text.
- Introduce shared result/report types for combined all-source search and per-source index reports.
- Update docs that currently describe generic flags as git-log-only.

**Acceptance Criteria:**

- [ ] Generic and source-specific commands match the CLI contract above.
- [ ] Ambiguous duplicate-source combinations fail with actionable Clap errors.
- [ ] Generic search output is one combined rank order with a source label on every result.

### Task 2: Add document schema and vector isolation

**Priority:** High  
**Status:** ⬜ Not Started

- Add `document_files` and lazy `vec_documents` creation.
- Add source-aware vector cleanup for document deletes/reindexes.
- Add document-filtered FTS and document-only KNN database queries.

**Acceptance Criteria:**

- [ ] Existing git-log and code indexes remain queryable after schema initialization.
- [ ] Document vectors cannot appear in git-log or code KNN results.
- [ ] Deleting/replacing a document removes its old vector and FTS entry through the established invariants.

### Task 3: Implement document discovery and incremental indexing

**Priority:** High  
**Status:** ⬜ Not Started

- Implement eligible-file discovery, path normalization, UTF-8 handling, xxh3 tracking, stale-file removal, and per-file transactions.
- Implement `index_documents`, `reindex_documents`, and `DocumentIndexReport`.
- Add fixtures/tests for Markdown, plain text, touch-without-content-change, edits, deletions, invalid UTF-8, and the non-git unsupported-path behavior.

**Acceptance Criteria:**

- [ ] Eligible Markdown/plain-text files are indexed once under `source_type = 'document'`.
- [ ] A touch without content change does not re-embed the file.
- [ ] Changed/deleted files update/remove all corresponding data.
- [ ] One bad file logs and does not abort the remaining index run.

### Task 4: Implement document hybrid search and unified search API

**Priority:** High  
**Status:** ⬜ Not Started

- Implement FTS-only, vector-only, and RRF document search methods.
- Implement all-source hybrid search with global candidate ranking followed by RRF, returning both the combined result list and groups derived from it.
- Ensure no-embedder mode returns FTS-only results for every applicable source.

**Acceptance Criteria:**

- [ ] A document can be found by keyword and by semantically related query when embeddings are available.
- [ ] Document search falls back to FTS5 when embeddings are unavailable.
- [ ] Unified search returns a combined, cross-source hybrid rank and source groups derived from the same result set.

### Task 5: Wire CLI indexing and search

**Priority:** High  
**Status:** ⬜ Not Started

- Replace the current generic git-log-only control flow with the all-source flow.
- Wire all explicit source commands, retaining clean output formatting per result type.
- Verify both index-first/search-second same-invocation flows.

**Acceptance Criteria:**

- [ ] `--index`, `--reindex`, and `--search` exercise all three sources; generic search output is combined and source-labelled.
- [ ] `--index-git-log` and `--search-git-log` retain git-log-only behavior.
- [ ] Code and document source-specific commands work independently.

### Task 6: Add reusable document and unified GUI result containers

**Priority:** High  
**Status:** ⬜ Not Started

- Add `DocumentSearchResults` and its card adapter/rendering.
- Change primary GUI search to the shared all-source search function.
- Rework result updates so each source has at most one stable container, preserving id, world position, size, and scroll offset across query changes.
- Clear/update all source containers coherently on empty query, close, and search errors.

**Acceptance Criteria:**

- [ ] One GUI query displays separate git-log, codebase, and document result containers when each has results.
- [ ] Repeated query edits do not increase the number of result containers.
- [ ] Document cards show path and preview.
- [ ] Existing card selection/click handling remains valid after in-place result updates.

### Task 7: Startup indexing, documentation, and verification

**Priority:** Medium  
**Status:** ⬜ Not Started

- Preserve GUI startup indexing semantics; when it indexes, ensure it calls the shared all-source indexing operations.
- Update `AGENTS.md`, `DEVELOP.md`, `README.md`, CLI help examples, and architecture comments.
- Add unit/integration coverage and manually validate GUI behavior.

**Acceptance Criteria:**

- [ ] Existing GUI startup-indexing behavior remains intact; any startup indexing uses the shared operation.
- [ ] `--no-index` retains its existing behavior.
- [ ] `cargo check --workspace` and `cargo test --workspace` pass.
- [ ] Documentation accurately describes three sources and the changed CLI contract.

## File-change Summary

| File | Change |
|---|---|
| `src/main.rs` | New CLI contract; generic all-source orchestration; git-log-specific flags. |
| `src/state.rs` | Unified search state/results and background indexing status as needed. |
| `src/ui/container.rs` | Document result container and stable in-place update helpers. |
| `src/ui/adapter.rs` | Stable document-card keys/adapters. |
| `src/ui/render.rs` | Unified search execution; three reusable result containers; document cards. |
| `crates/sugacode-indexer/src/lib.rs` | Document indexing/search API and combined all-source search types. |
| `crates/sugacode-indexer/src/db.rs` | Document FTS/vector operations and source-aware cleanup. |
| `crates/sugacode-indexer/src/documents.rs` | New discovery, tracking, extraction, and indexing module. |
| `crates/sugacode-indexer/src/schema.sql` | `document_files` table. |
| `AGENTS.md`, `DEVELOP.md`, `README.md` | Three-source architecture and revised CLI documentation. |

## Decisions Resolved from Review

- Index only Git-tracked documents in v1.
- Include `.rst`, `.adoc`, and extensionless `README`/`LICENSE` alongside the initial Markdown/plain-text extensions.
- Chunk long documents immediately, using the documented 16 KiB initial threshold and format-aware section/paragraph boundaries.
- Make generic `--search` a combined cross-source ranking; GUI uses groups derived from the same shared search operation.
- Remove the code-only keyboard shortcut and separate code-search mode.
- Defer any startup-indexing policy change. Preserve current behavior, but share indexing/search operations between CLI and GUI.
