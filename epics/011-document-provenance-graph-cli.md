# Epic 011: Document Provenance Graph CLI

## Introduction

Build a queryable graph of claims from explicitly selected, human-maintained
repository documents. `graph build` records document authority and extracts
claims with exact supporting passages. `graph search` answers questions from
those claims, cites the passages, and writes each query and response to a
Git-ignored folder in the repository for later search evaluation.

The graph is a separate interpretation layer over source documents. Existing
code, commit, and document indexes remain materialized search data; indexed
code is not treated as an authority for this epic. Store graph tables in the
same per-repository SQLite database already selected by
`daftprompt-indexer` (`dirs::cache_dir()/daftprompt/{repo_slug}.db`). Do not
create a second database or write graph data into `items` or `items_fts`.

**Status:** Complete

## Goals

- Let the caller declare authoritative documents with repeatable `--source`
  arguments, including a repo-relative glob for epics.
- Extract atomic, typed claims and source spans; retain historical and
  conflicting claims rather than overwriting document history.
- Answer paraphrased questions through graph relationships, with citations
  and honest `yes`, `no`, or `unknown` outcomes.
- Log the query, answer, evidence, and graph version for every search in a
  Git-ignored directory under the selected repository.
- Make rebuilding the same source set idempotent and update changed or removed
  sources without leaving active stale claims.

## Non-goals

- Treat source code or generated files as authoritative claims.
- Infer authorship from Git or automatically designate documents as human
  maintained. The CLI selection is the authority declaration.
- Prove runtime behavior or that an implementation matches its documentation.
- Add a graph visualization or GUI workflow.
- Invent a target date for a document's “future work” heading.
- Give a negative answer solely because no matching positive claim was found.

## CLI contract

Create a separate binary named `graph`, with `build` and `search` subcommands.
Keep the existing `cargo run` behavior by setting `default-run = "daftprompt"`
in the root package manifest. Examples:

```bash
cargo run --bin graph -- build --repo . \
  --source DEVELOP.md --source README.md --source AGENTS.md \
  --source 'epics/*.md'
cargo run --bin graph -- search --repo . "Can we search Python code?"
```

`--repo` defaults to the current directory. `--source` is required for build,
repeatable, and accepts a repo-relative file path or quoted glob. Expand globs
in sorted order, deduplicate paths, reject no-match patterns and paths that
escape the canonical repository root (including symlinks), and accept readable
UTF-8 Markdown/plain-text documents. Explicit selection may include untracked
documents. Read the selected working-tree bytes, record their content hash,
and record the current Git HEAD when available; the hash identifies the exact
text even if it is uncommitted. The selected set is **complete** for that
build: a previously selected source omitted from a successful new build is no
longer active.

`search` requires a completed graph build. Print a concise answer and
`repo-relative-path:start-end` citations. Return an explicit unknown answer
when support is insufficient or conflicting; never silently fall back to code
search. Build and search must report missing model configuration or failed
model calls as errors, never as an empty graph or a confident answer.

## Extraction and reasoning contract

The current embedder (`model2vec-rs`) generates vectors but cannot extract
propositions or interpret arbitrary questions. For the MVP, use a configured
structured-output language-model adapter for both claim extraction and query
interpretation. Configuration comes from environment variables for an
OpenAI-compatible base URL, model name, and optional API key; no credentials
are stored in SQLite or query logs. Keep the adapter behind a Rust trait so
tests can use a deterministic fixture implementation and later providers can
be added without changing the graph schema. Validate the model's JSON against
typed Rust structures before writing or querying. A malformed response is an
error with the source or query identified.

Extract from heading-aware passages, preserving exact line ranges and source
text. Each extracted claim must be atomic and use a controlled predicate such
as `indexes_language`, `searches_index`, `uses_parser`, `excludes_path`, or
`work_item_status`. Store the document's original wording as evidence. Map
synonyms such as “indexed”, “included in code search”, and “searchable” to
their proper predicates; do not turn them into a single undifferentiated edge.
Normalize entities and aliases (for example, `Python`/`.py`, `JSX`/`.jsx`),
while preserving the original surface form in the source span. Record whether
a statement is current, planned/deferred, completed, rejected, or historical,
and the scope to which that status applies (project, epic, task, or feature).
An epic's non-goal is scoped to that epic, not a permanent global prohibition.

