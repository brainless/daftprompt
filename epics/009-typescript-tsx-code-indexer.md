# Epic 009: TypeScript and TSX Code Indexer

## Introduction

Extend the existing Rust code indexer so Git-tracked TypeScript (`.ts`) and
TypeScript JSX (`.tsx`) files become inspectable code evidence in the same
SQLite/FTS5/vector index and unified code-search experience.

This is a language backend for the existing code indexer, not a separate
database, source type, CLI mode, or search UI. TypeScript records continue to
use `source_type = 'code'`, the shared `code_files` tracking table, and
`vec_code`. Their metadata distinguishes the language as `typescript` or
`tsx`.

Epic 004 established the Rust pipeline. Epic 008 established the evidence,
incremental-indexing, and deterministic-retrieval contracts. Those contracts
apply to TypeScript and TSX, but the current helpers cannot be reused as-is:
file discovery is `.rs`-only, parsing and queries are hardcoded to Rust,
metadata hardcodes `language = "rust"`, and test repositories hardcode Rust
filenames. This epic makes those seams language-neutral and runs the same
contracts for both TypeScript dialects.

## Goals

- Index Git-tracked `.ts` and `.tsx` files through the production
  `index_code()` path.
- Introduce a reusable language-dispatch layer without regressing Rust.
- Extract TypeScript declarations and React-style TSX components as useful
  evidence with canonical identifiers, source ranges, documentation,
  signatures, and bounded body context.
- Preserve standalone comments and imports as FTS5-only per-file evidence.
- Reuse Epic 008's no-embedder indexing and FTS5 retrieval contracts for
  `typescript` and `tsx`.
- Keep indexing resilient: one malformed or unreadable file must not roll back
  other files.

## Non-goals

- Add JavaScript or JSX; Epic 010 adds those on the shared foundation.
- Add type checking, module resolution, npm dependency analysis, or a
  TypeScript compiler dependency.
- Resolve re-exports across files or construct a call graph.
- Index generated output, declaration files, or dependencies under
  `node_modules` in the first version.
- Change hybrid ranking or assert exact vector ordering.
- Add a separate TypeScript search mode.

## Design Decisions

### 1. One code index with language dispatch

Refactor the Rust-only entry points into a shared registry/router:

```rust
pub enum CodeLanguage {
    Rust,
    TypeScript,
    Tsx,
}

pub struct LanguageConfig {
    pub language: CodeLanguage,
    pub extensions: &'static [&'static str],
    // Tree-sitter language and compiled query are owned by the extractor.
}
```

The exact Rust types may differ, but the resulting design must:

- select a dialect from the canonical file extension;
- compile each grammar query once when the extractor/indexer is constructed;
- reuse the compiled query for every file of that dialect;
- return `CodeSymbol` plus the selected metadata language;
- report unsupported extensions without falling back to Rust.

Keep `extract_symbols()` as a Rust compatibility wrapper if useful for existing
callers and tests. Production indexing must use explicit extension-based
dispatch.

Rejected alternative: separate `index_typescript()` and separate database
tables. That would duplicate discovery, incremental tracking, transactions,
search, CLI, and UI while preventing a single code search across languages.

### 2. Generic tracked-file discovery

Replace or wrap `list_tracked_rust_files()` with one traversal of the Git HEAD
tree that accepts the supported extension set. The production scan for this
epic includes `.rs`, `.ts`, and `.tsx`.

- Paths remain absolute during IO and canonical repo-relative strings in
  identifiers/tracking rows.
- Untracked and ignored files remain excluded.
- Skip `.d.ts` declaration files initially. They often contain dependency or
  generated API surfaces without implementation evidence. Keep this filter
  explicit and tested so it can be changed later.
- Submodules retain Epic 004's existing behavior.

### 3. Shared symbol kinds, expanded only where semantics require it

TypeScript proves that mapping every declaration to a Rust term would lose
important evidence. Extend the shared `SymbolKind` with:

- `Class`
- `Interface`
- `Method`
- `Variable`

