# Epic 010: JavaScript and JSX Code Indexer

## Introduction

Extend the language-neutral code-indexing foundation from Epic 009 so
Git-tracked JavaScript (`.js`) and JavaScript JSX (`.jsx`) files are indexed and
retrieved alongside Rust, TypeScript, and TSX.

JavaScript/JSX use the existing `source_type = 'code'`, `code_files` tracking
table, `vec_code` vector partition, search APIs, CLI commands, and unified UI.
This epic adds a JavaScript language backend; it does not create a second
database or a separate user-visible index.

Epic 008's evidence, incremental-update, and deterministic-FTS5 contracts apply
directly once its repository helpers and indexing harness have been
parameterized by Epic 009. JavaScript-specific extraction assertions remain
separate because grammar constructs and documentation conventions differ.

## Dependency

**Requires Epic 009 Task 1 and Task 4**: language dispatch, generic tracked-file
discovery, dynamic metadata, explicit shared symbol-kind parsing, query reuse,
and parameterized temporary-repository test helpers must exist first.

If Epic 010 is implemented independently, those shared pieces must be completed
as prerequisite work rather than recreating JavaScript-only variants.

## Goals

- Index Git-tracked `.js` and `.jsx` files through `index_code()`.
- Extract common modern JavaScript module, function, class, method, binding,
  CommonJS, and React component evidence.
- Preserve JSDoc, signatures/declarations, bounded body context, comments, and
  imports with stable canonical identifiers.
- Apply Epic 008's complete lifecycle contract to JavaScript and relevant JSX
  cases without embeddings or network access.
- Search JavaScript/JSX and all previously supported languages through the
  existing code-search APIs and UI.

## Non-goals

- Execute JavaScript, infer runtime types, or invoke Babel/TypeScript.
- Resolve npm packages, aliases, CommonJS require graphs, or re-export chains.
- Parse Vue/Svelte templates, `.mjs`, or `.cjs` in v1. These extensions can be
  added after module-semantics coverage is explicit.
- Index minified/vendor/generated bundles.
- Add framework-specific React, Next.js, Express, or Node semantic analysis.
- Change ranking, FTS tokenization, database schema, or search UI.

## Design Decisions

### 1. Reuse the shared language registry

Extend `CodeLanguage` with `JavaScript` and `Jsx`. Use
`tree-sitter-javascript` with a runtime-compatible version. If the grammar uses
one language for both JavaScript and JSX, retain two dispatch variants so
metadata and tests still distinguish `.js` from `.jsx`.

Compile the JavaScript query once during extractor/indexer construction and
reuse it for all `.js`/`.jsx` files. If JSX requires a distinct query, compile
both once. Do not create parsers/queries inside each per-file extraction call.

### 2. Discovery is extension-based and Git-tracked

Add `.js` and `.jsx` to the generic tracked-code extension set. Continue to:

- read files from the Git HEAD tree rather than walking the working directory;
- exclude untracked and ignored files;
- use canonical repo-relative forward-slash paths for tracking/identifiers;
- skip unreadable/non-UTF-8 files with a warning while continuing other files.

Explicitly exclude minified names (`*.min.js`) and common generated/vendor
directories if they can appear in the tracked tree:

- `node_modules/`
- `vendor/`
- `dist/`
- `build/`
- `.next/`
- `coverage/`

Centralize these policy filters so TypeScript and future languages can share or
override them. Test each exclusion; do not rely on `.gitignore` because tracked
generated files can still exist.

### 3. Use the shared symbol model honestly

Use the symbol kinds introduced by Epic 009:

- `Function`
- `Class`
- `Method`
- `Const`
- `Variable`
- `Module`
- `Comments`
- `Imports`

JavaScript does not emit TypeScript-only `Interface`, `TypeAlias`, or `Enum`.
Object literal methods assigned to a stable top-level binding may use `Method`
with `BindingName::method`. Prototype assignments use `Method` with
`TypeName::method` when the left-hand side can be resolved syntactically.

Do not label classes as `Struct` or ordinary methods as Rust `ImplMethod`.

### 4. Evidence granularity

| JavaScript construct | `SymbolKind` | Identifier suffix |
|---|---|---|
| Named function declaration | `Function` | function name |
| Arrow/function expression assigned to a binding | `Function` | binding name |
| Class declaration/expression with stable binding | `Class` | class/binding name |
| Constructor, method, getter, setter, static method | `Method` | `ClassName::method` |
| Top-level `const` data binding | `Const` | binding name |
| Top-level `let`/`var` data binding | `Variable` | binding name |
| Stable object literal method | `Method` | `ObjectName::method` |
| `Type.prototype.method = function...` | `Method` | `Type::method` |
| Named React function/arrow/class component | matching kind | declaration/binding name |