“Future work” becomes a `work_item_status = deferred` claim supported by the
section that says it. It means *described as remaining work in that document
revision*, not a calendar date. A later completed epic can establish a newer
current claim without deleting the older statement. For example, Epic 008
lists TypeScript and Python among follow-ups, while Epic 009 later marks
TypeScript work complete. Python must not inherit TypeScript's completion.

Support a small set of explicit derivation rules in the MVP. In particular,
`searches_index(CodeSearch, CodeIndex)` plus a current
`indexes_language(CodeIndex, Python)` entails that Python code is searchable.
A negative answer requires an explicit current exclusion or a current,
authoritative **exhaustive** supported-language set excluding Python, plus no
newer contradictory completed claim. A list introduced as examples is not
exhaustive. If neither proof exists, answer `unknown`. Model-generated answer
wording must be checked against the selected claim IDs; the model cannot add
facts or citations not present in the graph.

## SQLite schema

Use `graph_`-prefixed tables with foreign keys and a schema version. Keep
graph writes transactional and independent of existing `items`, FTS, and
vector tables. The conceptual schema is:

| Table | Required fields and purpose |
|---|---|
| `graph_builds` | Build ID, start/completion time, state, extractor model/version, Git HEAD if available. Search only uses a completed build. |
| `graph_sources` | Stable repo-relative path, `human_maintained` authority flag set by `--source`, active flag, latest content hash. |
| `graph_source_versions` | Source ID, content hash, Git HEAD if available, exact bytes or text snapshot, extraction version. Preserves passages after files change. |
| `graph_spans` | Source-version ID, heading path, one-based start/end lines, exact supporting text; verify line bounds and that the text matches the snapshot. |
| `graph_nodes` | Stable canonical key, kind (`feature`, `index`, `language`, `dialect`, `parser`, `work_item`, etc.), display name. |
| `graph_aliases` | Alias text to node ID, scoped by kind; avoid conflating a language with an extension or dialect. |
| `graph_claims` | Subject node, controlled predicate, object node or typed literal, polarity, modality/status, scope, source revision/build metadata. Claims are graph edges. |
| `graph_enumerations` | Subject, predicate, scope, exhaustive flag, source-version ID, and supporting span; groups claims that form one stated list. A closed set is valid only when its own wording says it is complete. |
| `graph_enumeration_members` | Enumeration ID to member claim ID; the set is not inferred from unrelated mentions of the same languages. |
| `graph_claim_evidence` | Claim ID to one or more span IDs; every active claim must have at least one valid span. |
| `graph_claim_supersession` | New claim ID to older claim ID, with a reason; retain both claims and their evidence. |

Add indexes on active source/path, node canonical key, claim
subject/predicate/object, and evidence claim/span. Add FTS5 over claim text
and aliases for candidate retrieval; its content is derived from graph tables,
not written as an independent source of truth. Keep stable IDs where content
and normalized claims are unchanged. A changed source replaces its active
claims atomically; deleted or deselected sources deactivate their claims.
History stays queryable for diagnostics, while ordinary search filters to the
latest completed build's active authoritative sources.

## Search pipeline and response

1. Interpret the question into a typed intent and candidate entities and
   relations. Use aliases, graph FTS, and the model adapter for paraphrases;
   the word “Tree-sitter” need not occur in a question about Python search.
2. Retrieve candidate current claims and traverse supported derivation rules.
   Resolve planned versus completed work by scope and source revision;
   preserve unresolved contradictions.
3. Produce `yes`, `no`, or `unknown`, a short explanation, and the exact claim
   and span IDs used. For `no`, record the explicit exclusion or exhaustive-set
   proof. For `unknown`, state what evidence is missing.
4. Render citations from stored source versions, not from line numbers in a
   later edited working tree. Expose a machine-readable JSON output option
   with outcome, answer, citations, claim IDs, and build ID.

Acceptance examples for this repo:

| Query | Expected result from selected documents |
|---|---|
| “Which languages can code search find?” | Rust, TypeScript/TSX, JavaScript/JSX with current README/AGENTS/Epic 010 citations. |
| “Can we search Python code?” | `no`, citing the current exhaustive language list and Python's deferred follow-up; do not claim a scheduled date. |
| “Can we search Lua code?” | `no` only when the current supported-language list was extracted and validated as exhaustive; otherwise `unknown`. |
| “Does the indexer use Tree-sitter?” | `yes`, with document passages that state the parser relationship. |

## Query logs

