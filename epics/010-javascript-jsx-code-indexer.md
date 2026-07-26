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

**Requires all of Epic 009, including its post-completion review fixes in
`9a0d46b`**: language dispatch, generic tracked-file discovery, dynamic
metadata, explicit shared symbol-kind parsing, query reuse, parameterized
temporary-repository tests, method-local evidence ranges, nested-executable
filtering, callable-binding body extraction, and complete import source-range
collection must already exist.

Epic 010 must extend those shared paths rather than copy the pre-review
TypeScript implementation or create JavaScript-only variants.

## Epic 009 Review Guardrails

Epic 009 passed its initial task validation but still needed a review-fix commit.
Epic 010 is not complete unless focused regression tests prove all of these
properties for JavaScript/JSX:

1. **Effective nodes are local.** Class/object methods use their individual
   method node; assigned callables use the smallest declarator or assignment
   that retains the binding/receiver. Text and one-based ranges must not use an
   enclosing class, object, or file-level node or leak sibling declarations.
2. **Nested executables stay nested.** Named functions, arrow/function
   bindings, callbacks, and JSX handlers beneath another executable body do
   not become top-level evidence records.
3. **Callable bindings retain behavior.** A binding record keeps its declaration
   or signature and derives its bounded body excerpt from the callable value's
   own body, including JSX expression bodies.
4. **Import evidence is complete.** Collection uses complete source byte ranges,
   not only starting rows, so multiline imports/re-exports, literal `require`
   bindings, and static dynamic imports retain their source strings.
5. **Completion includes production wording.** CLI help and every language list
   in user/developer/agent documentation are updated before the epic is marked
   complete; known stale wording is not deferred after Task 6.

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

Extend `CodeLanguage` with `JavaScript` and `Jsx`. Use a pinned,
runtime-compatible `tree-sitter-javascript` version and document the version
compatibility rationale next to the dependency. If the grammar uses one
language for both JavaScript and JSX, retain two dispatch variants so metadata
and tests still distinguish `.js` from `.jsx`.

Define a JavaScript-specific query validated against the JavaScript grammar; do
not reuse `TS_QUERY` as-is because it contains TypeScript-only node kinds. The
query may be shared by `.js` and `.jsx` only when grammar smoke tests prove it
compiles and behaves correctly for both. Compile each required query once
during extractor/indexer construction and reuse it for every file.

Adding enum variants requires updating every exhaustive registration and routing
site: `CodeLanguage::as_str`, `CodeExtractor::for_language`,
`language_for_extension`, `extract_symbols_with_extractor`, the extractor fields
and construction in `Indexer`, and `index_code()` dispatch. Update the existing
extension test that currently treats `js` as unsupported, including
case-insensitive `.js`/`.jsx` cases. Unsupported extensions must still return
`None`; no fallback parser is allowed.

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

For this epic, a stable object-literal method is a direct, non-computed property
of a named module-scope binding, producing `BindingName::method`. Nested object
literals and computed property names are skipped in v1. Prototype coverage is
limited to a direct module-scope
`TypeName.prototype.method = function/arrow...` assignment with identifier
property names, producing `TypeName::method`. `Object.assign`, computed
prototype properties, `__proto__`, and chained/dynamic assignments are out of
scope.

Each record uses the smallest complete effective declaration node. A class or
object method uses the individual method node, while a callable binding or
prototype/CommonJS assignment may use its direct declarator/assignment so the
binding or receiver remains visible. Its text and one-based `line_start` /
`line_end` come from that effective node; an enclosing class/object must never
make a method record include sibling methods or the rest of the container.

Exclude declarations and callable bindings with an ancestor that is a function
declaration/expression, arrow function, method, getter, setter, constructor, or
other executable body. This includes named nested functions, nested callable
bindings, ordinary local variables, callbacks, and JSX handlers. Only
module-scope declarations/exports and direct members of a stable module,
class, or object namespace are retained.

For `export default function/class`:

- use the declaration name when present;
- use `__default_export` for an anonymous default export.

For `module.exports = function/class` or `exports.name = ...`, use the exported
property name when syntactically available; otherwise use `__module_export`.