Keep existing kinds for `Function`, `Enum`, `TypeAlias`, `Const`, `Module`,
`Comments`, and `Imports`. Rust continues to use `Struct`, `Trait`,
`ImplMethod`, and `TraitMethod`; no Rust metadata is rewritten.

Update metadata serialization/deserialization explicitly. Unknown stored kinds
must not silently become `Function`; use an `Unknown` variant or return a
controlled parse fallback that preserves the original kind for diagnostics.

### 4. TypeScript and TSX are distinct dialects

Use `tree-sitter-typescript` with its TypeScript grammar for `.ts` and TSX
grammar for `.tsx`. Do not parse TSX with the plain TypeScript grammar.
Dependency versions must be compatible with the repository's current
`tree-sitter` runtime; pin the resolved compatible versions in `Cargo.lock`.

Each dialect has a query validated once at construction. Shared query fragments
may be kept together, but dialect-specific node differences must remain
testable.

### 5. Evidence granularity

Create per-symbol items for named declarations and named bindings:

| TypeScript construct | `SymbolKind` | Identifier suffix |
|---|---|---|
| Function declaration | `Function` | function name |
| Arrow/function expression assigned to a binding | `Function` | binding name |
| Class declaration | `Class` | class name |
| Class method/constructor/getter/setter | `Method` | `ClassName::method` |
| Interface declaration | `Interface` | interface name |
| Type alias | `TypeAlias` | alias name |
| Enum declaration | `Enum` | enum name |
| Namespace/module declaration | `Module` | nested namespace |
| `const` data binding | `Const` | binding name |
| `let`/`var` data binding | `Variable` | binding name |
| Named React function/arrow component in TSX | `Function` | declaration/binding name |

For destructuring declarations, emit one record per bound identifier only when
a stable identifier and useful declaration text can be retained. Otherwise
skip the binding and record the choice in a code comment.

Named default exports use their declared name. Anonymous default-exported
functions/classes use the stable per-file suffix `__default_export`; a file can
have only one default export. Re-exports are represented in the per-file
imports record in v1 rather than as resolved symbols.

TypeScript overload signatures are folded into the implementation record when
an implementation exists. Signature-only declarations remain attached to the
owning interface/type/class evidence rather than producing several ambiguous
records.

### 6. Text composition and namespaces

Reuse the bounded text-composition contract from Epic 004:

- attached JSDoc or immediately attached documentation comment;
- declaration signature, including generics and relevant type annotations;
- a bounded local body excerpt for executable functions/methods/components;
- complete declaration text for compact interfaces, types, and enums, bounded
  by the same documented safety limit used for other declarations.

Identifiers remain forward-slash, repo-relative paths followed by `::`
namespace segments. Include enclosing namespaces/classes:

```text
src/checkout/session.ts::createCheckoutSession
src/payments/gateway.ts::PaymentGateway::charge
src/components/CheckoutButton.tsx::CheckoutButton
```

Nested lexical functions are excluded in v1 unless they are exported or bound
directly in a module/class namespace. This limits noise and unstable
identifiers.

### 7. Comments and imports remain FTS5-only

Emit at most one `Comments` and one `Imports` record per file:

```text
src/checkout/session.ts::__comments__
src/checkout/session.ts::__imports__
```

- JSDoc attached to a symbol is included with that symbol and excluded from
  standalone comments.
- `import`, `import type`, dynamic import declarations when statically
  expressible, and re-export declarations are collected as import evidence.
- These records keep `embed = false`, matching Epic 004/008.

### 8. Existing incremental storage is shared

Do not add a language-specific tracking table or vector table. The canonical
file path is already the `code_files` key, and `vec_code` is partitioned by
source type rather than language.

For each item, write:

```json
{
  "file_path": "src/checkout/session.ts",
  "line_start": 12,
  "line_end": 29,
  "symbol_kind": "function",
  "language": "typescript",
  "content_hash": "..."
}
```

Use `language = "tsx"` for `.tsx`. Content hash and transaction behavior remain
unchanged.