Add `/.daftprompt/graph-search/` to this repository's `.gitignore`. For a
different `--repo`, use that repository's root at the same relative path,
but require it to be Git-ignored before writing; report how to add the ignore
rule if it is not. Never write query logs into the shared OS cache database.
Create the directory on search and write one UTF-8 JSON file per invocation using an
atomic temporary-file rename and a collision-resistant filename. Include
schema version, UTC timestamp, repository path, graph build ID, query, status
(`yes`/`no`/`unknown`/`error`), exact response, cited source paths/ranges,
claim IDs, and a concise error when applicable. Do not log API keys, model
request headers, full prompts, or full source documents. Log unknown and
failed searches as well as successful ones. If logging fails, report that
failure rather than implying the search was recorded. Document that this
directory is local and may contain sensitive questions.

## Tasks

Complete these tasks in order. Each task has one main responsibility and can
be handed to a separate coding agent. Keep the CLI and public APIs introduced
by earlier tasks usable while implementing later ones. Use temporary repositories
and a deterministic fixture model for tests; no test should call a live model,
download embeddings, or create a query log in this checkout. After each task,
run `cargo check --workspace` and mark that task complete only when its own
acceptance criteria pass. Run `cargo test --workspace` for the final task.

### Task 1: Add the CLI entry point

**Status:** Complete

**Depends on:** None.

**Change:** Add `src/bin/graph.rs` with clap subcommands `build` and `search`.
Parse `--repo` (default current directory), repeatable required `--source` for
build, the search question, and `--json` for search. Set the root package's
`default-run` to `daftprompt` in `Cargo.toml`. It is fine for subcommands to
return a clear “not implemented” error until their later tasks are complete.

**Acceptance criteria:**

- [ ] `cargo run` still selects the GUI binary.
- [ ] `cargo run --bin graph -- --help` and each subcommand's `--help` show
      the documented arguments; build without `--source` is rejected by clap.

### Task 2: Share the repository DB path and open it without embeddings

**Status:** Complete

**Depends on:** Task 1.

**Change:** Expose one indexer API for the existing per-repo DB path, including
the test cache-directory override used by `IndexerConfig`. Make graph code
open that SQLite file without constructing `Indexer` or loading a model. Do
not duplicate the slug calculation currently found in both `db.rs` and
`lib.rs`.

**Acceptance criteria:**

- [x] Graph and `Indexer` resolve the same path for the same repository and
      cache configuration.
- [x] Opening graph storage does not create or modify the existing search
      indexes and does not require embedding model configuration.

### Task 3: Add the versioned graph schema

**Status:** Complete

**Depends on:** Task 2.

**Change:** Create the `graph_` tables, foreign keys, and indexes described in
the SQLite schema section. Include schema versioning, build state, source
revisions, spans, nodes, aliases, claims, enumerations, evidence, and
supersession. Add build-to-source-version membership (or equivalent) so a
failed build cannot alter what the last completed build exposes. Keep graph
migration separate from the existing item/vector schema. Add graph FTS5 for
derived claim text and aliases, synchronized with the graph records.

**Acceptance criteria:**

- [x] Migration is safe to run twice on the same per-repo DB.
- [x] Foreign keys reject orphaned graph evidence or enumeration members.
- [x] Existing `items`, `items_fts`, and vector tables are unchanged by graph
      migration.

### Task 4: Resolve and validate selected sources

**Status:** Complete

**Depends on:** Task 1.

**Change:** Expand each `--source` file path or quoted repo-relative glob.
Sort matches, deduplicate them, canonicalize the repository and each result,
and accept only readable UTF-8 Markdown/plain-text files inside that root.
Explicitly selected untracked files are valid. Return validated repo-relative
paths and text to the build layer.

**Acceptance criteria:**

- [x] The documented command selects `DEVELOP.md`, `README.md`, `AGENTS.md`,
      and all matching `epics/*.md` exactly once in sorted order.
- [x] Missing files, empty glob matches, unreadable or non-UTF-8 files, `..`
      escapes, and symlinks escaping the root fail with actionable errors.
- [x] No source-selection failure publishes a completed build.

### Task 5: Snapshot sources and create evidence passages

**Status:** Complete

**Depends on:** Tasks 3 and 4.

**Change:** Record each selected path as `human_maintained`, hash its exact
working-tree bytes, store its text snapshot and current Git HEAD when
available, and split the snapshot into heading-aware passages with one-based
line ranges. Give the extractor passage IDs and their exact text.

**Acceptance criteria:**

- [ ] A passage's stored text equals the indicated lines in its stored source
      version, including when the working-tree file later changes.
- [ ] An uncommitted edit changes the content hash; a non-Git folder has no
      HEAD value and still builds.