### 5. JSDoc, comments, and imports

- Attach immediately preceding JSDoc to a declaration and exclude it from the
  standalone comment record.
- Preserve a useful declaration/signature and bounded local body excerpt. For
  an arrow or function expression assigned to a binding, preserve the binding
  declaration and derive the excerpt from the callable value's own body node;
  do not reduce the evidence to the first declaration line. Expression-bodied
  arrows retain the bounded expression itself.
- Emit one FTS5-only `Comments` record per file for remaining line/block
  comments.
- Emit one FTS5-only `Imports` record containing:
  - complete static ES `import` declarations;
  - complete re-export declarations;
  - complete top-level `require("literal")` binding declarations;
  - complete dynamic `import("literal")` expressions when statically
    recognizable.

Collect and deduplicate complete source byte ranges for import evidence. Tests
must include multiline imports and re-exports and assert their specifiers and
source strings survive, rather than checking only the first line. Do not
attempt to resolve import paths or execute conditional/non-literal `require`
calls.

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
**Status:** ✅ Done

- Add a compatible, pinned `tree-sitter-javascript` dependency with a comment
  documenting runtime compatibility.
- Add `JavaScript` and `Jsx` to the shared language registry and every
  exhaustive extractor/indexer routing site.
- Add `.js`/`.jsx` to generic tracked-code discovery and update the existing
  `js => None` extension test.
- Add a JavaScript-specific query; compile and validate the JavaScript and any
  JSX-specific queries once.
- Write dynamic `javascript`/`jsx` metadata.
- Add centralized generated/vendor/minified filtering in the shared code-file
  discovery path.

**Acceptance Criteria:**

- [x] Tracked `.js` and `.jsx` files are discovered alongside `.rs`, `.ts`,
      and `.tsx`; `language_for_extension` routes `js`, `jsx`, and mixed-case
      equivalents to the correct dialect and still rejects unsupported forms.
      Covered by `language_for_extension_routes_canonical_extensions` (mixed
      case), `list_tracked_code_files_supports_multi_extension_set`,
      `code_language_as_str_matches_metadata`, and
      `javascript_indexer_wires_metadata_language_javascript` /
      `jsx_indexer_wires_metadata_language_jsx`.
- [x] A committed fixture for every configured generated/vendor directory and
      `*.min.js` is excluded by the shared discovery filter; untracked and
      ignored files remain excluded. Covered by
      `is_excluded_generated_path_filters_documented_directories`,
      `list_tracked_code_files_excludes_tracked_generated_paths`, and
      `indexer_excludes_tracked_generated_vendor_js`.
- [x] `.js` and `.jsx` dispatch cannot fall through to Rust or TypeScript, and
      both production extractor fields/routes are constructed and exercised.
      Covered by `indexer_dispatches_js_and_jsx_to_their_respective_extractors`
      and `javascript_jsx_grammars_share_language_object`.
- [x] The JavaScript query contains no TypeScript-only captures and compiles
      against JavaScript and JSX grammars as configured. Covered by
      `code_extractor_javascript_compiles_query_once`,
      `code_extractor_jsx_compiles_query_once`, and
      `javascript_grammar_smoke_test_for_jsx_source`.
- [x] Query compilation is a startup/construction error, not a per-file error.
      Both JS and JSX `CodeExtractor::for_language` paths use
      `Query::new(...).expect(...)` so a misconfigured query panics at
      construction.
- [x] `CodeLanguage::as_str()` and indexed metadata report exactly
      `javascript` for `.js` and `jsx` for `.jsx`. Covered by
      `code_language_as_str_matches_metadata` and the
      `*_indexer_wires_metadata_language_*` integration tests.
- [x] No separate DB table, vector table, source type, or search API is added.
      The existing `code_files`, `vec_code`, `source_type = 'code'`, and
      `search_code_text` / `search_code_hybrid` paths serve all five
      languages unchanged.

### Task 2: Implement JavaScript symbol extraction

**Priority:** High  
**Status:** ✅ Complete