Exclude nested callback functions by default. Index a nested function only when
it is a named declaration that is directly exported or when a stable,
non-positional namespace can be constructed.

For `export default function/class`:

- use the declaration name when present;
- use `__default_export` for an anonymous default export.

For `module.exports = function/class` or `exports.name = ...`, use the exported
property name when syntactically available; otherwise use `__module_export`.

### 5. JSDoc, comments, and imports

- Attach immediately preceding JSDoc to a declaration and exclude it from the
  standalone comment record.
- Preserve a useful declaration/signature and bounded local body excerpt.
- Emit one FTS5-only `Comments` record per file for remaining line/block
  comments.
- Emit one FTS5-only `Imports` record containing:
  - static ES `import` declarations;
  - re-export declarations;
  - top-level `require("literal")` bindings;
  - dynamic `import("literal")` expressions when statically recognizable.

Do not attempt to resolve import paths or execute conditional `require` calls.

### 6. JSX is syntax, not a separate evidence hierarchy

JSX components are indexed from their surrounding named JavaScript declaration
or binding. JSX tag names, inline event callbacks, fragments, and embedded
expressions must not become top-level symbols.

The bounded body excerpt should retain enough JSX to explain visible behavior,
such as a disabled state, conditional error message, or click handler call.
Use `language = "jsx"` metadata for `.jsx`, even if JavaScript and JSX share a
Tree-sitter grammar.

### 7. Incremental and search behavior remains shared

No schema changes are planned. The shared `code_files` path key handles all
languages, and deletion/edit/touch behavior is language-independent.

JavaScript metadata example:

```json
{
  "file_path": "src/checkout/session.js",
  "line_start": 8,
  "line_end": 22,
  "symbol_kind": "function",
  "language": "javascript",
  "content_hash": "..."
}
```

JSX uses `language = "jsx"`.

## Tasks

### Task 1: Add JavaScript/JSX grammar and dispatch

**Priority:** High  
**Status:** ⬜ Planned

- Add a compatible `tree-sitter-javascript` dependency.
- Add `JavaScript` and `Jsx` to the shared language registry.
- Add `.js`/`.jsx` to generic tracked-code discovery.
- Compile/validate the JavaScript and any JSX-specific queries once.
- Write dynamic `javascript`/`jsx` metadata.
- Add centralized generated/vendor/minified path filtering.

**Acceptance Criteria:**

- [ ] Tracked `.js` and `.jsx` files are discovered alongside `.rs`, `.ts`,
      and `.tsx`.
- [ ] Untracked, ignored, configured generated/vendor, and `*.min.js` files are
      excluded.
- [ ] `.js` and `.jsx` dispatch cannot fall through to Rust or TypeScript.
- [ ] Query compilation is a startup/construction error, not a per-file error.
- [ ] No separate DB table, vector table, source type, or search API is added.

### Task 2: Implement JavaScript symbol extraction

**Priority:** High  
**Status:** ⬜ Planned

- Extract named functions, callable bindings, classes, class methods, stable
  object methods, prototype methods, constants, variables, comments, and
  imports/requires.
- Build canonical module/class/object namespaces.
- Attach JSDoc and preserve signatures/declarations plus bounded bodies.
- Handle ES modules and common CommonJS export forms.
- Add explicit fixtures for syntax errors, comment-only files, anonymous
  exports, destructuring, callbacks, getters/setters, and static methods.

**Acceptance Criteria:**

- [ ] A `.js` checkout fixture yields a public function, payment-provider class
      and method, configuration binding, imports/requires, and standalone
      limitation comment.
- [ ] Arrow/function bindings are `Function`; non-callable bindings are
      `Const` or `Variable`.
- [ ] Class/object/prototype method identifiers include the enclosing stable
      name.
- [ ] JSDoc appears in the owning symbol and not standalone comments.
- [ ] Imports/comments have `embed = false`.
- [ ] Nested anonymous callbacks do not create noisy top-level records.
- [ ] Malformed source degrades per file without panicking or rolling back
      other files.

### Task 3: Implement JSX component extraction

**Priority:** High  
**Status:** ⬜ Planned

- Add JSX fixtures for function, arrow, and class components.
- Preserve JSDoc, parameter/default-prop declaration context, and bounded JSX
  body excerpts.
- Ensure embedded callbacks, tag names, fragments, and expressions do not
  become separate top-level evidence.
- Cover named and anonymous default exports.

**Acceptance Criteria:**

- [ ] A `.jsx` checkout component is indexed under its stable component name.
- [ ] JSX body evidence retains a product-visible condition or action.
- [ ] Helper functions remain independently searchable.
- [ ] JSX-only syntax parses without error under the selected grammar.
- [ ] Metadata language is `jsx`, not `javascript`.

### Task 4: Apply Epic 008's indexing lifecycle contracts