- [ ] A repeated unchanged source reuses its existing source version.

### Task 6: Define the model adapter and typed responses

**Status:** Complete

**Depends on:** Task 1.

**Change:** Define a Rust trait for structured claim extraction and question
interpretation. Implement the OpenAI-compatible adapter with environment
configuration for base URL, model name, and optional API key. Define typed
request/response structures and a fixture adapter for tests. Keep credentials
out of SQLite and logs.

**Acceptance criteria:**

- [ ] Missing required configuration and failed HTTP/model calls return
      explicit errors that identify the build source or search question.
- [ ] Malformed JSON fails typed deserialization; the fixture adapter can
      exercise both operations without network access.

### Task 7: Validate extracted claims against passages

**Status:** Complete

**Depends on:** Tasks 5 and 6.

**Change:** Validate every extracted atomic claim before storage: controlled
predicate, subject and object/literal types, polarity, status, scope, and one
or more selected passage IDs. Check the claimed line range and exact evidence
text against its snapshot. Reject an invented or mismatched citation.

**Acceptance criteria:**

- [x] A valid fixture claim keeps its original wording and exact source span.
- [x] Unknown predicates, invalid status/scope, missing evidence, invented
      passages, and mismatched text fail with the source identified.
- [x] A malformed model response cannot create an active claim.

### Task 8: Normalize graph entities and aliases

**Status:** Complete

**Depends on:** Task 7.

**Change:** Give entities stable canonical keys and kinds. Store aliases
separately from the source wording; distinguish language, dialect, extension,
index, parser, and work item. Map equivalent surface phrases to the correct
controlled predicates without collapsing `indexes_language`, `searches_index`,
and `uses_parser` into one relation.

**Acceptance criteria:**

- [x] `Python`/`.py` and `JSX`/`.jsx` resolve as intended without conflating
      a language, dialect, and file extension.
- [x] “Indexed”, “included in code search”, and “searchable” fixtures produce
      the proper typed relationships, with original surface text still cited.

### Task 9: Persist claims, evidence, and stated lists

**Status:** Complete

**Depends on:** Tasks 3, 7, and 8.

**Change:** Store validated nodes, aliases, claims, and claim-to-span evidence
in one transaction. Store each stated language list as its own enumeration
with members, scope, source version, and supporting span. Set `exhaustive =
true` only when that list's wording explicitly claims completeness. Maintain
the derived graph FTS index as records change.

**Acceptance criteria:**

- [x] Every stored active claim has at least one verifiable span in an
      explicitly selected source.
- [x] An illustrative “such as” list is not exhaustive; unrelated language
      mentions do not become enumeration members.
- [x] Graph FTS retrieves stored claim text and aliases after a write.

### Task 10: Publish complete builds and handle source changes

**Status:** Complete

**Depends on:** Task 9.

**Change:** Make the selected source set complete for each build. Reuse
unchanged source versions and stable normalized claim IDs. Replace claims
from changed sources and deactivate claims from deleted or deselected sources
atomically; retain old versions and evidence for diagnostics. Publish a build
as completed only after every selected source is extracted and validated.

**Acceptance criteria:**

- [x] Two identical builds produce the same active sources and claim IDs,
      with no duplicate active claims.
- [x] Editing, deleting, or deselecting one source removes its stale claims
      from ordinary search while keeping historical evidence queryable.
- [x] Any source or model failure leaves the previous completed build usable
      and does not publish a partial completed build.

### Task 11: Record status, scope, and supersession

**Status:** Complete

**Depends on:** Task 10.

**Change:** Preserve current, planned/deferred, completed, rejected, and
historical status with project/epic/task/feature scope. Link a newer claim to
an older one only when it genuinely supersedes that claim in the same scope;
keep both citations. Treat “future work” as deferred in that document revision
without inventing a date.

**Acceptance criteria:**

- [x] Epic 008's TypeScript follow-up remains in history after Epic 009's
      completion; Python remains deferred absent newer evidence.
- [x] An epic's non-goal does not become a project-wide prohibition.
- [x] A supersession link preserves both claims and their evidence.

### Task 12: Interpret questions and retrieve candidates

**Status:** Complete

**Depends on:** Tasks 6, 9, and 10.

**Change:** Parse a question into typed intent, entities, and relations using
the model adapter. Resolve entity aliases and use graph FTS to retrieve
candidate claims only from the latest completed build's active authoritative
sources. Return an error if no completed build exists; do not fall back to
code search.