- Extract named functions, callable bindings, classes, class methods, stable
  object methods, prototype methods, constants, variables, comments, and
  imports/requires.
- Build canonical module/class/object namespaces.
- Attach JSDoc and preserve signatures/declarations plus bounded bodies.
- Handle ES modules and common CommonJS export forms.
- Add explicit fixtures for syntax errors, comment-only files, anonymous
  exports, destructuring, nested named/callable declarations, callbacks,
  getters/setters, static methods, adjacent sibling methods, stable object
  methods, direct prototype assignments, multiline imports/re-exports,
  literal `require`, and static dynamic imports.

**Acceptance Criteria:**

- [x] A `.js` checkout fixture yields a public function, payment-provider class
      and method, configuration binding, imports/requires, and standalone
      limitation comment.
- [x] Arrow/function bindings are `Function`; non-callable module-scope
      bindings are `Const` or `Variable`.
- [x] Callable-binding evidence contains both the binding/declaration and a
      distinctive bounded statement or expression from the callable body.
- [x] Class/object/prototype method identifiers include the enclosing stable
      name and use method-local text and exact one-based ranges. A fixture with
      adjacent methods proves one record does not contain a sibling or the rest
      of its class/object.
- [x] Named nested functions, nested arrow/function bindings, callbacks, local
      variables, and declarations inside methods produce no top-level records.
- [x] Direct object/prototype forms covered by this epic produce the documented
      identifiers; computed, nested, and dynamic forms are explicitly skipped.
- [x] Named and anonymous ES/CommonJS exports produce stable, non-duplicate
      identifiers, including `__default_export` and `__module_export` where
      specified.
- [x] JSDoc appears in the owning symbol and not standalone comments.
- [x] The single `Imports` record preserves complete multiline ES imports and
      re-exports, top-level literal `require` declarations, and static
      `import("literal")` expressions; imports/comments have `embed = false`.
- [x] Malformed source yields any parseable evidence and one malformed or
      unreadable file cannot panic or roll back successfully indexed files.

### Task 3: Implement JSX component extraction

**Priority:** High  
**Status:** ✅ Done

- Add JSX fixtures for function, arrow, and class components.
- Preserve JSDoc, parameter/default-prop declaration context, and bounded JSX
  body excerpts.
- Ensure embedded callbacks, tag names, fragments, and expressions do not
  become separate top-level evidence.
- Cover named and anonymous default exports.

**Acceptance Criteria:**

- [x] A `.jsx` checkout component is indexed under its stable component name.
      Covered by `jsx_task3_function_component_indexed_under_stable_name`,
      `jsx_task3_class_component_indexed_with_class_kind`, and
      `jsx_task3_module_level_arrow_const_component_indexed`.
- [x] Function and arrow components retain both their declaration/parameter
      context and a bounded JSX body expression containing a product-visible
      condition or action. Covered by
      `jsx_task3_function_component_retains_default_props_in_signature`,
      `jsx_task3_function_component_body_excerpt_contains_disabled_condition`,
      and `jsx_task3_arrow_component_retains_destructured_props_and_click_handler`.
- [x] Helper functions remain independently searchable. Covered by
      `jsx_task3_helper_function_remains_independently_searchable`.
- [x] JSX-only syntax parses without error under the selected grammar.
      Covered by `jsx_task3_jsx_only_syntax_parses_without_error` and the
      existing `javascript_grammar_smoke_test_for_jsx_source`.
- [x] Fixtures containing nested component tags, fragments, attributes,
      expression-container callbacks, and `items.map(item => ...)` assert that
      none become separate top-level symbols. Covered by
      `jsx_task3_jsx_fragments_nested_tags_and_expression_containers_do_not_become_symbols`
      and `jsx_task3_items_map_callback_does_not_become_symbol`.
- [x] Adjacent class-component methods use method-local text/ranges without
      sibling leakage. Covered by
      `jsx_task3_class_component_adjacent_methods_have_method_local_evidence`
      and `jsx_task3_anonymous_default_class_export_methods_are_namespaced`.
