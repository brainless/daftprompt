# Epic 008: Code Indexer Test Foundation

## Introduction

The code indexer is intended to provide concrete, inspectable evidence from a repository: symbols that describe what exists, comments that record intent or constraints, and enough local logic to show how a feature is implemented. That evidence must serve both technical and product-focused questions.

Tree-sitter grammars test their own parsing behavior. This epic tests daftprompt's contract on top of those grammars: file discovery, symbol extraction, context preservation, storage, incremental updates, and deterministic keyword retrieval.

The implementation currently supports Git-tracked Rust (`.rs`) files only. This epic deliberately establishes a small, language-neutral test foundation before TypeScript, Python, Go, and other languages are added. The current `SymbolKind` enum remains the shared representation; changing or expanding it is out of scope unless a later language proves the current set insufficient.

## Goals

- Protect the current Rust code-indexing behavior with focused automated tests.
- Establish reusable fixture and temporary-Git-repository helpers for forthcoming language support.
- Verify that indexed records retain the information needed to present code as evidence: path, line range, symbol kind, comments, signature, and useful local body context.
- Verify deterministic FTS5 retrieval for concrete implementation-oriented and product-oriented wording.
- Keep the initial test suite fast, local, and independent of model downloads.

## Non-goals

- Add TypeScript, Python, Go, or other language parsing and discovery.
- Change hybrid ranking, embedding model, database schema, or the current `SymbolKind` enum.
- Assert exact vector or hybrid-search ranking.
- Build a large benchmark corpus or a user-facing question-answering feature.
- Replace Tree-sitter grammar tests.

## Test Strategy

The tests in this epic are contracts for daftprompt, not tests for Tree-sitter itself.

| Layer | What it establishes | Test approach |
|---|---|---|
| Extraction | Parsed source becomes useful, correctly attributed evidence records. | Unit tests against compact source fixtures. |
| Indexing | Git-tracked source is stored, updated, skipped, and removed correctly. | End-to-end tests in a temporary Git repository and temporary index DB. |
| Retrieval | Concrete implementation evidence can be found from deterministic keyword queries. | FTS5-only integration tests; no exact vector-rank assertions. |

All fixtures should be intentionally small and describe a user-visible capability rather than only language syntax. A suitable fixture might include a `create_checkout_session` function with a doc comment describing the checkout flow, a body that records validation, and a standalone TODO/constraint comment. This makes failures legible and provides a pattern to reuse for future languages.

## Tasks

### Task 1: Add reusable Rust evidence fixtures and extraction assertions

**Priority:** High  
**Status:** ✅ Done

- Add compact Rust fixtures (inline or file-backed) that model a product capability, a supporting type, imports, attached documentation, standalone comments, nested modules, and an impl method.
- Extend extraction tests to assert the evidence contract for each produced symbol:
  - canonical, repo-relative identifier;
  - `SymbolKind`;
  - file path and one-based line range;
  - attached doc comments and signature in the symbol text;
  - bounded local body excerpt for executable symbols;
  - standalone comments and imports emitted as the existing per-file FTS-only records.
- Keep existing Rust-specific edge-case tests (trait default methods, syntax errors where applicable, empty/comment-only source) and make any missing expectations explicit.

**Acceptance Criteria:**

- [x] A fixture representing a product capability produces symbols and comments that explain both intent and implementation location.
- [x] Assertions verify metadata-bearing fields and text content, not only the number of extracted symbols.
- [x] Existing Rust extraction behavior remains covered, including nested namespaces and trait default methods.

### Task 2: Add no-embedder end-to-end indexing and incremental tests

**Priority:** High  
**Status:** ✅ Done

- Add a test helper that creates a temporary Git repository, commits tracked Rust fixture files, and constructs an `Indexer` with a temporary cache directory.
- Run the indexer with no embedder/model download, exercising the same `index_code()` path used in production.
- Cover first index, unchanged second index, content modification, touch without content change, tracked-file deletion, and an untracked Rust file.
- Inspect the database or public search API as appropriate to confirm stale code items and tracking rows are removed and unchanged content is not re-indexed.

**Acceptance Criteria:**