**Acceptance criteria:**

- [x] A paraphrase about Python code search retrieves relevant index and
      language claims even if the question does not name Tree-sitter.
- [x] Inactive and unselected source claims are absent from ordinary
      candidates; malformed interpretation and missing builds report errors.

### Task 13: Apply positive derivation rules

**Status:** Complete

**Depends on:** Task 12.

**Change:** Implement a bounded, explicit set of graph rules. In particular,
`searches_index(CodeSearch, CodeIndex)` plus current
`indexes_language(CodeIndex, Python)` proves Python is searchable. Return the
exact premise claim IDs used, including each premise's evidence IDs. Keep the
rules separate from model-generated answer wording.

**Acceptance criteria:**

- [x] Direct and indirect positive fixtures return `yes` with every required
      premise cited.
- [x] An unsupported relation cannot be invented by the model or traversal.

### Task 14: Resolve negative, unknown, and conflicting evidence

**Status:** Complete

**Depends on:** Tasks 11 and 13.

**Change:** Return `no` only for a current explicit exclusion or a current,
authoritative exhaustive supported-language set that omits the target and has
no newer contradictory completed claim. Keep planned work and epic-scoped
non-goals in their proper scope. Return `unknown` when evidence is absent,
illustrative, insufficient, or unresolvedly contradictory.

**Acceptance criteria:**

- [x] Python and Lua outcomes follow the acceptance examples above when the
      fixture contains a validated exhaustive list.
- [x] Missing evidence and illustrative lists alone yield `unknown`, never a
      negative from absence.
- [x] Contradictory current claims yield `unknown` with the conflict explained.

### Task 15: Render text and JSON answers from stored evidence

**Status:** Complete

**Depends on:** Task 14.

**Change:** Produce `yes`, `no`, or `unknown`, a short answer, claim IDs, build
ID, and citations rendered as `repo-relative-path:start-end`. Resolve cited
text and ranges from stored source versions. Validate any model-generated
wording against selected claim IDs before printing. Add the `--json` response
with outcome, answer, citations, claim IDs, and build ID.

**Acceptance criteria:**

- [x] Editing a source after build does not alter the answer's cited text or
      line range.
- [x] Text and JSON cite only claims and spans from the selected completed
      build; invented facts or citation IDs are rejected.
- [x] Unknown answers state which evidence is missing.

### Task 16: Ignore and write query logs

**Status:** Complete

**Depends on:** Task 15.

**Change:** Add `/.daftprompt/graph-search/` to this repo's `.gitignore`.
For any selected repo, confirm the path is Git-ignored before writing and
give the needed rule if it is not. Write one UTF-8 JSON file per search call
under that repo using a collision-resistant name and atomic temporary-file
rename. Include all fields in the Query logs section and the exact printed
response; record `yes`, `no`, `unknown`, and `error`. Never include API keys,
headers, full prompts, or full source documents.

**Acceptance criteria:**

- [x] `git check-ignore` confirms the log path is ignored in this repo.
- [x] Searches in a temporary repo produce distinct complete log files for
      positive, negative, unknown, and failed outcomes.
- [x] A repo without the ignore rule gets an actionable error; a write failure
      is reported instead of claiming that the query was logged.

### Task 17: Document and verify the CLI workflow

**Status:** Complete

**Depends on:** Tasks 1–16.

**Change:** Update `README.md`, `DEVELOP.md`, and `AGENTS.md` with the build
and search commands, required model environment variables, `--json` output,
source authority semantics, and the local query-log format/location. State
that logs may contain sensitive questions. Run the documented build and the
four acceptance example questions with a deterministic fixture model.

**Acceptance criteria:**

- [x] The documented build resolves the three root documents and epics in
      the existing per-repo DB; the four example questions have supported
      outcomes and snapshot-based citations.
- [x] `cargo check --workspace` and `cargo test --workspace` pass.
- [x] Every completed task above is marked complete in this file.

## File-change summary

| Path | Intended change |
|---|---|
| `Cargo.toml`, `src/bin/graph.rs` | Binary target/default run and CLI. |
| `crates/daftprompt-indexer/src/graph/` | Source selection, extraction, graph storage, search, and model adapter. |
| `crates/daftprompt-indexer/src/db.rs`, graph schema SQL | Shared DB path/API and namespaced graph schema migration. |
| `.gitignore` | Ignore `.daftprompt/graph-search/`. |
| `README.md`, `DEVELOP.md`, `AGENTS.md` | Build/search examples and provenance semantics. |