## Tasks

### Task 1: Add language-neutral extraction and discovery foundations

**Priority:** High  
**Status:** ✅ Complete

- Introduce `CodeLanguage` and the language configuration/registry.
- Generalize tracked-file discovery and preserve a Rust compatibility wrapper.
- Move query construction out of per-file Rust extraction and compile all
  configured queries once.
- Route production `index_code()` by extension and make metadata language
  dynamic.
- Add explicit symbol-kind serialization/parsing without the current
  unknown-to-`Function` behavior.

**Acceptance Criteria:**

- [x] The production scan discovers tracked `.rs`, `.ts`, and `.tsx` files in
      one pass and excludes untracked files and `.d.ts`.
- [x] Rust extraction and all 46 existing indexer tests still pass unchanged
      or with mechanical helper migration only.
- [x] Queries are compiled once per language/dialect, not once per file.
- [x] Unsupported extensions cannot be parsed as Rust accidentally.
- [x] Stored metadata reports the actual language/dialect.

### Task 2: Implement TypeScript symbol extraction

**Priority:** High  
**Status:** ✅ Complete

- Add the compatible `tree-sitter-typescript` dependency.
- Add and validate the TypeScript query.
- Extract functions, named callable bindings, classes, methods, interfaces,
  type aliases, enums, namespaces, constants, variables, comments, and imports.
- Implement namespace construction, attached JSDoc handling, signature
  extraction, overload handling, and bounded body excerpts.
- Cover malformed source with partial, non-panicking extraction where the
  grammar produces usable nodes.

**Acceptance Criteria:**

- [x] A `.ts` product fixture produces inspectable evidence for a public
      function, supporting interface/type, configuration constant, class
      method, imports, and a standalone limitation comment.
- [x] Identifiers include file, namespace, and enclosing class where relevant.
- [x] JSDoc is attached to its declaration and not duplicated as standalone
      comment evidence.
- [x] Callable bindings and ordinary variables receive distinct appropriate
      kinds.
- [x] Imports/comments are FTS5-only.
- [x] Empty, comment-only, syntax-error, overload, anonymous-default-export,
      and nested-namespace fixtures have explicit expectations.

### Task 3: Implement TSX extraction

**Priority:** High  
**Status:** ✅ Complete

- Add and validate the TSX dialect configuration/query.
- Extract named function, arrow, and class components using the shared symbol
  representation.
- Preserve component props signatures/types and bounded JSX body context.
- Ensure JSX tags and embedded expressions do not become spurious symbols.

**Acceptance Criteria:**

- [x] A `.tsx` fixture indexes a named component, typed props, a helper
      function, imports, and an attached JSDoc comment.
- [x] Component identifiers use the declaration or binding name.
- [x] JSX syntax parses without errors under the selected TSX grammar.
- [x] JSX callbacks and tag names do not create top-level evidence records.
- [x] TypeScript-only and TSX fixtures are dispatched to different grammars.

### Task 4: Parameterize and apply the Epic 008 test foundation

**Priority:** High  
**Status:** ✅ Complete

- Generalize the temporary Git repository helper to accept fixture paths and
  contents rather than hardcoded `.rs` files.
- Define a reusable language contract harness or shared assertion helpers for:
  first index, unchanged run, touch without content change, content edit,
  tracked deletion, untracked exclusion, and metadata language.
- Run that contract for `.ts` and `.tsx` without an embedder/model download.
- Keep focused extraction tests per dialect rather than forcing grammar-specific
  syntax into one generic assertion.

**Acceptance Criteria:**

- [x] Epic 008's six indexing lifecycle behaviors pass for TypeScript.
- [x] Discovery, first-index, unchanged-run, edit, deletion, and untracked-file
      coverage includes TSX (parameterized cases are acceptable).
- [x] Tests verify language, path, lines, kind, text, and identifiers.
- [x] Helpers can accept `.js`/`.jsx` fixtures in Epic 010 without duplicating
      Git/cache setup.