- [x] Only committed/tracked `.rs` files are indexed; an untracked `.rs` file is excluded.
- [x] A second unchanged run reports zero changed files.
- [x] A timestamp-only change updates tracking without replacing indexed content.
- [x] Content edits replace the affected file's evidence records, and deletion removes its records.
- [x] Tests are hermetic: no network access, persistent cache writes, or embedding-model dependency.

### Task 3: Add deterministic evidence-retrieval tests

**Priority:** High  
**Status:** ⬜ Not Started

- Use the end-to-end fixture repository to test `search_code_text()` / FTS5-only retrieval.
- Add a small set of wording that reflects the intended user questions, including at least one product-focused query and one implementation-focused query.
- Assert that each query returns an expected evidence record by identifier and preserves the relevant path, line range, symbol kind, and explanatory text.
- Do not assert an exact order beyond the required expected hit, and do not make vector/hybrid rankings a pass/fail condition.

**Initial query examples:**

| User wording | Expected evidence |
|---|---|
| `checkout validation` | The checkout-session function with its documentation and validation logic excerpt. |
| `where is the payment provider configured` | The configuration type/constant or setup function. |
| `temporary checkout limitation` | The standalone comment record containing the stated constraint. |

**Acceptance Criteria:**

- [ ] Each query returns at least one expected code evidence record via FTS5 without an embedder.
- [ ] Assertions verify the result is inspectable evidence, not merely a keyword-containing row.
- [ ] The test suite does not depend on the current embedding model or its exact ranking behavior.

### Task 4: Verify and document the test foundation

**Priority:** Medium  
**Status:** ⬜ Not Started

- Run `cargo test --workspace` and `cargo check --workspace`.
- Add concise comments only where test helpers encode non-obvious Git/indexer setup constraints.
- Update this epic's task status and note any gaps discovered in the current implementation that block multi-language work.

**Acceptance Criteria:**

- [ ] `cargo test --workspace` passes.
- [ ] `cargo check --workspace` passes.
- [ ] Test helpers are reusable for a second language without duplicating temporary-repository setup.

## Future Follow-ups

These are intentionally deferred. Revisit them after the test foundation is complete and use them to shape separate, focused epics where appropriate.

### Multi-language support

- Add language dispatch based on tracked file extension, beginning with TypeScript/TSX, Python, and Go.
- Add each grammar and language-specific query set, while normalizing its output into the current `SymbolKind` set and `language` metadata field.
- Reuse the fixture and end-to-end contracts from this epic for every new language; add a symbol kind only when the common representation demonstrably loses important evidence.
- Decide language-specific handling for concepts such as TypeScript interfaces, Python decorators/classes, Go receivers/interfaces, generated files, and TSX syntax.

### Retrieval-quality evaluation

- Grow the initial query examples into a curated question-and-evidence corpus spanning technical and product-focused questions.
- Measure a minimum retrieval threshold such as “an expected evidence record appears in the top N,” separately from fast unit tests.
- Evaluate semantic/hybrid retrieval across model and text-composition changes without asserting brittle exact rankings.

### Evidence presentation and context

- Evaluate whether the current ten-line body excerpt is sufficient for explaining implementation behavior, especially for long functions.
- Consider language-aware context extraction, chunking, call-site or configuration relationships, and richer result previews.
- Test that results clearly distinguish documented intent, executable logic, imports/configuration, and standalone constraints.

### Resilience and scale

- Extend integration coverage to invalid UTF-8, malformed source, very large files, and partial indexing failure across all supported languages.
- Add performance/regression measurements for large multi-language repositories.

## File-change Summary

| File | Change |
|---|---|
| `crates/daftprompt-indexer/src/code.rs` | Extend Rust evidence-extraction tests and add/reuse fixture helpers as appropriate. |
| `crates/daftprompt-indexer/src/lib.rs` | Modified `Indexer::new` to respect `config.cache_dir` when set (test-friendly DB path). Fixed init_schema ordering (call before repo_meta_get so fresh DBs work). Added 6 end-to-end integration tests: first index, unchanged re-index, touch-without-content-change, content edit, file deletion, untracked file exclusion. |
| `crates/daftprompt-indexer/Cargo.toml` | Added `tempfile = "3"` as `[dev-dependencies]`. |
| `epics/008-code-indexer-test-foundation.md` | Track the test foundation and deferred multi-language/retrieval work. |