- [x] Named and anonymous default exports are covered explicitly. Covered by
      `jsx_task3_named_default_function_export_uses_name_not_default_export`,
      `jsx_task3_anonymous_default_arrow_export_uses_default_export_identifier`,
      and `jsx_task3_anonymous_default_class_export_methods_are_namespaced`.
- [x] Metadata language is `jsx`, not `javascript`. Covered by the end-to-end
      `jsx_task3_metadata_language_is_jsx_not_javascript` test plus the
      existing `jsx_indexer_wires_metadata_language_jsx` integration test.

### Task 4: Apply Epic 008's indexing lifecycle contracts

**Priority:** High  
**Status:** ⬜ Planned

Use the parameterized helpers from Epic 009 rather than creating another Git
setup implementation.

**Acceptance Criteria:**

- [ ] The shared parameterized lifecycle contract runs for both `.js` and
      `.jsx`: first index, unchanged second run, touch without content change,
      content replacement, tracked deletion, and untracked exclusion.
- [ ] Content edits remove stale evidence before inserting replacement
      evidence.
- [ ] Deleting a JavaScript/JSX file removes both items and its `code_files`
      row.
- [ ] Lifecycle assertions verify identifier, path, one-based range, kind,
      language, text, and tracking-row behavior.
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
- [ ] Results preserve path, one-based line range, kind, exact language, and
      useful explanatory text.
- [ ] At least one retrieval assertion depends on content beyond the first line
      of a multiline import/re-export or on a static dynamic-import literal.
- [ ] No assertion depends on exact FTS result order or vector ranking.
- [ ] One mixed repository and search path returns evidence from all five
      supported languages: Rust, TypeScript, TSX, JavaScript, and JSX.

### Task 6: Validate CLI/UI integration and documentation

**Priority:** Medium  
**Status:** ⬜ Planned

- Verify existing code/unified indexing, reindexing, searching, startup
  background indexing, and result cards require no JavaScript-specific UI
  branch.
- Audit `src/main.rs` CLI help and every supported-language enumeration in
  `README.md`, `DEVELOP.md`, and `AGENTS.md`; all must list `.rs`, `.ts`,
  `.tsx`, `.js`, and `.jsx` consistently.
- Run `cargo run -- --help` and verify source-specific help names all supported
  languages.
- Run `cargo check --workspace` after every implementation change and
  `cargo test --workspace` at completion.
- Before marking the epic complete, perform a focused review against the five
  Epic 009 review guardrails above and add any discovered regression test first.
- Update this epic's status, checklists, final test counts, review outcome, and
  discovered follow-ups only after that review passes.
- Update `README.md`, `DEVELOP.md`, and `AGENTS.md` with `.js`/`.jsx` support,
  generated-file policy, and shared-language test requirements.

**Acceptance Criteria:**

- [ ] Existing CLI and UI show JavaScript/JSX code evidence without a separate
      search mode.
- [ ] `--reindex-code` rebuilds all supported code languages and does not
      affect commit/document records.
- [ ] `cargo run -- --help` identifies Rust, TypeScript, TSX, JavaScript, and
      JSX for code indexing.
- [ ] `README.md`, `DEVELOP.md`, and `AGENTS.md` contain no stale three-language
      code-indexing lists and document the generated-file policy.
- [ ] A focused pre-completion review verifies method locality, nested-symbol
      exclusion, callable bodies, complete import ranges, and production/docs
      wording, with the outcome recorded in this epic.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes and the final indexer/main/doc test counts
      are recorded.

## Test Matrix

| Layer | JavaScript | JSX |
|---|---:|---:|
| Grammar smoke test | Required | Required |
| Evidence extraction contract | Full | Full |
| ES module imports/exports | Required | Required |
| CommonJS require/exports | Required | Optional where meaningful |
| Class/object/prototype method identifiers and local ranges | Required | Class component coverage |
| Nested executable/JSX leakage negative tests | Required | Required |
| Callable binding declaration + bounded body | Required | Required |
| Complete multiline imports/re-exports/require/dynamic import | Required | Required where syntax appears |
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
| `src/main.rs` | Keep CLI help and any shared kind labels exhaustive and list JavaScript/JSX support. |
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