- [x] Tests are hermetic and FTS5-only.

### Task 5: Add deterministic TypeScript/TSX evidence retrieval tests

**Priority:** High  
**Status:** ⬜ Planned

Use product-oriented fixtures and FTS5 queries such as:

| User wording | Expected evidence |
|---|---|
| `checkout validation` | TypeScript checkout function with JSDoc and validation excerpt |
| `payment provider configured` | Typed configuration constant/interface or gateway method |
| `temporary checkout limitation` | Standalone TypeScript comment record |
| `checkout button disabled` | TSX component with props and JSX condition |

Do not assert exact result order or vector ranking.

**Acceptance Criteria:**

- [ ] Every query returns the expected identifier through
      `search_code_text()` with no embedder.
- [ ] Hits preserve language, path, one-based line range, symbol kind, and
      explanatory text.
- [ ] TS and TSX results coexist with Rust results in the same code search.

### Task 6: Validate production integration and document the result

**Priority:** Medium  
**Status:** ⬜ Planned

- Verify `--index-code`, `--reindex-code`, `--search-code`, unified `--index`,
  unified `--search`, GUI startup indexing, and Cmd/Ctrl+K unified search need
  no language-specific branch outside the indexer.
- Run `cargo check --workspace` after every implementation change and
  `cargo test --workspace` at completion.
- Update this epic's statuses, acceptance criteria, test counts, and discovered
  follow-ups.
- Update `README.md`, `DEVELOP.md`, and `AGENTS.md` to describe `.ts`/`.tsx`
  support and the language-neutral invariants.

**Acceptance Criteria:**

- [ ] Existing CLI/UI code search shows TypeScript and TSX cards without a new
      mode.
- [ ] Reindex removes and rebuilds Rust, TypeScript, and TSX code records
      without affecting commits/documents.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes.
- [ ] Documentation no longer describes code indexing as Rust-only.

## Test Matrix

| Layer | Rust regression | TypeScript | TSX |
|---|---:|---:|---:|
| Grammar smoke test | Existing | Required | Required |
| Evidence extraction contract | Existing | Full | Full |
| Namespace/class method identifiers | Existing | Required | Required where applicable |
| Comments/imports FTS-only | Existing | Required | Required |
| First/unchanged/touch/edit/delete/untracked | Existing | Full | Discovery + lifecycle coverage |
| Deterministic FTS5 retrieval | Existing | 3+ queries | 1+ query |
| No model/network dependency | Existing | Required | Required |

## File-change Summary

| File | Planned change |
|---|---|
| `crates/daftprompt-indexer/Cargo.toml` | Add a compatible `tree-sitter-typescript` dependency. |
| `crates/daftprompt-indexer/src/code.rs` | Introduce language dispatch/configuration, generic discovery, TypeScript/TSX queries and extraction, and language-contract tests; split into a `code/` module if size warrants it. |
| `crates/daftprompt-indexer/src/lib.rs` | Route `index_code()` by extension, write dynamic language metadata, extend symbol-kind parsing, and parameterize Epic 008 integration tests. |
| `Cargo.lock` | Record resolved grammar dependencies. |
| `README.md` | Document TypeScript and TSX code search. |
| `DEVELOP.md` | Document language routing and test commands. |
| `AGENTS.md` | Replace Rust-only indexer invariants with language-neutral ones. |
| `epics/009-typescript-tsx-code-indexer.md` | Track implementation and acceptance criteria. |

## Risks and Follow-ups

- Tree-sitter TypeScript and TSX node shapes overlap but are not identical;
  query tests must cover both dialects.
- Large interfaces/type literals may require a separate declaration excerpt
  limit after real-repository testing.
- FTS5 `unicode61` does not split camelCase (`PaymentGateway`); query wording
  tests should expose this limitation without changing tokenization in this
  epic.
- `.d.ts` indexing may be valuable for dependency/API exploration but needs a
  deliberate noise and generated-file policy.
- Decorators, ambient declarations, declaration merging, and cross-file
  re-exports may need later focused epics.