**Priority:** High  
**Status:** ⬜ Planned

Use the parameterized helpers from Epic 009 rather than creating another Git
setup implementation.

**Acceptance Criteria:**

- [ ] For `.js`, tests cover first index, unchanged second run, touch without
      content change, content replacement, tracked deletion, and untracked
      exclusion.
- [ ] `.jsx` is included in discovery and lifecycle coverage; parameterized
      cases are acceptable.
- [ ] Content edits remove stale evidence before inserting replacement
      evidence.
- [ ] Deleting a JavaScript/JSX file removes both items and its `code_files`
      row.
- [ ] Rust, TypeScript, and TSX lifecycle tests continue to pass.
- [ ] Tests use temporary repositories/cache directories and no embedder.

### Task 5: Add deterministic JavaScript/JSX retrieval tests

**Priority:** High  
**Status:** ⬜ Planned

Use FTS5-only product and implementation queries:

| User wording | Expected evidence |
|---|---|
| `checkout validation` | JavaScript checkout function with JSDoc/body validation |
| `payment provider configured` | Provider constant, require/import, or gateway class method |
| `temporary checkout limitation` | Standalone JavaScript comment |
| `checkout button disabled` | JSX component with conditional/disabled behavior |
| `where payment client exported` | CommonJS or ES export evidence |

**Acceptance Criteria:**

- [ ] Every query returns its expected stable identifier via
      `search_code_text()` without an embedder.
- [ ] Results preserve path, one-based line range, kind, language, and useful
      explanatory text.
- [ ] No assertion depends on exact FTS result order or vector ranking.
- [ ] A mixed-language repository returns Rust, TypeScript/TSX, and
      JavaScript/JSX evidence through the same search call.

### Task 6: Validate CLI/UI integration and documentation

**Priority:** Medium  
**Status:** ⬜ Planned

- Verify existing code/unified indexing, reindexing, searching, startup
  background indexing, and result cards require no JavaScript-specific UI
  branch.
- Run `cargo check --workspace` after every implementation change and
  `cargo test --workspace` at completion.
- Update this epic's status/checklists/test counts.
- Update `README.md`, `DEVELOP.md`, and `AGENTS.md` with `.js`/`.jsx` support,
  generated-file policy, and shared-language test requirements.

**Acceptance Criteria:**

- [ ] Existing CLI and UI show JavaScript/JSX code evidence without a separate
      search mode.
- [ ] `--reindex-code` rebuilds all supported code languages and does not
      affect commit/document records.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes.
- [ ] Documentation lists `.rs`, `.ts`, `.tsx`, `.js`, and `.jsx` accurately.

## Test Matrix

| Layer | JavaScript | JSX |
|---|---:|---:|
| Grammar smoke test | Required | Required |
| Evidence extraction contract | Full | Full |
| ES module imports/exports | Required | Required |
| CommonJS require/exports | Required | Optional where meaningful |
| Class/object/prototype method identifiers | Required | Class component coverage |
| Comments/imports FTS-only | Required | Required |
| First/unchanged/touch/edit/delete/untracked | Full | Discovery + lifecycle coverage |
| Deterministic FTS5 retrieval | 4+ queries | 1+ query |
| Mixed-language coexistence | Required | Required |
| No model/network dependency | Required | Required |

## File-change Summary

| File | Planned change |
|---|---|
| `crates/daftprompt-indexer/Cargo.toml` | Add a compatible `tree-sitter-javascript` dependency. |
| `crates/daftprompt-indexer/src/code.rs` or `src/code/` | Register JS/JSX, add queries/extraction, generated-file filtering, and focused tests. |
| `crates/daftprompt-indexer/src/lib.rs` | Extend language routing/metadata and instantiate parameterized Epic 008 tests for JS/JSX. |
| `Cargo.lock` | Record the resolved JavaScript grammar dependency. |
| `README.md` | Document JavaScript and JSX code search. |
| `DEVELOP.md` | Document supported extensions and validation. |
| `AGENTS.md` | Document JS/JSX evidence and generated-file invariants. |
| `epics/010-javascript-jsx-code-indexer.md` | Track implementation and acceptance criteria. |

## Risks and Follow-ups

- JavaScript permits many anonymous and dynamically assigned forms. V1 favors
  stable identifiers over exhaustive capture.
- CommonJS exports and prototype mutation are syntactically diverse; only
  literal, statically attributable forms should be indexed initially.
- JSX callbacks can overwhelm evidence if queries capture every function
  expression; explicit negative tests are required.
- Tracked generated bundles need policy, not merely `.gitignore`; repository
  conventions may require configurable exclusions later.
- `.mjs`/`.cjs`, Flow syntax, decorators, class fields/private names, and
  framework-aware route/component relationships are follow-up candidates.

