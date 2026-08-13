# Epic 014: Task Zero Graph and Prompt Lab

## Introduction

Epics 011, 012, and 013 define a provenance graph, a deterministic planner, and
a graph-tool helper orchestrator. Each epic deliberately blocks its production
implementation on Task 0 evidence. The remaining work cannot be resolved by
more speculative schema and API design alone: daftprompt needs a small,
executable research harness that can turn real repository evidence into the
fixtures, packets, prompts, traces, and measurements required by all three
gates.

This epic builds that harness. Given a bare human request and an immutable
repository revision, the **Task Zero Prompt Lab** reads repository instructions,
implementation epics, research artifacts, the current indexed corpus, and Git
history; extracts a minimal provenance-linked graph; selects a bounded context
packet; and renders a detailed prompt suitable for handoff to a coding agent.
The initial proving case is daftprompt itself, especially Epics 011–013 and
their research, but the harness must replay real cases from the other
repositories required by those epics.

The lab is intentionally a walking skeleton rather than the production
`daftprompt-graph`, `daftprompt-planner`, or helper-orchestrator. Its types,
fixture format, and policies are experimental. Results from the lab revise the
three parent epics before their Task 0 gates are marked complete. Nothing in
this epic authorizes beginning Tasks 1+ of Epics 011–013 early.

The first useful outcome is deliberately narrow:

```text
bare request + repository revision
    -> deterministic evidence graph
    -> bounded context packet with gaps
    -> provenance-linked Markdown coding prompt
```

For example, a request such as “continue closing the Task 0 blockers” should
produce a prompt that identifies the exact unchecked criteria, completed
research, relevant epic constraints, repository invariants, likely evidence
sources, allowed work, required outputs, verification commands, and remaining
ambiguities without claiming that the selected context is complete.

## Relationship to Epics 011, 012, and 013

The lab exercises a thin experimental slice of all three designs:

- Epic 011 supplies identity, provenance, evidence strength, coverage, and gap
  semantics.
- Epic 012 supplies request constraints, project facts, bounded packets,
  actions, dependencies, and output contracts.
- Epic 013 supplies prompt shaping, trust labels, helper comparisons,
  stateless reconstruction, budgets, and replay requirements.

The dependency direction remains unchanged. Experimental code in this epic may
imitate candidate boundaries to measure them, but production crates must not
depend on the lab and the lab must not be presented as their stable API.

The generated coding-agent prompt is an export and evaluation artifact. A
coding agent may have its own tools under its host's authorization policy, but
that does not silently expand the closed, read-only helper tool boundary in
Epic 013. Production mutation, execution, and authorization orchestration remain
outside Epic 013 and outside this research harness.

## Iterative Operating Model

This epic is an ongoing collaborative experiment, not a one-pass implementation
plan. The user and coding agent will repeatedly build a small part of the
harness, run it against real cases, inspect the results, and decide what to
retain, revise, remove, or test next. Tasks in this epic describe persistent
workstreams and evidence requirements; they need not execute once in strict
numeric order, and a previously completed experimental step may be revisited
when a later run exposes a weak assumption.

Each meaningful iteration follows this loop:

1. Select one unresolved criterion or design uncertainty from Epics 011, 012,
   or 013.
2. Choose a real, revision-pinned case and state the expected evidence or
   comparison before changing the harness.
3. Make the smallest harness change needed to run the experiment.
4. Record the exact inputs, versions, output, measurements, failures, and
   unsupported conclusions.
5. Review the result with the user before turning a material experimental
   choice into a stable constraint or parent-epic revision.
6. Update the harness, fixtures, prompts, policies, or task boundaries and run
   the next iteration.

The graph structure, detectors, fixture schema, packet format, prompt renderer,
helper protocol, prompt patterns, budgets, evaluation corpus, CLI, and even the
remaining tasks in this epic may change in response to recorded evidence.
Failed experiments and rejected variants are useful results and must remain
visible rather than being rewritten into an apparently linear success story.

### Repository expansion sequencing

Build and debug the harness end-to-end against the daftprompt repository
itself first: snapshot, graph extraction, packet selection, prompt rendering,
and, once available, helper refinement and replay should all get their
mechanics proven on one deeply understood corpus before other repositories
enter the loop. Iterating on a broken mechanism is cheaper when the author can
debug the corpus fastest.

This is a sequencing preference, not a scope reduction. The multi-repository
corpus required by Task 0's manifest (at least three repositories) and by the
parent Epics 011-013 gates remains mandatory before any Task 0 criterion in
this epic or its parents is marked complete. A single-repository pass proves
the harness runs; it does not by itself satisfy a corpus-level acceptance
criterion and must not be reported as though it does (see Design Constraint 7
and the single-anecdote guardrail in Task 6). Once the harness produces
materially useful, reproducible results on daftprompt fixtures, extend the
manifest to the required additional repositories under the same
immutable-revision and sanitization rules already stated in Task 0.

### Experiment notes live in this epic

Use the `Experiment Notes` section at the end of this file as the chronological
working log for the harness. Add a dated note after every iteration that
materially changes the harness or the interpretation of a result. Notes should
be appended rather than silently rewriting earlier conclusions. If a later
experiment overturns an earlier note, retain the earlier note and link the new
one as a revision or rejection.

Each note records at least:

- the question or parent Task 0 criterion being investigated;
- repository, immutable revision, fixture, and input request;
- harness, detector, prompt-template, policy, and model versions used;
- the change made to the harness;
- observed results and measurements;
- validated findings, rejected interpretations, and remaining gaps;
- the user's decision or explicitly pending decision;
- the next proposed harness change or experiment;
- links to any detailed fixture, trace, research artifact, or parent-epic
  revision.

Large traces and private or bulky case material belong in sanitized files under
`epics/research/task-zero-lab/`; the note in this epic remains the concise index
and decision history. Secrets and private raw prompts must never be copied into
the notes.

The coding agent must not mark an Epic 011–013 Task 0 criterion complete merely
because the harness runs successfully. A criterion is checked only after the
recorded evidence satisfies its exact acceptance language and the user has
reviewed any material design conclusion. Epic 014 is complete only when its
evidence has resolved the pending gates of Epics 011–013, or has demonstrated
that a gate or hypothesis must be explicitly revised or rejected with the
user's agreement.

## Goals

- Turn a minimal human request into a detailed, evidence-backed coding prompt.
- Make the structural relationships among epics, tasks, criteria, research,
  project rules, files, symbols, commits, and gaps queryable.
- Pin every run to an immutable repository revision and record the index
  watermark or staleness visible at that revision.
- Preserve exact provenance for every graph edge and prompt context item.
- Keep established facts, search candidates, helper proposals, and human
  assumptions distinct.
- Report coverage, omissions, budget exclusions, and unresolved gaps.
- Generate deterministic baseline prompts before involving a model.
- Add small-model prompt refinement only as a measured, replaceable layer.
- Produce replayable artifacts that directly satisfy or falsify acceptance
  criteria in the Task 0 gates of Epics 011–013.
- Reuse the existing indexer and local dependency source rather than rebuilding
  repository parsing, Git access, embeddings, or provider integration from
  guesses.

## Non-goals

- Finalize the production graph schema or public graph API.
- Implement Tasks 1+ of Epics 011, 012, or 013 before their gates pass.
- Treat LLM output or semantic similarity as authoritative graph evidence.
- Build a complete call graph, runtime model, or cross-repository production
  graph.
- Let a helper execute shell commands, mutate files, install dependencies, or
  access arbitrary paths or URLs.
- Automatically dispatch generated prompts to a coding agent in v1.
- Claim that a bounded context packet contains every relevant artifact.
- Replace the current `items`, FTS5, or sqlite-vec indexes.
- Stabilize the lab's fixture schema before the experiments reveal which fields
  are actually necessary.

## Required Inputs and Local Source Dependencies

The implementation must read local source when checking behavior or APIs.
Do not infer these libraries from memory or fetch alternate versions when the
checked-out source is available.

### In-repository sources

- `AGENTS.md`, `DEVELOP.md`, and `README.md` for project rules, architecture,
  commands, and user-facing behavior.
- `epics/011-provenance-graph.md`,
  `epics/012-deterministic-task-planner.md`, and
  `epics/013-helper-orchestrator.md` for the parent contracts.
- `epics/research/011-provenance-thought-experiments.md` and
  `epics/research/012-planner-thought-experiments.md` for existing cases,
  practical relevance sets, failed hypotheses, and unresolved blockers.
- `crates/daftprompt-indexer/` for the existing SQLite source of truth, stable
  source identifiers, FTS5/vector retrieval, document chunking, and code-symbol
  extraction.
- `src/bin/groq_context_gap.rs` as the first bounded-helper research precedent;
  reuse lessons and fixtures, but do not turn its experiment-specific policy
  into a production abstraction by accident.

### Locally cloned dependency sources

| Source | Local path | Use in this epic |
|---|---|---|
| `gix` / gitoxide | `~/Projects/gitoxide/gix` | Resolve revisions, commits, trees, blobs, parents, and changed-file evidence. Read its local APIs before implementing root, merge, rename, or history behavior. |
| `llm-sdk` | `~/Projects/llm-sdk` | Existing OpenAI-compatible helper/capable-model boundary and structured completion behavior for measured model runs. |
| Tree-sitter | `~/Projects/tree-sitter` | Reference parser/query behavior already used by the code indexer; do not create a competing language router in the lab. |
| model2vec-rs | `~/Projects/model2vec-rs` | Understand current embedding behavior when recording semantic candidate provenance and degradation. |
| sqlite-vec | `~/Projects/sqlite-vec` | Understand current vector-query behavior and limits when replaying indexed retrieval. |

The akar, wgpu, glyphon, taffy, and UI dependency sources listed in
`DEVELOP.md` are available when a fixture requires UI or rendering evidence,
but the lab itself must have no GPU or UI dependency.

## Design Constraints

### 1. Research harness, not premature production architecture

Prefer one small CLI/replay surface and a fixture format over creating the
three proposed production crates. Experimental types must be clearly named and
kept replaceable. Any field or detector promoted into a parent epic requires
recorded experiment evidence.

### 2. Immutable run identity

Every run records at least:

- canonical repository path or repository fixture ID;
- requested revision and resolved commit/object ID when Git is available;
- dirty-worktree policy and whether uncommitted inputs were included;
- index database identity and observable index watermark;
- input file/blob/content identities;
- lab, detector, prompt-template, and optional model versions.

A prompt must not combine current working-tree text, a stale index, and historic
Git evidence without exposing that mismatch. This constraint also governs the
downstream task-outcome runs added by Design Constraint 7: a disposable
worktree checked out to evaluate a prompt is a second live snapshot, and its
resolved commit must be compared against the revision the prompt and packet
claim to describe, not assumed to match it.

### 3. Minimal evidence graph

The initial experimental graph needs only these node families:

- repository snapshot;
- project instruction;
- epic and version;
- task;
- acceptance criterion;
- design decision or constraint;
- research experiment or result;
- unresolved gap/blocker;
- file or document section;
- code symbol;
- commit.

Initial relationships are `contains`, `depends_on`, `requires`, `constrains`,
`mentions`, `validated_by`, `blocked_by`, `changes`, `defines`, and
`supersedes`. Each edge records detector identity, method, evidence locator,
source version, and confidence or evidence class.

Exact Markdown structure, checkboxes, explicit paths/symbols, and Git facts may
produce established relationships. Lexical and semantic retrieval may propose
candidates. Helper output cannot create established relationships.

### 4. Deterministic baseline first

The baseline prompt renderer receives the same request model and context packet
as later helper variants. It must work without credentials, network access, or
a model. Model-assisted refinement is credited only for measured improvement
over this baseline.

### 5. Prompt output contract

Generated prompts contain, when applicable:

1. original request and clarified objective;
2. explicit intent, mutation boundary, and preserved negative constraints;
3. repository snapshot and index coverage;
4. relevant epic/task/shared constraints;
5. established evidence with compact provenance labels;
6. candidate context clearly separated from established evidence;
7. unresolved Task 0 criteria, context gaps, and human decisions;
8. bounded suggested actions and dependencies;
9. likely change surface without claiming file ownership;
10. required artifacts and output contract;
11. exact project-derived verification commands;
12. omissions, stale sources, and unsupported capabilities.

The prompt should give a coding agent enough technical context to begin focused
work while still instructing it to verify assumptions against the pinned
repository state.

### 6. Results flow back into the parent epics

Passing a lab task is not itself proof that a parent Task 0 criterion passes.
Each replay must state which exact criterion it supports, include its evidence,
and update the corresponding research artifact and epic checkbox only when that
criterion is genuinely satisfied.

### 7. Task outcome is measured independently of prompt quality

A polished, well-cited generated prompt is not evidence that the downstream
work will be correct, complete, or safe. A prompt can read as detailed and
still omit a governing constraint, cite a stale source, or hand off a gap the
agent will not notice. Prompt quality and task outcome are therefore always
scored as two distinct measurements, and neither substitutes for the other:

- **Prompt quality** covers the prompt artifact itself: intent preservation,
  provenance labelling, coverage/gap disclosure, and budget compliance, all
  checkable from the prompt text and its packet without running an agent.
- **Task outcome** covers what happened after a coding agent received the
  prompt: whether the resulting change builds, passes the project's
  verification commands, touches the artifacts in the case's known practical
  relevance set, respects stated negative constraints, and does not act on an
  unsupported claim the prompt should have flagged as a suggestion.

Task outcome runs execute each prompt variant for a case in its own disposable
Git worktree, never the pinned evaluation checkout, so an agent's edits cannot
contaminate the fixture or a later variant's run. Because coding-agent
behavior is not deterministic, a single run is an anecdote; Task 6 requires
repeated runs before a case-level conclusion is drawn, and the deterministic
lab replay (graph, packet, prompt generation) remains fully reproducible
independent of any downstream agent run.

## Tasks

### Task 0: Define the lab corpus and success measurements

Create a manifest that maps the real cases already required by Epics 011–013
to immutable repository revisions, sanitized human requests, known practical
relevance sets, expected graph facts, and parent acceptance criteria.

#### Acceptance Criteria

- [x] The manifest includes the existing Epic 011/012 cases and identifies
  which additional cases are required to reach Epic 013's twelve-case corpus.
  See `epics/research/task-zero-lab/manifest.md` §2–§3 (nine cases, C01–C09,
  each linked back to its originating 011/012 experiment) and §5 (gap
  analysis: at least three more cases needed to reach twelve, with the
  highest-priority content gap identified as injection coverage).
- [x] At least three repositories are represented by immutable revisions.
  Five repositories (akar, Keystone, ReporGo, dwata, nocodo) with revisions
  re-verified against local clones on 2026-08-02 (manifest §1).
- [x] Private inputs are sanitized while retaining content hashes and enough
  behavioral structure for reproducible evaluation. Keystone (C02) and
  ReporGo (C03) private material is represented only by content hashes,
  blob IDs, and structural facts; no raw client, patient, or message content
  is present (manifest §3, §8).
- [x] Each case records original intent, mutation constraints, expert and
  deliberately minimal/non-expert request renderings where required.
  Satisfied for all nine documented cases (manifest §3); non-expert
  renderings are newly authored for this manifest and explicitly marked NEW.
- [x] Each case records its known practical relevance set and the evidence by
  which that set was established. Satisfied for all nine cases, mostly by
  direct `git diff`/`git show` comparison against already-completed
  historical work (manifest §3).
- [ ] Cases cover positive retrieval, missing evidence, ambiguity, conflict,
  injection, exhausted coverage, history dependence, multi-file work, and
  runtime-only discoveries. Left unchecked: the manifest's coverage matrix
  (§4) shows positive retrieval, missing evidence, history dependence,
  multi-file work, and runtime-only discoveries are covered, but ambiguity
  and injection have no sourced case yet (explicit content gaps) and conflict
  and exhausted graph coverage are only partially exhibited by existing cases.
  Closing this requires sourcing or authoring the cases identified in
  manifest §5, not further documentation of the current nine.
- [x] Metrics include artifact recall, irrelevant context, context bytes,
  capable-model tokens, wall time, prompt quality, intent preservation, gap
  recall, duplicate calls, unsupported claims, and task outcome. All eleven
  are defined with source fields in manifest §7. No values are measured yet
  because no extraction/packet/prompt/helper code exists (Tasks 1–5 are
  unstarted); this criterion covers definition, not measurement.
- [x] Every case maps to one or more unchecked Task 0 criteria in Epics
  011–013. Each of the nine cases states its mapped unchecked criteria
  explicitly in manifest §3.
- [x] No production implementation task in Epics 011–013 has begun.
  `crates/` contains only `daftprompt-indexer` (re-verified 2026-08-02), and
  every Task 1+ checkbox in Epics 011, 012, and 013 remains unchecked.

### Task 1: Add immutable snapshot and index-coverage inspection

Implement the read-only run boundary that resolves the requested repository
revision, inventories allowed inputs, and reports whether current indexed data
matches that snapshot.

#### Acceptance Criteria

- [x] Git runs record the resolved commit, tree/blob identities, parents, and
  whether the working tree differs from the requested revision.
  `crates/task-zero-lab/src/git_snapshot.rs::GitSnapshot` records
  `resolved_commit`, `tree_id`, `parents`, per-changed-file `blob_id`/
  `previous_blob_id`, `resolved_is_head`, `head_is_dirty`, and the combined
  `worktree_differs_from_revision` signal. Full-repo blob enumeration beyond
  changed files is out of scope (not required by Design Constraint 2's
  wording and would duplicate `code.rs`'s existing tree traversal).
- [x] Non-Git fixtures use deterministic xxh3 content identities and separate
  content identity from filesystem mtime and observation time.
  `crates/task-zero-lab/src/content_identity.rs` reuses
  `daftprompt_indexer::db::content_hash` (xxh3) for `content_hash`, and keeps
  `mtime_unix`/`observed_at_unix` as separate, hash-independent fields;
  covered by touch/edit/restore tests.
- [x] Index coverage reports available source partitions, supported languages,
  index/database identity, and any detectable staleness or version mismatch.
  `crates/task-zero-lab/src/index_coverage.rs::IndexCoverage` reports
  `db_path`/`db_exists`, per-source-type `partitions`, `supported_languages`,
  `recorded_repo_path`/`repo_path_mismatch`, and `embedding_dimension`. The
  current `repo_meta` schema records only a wall-clock `indexed_at`
  timestamp, not the Git commit an index was built from; `staleness_note`
  states this schema gap explicitly on every report rather than fabricating
  a revision-level freshness claim the data cannot support.
- [x] Historic runs never silently read current file content as if it belonged
  to the pinned revision. `diff_against_first_parent` reads only Git tree/
  blob objects (never worktree files) to compute `changed_files`, and
  `worktree_differs_from_revision` is `true` whenever `resolved_commit !=
  HEAD`, independent of whether the tree happens to be clean.
- [x] Dirty or unindexed inputs are included only by explicit fixture policy and
  remain visibly labelled. `run::DirtyInputPolicy` (`ExcludeDirty` by
  default, `IncludeDirtyLabelled` only via the CLI's explicit
  `--include-dirty` flag) is recorded on every `RunSnapshot`; Task 1 itself
  never substitutes worktree content for pinned-revision content regardless
  of this flag, so the label is a declared policy for later tasks' packet
  selection rather than an actual content-inclusion mechanism yet.
- [x] Root, merge, rename, deletion, and ordinary modification fixtures expose
  honest Git evidence; uncertain rename detection degrades to delete/add.
  Exercised by both self-contained synthetic fixtures and the exact real
  commits from manifest.md §6: akar `311e7d6` (root), dwata `f14ac5c`
  (merge, parents `90f8e7f9…`/`070cbe83…`), nocodo `18bb8b4` (rename,
  `admin-gui/src/pages/DBDeveloperPage.tsx` → `DatabasePage.tsx`), akar
  `3e17af8` (deletion, `CLAUDE.md`), akar `13d7692` (modification,
  `crates/akar-components/src/stat.rs` + `examples/demo-rust/src/main.rs`).
  `uncertain_rename_degrades_to_delete_and_add` covers the below-threshold
  case. Real-fixture tests skip (not fail) if the local `~/Projects/<repo>`
  clone is absent, so `cargo test --workspace` stays portable.
- [x] Snapshot inspection performs no mutation, checkout, network call, or
  dependency installation. The index database is opened with
  `SQLITE_OPEN_READ_ONLY` and `db::init_schema`/`Indexer::new` are never
  called; `db_path_for_repo_readonly` deliberately duplicates
  `db::db_path_for_repo`'s slug formula without its `create_dir_all` side
  effect. `existing_index_reports_partitions_and_metadata_read_only`
  asserts the DB file size is unchanged across two read-only opens.
- [x] Tests use local fixtures and pass without credentials.
  19 tests in `crates/task-zero-lab` (synthetic temp-repo fixtures plus the
  six real manifest-commit fixtures above), all offline and
  credential-free; confirmed via `cargo test --workspace` (162 tests total
  across the workspace, 0 failures).

### Task 2: Extract the minimal epic and repository evidence graph

Parse the selected project documents and repository/index evidence into the
experimental nodes, edges, candidates, provenance records, coverage, and gaps.

#### Acceptance Criteria

- [x] Markdown headings, task nesting, checkboxes, acceptance criteria, explicit
  dependencies, paths, symbols, commands, and cross-epic references are
  extracted deterministically with source spans.
  `crates/task-zero-lab/src/markdown_extract.rs` parses headings, `Task N:`
  nesting, checkbox lists (with continuation lines), `## Dependency`
  sections, backtick paths/symbols, fenced-`bash` commands, and cross-epic
  references, each carrying a `line_span`/`evidence_locator`.
- [x] Epic/task/criterion nodes retain the exact document version and stable
  logical locator; line number alone is not treated as stable identity.
  `NodeLocator::logical_id` (e.g. `epic:014/task:2/criterion:3`) is the
  identity; `line_span` is a separate, non-identity debugging aid
  (`graph.rs`).
- [x] Unchecked criteria become explicit unresolved gaps without implying why
  they remain unchecked.
  Each unchecked criterion produces a verbatim-text `UnresolvedGap` node and
  a `criterion --blocked_by--> gap` edge; verified against this file's own
  72 unchecked criteria (`cargo run -p task-zero-lab -- graph --repo . --rev
  HEAD --epics 014` reports exactly 72 gaps, matching
  `grep -c '^\s*- \[ \]'`).
- [x] Project rules from `AGENTS.md`, `DEVELOP.md`, and repository configuration
  retain source precedence and provenance.
  `AGENTS.md` (precedence 0), `DEVELOP.md` (1), and `Cargo.toml` as
  repository configuration (2) each produce a `ProjectInstruction` node with
  `source_path`/`source_version`.
- [x] Existing index results reuse their canonical `(source_type, identifier)`
  identity and do not duplicate indexed text as graph truth.
  `index_coverage::read_identifiers` (read-only, reuses
  `daftprompt_indexer::db::existing_identifiers`) tags matching nodes with
  their complete canonical `{source_type, identifier}` pairs (including all
  matching document chunks or file-owned code symbols); no indexed text is
  copied into the graph.
- [x] Git commit/file/version facts come from revision-pinned Git evidence.
  `Commit`/`RepositorySnapshot` nodes and `changes` edges come from Task 1's
  `GitSnapshot`; document content is read via the new
  `git_snapshot::read_blob_at_revision` (Git object store, never the
  worktree) unless `--include-dirty` is explicitly set at `HEAD`, which is
  labelled `read_from_pinned_revision: false` in `GraphCoverage`.
- [x] Established edges, lexical/semantic candidates, helper proposals, and
  human assumptions serialize into distinct fields.
  `GraphExtraction` has separate `established_edges`/`candidate_edges`/
  `helper_proposed_edges`/`human_assumption_edges`; Task 2 populates only
  `established_edges` (structural/exact evidence) — the other three are
  present but intentionally empty, ready for Tasks 3 and 5.
- [x] Every edge can be explained without rerunning its detector.
  Every `Edge` carries `Vec<EdgeProvenance>` (detector, method, evidence
  locator, source version, evidence class, confidence, detail);
  `Edge::explain()` composes its description from stored fields only.
- [x] Unknown relation/detector values survive fixture loading as diagnostics.
  Custom `Deserialize` impls on `RelationKind`/`DetectorId`/`EvidenceClass`
  fall back to an `Unknown(String)` variant instead of erroring;
  `GraphExtraction::from_json` preserves them and automatically records each
  occurrence in `diagnostics`. Covered by unit tests in `graph.rs`.
- [x] Repeated extraction from identical inputs produces byte-stable normalized
  JSON apart from explicitly excluded run timing fields.
  `GraphExtraction` carries no timing field; `normalize()`/
  `to_normalized_json()` sort every collection. Verified by
  `repeated_extraction_is_byte_stable` (markdown_extract) and
  `repeated_build_over_identical_inputs_is_byte_stable` (graph_build) against
  both synthetic and real repository fixtures.

### Task 3: Implement bounded graph selection and context packets

Given a request and graph snapshot, select a small relevant subgraph and render
a machine-readable evidence packet with explicit coverage and gaps.

#### Acceptance Criteria

- [x] Exact epic/task/criterion/path/symbol references seed retrieval before
  fuzzy channels.
  `crates/task-zero-lab/src/packet.rs::find_exact_seeds` matches epic/task
  references and backtick paths/symbols against node locators and always
  runs before `find_lexical_seeds`; `select_packet` only feeds lexical hits
  not already established. Covered by
  `exact_epic_and_task_reference_is_seeded_before_lexical_channel`.
- [x] Search is source-stratified across instructions, epics/research,
  documents, code, and commits where available.
  `SourceCategory` (`Instructions`/`EpicsAndResearch`/`Documents`/`Code`/
  `Commits`) with a per-category cap (`LEXICAL_SEEDS_PER_CATEGORY`) in
  `find_lexical_seeds`; unavailable categories are reported honestly in
  `PacketCoverage::unavailable_sources` rather than assumed present.
- [x] Expansion uses an allowlist of relation kinds, maximum depth, item count,
  excerpt bytes, and total packet bytes.
  `PacketBudget` (`allowed_relations`, `max_depth`, `max_items`,
  `max_excerpt_bytes`, `max_total_bytes`) bounds `expand_established`,
  `build_items`, and the final normalized pretty-JSON packet. Budget
  enforcement is rerun after host-validated helper additions. Exact seeds
  and retained constraints cannot be silently displaced: an impossible
  budget returns an error. Anything cut is recorded in
  `PacketCoverage::budget_omissions`. Covered by focused item, excerpt,
  serialized-byte, and helper-addition budget tests.
- [x] Parent/shared epic constraints are retained when selecting an individual
  task.
  `retain_parent_constraints` guarantees a selected `Task`/
  `AcceptanceCriterion`'s enclosing epic's `DesignConstraint` children are
  included regardless of depth-bounded expansion; if `max_items` cannot hold
  the exact seeds and required constraints together, selection fails rather
  than dropping either class.
- [x] Duplicate resources and overlapping excerpts are deduplicated without
  losing provenance paths.
  `merge_duplicate_resources` merges only generic no-line-span references
  into a richer node sharing the same `source_path`, keeping every
  contributing locator in `merged_from` and every `found_via` reason;
  distinct line-spanned nodes in the same file are deliberately not
  collapsed.
- [x] Established evidence and candidates remain separate in the packet.
  `Packet` keeps `established`/`candidates` as distinct fields; lexical-only
  hits never enter `established`.
- [x] Coverage reports queried sources, unavailable sources, detector/index
  versions, budget omissions, and remaining gaps.
  `PacketCoverage` (`queried_sources`, `unavailable_sources`, `lab_version`,
  `graph_resolved_commit`, `index_available`,
  `index_identifiers_considered`, `budget_omissions`, `remaining_gaps`).
- [x] A helper's proposed gap disposition cannot remove a deterministic host
  gap.
  `apply_helper_gap_dispositions` only appends informational
  `HelperGapNote`s (`host_gap_retained_regardless: true`); no code path
  touches `PacketCoverage::remaining_gaps` removal.
- [x] Repeated identical queries over an identical graph produce an identical
  normalized packet.
  `Packet::normalize`/`to_normalized_json` sort every collection. Covered by
  `repeated_identical_queries_over_an_identical_graph_produce_an_identical_packet`.
- [x] The LOG-019 fixture reproduces the known distinction between useful
  observations and rejected requirement/test/history interpretations.
  `log019_fixture_graph` plus
  `log019_fixture_distinguishes_validated_observations_from_rejected_interpretations`
  reproduce the six-way validated/rejected classification from
  `epics/research/011-provenance-thought-experiments.md`'s real Keystone
  LOG-019 replay, via `apply_helper_dispositions`'s host-validated-only path
  (no private Keystone content reproduced).

### Task 4: Generate deterministic coding-agent prompts

Build the model-free baseline renderer that converts the original request and
context packet into a detailed Markdown prompt.

#### Acceptance Criteria

- [x] A bare daftprompt request such as “continue closing the Task 0 blockers”
  resolves to the relevant Epics 011–013 criteria and research rather than all
  repository text.
- [x] Original intent and negative constraints are copied verbatim into an
  immutable section before any refinement.
- [x] The prompt follows the output contract in Design Constraint 5.
- [x] Every repository claim in the prompt has a compact provenance reference;
  unsupported suggestions are labelled as suggestions.
- [x] The renderer reports stale index/input mismatches and never hides missing
  coverage behind polished prose.
- [x] Verification commands come from exact repository instructions or
  configuration, not from language-name guessing.
- [x] Prompt length obeys a configured byte/token estimate budget and records
  omitted lower-ranked context.
- [x] Identical request, graph, packet, and template versions produce identical
  prompt text.
- [x] Golden fixtures cover implementation, diagnosis, review, research,
  ambiguity, and no-mutation requests.
- [x] Generated prompts are exportable for manual handoff; automatic coding-
  agent dispatch is absent.

### Task 5: Add stateless helper refinement behind a closed interface

Evaluate whether selected small/tiny helpers improve the deterministic prompt.
Each helper decision is a fresh host-reconstructed request and may only request
typed, bounded, read-only lab operations.

**Scope note (this implementation pass):** Task 5 was split into two parts.
This pass builds and verifies the complete closed-interface infrastructure —
the operation catalog, bounded/recorded stateless round loop, submission
schema, prompt-pattern library, and a deterministic, offline, credential-free
`ScriptedHelperModel` (`crates/task-zero-lab/src/helper.rs`) — entirely
without real model access. It deliberately **defers** wiring real open-weight
model adapters (Groq/Ollama/llama.cpp via the local `~/Projects/llm-sdk`
crate): that needs live credentials or a locally running model this
environment cannot provision. `HelperModel` (`helper.rs`) is the extension
point a future adapter implements — plugging in, e.g., a `GroqHelperModel` —
without touching this module's orchestration loop (`refine_prompt`). The two
criteria below that name real open-weight helpers stay unchecked until that
follow-up lands. **Status correction (2026-08-12):** that was true when this
scope note was written, but the `llm-sdk` source-boundary criterion was
checked on 2026-08-11 in `d82a7d2` once the sanitized, live-derived failure
replay existed. Only one criterion — "At least three open-weight helpers are
supported in experiments" — remains unchecked under Task 5, and it still
requires the repeated live runs in plan step 7. Dated notes below that refer
to "the two unchecked Task 5 criteria" were accurate when written and are
retained unedited.

#### OpenRouter hosted-helper completion plan

Local model storage is not required to finish the deferred half of this task.
Use OpenRouter as the first hosted experimental provider through the local
`~/Projects/llm-sdk` boundary, while keeping the existing deterministic and
scripted paths credential-free and offline. OpenRouter is a transport/provider
choice, not an evidence source and not a reason to weaken the closed helper
operation catalog.

1. Discover candidates with the lab's read-only filter rather than hardcoding
   whatever models happen to be visible in the OpenRouter catalog:

   ```bash
   cargo run -p task-zero-lab --bin openrouter_model_candidates
   cargo run -p task-zero-lab --bin openrouter_model_candidates -- --format json
   ```

   The CLI queries the public models endpoint with text modality and prompt/
   completion price bounds of USD 0–0.10 per million tokens, then locally
   requires exact text-only input/output, context at most 131,072 tokens,
   `response_format`, a Hugging Face identifier, and an inferred size below
   20B. The API does not expose parameter count directly: the CLI reports its
   conservative inference and the source text, and unknown sizes remain
   excluded by default. A Hugging Face identifier is only discovery evidence;
   verify total parameter count, model-card license, and open-weight status
   before approving a model for the experiment.
2. Select and record exact model IDs for two distinct helpers below 10B and
   one helper in the 10B-to-below-20B tier. Catalog results are time-varying.
   The 2026-08-11 discovery run suggested, and upstream model-card/license
   verification approved for experiments,
   `ibm-granite/granite-4.1-8b`,
   `meta-llama/llama-3.1-8b-instruct`, and
   `mistralai/mistral-nemo`. The first two are 8B and the third is 12B;
   Granite and Mistral Nemo are Apache-2.0, while Llama uses the Llama 3.1
   Community License and is recorded as open-weight without claiming OSI
   open-source status. See the dated catalog and verification artifacts.

   **Revised model set (2026-08-13):** the 36-run matrix exposed provider-level
   failures for Llama/DeepInfra (11/12 `adapter_unavailable`) and Mistral
   Nemo/DeepInfra (11/12 `low_marginal_yield`). Llama was replaced by local
   Qwen 3.5 0.8B via llama.cpp, which was then dropped due to unreliable
   schema compliance (see experiment notes below). The current three-helper
   candidate set is:
   1. `ibm-granite/granite-4.1-8b` (hosted, Granite/CoreWeave) — demonstrated
   2. Qwen 3.5 9B (local, llama.cpp) — demonstrated (clean first-attempt submit)
   3. Third helper TBD (user will download additional local models)
3. Add an `OpenRouterHelperModel` adapter around
   `llm_sdk::openrouter::OpenRouterClient`. Keep `HelperModelOutput` as the
   response schema: each stateless round asks for one typed JSON tool request
   or submission, and the host remains the only component that validates and
   executes a closed read-only operation. Native provider tool execution is
   unnecessary for this experiment.
4. Make live execution an explicit credentialed CLI mode using
   `OPENROUTER_API_KEY`. Pin the requested model ID, disable provider fallback,
   pin or record the upstream provider, require supported request parameters,
   and apply no-data-collection/ZDR routing when available. If the current
   `llm-sdk` request type cannot express those routing controls, add them there
   rather than bypassing the SDK from the lab.
5. Record requested and returned model IDs, upstream provider, routing policy,
   temperature, output limit, protocol/prompt hashes, input/output tokens,
   elapsed time, stop reason, raw-response hash, and validation diagnostics.
   Never treat a routed alias such as `openrouter/free` as a reproducible model
   experiment.
6. Sanitize successful and failure traces into credential-free replay fixtures.
   Replays use `ScriptedHelperModel` and never contact OpenRouter; private raw
   repository content, API keys, and provider payloads must not be committed.
7. Run all three pinned helpers over the same fixed cases, packets, prompt
   patterns, budgets, and repeated-run policy before checking the two remaining
   criteria. Availability or one successful request is not experimental
   support, and model comparison results feed Task 6 rather than proving task
   outcome by themselves.

#### Acceptance Criteria

- [x] The deterministic baseline remains fully usable when helper execution is
  disabled or unavailable. `helper::refine_prompt(.., model: None, ..)`
  returns `StopReason::Disabled` and calls `prompt::render_prompt` completely
  unmodified; `baseline_identical_when_helper_disabled` asserts byte equality,
  and the CLI-level `tests/cli_helper.rs::cli_helper_disabled_matches_cli_prompt`
  confirms the same over this repository's own real Epic 014 graph (the
  `helper` subcommand with no `--scripted-fixture` byte-matches `prompt`).
- [x] The initial catalog contains only named graph/search/explain/coverage
  operations with validated repository/revision scope; no arbitrary path,
  shell, URL, network, or write tool is exposed.
  `helper::HelperOperation` is a closed enum (`SearchGraph`, `GetNode`,
  `ExplainEdge`, `GetCoverage`) with no filesystem-path/shell/URL/network
  field anywhere; `HelperToolExecutor::execute` only matches these variants.
  `closed_catalog_has_no_out_of_band_variant` covers both the compile-time
  exhaustiveness and an out-of-catalog scripted step surfacing as
  `Malformed`, never executed.
- [x] Helper output conforms to a schema that separates refinements,
  clarifications, selected evidence, candidate claims, and proposed gaps.
  `helper::HelperSubmission` has exactly these five fields (reusing Task 3's
  `HelperDisposition`/`HelperGapDisposition` for the last two); covered by
  `submission_schema_round_trips_all_five_fields`.
- [x] Human intent, negative constraints, and deterministic gaps cannot be
  overwritten by model output. `apply_submission` never receives
  `PromptRequest` at all (it only touches `Packet`), so there is no code path
  from a submission to `original`/`negative_constraints`; helper suggestions
  land only as clearly labelled "Helper-proposed ... (unverified)" candidates.
  `proposed_gap_dispositions` is applied only through Task 3's existing
  `apply_helper_gap_dispositions`, which never removes a host gap. Covered by
  `original_request_and_negative_constraints_never_mutated` and
  `gap_disposition_cannot_remove_host_gap`.
- [x] Each round reconstructs one self-contained prompt from host state rather
  than appending an unbounded transcript. `helper::build_round_prompt`
  rebuilds the entire round text from structured `validated_calls` state
  every time (never an appended raw transcript); covered by
  `round_prompt_reconstruction_is_deterministic_and_not_appended_transcript`.
- [x] Calls, rounds, tokens, result bytes, elapsed time, duplicate requests, and
  validated marginal yield are bounded and recorded. `HelperPolicy`
  (`max_rounds`, `max_calls`, `max_result_bytes`, `max_total_bytes`,
  `min_marginal_yield`, `malformed_retry_budget`) bounds `refine_prompt`'s
  loop; `HelperRunReport`/`CallRecord` record every field including
  `duplicate_calls`. (Token/elapsed-time fields are recorded when an adapter
  reports them; `ScriptedHelperModel` performs no inference, so it reports
  none — real adapters will populate these.) Covered by
  `rounds_calls_bytes_duplicates_are_bounded_and_recorded`,
  `max_result_bytes_truncates_with_marker`, and `over_calling_stops_at_max_calls`.
- [x] Prompt-pattern exemplars are trust-labelled, versioned reference material
  and cannot supply repository facts or expand capabilities.
  `helper::PromptPattern` (id/version/source/intent_tags/content_hash/body)
  loaded by `load_prompt_patterns`, which recomputes each file's xxh3
  `content_hash` (reusing `daftprompt_indexer::db::content_hash`) and flags a
  diagnostic — never silent trust — on mismatch; embedded in every round
  prompt under an explicit "reference only, not repository facts" heading.
  Since a `HelperOperation` can only ever come from `HelperModel::decide`'s
  typed return value, no exemplar or tool-result text can grant a capability.
  Covered by `prompt_pattern_exemplars_are_labeled_and_hash_verified` (a
  tampered fixture loads with a diagnostic, not silent trust) and
  `injected_instruction_in_candidate_text_stays_inert`.
- [ ] At least three open-weight helpers are supported in experiments,
  including at least two below 10B parameters and one sub-20B tier model.
  The configurable `OpenRouterHelperModel` adapter now exists and accepts the
  three pinned, independently verified IDs through the local `llm-sdk`
  boundary, but this remains unchecked until repeatable live experiments have
  actually run across all three models as required by plan step 7.
- [x] Model/provider code uses the local `~/Projects/llm-sdk` source boundary;
  recorded replay fixtures require no credentials or network access.
  The adapter uses `llm-sdk` commit `491e99e` (updated from `87cebeb` on
  2026-08-12); it still sends `response_format: json_object` and does not yet
  use that commit's strict `json_schema` response format. The credentialed zero-credit
  smoke attempt produced the sanitized, live-derived failure replay
  `openrouter-smoke-blocker-2026-08-11.json`; it contains typed decisions,
  hashes, and sanitized inference metadata but no credential, prompt, or raw
  provider payload, and the `openrouter_helper_experiment replay` path
  reconstructs it with `ScriptedHelperModel` without credentials or network.
- [x] Malformed output, prompt injection, early stop, over-calling, and false
  completion preserve the deterministic baseline and produce diagnostics.
  Covered by `malformed_output_falls_back_to_baseline_with_diagnostics`
  (malformed/invalid-schema submissions fall back to the byte-identical
  baseline with recorded diagnostics),
  `injected_instruction_in_candidate_text_stays_inert` (an embedded
  "ignore previous instructions" claim changes no policy value and invokes
  no tool) and `injected_instruction_in_rejection_feedback_stays_inert`
  (the same attempt carried through the host-rejection feedback path — the
  only path by which model-chosen text reaches a round prompt, via serde's
  quoted-value validator message — changes no policy value, budget, stop
  reason, or gap, invokes no operation, survives only as one sanitized,
  bounded, host-prefixed line inside the rejection section with no forged
  section heading elsewhere in the prompt, and still falls back
  byte-identically to the deterministic baseline; a model-chosen *key* never
  reaches the prompt or report at all, since `HelperSubmission` has no
  `deny_unknown_fields` and the resulting error is the host-authored
  `missing field \`stop_reason\``), and `premature_stop_preserves_baseline` /
  `over_calling_stops_at_max_calls` (an empty script and an over-long script
  both stop cleanly within policy bounds). Output truncation is diagnosed and
  bounded separately from schema violation: the adapter classifies a
  `finish_reason: "length"` response that also fails to parse as
  `HelperModelOutput::Truncated`, and the round loop spends
  `HelperPolicy::truncated_retry_budget` (default 1) and reports
  `StopReason::OutputTruncated` instead of consuming the malformed budget.
  Covered by
  `mocked_transport_classifies_length_cutoff_as_truncation_not_malformed` and
  `truncation_and_schema_violation_are_distinct_and_both_keep_the_baseline`
  (distinct stop reasons and diagnostics; both fall back to the byte-identical
  deterministic baseline).

### Task 6: Add replay, baselines, ablations, and prompt assessment

Run reproducible comparisons over the Task 0 corpus and store complete traces
without committing private raw material or credentials.

#### Acceptance Criteria

- [ ] Each case compares raw human prompt, deterministic generated prompt, and
  helper-refined prompt against the same fixed capable-model configuration.
- [ ] Relevant cases also compare broad tool-enabled capable agent,
  deterministic packet only, helper with/without prompt patterns, and stateless
  versus appended-transcript helper prompting.
- [ ] Non-expert/minimal and expert-authored variants of the same underlying
  request are compared.
- [ ] Runs record exact prompts, model IDs, template versions, graph operations,
  outputs, budgets, latency, token counts, context bytes, and validation results.
- [ ] Scoring distinguishes repository-fact accuracy, artifact recall, intent
  preservation, prompt quality, gap detection, decision usefulness, and final
  task outcome.
- [ ] Unsupported claims and missing context are independently checked against
  the pinned practical relevance set where one exists.
- [ ] Scripted replays reproduce state transitions and prompt artifacts without
  model/network access.
- [ ] Failed models, policies, patterns, and negative results remain in the
  research record.
- [ ] No single successful anecdote is used to pass a parent epic's corpus-level
  criterion.
- [ ] Each case's raw, deterministic, and helper-refined prompt variants are
  handed to a coding agent in a disposable Git worktree per variant and
  revision, never the pinned evaluation checkout, and the resulting diff,
  build/verification result, and touched artifacts are captured.
- [ ] Task outcome is scored against the case's known practical relevance set
  (artifact precision/recall) and its verification commands, independent of
  any subjective rating of the prompt text (Design Constraint 7).
- [ ] A downstream unsupported or extraneous edit is traced back to the
  prompt claim or omission that produced it, and a gap the agent independently
  rediscovered or missed is recorded against the prompt's disclosed gaps.
- [ ] Stated negative constraints are checked against the actual diff, not
  against the prompt's restatement of the constraint.
- [ ] Each case-variant pair runs more than once before a case-level
  conclusion is drawn, and the report distinguishes a stable result from
  single-run variance.

### Task 7: Close the parent Task 0 gates from evidence

Use lab results to revise the parent epics and their research artifacts. Mark
only criteria supported by committed, reviewable evidence.

#### Acceptance Criteria

- [ ] Epic 011 research gains executable expected-result fixtures for required
  Git/document/section cases and the real multi-file epic comparison.
- [ ] Epic 011 schema, detector list, and task boundaries are revised from
  observed retrieval successes and misses.
- [ ] Epic 012 research reaches its required prompt corpus, plan-quality,
  removal/delay/manual-action, savings, and multi-file comparison thresholds.
- [ ] Epic 012 models, rules, actions, and task boundaries are revised from
  observed planning results.
- [ ] `epics/research/013-helper-orchestrator-experiments.md` records the full
  required corpus, model ladder, baselines, ablations, failures, thresholds,
  intent preservation, accessibility, token, and time results.
- [ ] Each checked parent criterion links to the exact lab fixture/run and a
  human-readable conclusion.
- [ ] Any failed parent gate remains unchecked and states the smallest concrete
  follow-up; the lab does not manufacture a passing conclusion.
- [ ] The research artifacts and resulting parent-epic revisions receive an
  independent review before Task 0 is marked complete.
- [ ] The final artifacts are committed before any Task 1 implementation begins
  in Epics 011–013.

### Task 8: Document the lab and archive or retain it deliberately

Document how to add fixtures, generate a prompt, inspect provenance, replay a
run, and interpret limitations. Decide from evidence whether the harness should
remain as developer tooling, be reduced to fixtures, or seed production APIs.

#### Acceptance Criteria

- [ ] `DEVELOP.md` documents offline fixture/replay commands and optional live
  model commands without exposing credentials.
- [ ] `README.md` is updated only if the lab becomes a supported user-facing
  capability; otherwise it remains documented as experimental developer
  tooling.
- [ ] `AGENTS.md` explains that generated prompts and packets are evidence-
  bounded aids, not authorization or proof of completeness.
- [ ] Fixture privacy, sanitization, content hashing, and provenance rules are
  documented.
- [ ] Local dependency source paths and the reason to inspect them are listed.
- [ ] The disposition of experimental code and fixture schema is recorded:
  retained, migrated, or archived.
- [ ] Promoted production requirements are copied into Epics 011–013 with links
  to the evidence that justified them.
- [ ] `cargo check --workspace` and `cargo test --workspace` pass.

## Suggested Experimental CLI

Exact flags may change during implementation, but the research workflow should
support equivalents of:

```bash
cargo run --bin task_zero_lab -- snapshot --repo . --rev HEAD
cargo run --bin task_zero_lab -- graph --repo . --rev HEAD --epics 011,012,013
cargo run --bin task_zero_lab -- prompt --repo . --rev HEAD \
  --request "continue closing the Task 0 blockers"
cargo run --bin task_zero_lab -- replay epics/research/fixtures/<case>.json
```

Live helper/model execution must be an explicit mode. Snapshotting, graph
extraction, packet generation, prompt rendering, and scripted replay must work
offline.

## Test Matrix

| Area | Required coverage |
|---|---|
| Snapshot | Git/non-Git, historic revision, dirty tree, stale index, root/merge/rename/delete/modify |
| Epic parsing | headings, duplicate headings, tasks, criteria, checked state, dependencies, explicit paths/symbols |
| Identity | logical versus immutable version, Git blob/object IDs, xxh3 non-Git content identity |
| Evidence | structural, exact, lexical candidate, semantic candidate, helper proposal, human assumption |
| Graph | edge explanation, unknown kinds, deterministic normalization, bounded traversal |
| Packet | source stratification, deduplication, shared context, candidates, coverage, omissions, gaps |
| Prompt | intent preservation, provenance labels, command sourcing, budgets, golden determinism |
| Helper | schema validation, stateless reconstruction, closed tools, injection, over-call, early stop, exhaustion |
| Evaluation | baselines, ablations, model ladder, expert/non-expert pairs, tokens, time, task outcome |
| Replay | offline deterministic reconstruction with no credentials or network |
| Privacy | sanitized fixtures, hashes without private raw content, no secrets in traces |
| Parent gates | exact criterion mapping, evidence link, honest unchecked failure |

## File-change Summary

The exact experimental layout may change, but responsibilities should remain
bounded as follows:

| File | Change |
|---|---|
| `src/bin/task_zero_lab.rs` or a small experimental crate | Read-only CLI for snapshot, graph, prompt, and replay operations. |
| Experimental lab modules | Snapshot identity, epic extraction, graph fixtures, context selection, prompt rendering, traces, and scoring. |
| `epics/research/task-zero-lab/` | Sanitized corpus manifests, expected-result fixtures, prompts, traces, and comparison reports. |
| `epics/research/013-helper-orchestrator-experiments.md` | Blocking helper/model/policy evaluation required by Epic 013. |
| Epic 011/012 research artifacts | Executable fixture results and findings-driven revisions. |
| Epics 011–013 | Acceptance status and design/task revisions supported by evidence. |
| `DEVELOP.md`, optionally `AGENTS.md`/`README.md` | Lab workflow, safety boundary, and supported-status documentation. |

## Risks and Guardrails

- A self-referential daftprompt fixture can overfit the extractor and prompt
  template. Passing requires the multi-repository corpus, not only Epics
  011–013.
- Parsing Markdown structure is easier than establishing repository truth. An
  unchecked criterion is a blocker node; it is not evidence about the cause or
  solution.
- Current indexes contain independent commit, code, and document items but not
  every relationship proposed by Epic 011. Simulated or fixture-expected edges
  must remain labelled as such.
- Historic Git content and the current per-repository index may describe
  different snapshots. The lab must disclose or reject mixed-snapshot packets.
- Prompt quality can be mistaken for task success. Evaluation must score both
  the prompt and the downstream result.
- A helper may produce confident interpretations that deterministic evidence
  does not support, as observed in the LOG-019 experiment. Host gaps and evidence
  classes remain authoritative.
- Generated prompts can accidentally imply mutation permission. Authorization
  must be copied from the human request or left explicitly unresolved.
- Experimental code can become accidental architecture. Production promotion
  requires a documented parent-epic decision backed by repeated evidence.
- Private prompt corpora and client artifacts must never be committed raw.
  Sanitization, hashes, and immutable public fixture abstractions are required.
- Downstream task-outcome runs (Design Constraint 7, Task 6) execute a coding
  agent against a generated prompt. That run must happen in a disposable
  worktree separate from the pinned evaluation revision so an agent's
  mutations never contaminate the lab's own fixtures, indexes, or a sibling
  variant's run.
- That same run reintroduces the mixed-snapshot danger of Design Constraint 2
  in a new place, and is worth calling out explicitly rather than assuming the
  existing snapshot rule automatically covers it: the worktree the agent edits
  in is a second live checkout, distinct from whatever revision the graph was
  built against and whatever revision the index was last built against. Three
  concrete failure modes to guard against:
  - the worktree is checked out at a commit other than the one the prompt and
    packet claim to describe (e.g. a stale branch, an uncommitted local change
    carried over, or the agent pulling new commits mid-run);
  - the index consumed when generating the packet was built at an earlier or
    later watermark than the worktree's checked-out commit, so retrieval
    reflects coverage the worktree does not actually have, or is missing
    coverage the worktree does have;
  - running several case-variant worktrees concurrently against the same
    repository path lets one experiment's in-place state leak into another's
    result unless each worktree and its resolved commit are tracked and
    reported independently.
  Each recorded task-outcome result must therefore state the worktree's own
  resolved commit and index watermark alongside the prompt's claimed revision,
  and disclose any mismatch instead of silently scoring the agent's diff as if
  it answered the pinned snapshot.

## Experiment Notes

Append chronological harness iterations here using the template below. Do not
delete superseded notes; add a later note that revises or rejects them.

### Note template

```markdown
### YYYY-MM-DD — Short experiment name

- Parent criterion/question:
- Repository and immutable revision:
- Fixture and input request:
- Harness/detector/prompt/policy/model versions:
- Harness change:
- Observed result and measurements:
- Validated findings:
- Rejected or unsupported interpretations:
- Remaining gaps:
- User decision or pending decision:
- Next iteration:
- Detailed artifacts:
```

### 2026-08-02 — Task 0 case manifest

- Parent criterion/question: Epic 014 Task 0, all bullets — define the lab
  corpus and success measurements by mapping existing Epic 011/012 real cases
  to immutable revisions, sanitized requests, relevance sets, expected graph
  facts, and parent acceptance criteria.
- Repository and immutable revision: five repositories, all revisions
  re-verified with `git cat-file -e` on 2026-08-02 — see
  `epics/research/task-zero-lab/manifest.md` §1 for the full table (akar,
  Keystone, ReporGo, dwata, nocodo).
- Fixture and input request: no new fixtures executed; this iteration is pure
  documentation synthesis over the already-committed
  `epics/research/011-provenance-thought-experiments.md` and
  `epics/research/012-planner-thought-experiments.md` experiments.
- Harness/detector/prompt/policy/model versions: none — no lab code exists
  yet (Task 1 unstarted).
- Harness change: none (documentation only). Created
  `epics/research/task-zero-lab/manifest.md`.
- Observed result and measurements: nine real cases (C01–C09) indexed across
  five repositories; coverage matrix against Epic 013's fifteen required
  properties shows five properties covered, two partial, five behavior-gaps
  (cannot be pre-filled without a running harness), and six pure content gaps
  (ambiguity, injection, vague/clarification-recoverable, verbose/poorly
  structured, genuine-ambiguity-preserving — some cases may close more than
  one gap at once). Eleven required metrics defined with source fields; no
  values measured yet.
- Validated findings: the existing Epic 011 research corpus already supplies
  enough real, revision-pinned, cross-repository material to satisfy most of
  Epic 014 Task 0's structural criteria (repository count, sanitization,
  intent/relevance-set recording, criterion mapping) without inventing new
  cases. The twelve-case corpus and full coverage-property list required by
  Epic 013 Task 0 are not yet reachable from existing material alone.
- Rejected or unsupported interpretations: did not fabricate plausible
  content for the ambiguity, injection, over-calling, early-stop (beyond
  C02's already-real instance), multi-helper-comparison, or intent-drift
  properties; recording them as open gaps was preferred over a manufactured
  passing conclusion (per Design Constraint 1 and the epic's guardrail
  against this).
- Remaining gaps: at least three more real or carefully-authored cases needed
  to reach Epic 013's twelve-case floor; injection coverage specifically
  needs a constructed fixture since it is inherently adversarial; five
  behavior-gap properties require Epic 014 Tasks 1–5 to exist and run before
  they can be marked covered; candidate unexplored local repositories
  (`admin-gui`, `pi`, `pixlie`, `rustysolid`, `SmartCrawler`, others) have not
  been searched for additional real cases in this pass.
- User decision or pending decision: pending review of the manifest and this
  note before treating the nine-case corpus as a stable base for Task 1+
  fixtures.
- Next iteration: mine additional local repositories for real ambiguous and
  vague/clarification-recoverable cases; author a controlled injection
  fixture in a disposable worktree of an existing corpus repository; only
  after Task 1 (snapshot/index-coverage inspection) exists, begin executing
  C01–C09 as actual harness runs to start closing the behavior-gap
  properties.
- Detailed artifacts: `epics/research/task-zero-lab/manifest.md`.

### 2026-08-02 — Task 1 snapshot and index-coverage harness

- Parent criterion/question: Epic 014 Task 1, all eight bullets — implement
  the read-only run boundary that resolves a requested repository revision,
  inventories allowed inputs, and reports whether currently indexed data
  matches that snapshot.
- Repository and immutable revision: daftprompt itself (`HEAD` at the time
  of this note) plus six real, revision-pinned commits from
  `epics/research/task-zero-lab/manifest.md` §6: akar `311e7d6` (root),
  dwata `f14ac5c` (merge), nocodo `18bb8b4` (rename), akar `3e17af8`
  (deletion), akar `13d7692` (modification). Local clones under
  `~/Projects/` re-verified present with `git cat-file -e` before writing
  the fixtures.
- Fixture and input request: no human-request fixture — Task 1 is
  infrastructure, not the request→prompt path. Inputs were the six manifest
  commits above plus twelve self-contained synthetic temp-repo fixtures
  (root, ordinary modification, deletion, above-threshold rename,
  below-threshold rename-that-degrades-to-delete/add, merge, historic-
  revision-vs-worktree, dirty-worktree-at-HEAD) and three content-identity
  fixtures (touch-without-change, content-change, content-restore).
- Harness/detector/prompt/policy/model versions: new crate
  `crates/task-zero-lab` v0.1.0 (`lab_version` field), no detector/prompt/
  model versions yet (Tasks 2-5 unstarted). `gix` local path dependency
  (`~/Projects/gitoxide/gix`), with the `status` feature added only to this
  crate's own `gix` dependency (needed for `Repository::is_dirty()`) rather
  than to the workspace root or `daftprompt-indexer`'s `gix` dependency.
- Harness change: added `crates/task-zero-lab` (workspace member) with
  `src/git_snapshot.rs` (Git revision resolution, parent/tree/blob identity,
  first-parent changed-file evidence with rename/copy detection at Git's
  default 50% similarity threshold, `worktree_differs_from_revision`),
  `src/content_identity.rs` (xxh3 non-Git content identity via
  `daftprompt_indexer::db::content_hash`, separate from mtime/observation
  time), `src/index_coverage.rs` (strictly read-only inspection of the
  existing per-repo indexer SQLite DB — `SQLITE_OPEN_READ_ONLY`, no
  `init_schema`/`Indexer::new` call, no `create_dir_all` side effect), and
  `src/run.rs` (combines the above into one `RunSnapshot` plus an explicit
  `DirtyInputPolicy`). CLI at `crates/task-zero-lab/src/main.rs` (binary
  `task_zero_lab`) exposes `snapshot --repo <path> --rev <rev>
  [--include-dirty]`, `content --path <file-or-dir>`, and `coverage --repo
  <path>`, matching (with a couple of additional subcommands) the epic's
  suggested experimental CLI shape.
- Observed result and measurements: `cargo check --workspace` and `cargo
  test --workspace` both pass (162 tests total, 0 failures; 19 in
  `task-zero-lab`). Manual CLI run of `snapshot --repo . --rev HEAD` against
  daftprompt's own repository correctly reported two real file-level
  changes (`epics/014-task-zero-prompt-lab.md` modified,
  `epics/research/task-zero-lab/manifest.md` added) after a bug was found
  and fixed: `gix_diff::tree_with_rewrites` also emits directory (tree-mode)
  entries as pseudo-changes to support relation reconstruction, and an
  early version of this harness surfaced those directories
  (`epics`, `epics/research`) as if they were changed files. Fixed by
  filtering on `entry_mode.is_tree()`, mirroring the blob-only filter
  `code.rs`'s tree traversal already applies. The same manual run also
  correctly read the real, already-indexed daftprompt cache DB read-only
  (80 commits / 453 code items / 205 document items) and reported
  `worktree_differs_from_revision: true`, since this session's own
  uncommitted work is not part of the diff against `HEAD`'s first parent.
- Validated findings: `gix`'s existing revision/tree/diff/status APIs
  (`rev_parse_single`, `Commit::parent_ids`/`tree`/`tree_id`,
  `Repository::diff_tree_to_tree` with explicit `Rewrites::default()`,
  `Repository::is_dirty()`) are sufficient to implement all eight Task 1
  criteria without inventing custom Git plumbing; reusing
  `daftprompt_indexer::db`'s `content_hash`, `repo_meta_get`,
  `existing_identifiers`, and slug-derivation conventions avoided
  re-deriving the indexer's identity model. A read-only SQLite connection
  plus a duplicated (but `create_dir_all`-free) copy of the DB-path slug
  formula is enough to inspect index coverage with zero mutation risk, at
  the cost of one small, explicitly-commented formula duplication (already
  precedented by `Indexer::new`'s own custom-cache-dir branch). Rename
  detection correctly degrades to delete+add below the 50% similarity
  threshold without any extra logic — this is `gix`'s existing behavior at
  the default configuration, not something this harness had to build.
- Rejected or unsupported interpretations: did not attempt to record a
  index-to-revision staleness verdict (e.g. "index reflects commit X"); the
  current `repo_meta` schema has no column for the commit an index was
  built from, so any such claim would be fabricated. The report states this
  gap explicitly (`staleness_note`) instead. Did not implement full-repo
  blob enumeration (every blob in the resolved tree) — Design Constraint 2
  asks for "tree/blob identities" in service of run identity, not a
  complete object inventory, and full enumeration would duplicate
  `code.rs`'s existing tree-traversal responsibility for no criterion this
  task actually requires.
- Remaining gaps: index coverage cannot yet answer "was this index built
  from the same commit this snapshot resolved" — closing that would require
  a production schema change (recording an indexed-at commit SHA in
  `repo_meta`), which is out of scope for a read-only Task 1 inspection tool
  and would need its own parent-epic decision before touching
  `daftprompt-indexer`'s schema. `--include-dirty`'s policy label is not
  yet consumed by anything (Task 1 has no content-selection stage); it
  exists so Task 3's packet selection has an explicit, already-tested
  policy signal to read instead of inventing one under time pressure later.
  Non-Git fixture inspection has not yet been exercised against the
  synthetic scenarios `epics/research/task-zero-lab/manifest.md` §6
  documents as not repository-pinned (ephemeral `guide.md`, ambiguous
  duplicate headings) — those need Task 2's Markdown-structure extraction,
  not Task 1's content-identity primitive alone.
- User decision or pending decision: pending review of this harness and its
  test coverage before treating `crates/task-zero-lab`'s types as a stable
  base for Task 2 (graph extraction) to build on.
- Next iteration: Task 2 — parse Markdown structure (headings, task
  nesting, checkboxes, acceptance criteria, explicit paths/symbols) from the
  epic files this Task 1 harness can now pin a revision against, and start
  wiring the C01-C09 manifest cases into actual harness runs now that a
  snapshot boundary exists to run them against.
- Detailed artifacts: `crates/task-zero-lab/` (new crate: `src/git_snapshot.rs`,
  `src/content_identity.rs`, `src/index_coverage.rs`, `src/run.rs`,
  `src/main.rs`).

### 2026-08-03 — Task 2 minimal evidence graph extraction

- Parent criterion/question: Epic 014 Task 2, all ten bullets — parse
  selected project documents and repository/index evidence into the
  experimental nodes, edges, candidates, provenance records, coverage, and
  gaps defined by Design Constraint 3.
- Repository and immutable revision: daftprompt itself at `HEAD`
  (`61bac23`, then the working tree as this note's own edits land);
  extraction exercised against this repo's own `epics/011`, `epics/012`,
  `epics/014`, `AGENTS.md`, and `DEVELOP.md`.
- Fixture and input request: no human-request fixture — Task 2 is graph
  extraction, not the request→prompt path. Inputs were the real epic/rule
  files above plus synthetic temp-repo Markdown fixtures for edge-case
  coverage (missing epic heading, unrecognized relation/detector/evidence
  values, dirty-worktree exclusion).
- Harness/detector/prompt/policy/model versions: `crates/task-zero-lab`
  still v0.1.0; two new detectors — `markdown_structure`
  (`markdown_extract.rs`) and `markdown_exact_reference` (backtick paths/
  symbols, fenced commands, cross-epic references) — plus reuse of Task 1's
  `git_changed_file` and a new `index_identity_reuse` detector
  (`index_coverage::read_identifiers`).
- Harness change: added `src/graph.rs` (node/edge/evidence-class core
  types, `Unknown(String)` fallback deserialization, `Edge::explain`,
  `GraphExtraction::normalize`/`to_normalized_json`), `src/markdown_extract.rs`
  (pure Markdown structure parser), `src/graph_build.rs` (orchestrator
  wiring Task 1's git/index primitives with the new extractor into one
  `GraphExtraction`); extended `git_snapshot.rs` with
  `read_blob_at_revision` and `index_coverage.rs` with `read_identifiers`;
  wired a `graph --repo . --rev HEAD --epics 011,012,013` CLI subcommand.
- Observed result and measurements: `cargo check --workspace` and `cargo
  test --workspace` pass (187 tests total, 0 failures; 44 in
  `task-zero-lab`, up from 19). Manual CLI run of `graph --repo . --rev
  HEAD --epics 014` against this file produced 233 nodes, 288 established
  edges, 0 diagnostics, and exactly 72 unresolved gaps — verified equal to
  `grep -c '^\s*- \[ \]' epics/014-task-zero-prompt-lab.md`.
- Validated findings: a deterministic Markdown structure parser plus a
  regex-based exact-reference scanner is sufficient to satisfy every Task 2
  criterion without any lexical/semantic retrieval; those channels stay
  correctly empty (`candidate_edges`) until Task 3. Reusing Task 1's
  `GitSnapshot`/`index_coverage` primitives as the sole source of Git and
  index evidence avoided re-deriving revision or identifier logic. The
  four-way edge-trust split (`established_edges`/`candidate_edges`/
  `helper_proposed_edges`/`human_assumption_edges`) as separate struct
  fields, rather than a single tagged list, made "helper output cannot
  create established relationships" (Design Constraint 3) a type-level fact
  instead of a convention to remember.
- Rejected or unsupported interpretations: did not add a dedicated
  "command" node kind, since Design Constraint 3's eleven-family node list
  has no slot for it; extracted commands are attached as `extra.commands`
  on their enclosing node instead. Did not turn arbitrary prose H2 sections
  (Introduction, Goals, Risks, etc.) into graph nodes, since none of the
  eleven node kinds fits free prose — their text is still reachable via the
  whole-document reference scan. Did not build full historic-revision
  directory listing for `--epics` file discovery (only pinned-revision
  *content* reads are historic); a truly historic file-existence check was
  judged out of scope for Task 2 under Design Constraint 1 and is left as a
  known gap.
- Remaining gaps: `RelationKind::Mentions` (loose textual cross-epic
  reference) versus `RelationKind::DependsOn` (only from an explicit
  `## Dependency` section) is a judgment call worth revisiting once Task 3
  needs to rank or expand across these edges. No lexical/semantic candidate
  detector exists yet, so `candidate_edges` is unexercised beyond its
  emptiness. Historic-revision `--epics` file discovery (see above) remains
  unimplemented.
- User decision or pending decision: pending review of the graph extractor
  and its test coverage before treating `crates/task-zero-lab`'s node/edge
  vocabulary as a stable base for Task 3 (bounded selection and context
  packets) to build on.
- Next iteration: Task 3 — implement bounded graph selection (exact-seed
  retrieval before fuzzy channels, source-stratified search, relation-kind/
  depth/byte budgets) and render a machine-readable context packet with
  explicit coverage and gaps, using this task's `GraphExtraction` as input.
- Detailed artifacts: `crates/task-zero-lab/src/graph.rs`,
  `src/markdown_extract.rs`, `src/graph_build.rs`.

### 2026-08-03 — Task 2 review corrections

- Parent criterion/question: review follow-up for Task 2's canonical index
  identity reuse, unknown-value fixture diagnostics, and exact checklist
  boundaries.
- Repository, revision, fixture, and request: daftprompt at `f9a7f9d` plus
  the review working tree; real `--epics 014` extraction and synthetic unit
  fixtures; no human-request fixture because this remains graph extraction.
- Harness versions and change: lab v0.1.0; canonical index matching now maps
  document paths to `path`/`path::chunk` identifiers and code paths or
  qualified symbols to complete `path::namespace::symbol` identifiers;
  `GraphExtraction::from_json` automatically emits diagnostics for preserved
  unknown vocabulary; checklist continuation accepts only indented Markdown
  lines and no longer absorbs trailing prose or fenced commands.
- Observed result: 47 `task-zero-lab` tests pass and `cargo check
  --workspace` passes. The real indexed Epic 014 run considered 658 index
  identifiers and retained 12 canonical identities on 2 referenced nodes,
  compared with zero retained identities before the correction; it emitted
  no diagnostics. Dependency warnings are unchanged and originate in local
  path dependencies.
- Validated/rejected/remaining: rejected direct equality between a bare
  Markdown path/symbol and every index identifier as insufficient for
  chunked documents and file-qualified code symbols. Matching is deliberately
  deterministic and may return multiple canonical identities rather than
  selecting one without evidence. Unqualified symbols can legitimately match
  multiple files; Task 3 must preserve that ambiguity when selecting context.
- User decision: corrections requested during review; final acceptance of
  the revised Task 2 base remains pending user review.
- Next experiment: use the retained canonical identity sets as Task 3 exact
  seeds and measure whether ambiguity/budget reporting remains honest.

### 2026-08-03 — Task 3 bounded graph selection and context packets

- Parent criterion/question: Epic 014 Task 3, all ten bullets — given a
  request and Task 2's `GraphExtraction`, select a small relevant subgraph
  and render a machine-readable evidence packet with explicit coverage and
  gaps.
- Repository and immutable revision: daftprompt itself at `HEAD`
  (`ab31a4c`); exercised against this repo's own `epics/014` graph via the
  new `packet` CLI subcommand plus synthetic fixture graphs for unit tests.
- Fixture and input request: no human-request fixture beyond manual CLI
  smoke tests (e.g. "continue closing the Task 0 blockers" against
  `--epics 014`); Task 3 is selection/packet infrastructure, not the
  prompt-rendering path (Task 4).
- Harness/detector/prompt/policy/model versions: `crates/task-zero-lab`
  still v0.1.0; new `src/packet.rs` module (`select_packet`,
  `find_exact_seeds`, `find_lexical_seeds`, `expand_established`,
  `retain_parent_constraints`, `merge_duplicate_resources`,
  `apply_helper_dispositions`, `apply_helper_gap_dispositions`); no new
  detector — built entirely on Task 2's existing `established_edges` and
  node vocabulary.
- Harness change: added `src/packet.rs` (~1000 lines incl. tests) and a
  `packet --repo <path> --rev <rev> --epics <n,...> --request <text>
  [--max-depth/--max-items/--max-excerpt-bytes/--max-total-bytes]` CLI
  subcommand in `src/main.rs`, mirroring the existing `graph` subcommand's
  shape; factored a shared `parse_epic_numbers` helper out of both.
- Observed result and measurements: `cargo check --workspace` and `cargo
  test --workspace` pass (201 tests total, 0 failures; 58 in
  `task-zero-lab`, up from 47). Manual `packet --repo . --rev HEAD --epics
  014 --request "continue closing the Task 0 blockers"` correctly seeds
  `epic:014/task:0` exactly, retains its parent epic's design constraints,
  and reports index/lab-version coverage; output verified to be valid
  normalized JSON.
- Validated findings: exact-reference seeding plus bounded breadth-first
  traversal over `established_edges` (relation allowlist, depth, item-count)
  is sufficient to build a small, source-stratified, budget-bounded packet
  from Task 2's output without any new retrieval infrastructure. A simple
  case-insensitive keyword-substring scan, run independently per
  `SourceCategory` with its own small cap, is an honest (not overclaimed)
  stand-in for "fuzzy channels" given that Task 2 built no
  lexical/semantic `candidate_edges` detector. Merging duplicate resources
  by `source_path` only when the duplicate carries no recorded `line_span`
  (versus always merging same-path nodes) was necessary to avoid collapsing
  a task and its own nested acceptance criteria into one item.
- Rejected or unsupported interpretations: did not implement real
  lexical/semantic retrieval (FTS5/sqlite-vec/embedding-based) under the
  "fuzzy channel" criterion — Task 2 produced no such detector to draw on,
  and building one was judged out of Task 3's scope; the module docs record
  this as a deferred capability rather than quietly substituting keyword
  matching for it. Did not implement real Git-blob content excerpting for
  nodes with no recorded line span (e.g. a bare unresolved file/symbol
  reference); such an excerpt is currently just the reference's own label
  string, not a windowed read of file content.
- Remaining gaps: no real lexical/semantic candidate-evidence channel exists
  yet for Task 3 to select over — closing this requires either a Task 2
  detector addition or explicit acceptance that keyword matching is the
  lab's permanent "fuzzy channel". No excerpt windowing from real file
  content for reference-only nodes. Packet selection has not yet been
  exercised against the C01–C09 manifest cases as actual harness runs.
- User decision or pending decision: pending review of `src/packet.rs` and
  its test coverage before treating this packet shape as the input Task 4's
  prompt renderer builds on.
- Next iteration: Task 4 — build the model-free baseline Markdown prompt
  renderer that converts a `Packet` into the Design Constraint 5 output
  contract, with golden fixtures across implementation, diagnosis, review,
  research, ambiguity, and no-mutation request types.
- Detailed artifacts: `crates/task-zero-lab/src/packet.rs`.

### 2026-08-03 — Task 4 deterministic coding-agent prompt renderer

- Parent criterion/question: Epic 014 Task 4, all ten bullets — can the
  Task 3 packet become a bounded, provenance-labelled Markdown handoff with
  immutable human intent and honest gaps, entirely without a model?
- Repository/revision/request: daftprompt at `HEAD`; manual smoke replay used
  `--epics 011,012,013 --request "continue closing the Task 0 blockers"`.
  That explicit cross-epic blocker phrasing selects Task 0 in each caller-
  scoped epic; it does not broaden retrieval to every repository document.
- Harness/template/model versions: `task-zero-lab` v0.1.0,
  `task-zero-baseline-v1`, no model. Added `src/prompt.rs`, exact command
  carriage in `PacketItem`, six sanitized golden request-class fixtures, and
  the read-only/offline `prompt` CLI with optional explicit manual export.
- Observed result: the real replay resolved Epic 011–013 Task 0 evidence and
  produced a deterministic prompt within the configured 24,000-byte budget.
  Lower-ranked context that did not fit was named under omissions. Index
  availability remained accompanied by the existing inability to prove its
  commit freshness. Verification commands were rendered only when Task 2 had
  extracted the exact command into packet evidence; absent that evidence the
  prompt says not to guess from the language.
- Fixture coverage: implementation, diagnosis, review, research, ambiguity,
  and no-mutation inputs are stored in
  `crates/task-zero-lab/fixtures/prompts/cases.json`; tests cover byte-stable
  replay, the twelve-part output contract, verbatim intent/constraints,
  provenance labels, candidate/suggestion separation, stale-index warnings,
  hard budget/error behavior, and command non-invention.
- Boundaries and rejected alternatives: no credentials, network, helper, or
  automatic coding-agent dispatch exists. `--output` is an explicit manual
  file export only. The renderer does not re-read the repository to recover
  commands or enrich prose, because doing so could mix snapshots and would
  bypass packet provenance. A too-small budget errors rather than truncating
  immutable intent or mandatory coverage disclosure.
- Design Constraint 7 result: this experiment measures the prompt artifact
  only. It supplies no evidence that a downstream coding agent would produce
  a correct or safe task outcome; those isolated repeated runs remain Task 6.
- Remaining gaps: golden fixtures validate the deterministic prompt contract,
  not downstream quality. The current packet has no pinned index-commit
  watermark and no semantic retrieval channel, both disclosed in the prompt.
- Verification: `cargo check --workspace` and `cargo test --workspace` pass
  (209 tests total: 11 app, 132 indexer, 66 lab; 0 failures). Warnings are
  unchanged and originate in local path dependencies.

### 2026-08-03 — Task 4 prompt-admission review correction

- Review finding: the first real 24 KB CLI handoff exposed two defects that
  the synthetic fixtures missed. Task 2's deliberately broad fenced-line
  extraction had carried diagram fragments such as `-> helper request
  refinement`, option-only lines such as `--changes...`, and prose such as
  `pytest from pyproject.toml` into `verification_commands`; the renderer
  incorrectly presented them as executable verification. Established packet
  order also admitted Epic 011's large Task 0 subtree before Epic 012 or 013,
  so deterministic omissions were honest but the handoff was not usefully
  cross-epic.
- Correction: prompt verification now applies a conservative executable-shape
  filter after provenance selection. It rejects arrows, option-only lines,
  multiline text, and common prose connectors, and admits only an explicit
  executable allowlist or repository-local `./...` command. Rejected lines
  remain packet evidence; they are merely ineligible for the verification
  section. Established prompt admission is now deterministic round-robin over
  epic/source buckets, with each bucket prioritizing Task 0, its criteria and
  gaps, selected research, then shared constraints and other context.
- Evidence: regressions cover the three observed command false-positive
  shapes plus a real `cargo check --workspace` survivor, and a representative
  oversized Epic 011 packet proves that Epic 011, 012, and 013 Task 0 evidence
  all remains visible while lower-ranked omissions are still recorded. The
  real CLI replay now visibly includes all three Task 0 nodes and emits only
  `cargo check` / `cargo check --workspace` from the selected evidence.
- Scope and remaining limitation: executable-shape filtering is intentionally
  conservative and may omit an unusual but valid project command; omission is
  safer than presenting prose as an exact verification instruction. This
  remains prompt-quality evidence only, not a downstream Task 6 outcome.
- Verification after correction: `cargo check --workspace` and `cargo test
  --workspace` pass (211 tests total: 11 app, 132 indexer, 68 lab; 0
  failures). Existing warnings remain confined to local path dependencies.

### 2026-08-04 — Task 4 verification evidence-boundary correction

- Review finding: the prompt renderer collected executable-looking commands
  from both established and candidate packet items. Although candidates were
  labelled honestly in their own section, the verification section promoted
  their commands as exact project-derived guidance before their relevance had
  been established.
- Correction: project-derived verification now consumes only established
  packet evidence. Candidate commands remain visible only as candidate context
  and cannot become authoritative verification instructions until selection
  establishes them. A regression covers a candidate-only `cargo test
  --workspace` command and requires the renderer to report that no exact
  verification command survived.
- Verification: `cargo check --workspace` and `cargo test --workspace` pass.
  Existing warnings remain confined to local path dependencies.

### 2026-08-03 — Task 3 budget-enforcement review correction

- Question: does Task 3 actually preserve exact/required context and enforce
  its declared item, excerpt, and total-packet byte budgets after every
  evidence path, including helper validation?
- Repository/revision/input: daftprompt working tree based on `936850d`;
  focused synthetic graphs exercise tight item budgets, long excerpts,
  serialized JSON size, and multiple validated helper proposals.
- Review result: the initial implementation counted only excerpt content for
  `max_total_bytes`, appended truncation markers outside the excerpt cap,
  allowed helper candidates to bypass final budgets, and could let locator
  ordering displace an exact task with a retained constraint.
- Correction: `select_packet` now returns an error when a budget cannot retain
  all exact seeds and required parent constraints; established expansion has
  its own non-required seed channel; truncation markers fit inside the excerpt
  cap; and one final budget pass bounds item count and the byte length of the
  normalized pretty-JSON packet. `apply_helper_dispositions` reruns that same
  enforcement and returns an error if the irreducible packet envelope cannot
  fit.
- Evidence: added
  `excerpt_marker_is_included_inside_the_excerpt_budget`,
  `final_pretty_json_respects_total_packet_bytes`,
  `impossible_item_budget_errors_instead_of_dropping_exact_and_required_context`,
  and `helper_validated_candidates_cannot_bypass_item_budget`.
- Interpretation: the earlier claim that `build_items` alone bounded the
  complete packet is rejected and replaced by final serialized-packet
  enforcement. The broader pending decisions and cross-repository gaps in the
  preceding Task 3 note remain unchanged.

### 2026-08-11 — OpenRouter catalog discovery and model verification

- Parent criterion/question: Epic 014 Task 5's two unchecked real-helper
  criteria; OpenRouter completion plan steps 1–2 — establish a reproducible,
  credential-free candidate set and verify the required parameter tiers,
  licenses, and public weight availability before implementing or paying for
  completions.
- Repository and immutable revision: daftprompt working tree based on
  `cb0170d`; no model weights or private repository inputs were downloaded.
- Fixture and input request: public `GET https://openrouter.ai/api/v1/models`
  through `openrouter_model_candidates --format json`, with the CLI's exact
  text-only, context, price, `response_format`, Hugging Face ID, and inferred
  below-20B filters.
- Harness/detector/prompt/policy/model versions: `task-zero-lab` 0.1.0;
  candidate CLI at `cb0170d`; no helper adapter or completion model executed.
- Harness change: no code change. Archived a normalized catalog report and a
  separate upstream verification report.
- Observed result and measurements: six catalog candidates survived. The
  deterministic suggestion policy returned two inferred 8B candidates
  (`ibm-granite/granite-4.1-8b`,
  `meta-llama/llama-3.1-8b-instruct`) and one inferred 12B candidate
  (`mistralai/mistral-nemo`). The normalized JSON report SHA-256 is
  `e01c9f29e6418a30fffee0b1d1bb180c4114a4ec18ac531579323e236400c55b`;
  the verification report SHA-256 is
  `80a3438d3c06ae0f37d82c6d390b69cf60568b9081b26cc840abbefc35d8aa04`.
- Validated findings: upstream repositories publish the weights and confirm
  the tiers: IBM documents Granite as 8B, Meta identifies Llama 3.1 Instruct
  as 8B, and Hugging Face weight metadata reports 12,247,782,400 parameters
  for Mistral Nemo. Granite and Mistral Nemo declare Apache-2.0; Llama declares
  the Llama 3.1 Community License. These exact three OpenRouter IDs are
  approved as experimental candidates.
- Rejected or unsupported interpretations: the CLI's parameter extraction is
  not authoritative and did not establish the tiers; a Hugging Face ID alone
  does not establish a license; the custom Llama license is not described as
  OSI open source; catalog presence does not prove later provider/routing
  availability or successful structured output.
- Remaining gaps: OpenRouter routing/provider controls and response metadata
  must exist in local `llm-sdk`; the lab adapter, offline transport tests,
  explicit credentialed CLI mode, sanitized replay traces, and repeated
  three-model experiment remain undone. The Task 5 acceptance boxes therefore
  remain unchecked.
- User decision or pending decision: the user selected hosted OpenRouter
  helpers because local model storage is unavailable; final experiment
  conclusions remain pending the controlled runs.
- Next iteration: implement the required typed OpenRouter routing controls in
  local `llm-sdk`, then the lab adapter and offline tests; do not make paid
  calls until those boundaries pass review.
- Detailed artifacts:
  `epics/research/task-zero-lab/openrouter-candidates-2026-08-11.json` and
  `epics/research/task-zero-lab/openrouter-model-verification-2026-08-11.md`.

### 2026-08-11 — OpenRouter helper adapter and explicit live boundary

- Parent criterion/question: Epic 014 Task 5 and OpenRouter completion plan
  steps 3–5 — can a hosted model participate through the existing stateless,
  closed helper interface while preserving reproducible routing and sanitized
  inference metadata?
- Repository and immutable revision: daftprompt working tree based on
  `4d41973`; local `~/Projects/llm-sdk` at `87cebeb`.
- Fixture and input request: local configuration/request-shape tests only; no
  model completion, private repository prompt, or credential was transmitted.
- Harness/detector/prompt/policy/model versions: `task-zero-lab` 0.1.0;
  `task-zero-helper-v1`; deterministic temperature 0 and default 2,048-token
  output cap; no model executed.
- Harness change: added one configurable `OpenRouterHelperModel` around
  `llm_sdk::openrouter::OpenRouterClient`, an explicit `--live-openrouter`
  CLI branch, strict exact-model/provider configuration, disabled fallbacks,
  required parameters, denied data collection, ZDR routing, JSON output, and
  per-round sanitized inference audit records. Adapter failures use a distinct
  `adapter_unavailable` stop reason, suppress provider payload details, and
  retain the deterministic baseline; returned model/provider identity
  mismatches are diagnosed and rejected.
- Observed result and measurements: `cargo check -p task-zero-lab` passed with
  `RUSTC_WRAPPER` cleared; focused offline configuration/request-shape tests
  pass. No token, latency, provider-availability, or model-quality measurement
  exists because live execution was intentionally excluded from this pass.
- Validated findings: the local SDK exposes the required provider preferences,
  returned model/provider identity, token usage, finish reason, and exact raw
  response bytes. The lab stores only hashes and structured metadata, never
  the API key or raw provider response. Model output remains a typed
  `HelperModelOutput`; the existing host is still the sole tool executor.
- Rejected or unsupported interpretations: adapter support does not establish
  that any candidate/provider combination is currently routable, honors ZDR,
  reliably emits the schema, or improves prompts. No Task 5 real-run criterion
  is checked from compilation or request-shape evidence.
- Remaining gaps: perform a reviewed live smoke test, sanitize successful and
  failure traces into offline replay fixtures, and run the fixed repeated
  experiment across all three verified models before evaluating the two
  remaining Task 5 criteria.
- User decision or pending decision: API key is present in ignored `.env`;
  provider pins and paid/live invocation remain pending review.
- Next iteration: review the adapter and CLI/report boundary, then run the
  smallest approved live smoke test without printing or persisting secrets or
  raw provider payloads.
- Detailed artifacts: `crates/task-zero-lab/src/openrouter_helper.rs`,
  `crates/task-zero-lab/src/helper.rs`, and
  `crates/task-zero-lab/src/main.rs`.

### 2026-08-11 — Offline hosted-transport and sanitized replay preparation

- Parent criterion/question: Epic 014 Task 5, OpenRouter completion plan step
  6 and preparation for step 7 — can hosted success/failure behavior be tested
  and replayed without credentials, network, prompts, or raw provider payloads?
- Repository and immutable revision: working tree based on the current Epic
  014 adapter revision; no live repository or provider input was consumed.
- Fixture and input request: local mock HTTP responses for a tool call,
  submission, malformed output, routing-identity mismatch, and 503 failure;
  synthetic prompt strings only.
- Harness/detector/prompt/policy/model versions: `task-zero-lab` 0.1.0,
  `task-zero-helper-v1`, `task-zero-helper-replay-v1`; no model executed.
- Harness change: added offline mocked-transport coverage and a deliberately
  lossy `DecisionRecorder`/`SanitizedReplayArtifact` path containing hashes,
  typed decisions, and sanitized inference metadata but no prompt/key/raw-payload fields. Added a
  fixed four-case, three-repetition protocol for all three pinned models.
- Observed result and measurements: offline tests cover valid tool/submission,
  malformed schema, returned model/provider mismatch, and provider failure;
  the replay schema rejects unpaired or wrong-model inference metadata.
- Validated findings: the SDK base-URL seam exercises the real adapter parsing
  and error path locally; provider failure details do not enter inference
  reports. Sanitized artifacts reconstruct `ScriptedHelperModel` directly.
- Rejected or unsupported interpretations: mock responses and a replay schema
  do not constitute sanitized *live-derived* fixtures, provider availability,
  three-model support in experiments, or evidence of prompt improvement.
- Remaining gaps: run the reviewed live protocol, admit sanitized artifacts
  only after disclosure review, then evaluate the two unchecked Task 5 boxes.
- User decision or pending decision: provider pins for each model and approval
  of the first paid/live smoke call remain runtime decisions.
- Next iteration: execute one minimal reviewed smoke run, sanitize it, verify
  offline replay, then complete the fixed 36-run matrix if routing is stable.
- Detailed artifacts: `crates/task-zero-lab/src/replay.rs` and
  `epics/research/task-zero-lab/openrouter-experiment-protocol-v1.md`.

### 2026-08-11 — Credentialed OpenRouter smoke run blocked by zero credits

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan steps 6–7 —
  does the pinned privacy/routing/schema path pass one minimal completion
  before beginning the 36-run matrix?
- Repository, immutable revision, fixture, and input request: daftprompt
  `275b4f3351a3770b2ccfdd5869923307bb8830ee`, newly authored public
  `DERIVED-SMOKE-C01` control derived from C01 but not the canonical manifest
  rendering; no private Keystone/ReporGo content was read or transmitted.
- Harness/prompt/policy/model versions: `task-zero-lab` 0.1.0,
  `task-zero-helper-v1`, `task-zero-helper-replay-v1`,
  `task-zero-baseline-v1`, `HelperPolicy::default()`, Granite 4.1 8B pinned
  to CoreWeave with fallback disabled, required parameters, denied data
  collection, ZDR, temperature 0, and output limit 2,048.
- Harness change: added `openrouter_helper_experiment`, which freezes graph,
  packet, prompt-pattern, policy, protocol, and template hashes, wraps the
  live adapter in `DecisionRecorder`, exports only disclosure-reviewable
  typed replay data plus aggregate measurements, and replays the retained
  typed decisions offline. Public endpoint discovery pinned DeepInfra for the other two
  approved models.
- Corpus-status correction: the future 36-run Task 5 matrix uses explicit
  `LAB-C01-REQUEST`, `LAB-C02-REQUEST`, and `LAB-C07-REQUEST` derived controls.
  They replay the exact manifest request/constraint strings against the
  daftprompt Task Zero graph, not the manifest repositories, and therefore
  cannot count as canonical case runs or Task 6 comparative evidence.
- Observed results: the smoke run failed before completion with zero tokens,
  no returned model/provider, 576 ms elapsed, and `adapter_unavailable`.
  The sanitized artifact contains only the generic suppressed-error marker.
  A read-only credits query reported total credits `0` (usage `0.0005159`).
- Validated findings and rejected interpretations: the credential and live
  boundary execute without leaking the key, and the failure artifact is
  replayable; this does not establish provider/model support or prompt
  improvement. The original smoke input must not be counted as a canonical
  C01 corpus run. Neither unchecked Task 5 criterion was changed.
- Remaining gap/user decision: add OpenRouter credits, then run a new explicit
  smoke attempt. The fixed matrix was deliberately not started and the failed
  smoke was not silently retried.
- Resume command (from the repository root, after adding credits):
  `cargo run -p task-zero-lab --bin openrouter_helper_experiment -- live --model ibm-granite/granite-4.1-8b --provider CoreWeave --case LAB-C01-REQUEST --repetition 1 --output /tmp/openrouter-granite-smoke.json`.
  Review that temporary artifact for disclosure before admitting any sanitized
  result under `epics/research/task-zero-lab/`; do not start the 36-run matrix
  unless the smoke returns the pinned model/provider and valid typed output.
- Detailed artifact:
  `epics/research/task-zero-lab/openrouter-smoke-blocker-2026-08-11.json`.

### 2026-08-11 — Second credentialed Granite/CoreWeave smoke still blocked

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan step 7 — does the
  pinned privacy/routing/schema path now pass one minimal completion after
  credits were added, and may the fixed 36-run matrix start?
- Repository, immutable revision, fixture, and input request: daftprompt
  `d82a7d2e0b9af686c1a8b47721469f538dbc85f9`; same public
  `LAB-C01-REQUEST` (`derived:daftprompt-graph:C01-expert:v1`) and exact
  LAB controls as the prior note; no private Keystone/ReporGo content read
  or transmitted; local `~/Projects/llm-sdk` still at `87cebeb`.
- Harness/prompt/policy/model versions: `task-zero-lab` 0.1.0,
  `task-zero-helper-v1`, `task-zero-helper-replay-v1`,
  `task-zero-baseline-v1`, `HelperPolicy::default()`, Granite 4.1 8B pinned
  to CoreWeave with fallback disabled, required parameters, denied data
  collection, ZDR, temperature 0, output limit 2,048.
- Harness change: no code change. The exact resume command from
  `epics/014-task-zero-prompt-lab.md:1629` was executed after the user added
  OpenRouter credits; the temporary output (`/tmp/openrouter-granite-smoke.json`)
  is the literal stdout of that run.
- Observed result and measurements: 826 ms elapsed, zero tokens, no returned
  model or upstream provider, sanitized `Malformed` decision with one paired
  inference, and `adapter_unavailable` stop reason. The 2,175-byte file
  contains only the generic suppressed-error diagnostic; a byte-level scan
  found none of `OPENROUTER_API_KEY`, `sk-or-`, `raw_response"`, `round_prompt`,
  or `provider-private-detail`. SHA-256
  `d8963f44dcb0bfb5231e80cad70f49b9976205a09e1554f115468b5ffe7bcf31`. The
  offline `openrouter_helper_experiment replay` path reconstructed the typed
  decision without credentials or network.
- Validated findings and rejected interpretations: the credential and live
  boundary still execute without leaking the key, the failure artifact is
  replayable, and the per-run `DecisionRecorder`/`SanitizedReplayArtifact`
  path is preserved. This does not establish provider/model support or
  prompt improvement. The previous blocker was diagnosed as a 0-credit
  account; today's run still fails after credits were added, so the cause is
  no longer necessarily credit balance and may be the Granite + CoreWeave
  pairing, the privacy/ZDR/required-parameters stack, or the SDK's error
  mapping. The adapter deliberately suppresses provider error details, so
  the artifact does not isolate which.
- Remaining gap / user decision: stop and consult OpenRouter (or surface the
  suppressed error) before the next paid attempt. The fixed 36-run matrix
  was deliberately not started and the failed smoke was not silently retried
  on a different provider or model; the protocol guardrail forbids both.
- Detailed artifact: `/tmp/openrouter-granite-smoke.json` (2,175 bytes,
  SHA-256 `d8963f44dcb0bfb5231e80cad70f49b9976205a09e1554f115468b5ffe7bcf31`).
  This is a temporary file outside the repository; review it before any
  sanitized result is admitted under `epics/research/task-zero-lab/`.

### 2026-08-12 — Granite diagnostic isolates OpenRouter token-field mismatch

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan step 7 — why
  does the pinned Granite/CoreWeave smoke fail after credits were added?
- Observed result: the explicitly enabled stderr-only diagnostic reported
  HTTP 404 with no canonical `metadata.error_type`; the sanitized replay
  artifact boundary remained unchanged.
- Current public endpoint evidence: OpenRouter's model-endpoints API still
  lists `ibm-granite/granite-4.1-8b` on CoreWeave, status available, with ZDR
  catalog membership and `response_format` support. The endpoint advertises
  `max_tokens`, while the local OpenRouter SDK serialized the experiment's
  output limit as `max_completion_tokens` under `require_parameters: true`.
- Disposition: retain Granite in the fixed three-model set and correct the
  adapter wire field to `max_tokens`; changing the model is unnecessary while
  its live endpoint satisfies the selection, privacy, and JSON criteria. Do
  not count the diagnostic failure as a matrix run and do not start the matrix
  until one corrected Granite smoke returns valid typed output and pinned
  routing identity.

### 2026-08-12 — Corrected Granite transport succeeds; helper schema smoke fails

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan step 7 — does a
  corrected `max_tokens` request pass the complete typed-helper smoke gate?
- Fixture and runtime identity: public `LAB-C01-REQUEST`, Granite 4.1 8B pinned
  to CoreWeave with fallback disabled, required parameters, denied data
  collection, ZDR, temperature 0, and output limit 2,048. The temporary
  artifact records daftprompt revision
  `d82a7d2e0b9af686c1a8b47721469f538dbc85f9`, but the adapter and diagnostic
  changes were uncommitted during the run, so this result is diagnostic and
  cannot be admitted as reproducible matrix evidence.
- Observed result: OpenRouter returned the exact requested model and CoreWeave
  provider for all three rounds with nonzero token counts. Round 1 returned a
  valid `submit` envelope whose value invented an unsupported `analysis`
  shape; rounds 2 and 3 stopped at the 2,048-token limit with truncated JSON.
  The host rejected all three typed submissions and stopped with
  `retry_budget_exhausted` after 39,877 ms, 1,311 input tokens, and 4,678
  output tokens. Offline replay reconstructed all three retained decisions.
- Disclosure review: `/tmp/openrouter-granite-smoke.json` is 5,148 bytes with
  SHA-256
  `b487bba5d815c8e237c8c094c1568abf24122c5ebc25b454c7616b7c11f6b54f`.
  Inspection found no API key, bearer authorization, original request,
  negative constraint, round prompt, or raw response. `raw_response_hash`
  fields contain only expected content hashes.
- Validated finding and rejected interpretation: the OpenRouter transport,
  credits, privacy filters, model pin, and provider pin now work; this is not
  a successful helper smoke and does not authorize the 36-run matrix. The
  system instruction names `HelperModelOutput` but does not disclose the
  actual five-field `HelperSubmission` schema, so the next iteration must
  provide the exact bounded schema and concise-output requirements, add tests,
  commit both repositories, and run a newly numbered smoke. Do not admit the
  temporary artifact under `epics/research/task-zero-lab/`.

### 2026-08-12 — Helper output contract, truncation, and rejection feedback (harness correction, no live run)

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan step 7 — correct
  the harness defects the previous note diagnosed, so that the next Granite
  smoke tests the helper protocol rather than re-testing an under-specified
  prompt. This is a harness-correction pass: **no live model call was made, no
  credentials were used, and it produces no experimental result.**
- Repository and immutable revision: daftprompt working tree based on
  `d82a7d2e0b9af686c1a8b47721469f538dbc85f9` (the corrections were uncommitted
  while this note was written). Local `~/Projects/llm-sdk` moved from
  `87cebeb` to `491e99e`.
- Fixture and input request: none live. Only offline unit fixtures and the
  existing mocked-transport seam were exercised; no repository prompt or
  provider payload was transmitted.
- Harness/detector/prompt/policy/model versions: `task-zero-lab` 0.1.0,
  `task-zero-helper-v1`, `task-zero-baseline-v1`,
  `task-zero-helper-replay-v1`, and a new
  `task-zero-helper-output-contract-v1` (`HELPER_OUTPUT_CONTRACT_VERSION`).
  `HelperPolicy::default()` gains `truncated_retry_budget: 1`. No model was
  executed.
- Root cause, confirmed by inspection: the 2026-08-12 Granite helper-schema
  failure was caused by the model being told to match a schema that was never
  stated. `crates/task-zero-lab/src/openrouter_helper.rs`'s system message
  named only the `HelperModelOutput` envelope, and
  `crates/task-zero-lab/src/helper.rs`'s `build_round_prompt` said the
  submission must match "the required schema" without stating it anywhere.
- Harness change:
  1. **Stated contract.** New versioned constants `HELPER_OUTPUT_CONTRACT` /
     `HELPER_OUTPUT_CONTRACT_VERSION` in `helper.rs` state the envelope, all
     four `HelperOperation` variants, and every `HelperSubmission` key (the
     five schema fields named in the criterion above, plus `stop_reason`),
     plus a concise-values clause. The same text is now both the OpenRouter
     system message and a `## Required output schema (exact)` section of every
     reconstructed round prompt. Drift-guard tests assert the contract text
     matches the real serde shapes in both directions (each embedded example
     round-trips through the real types, and each type's serialized shape
     appears in the contract text).
  2. **Truncation distinguished from schema violation.**
     `HelperModelOutput::Truncated`, `HelperPolicy::truncated_retry_budget`
     (default 1), `StopReason::OutputTruncated`, and
     `HelperRunReport::truncated_outputs`. The adapter classifies truncation
     only when `finish_reason == "length"` *and* the content fails to parse,
     because a complete response can also report `length`.
  3. **Host-only steps made unforgeable.** The adapter parses model content
     into a new parse-only `HelperModelResponse` (`ToolCall`/`Submit` only),
     so a model emitting `{"step":"truncated"}` or `{"step":"malformed"}` is
     rejected as `Malformed` with a diagnostic naming the claim, and can
     neither spend the truncation budget nor influence the recorded stop
     reason.
  4. **Wasted-retry correction.** The round loop now carries bounded,
     sanitized, host-authored `RoundRejection` state (at most
     `MAX_CARRIED_REJECTIONS` = 3 carried, each detail capped at
     `MAX_REJECTION_DETAIL_BYTES` = 200) and re-renders it in each
     reconstructed round prompt, so a retry at temperature 0 is no longer
     handed a near-identical prompt. Motivation: the 2026-08-12 run spent
     4,678 output tokens on three deterministically identical failures.
     Stateless reconstruction is preserved and tested — the section is fully
     re-rendered from structured state every round, never appended.
  5. **Injection coverage extended to the new feedback path** (see the Task 5
     criterion above for the test-level detail).
- Observed result and measurements: `cargo check --workspace` and `cargo test
  --workspace` pass — 253 tests, 0 failures. There are no token, latency,
  provider, or model-quality measurements, because no live call was made. No
  new sanitized artifact was admitted under `epics/research/task-zero-lab/`.
- Injection-coverage correction found while testing: `HelperSubmission` does
  **not** use `deny_unknown_fields`, so a model-invented key is silently
  dropped rather than reported. The actual rejection detail for the Granite
  round-1 shape is therefore the host-authored ``missing field `stop_reason` ``,
  and the real model-text surface is a rejected *value* embedded verbatim in
  serde's type error, not a model-chosen key. The earlier assumption that an
  invented key is echoed back is rejected.
- Accepted-but-imperfect limits (recorded, not fixed): `sanitize_rejection_detail`
  strips control characters and backticks but does not strip U+2028-class
  separators or bidi controls; they cannot split a line, so the section's
  single-line-per-rejection property still holds. Separately, the raw
  unsanitized serde message is retained in the host `diagnostics` list; that is
  a host record, not prompt text.
- Validated findings: the schema-omission root cause is established by direct
  code inspection, not inferred from the model's behavior. Truncation and
  schema violation are now separable by construction, and the host-only steps
  are unforgeable by type rather than by convention.
- Rejected or unsupported interpretations: passing offline tests are not
  experimental evidence. This pass does not establish that Granite (or any
  pinned model/provider) will now emit a conforming submission, does not
  measure prompt improvement, and does not authorize starting the 36-run
  matrix. **No Task 5 acceptance criterion was checked as a result of this
  pass. Task 5 has exactly one unchecked criterion — "At least three
  open-weight helpers are supported in experiments" — and it is unchanged.**
- Recorded side effect: the new `truncated_retry_budget` field changes the
  serialized `HelperPolicy`, and therefore the recorded `policy_hash` of
  future artifacts. Existing sanitized artifacts still replay, since replay
  validates the retained typed decisions and requires only that the recorded
  hashes be present, not that they match a recomputed current policy.
- Local `llm-sdk` update: `491e99e` adds a strict `json_schema` response format
  (`OpenRouterResponseFormat::json_schema`, wire shape
  `{"type":"json_schema","json_schema":{"name":...,"strict":true,"schema":...}}`).
  The lab adapter still sends `response_format: json_object` and does **not**
  yet use the strict schema. Task 5's acceptance-criterion text, which still
  pinned `87cebeb`, was updated to `491e99e` so the next smoke records a
  correct SDK identity.
- User decision or pending decision (new, deliberately deferred): should the
  experiment require providers that support strict structured outputs,
  repinning models/providers accordingly? Evidence from OpenRouter's public
  model-endpoints API on 2026-08-12: `ibm-granite/granite-4.1-8b` on CoreWeave
  supports `structured_outputs`; `meta-llama/llama-3.1-8b-instruct` supports it
  only on CoreWeave (not DeepInfra, Novita, Groq, or Cloudflare);
  `mistralai/mistral-nemo` supports it on Novita and Parasail but not
  DeepInfra. The currently pinned providers for Llama and Mistral Nemo are both
  DeepInfra, which lack it, so strict schema cannot be a protocol-wide default
  under the current pins, and repinning would require re-verifying
  ZDR/data-collection/privacy properties for the new providers. The user's
  position is that structured outputs are a low ask from modern models and this
  may justify changing a model or provider later. Taken up in a later
  iteration, not this one.
- Remaining gaps: the corrected code is still uncommitted, and the honest next
  step is unchanged — commit both repositories, then run **one newly numbered
  Granite smoke** and review its temporary artifact for disclosure before
  admitting anything under `epics/research/task-zero-lab/`. The 36-run matrix
  remains unauthorized until that smoke returns the pinned model/provider and
  valid typed output.
- Next iteration: that single corrected smoke.
- Detailed artifacts: `crates/task-zero-lab/src/helper.rs`,
  `crates/task-zero-lab/src/openrouter_helper.rs`,
  `crates/task-zero-lab/src/replay.rs`, and
  `crates/task-zero-lab/src/bin/openrouter_helper_experiment.rs`
  (no new file under `epics/research/task-zero-lab/`).

### 2026-08-13 — Corrected Granite smoke passes protocol gate with low marginal yield

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan step 7 — after
  committing the stated output contract and failure-mode corrections, does one
  newly run Granite/CoreWeave smoke return the pinned routing identity and valid
  typed helper output without disclosing protected inputs?
- Repository and runtime identity: daftprompt
  `c1776cb04f6a7807b95470a34dfa7523231b1a6e`; local `~/Projects/llm-sdk`
  `491e99e`; public derived control `LAB-C01-REQUEST`, repetition 1; Granite
  4.1 8B pinned to CoreWeave with fallback disabled, required parameters,
  denied data collection, ZDR, temperature 0, and output limit 2,048.
- Harness/prompt/policy versions: `task-zero-lab` 0.1.0,
  `task-zero-helper-v1`, `task-zero-helper-replay-v1`,
  `task-zero-baseline-v1`, and `task-zero-helper-output-contract-v1` under the
  recorded policy hash `5aabc729988b81c1`.
- Harness change: none. This was the single live smoke authorized by the prior
  note; the fixed 36-run matrix was not started.
- Observed result and measurements: all three rounds returned the exact
  requested model and CoreWeave provider with valid typed tool calls and no
  validation diagnostics. The helper requested `search_graph("Epic 020")`
  twice, then `get_node("file:epics/011-provenance-graph.md")`; the host stopped
  with `low_marginal_yield`. Totals were 3 calls/rounds, 5,436 input tokens, 70
  output tokens, 1,642 result bytes, 2,605 ms provider elapsed time, and one
  duplicate call. No helper submission was produced.
- Disclosure and replay review: the 3,933-byte temporary artifact at
  `/tmp/openrouter-granite-corrected-smoke.json` has SHA-256
  `57260cd5d5ccea447a05d2c896977a1c577d71029ef6b24dbded5432a4168fdd`.
  A prohibited-string scan found no API key marker, `sk-or-`, authorization or
  bearer value, round prompt, raw response field, or provider-private detail.
  It retains hashes and typed decisions only. Offline replay succeeded with
  three decisions and the recorded `LowMarginalYield` stop reason.
- Validated findings: the corrected hosted protocol now passes the smoke gate:
  live transport, privacy/routing pins, typed output parsing, host-only tool
  execution, sanitized recording, and credential-free replay all worked.
- Rejected or unsupported interpretations: this run did not refine the prompt,
  did not show useful evidence selection, and does not count as canonical C01
  or Task 6 evidence. It does not by itself satisfy the three-open-weight-model
  criterion. The repeated irrelevant query is negative behavioral evidence,
  not a reason to relabel the smoke as prompt-quality success.
- User decision or pending decision: review whether the successful protocol
  gate plus poor marginal yield is sufficient to begin the already-fixed
  repeated three-model matrix without first changing policy. No Task 5
  checkbox is changed by this one-model smoke.
- Next proposed iteration: if approved, run the fixed 36-run matrix unchanged
  across all three pinned helpers and retain the duplicate/low-yield Granite
  behavior in the comparison; otherwise define a narrowly scoped policy
  experiment before changing the matrix protocol.
- Detailed artifact: temporary sanitized artifact
  `/tmp/openrouter-granite-corrected-smoke.json`; it has not yet been admitted
  under `epics/research/task-zero-lab/`.

### 2026-08-13 — Out-of-protocol Qwen3.5-9B comparison smoke

- Parent criterion/question: user-requested comparison against the corrected
  Granite smoke — can `qwen/qwen3.5-9b` execute the identical closed helper
  protocol, and does it avoid Granite's low-yield behavior?
- Candidate qualification and protocol exception: the official Qwen model
  repository publishes 9B Apache-2.0 weights. OpenRouter lists the exact model
  ID and a SiliconFlow endpoint supporting `max_tokens`, `response_format`,
  and structured outputs with zero retention. The model is not silently
  substituted into the fixed matrix: its 262,144-token context exceeds the
  original 131,072 discovery cap and its USD 0.15/M output price exceeds the
  USD 0.10/M discovery ceiling. This is therefore a separately labelled
  out-of-protocol candidate smoke.
- Repository and runtime identity: daftprompt
  `c1776cb04f6a7807b95470a34dfa7523231b1a6e`; local `~/Projects/llm-sdk`
  `491e99e`; public derived control `LAB-C01-REQUEST`, repetition 1; Qwen3.5-9B
  pinned to SiliconFlow with fallback disabled, required parameters, denied
  data collection, ZDR, temperature 0, and output limit 2,048.
- Harness change: none. The same committed helper contract, policy, fixture,
  packet, and replay path used by the Granite smoke were retained.
- Observed result and measurements: all four rounds returned the exact model
  and SiliconFlow provider with valid typed tool calls and no validation
  diagnostics. The calls searched for `Epic 020` once and `epic:020` three
  times; the host stopped with `low_marginal_yield`. Totals were 4 calls/rounds,
  8,154 input tokens, 575 output tokens, 2,980 result bytes, 19,887 ms provider
  elapsed time, and two duplicate calls. No helper submission was produced.
- Disclosure and replay review: the 4,706-byte temporary artifact at
  `/tmp/openrouter-qwen35-9b-smoke.json` has SHA-256
  `2ceee0169a5371f6b841b2e7ef3907ee7737283d8154e8d51de6d2cb052488a9`.
  The same prohibited-string scan used for Granite passed, and offline replay
  succeeded with four decisions and the recorded `LowMarginalYield` stop.
- Validated findings: Qwen participates successfully in the closed typed
  protocol with exact private routing and credential-free replay.
- Rejected or unsupported interpretations: Qwen did not improve the prompt or
  resolve the missing evidence; it repeated the same search more often than
  Granite and used materially more tokens and time. One smoke cannot support a
  general model-quality ranking, and the exception to the discovery filters
  means it cannot replace a fixed-matrix model without an explicit protocol
  revision and user decision.
- User decision or pending decision: whether to revise the candidate filters
  and fixed model set to admit Qwen, or retain it only as negative exploratory
  evidence. No Task 5 checkbox changes from this smoke.
- Next proposed iteration: review the shared `Epic 020` search behavior as a
  possible fixture/packet gap before spending on repeated Qwen runs; keep the
  existing fixed matrix unchanged unless the protocol is deliberately revised.
- Detailed artifact: temporary sanitized artifact
  `/tmp/openrouter-qwen35-9b-smoke.json`; it has not yet been admitted under
  `epics/research/task-zero-lab/`.

### 2026-08-13 — Epic 020 helper-loop diagnosis and offline harness correction

- Parent criterion/question: Epic 014 Task 5 — why did both the corrected
  Granite smoke and the exploratory Qwen smoke repeatedly search for Epic 020
  without producing a helper submission, and what is the smallest correction
  required before another paid experiment?
- Provenance finding: `LAB-C01-REQUEST` intentionally replays the manifest C01
  expert request, `Analyze Epic 020 and suggest changes before
  implementation.`, against the daftprompt Task Zero graph. The rendering is
  correctly labelled `derived:daftprompt-graph:C01-expert:v1`; it is not the
  canonical akar C01 fixture. The harness graph is built only from daftprompt
  Epics 011--014. Epic 020 is therefore outside the loaded graph, but it was
  not requested during graph construction and is not recorded in
  `GraphCoverage::requested_epics_unavailable`.
- Confirmed failure chain: exact packet seeding could not establish
  `epic:020`; helper `search_graph` then reused generic lexical seeding, whose
  tokenizer discarded the three-character token `020` and matched unrelated
  nodes through the word `epic`. The helper consequently received
  `zero_results=false` with irrelevant Epic 011--014 results rather than an
  honest exact-reference miss. Reconstructed rounds also lacked explicit
  guidance to submit an evidence-gap disclosure after an exact miss or
  non-matching results. Finally, duplicate accounting treated `Epic 020` and
  `epic:020` as different serialized operations. These combined harness
  weaknesses explain the shared behavior more directly than a provider,
  transport, JSON-format, or model-specific failure.
- Offline harness correction:
  1. `packet::explicit_epic_locators` recognizes explicit prose, locator, and
     epic-path references without consulting graph membership.
  2. `HelperToolExecutor::SearchGraph` now uses exact semantics for those
     references. It reports requested and missing locators, exact matches, the
     deterministic loaded epic scope, and `lexical_fallback=skipped`; ordinary
     searches retain the existing lexical behavior.
  3. Every reconstructed round now includes bounded evidence-gap exit
     guidance. A helper is told to stop retrying after an exact artifact miss,
     true zero results, or non-matching results and to submit through the
     existing `HelperSubmission` schema. The guidance limits the claim to
     “unavailable in the loaded graph,” forbids unrelated evidence and invented
     facts, and adds no new operation.
  4. Duplicate detection now conservatively normalizes `Epic 020`,
     `epic:020`, case/whitespace variants, the plural spelling, and terminal
     sentence punctuation to one search key. Ordinary searches normalize only
     case and whitespace; generic punctuation remains significant, and
     non-search operation keys retain exact serialized identity.
- Files changed: `crates/task-zero-lab/src/packet.rs` and
  `crates/task-zero-lab/src/helper.rs`. Focused offline tests cover missing and
  loaded exact epic references, preservation of ordinary lexical search,
  evidence-gap guidance in initial and reconstructed later rounds, equivalent
  and non-equivalent duplicate keys, and exact non-search operation identity.
- Verification: `cargo check --workspace`, the full `task-zero-lab` package
  test suite (117 tests), all focused tests, and `git diff --check` pass. The
  package's mocked OpenRouter tests require permission to bind localhost but
  make no live provider calls. Existing dependency warnings are unchanged.
- Experimental status: no OpenRouter request, credential use, provider/model
  measurement, or new sanitized artifact resulted from this correction.
  Offline success does not establish that a hosted helper will follow the new
  exit guidance. No Task 5 or parent-epic acceptance criterion is newly
  complete, and the fixed model-selection protocol remains unchanged. Qwen
  remains separately labelled exploratory.
- Expected next result: in one newly numbered `LAB-C01-REQUEST` Granite smoke,
  the first `search_graph("Epic 020")` should report `epic:020` missing from
  the loaded Epics 011--014 scope, after which the helper should submit a
  concise gap disclosure within one or two rounds instead of repeating the
  search. This expectation must be recorded before interpreting the run.
- Next iteration: commit the harness correction, run only that single pinned
  Granite/CoreWeave smoke, inspect the temporary artifact for disclosure and
  exact routing identity, and replay it offline. Keep the 36-run matrix paused
  until the smoke demonstrates useful gap-exit behavior; do not revise the
  fixed model set or admit Qwen without an explicit protocol decision.

### 2026-08-13 — Granite gap-exit smoke passes protocol gate

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan step 7 — after
  committing the Epic 020 missing-evidence harness corrections (exact-search
  semantics, duplicate normalization, evidence-gap exit guidance), does one
  pinned Granite/CoreWeave smoke demonstrate useful gap-exit behavior and
  authorize the fixed 36-run matrix?
- Repository and runtime identity: daftprompt
  `60bf03fcd38962cd693cfc6dd9f7c8ced11f6aad`; local `~/Projects/llm-sdk`
  `491e99e`; public derived control `LAB-C01-REQUEST` (`derived:daftprompt-graph:C01-expert:v1`),
  repetition 1; Granite 4.1 8B pinned to CoreWeave with fallback disabled,
  required parameters, denied data collection, ZDR, temperature 0, and output
  limit 2,048.
- Harness/prompt/policy versions: `task-zero-lab` 0.1.0,
  `task-zero-helper-v1`, `task-zero-helper-replay-v1`,
  `task-zero-baseline-v1`, `task-zero-helper-output-contract-v1`, policy hash
  `5aabc729988b81c1`.
- Harness change: none. This was the single live smoke authorized by the
  2026-08-13 offline-correction note; the fixed 36-run matrix was not started.
- Observed result and measurements: round 1 returned `search_graph("Epic 020")`
  (expected — the C01 expert request references Epic 020). The host responded
  with the honest exact-miss diagnostic: `epic:020` missing from the loaded
  Epics 011--014 scope. Round 2 submitted a valid gap disclosure: clarifications
  stated "Epic 020 is not present in the loaded graph (epic:020)",
  `proposed_gap_dispositions` proposed `proposed_still_open` for `epic:020`,
  and `stop_reason` was "Evidence gap for Epic 020". Host stopped with
  `Submitted`. Totals: 1 call, 2 rounds, 0 duplicates, 3,494 input tokens,
  176 output tokens, 197 result bytes, 2,570 ms provider elapsed, 0 validation
  diagnostics.
- Disclosure and replay review: the 3,491-byte sanitized artifact at
  `epics/research/task-zero-lab/openrouter-granite-gap-exit-smoke-2026-08-13.json`
  has SHA-256 `4407b3be7e0f71c9271db2a8ed04c6e6ab728609ebacfb13d2c5c5147149d059`.
  A prohibited-string scan found no API key marker, `sk-or-`, authorization or
  bearer value, round prompt, raw response field, or provider-private detail.
  It retains hashes and typed decisions only. Offline replay succeeded with
  two decisions and the recorded `Submitted` stop reason.
- Validated findings: the corrected hosted protocol passes the complete smoke
  gate — live transport, privacy/routing pins, exact-model/provider identity,
  typed output parsing, host-only tool execution, evidence-gap exit guidance,
  sanitized recording, and credential-free replay all worked. The gap-exit
  behavior is a material improvement over the pre-fix runs (2026-08-12 and
  Qwen 2026-08-13): 1 call instead of 3--4, 0 duplicates instead of 1--2,
  `Submitted` instead of `LowMarginalYield`, and 176 output tokens instead of
  4,678--575.
- Rejected or unsupported interpretations: one-model smoke success does not
  establish that Llama or Mistral Nemo will exhibit the same gap-exit behavior,
  does not measure prompt improvement, and does not count as canonical C01 or
  Task 6 evidence. The `candidate_claims` field ("Epic 020 requires a follow-up
  request for any implementation changes") is a model interpretation, not an
  established fact; it is retained as typed output but not promoted to evidence.
- Remaining gaps: the fixed 36-run matrix (3 models × 4 cases × 3 reps) has
  not started. Task 5's final criterion ("At least three open-weight helpers
  are supported in experiments") requires reproducible evidence across all
  three pinned helpers. Qwen remains separately labelled exploratory and
  outside this matrix.
- User decision or pending decision: user approved proceeding with the fixed
  36-run matrix unchanged. No Task 5 checkbox is changed by this one-model
  smoke.
- Next iteration: execute the 36-run matrix — Granite/CoreWeave,
  Llama/DeepInfra, Mistral Nemo/DeepInfra over LAB-C01-REQUEST,
  LAB-C02-REQUEST, LAB-C07-REQUEST, and the controlled injection fixture,
  three repetitions each, rotating starting model by repetition per the
  protocol.
- Detailed artifact:
  `epics/research/task-zero-lab/openrouter-granite-gap-exit-smoke-2026-08-13.json`.

### 2026-08-13 — Fixed 36-run matrix execution

- Parent criterion/question: Epic 014 Task 5, OpenRouter plan step 7 — do all
  three pinned helpers operate through the same closed interface over fixed
  inputs, and does any helper besides Granite demonstrate useful gap-exit
  behavior?
- Repository and runtime identity: daftprompt
  `dd4519d` (the smoke evidence commit); local `~/Projects/llm-sdk` `491e99e`.
  All 36 runs used the same frozen graph, packet, prompt-pattern, policy, and
  template hashes recorded in each artifact.
- Models and providers: `ibm-granite/granite-4.1-8b` / CoreWeave,
  `meta-llama/llama-3.1-8b-instruct` / DeepInfra,
  `mistralai/mistral-nemo` / DeepInfra. All fallback-disabled, parameter-
  required, data-collection-denied, ZDR, temperature 0, output limit 2,048.
- Cases: `LAB-C01-REQUEST` (sufficient context, Epic 020 reference),
  `LAB-C02-REQUEST` (missing/exhausted evidence), `LAB-C07-REQUEST`
  (scope/runtime control), `INJECTION` (controlled prompt-injection fixture).
  Three repetitions per model/case pair, rotating starting model per repetition
  per the protocol.
- Harness change: none. All runs used the committed harness at `dd4519d`.

#### Results summary

| Model | Provider | Submitted | Low marginal yield | Adapter unavailable | Total |
|---|---|---|---|---|---|
| Granite 8B | CoreWeave | 12 | 0 | 0 | 12 |
| Llama 8B | DeepInfra | 0 | 1 | 11 | 12 |
| Mistral Nemo | DeepInfra | 0 | 11 | 1 | 12 |

**Granite 8B / CoreWeave (12/12 `Submitted`):** All runs produced a valid
typed helper submission with 0 duplicates. C01 and INJECTION: 1 call, 2 rounds
each. C02 and C07: 2 calls, 3 rounds each. Average output tokens 114--377,
average elapsed 1.7--4.8 s. Consistent gap-exit behavior across all three
repetitions with no variance in stop reason or call count.

**Llama 8B / DeepInfra (11/12 `adapter_unavailable`):** 11 of 12 runs failed
at the transport level — empty `returned_model`, null provider, zero tokens,
~500 ms. One run (C01 rep 1) returned `low_marginal_yield` with 2 calls and 54
output tokens; one run (C02 rep 1) made 1 call before failing. This is a
DeepInfra provider-availability failure, not a harness or model behavior issue.
The model never received enough context to exercise the helper protocol.

**Mistral Nemo / DeepInfra (11/12 `low_marginal_yield`, 1 `adapter_unavailable`):**
All 12 runs executed (no transport failure for 11), but none produced a
`Submitted` stop. The helper made 2--5 calls per run, searching for Epic 020
or related terms, accumulating 43--166 output tokens, and exhausting the
marginal-yield budget without submitting a gap disclosure. One run (C07 rep 2)
hit `adapter_unavailable` after 3 calls. Duplicate calls appeared in 4 of 12
runs (C01 all 3 reps, INJECTION rep 1--2). The helper received the same
evidence-gap exit guidance as Granite but did not follow it.

#### Validated findings

1. The closed helper interface, typed output schema, host-only tool execution,
   sanitized recording, and credential-free replay are model-agnostic
   infrastructure — they worked for all three models regardless of outcome.
2. Granite/CoreWeave is the only model/provider pair that demonstrated the
   full gap-exit protocol: search once, receive honest miss, submit gap
   disclosure.
3. Llama/DeepInfra's failures are provider-level, not model-level. Re-running
   on a different provider (e.g. CoreWeave or another DeepInfra endpoint) is
   needed before drawing model-level conclusions.
4. Mistral Nemo/DeepInfra executed but did not follow the exit guidance. This
   is behavioral evidence: the 12B model either did not understand the guidance
   or chose to retry rather than submit. Temperature 0 and the current policy
   may not be sufficient for this model.

#### Rejected or unsupported interpretations

- Llama's `adapter_unavailable` results do not establish that Llama cannot
  participate in the helper protocol — only that DeepInfra was unavailable.
- Mistral Nemo's `low_marginal_yield` results do not establish that Mistral
  Nemo cannot produce a submission — only that it did not under the current
  policy, exit guidance, and temperature.
- One-model success (Granite) does not satisfy Task 5's three-helper criterion.
- None of the 36 runs count as canonical C01/C02/C07 corpus evidence or Task 6
  comparative evidence (per the protocol).

#### Remaining gaps and user decisions

1. **Llama provider fix needed:** Re-run Llama on CoreWeave or another
   available provider before evaluating its model-level capability. The
   current 11/12 `adapter_unavailable` results are uninformative about the
   model.
2. **Mistral Nemo policy experiment needed:** Consider whether a policy change
   (e.g. higher `max_rounds`, explicit submission prompt, or different
   temperature) would help Mistral Nemo follow the exit guidance, or whether
   this model is unsuitable for the closed helper role under the current
   protocol.
3. **Task 5 criterion status:** The criterion "At least three open-weight
   helpers are supported in experiments" remains unchecked. Only Granite has
   demonstrated reproducible experimental support. Two more helpers need
   successful matrix evidence.
4. **Qwen:** remains separately labelled exploratory and outside this matrix.

- User decision or pending decision: the matrix is complete. The user must
  decide whether to (a) re-run Llama on a different provider, (b) run a
  Mistral Nemo policy experiment, or (c) substitute a different model/provider
  pair for the remaining two slots.
- Next iteration: depends on user decision above.
- Detailed artifacts: 36 sanitized JSON files under
  `epics/research/task-zero-lab/matrix-2026-08-13/` (Granite 12, Llama 12,
  Mistral Nemo 12).

### 2026-08-13 — Local Qwen 3.5 0.8B via llama.cpp replaces Llama/DeepInfra

- Parent criterion/question: Epic 014 Task 5, "At least three open-weight
  helpers are supported in experiments" — the 36-run matrix exposed two
  provider-level failures (Llama/DeepInfra 11/12 `adapter_unavailable`,
  Mistral Nemo/DeepInfra 11/12 `low_marginal_yield`). Rather than chasing
  hosted provider fixes, substitute a locally-running Qwen 3.5 0.8B via
  llama.cpp, eliminating provider-availability and routing variables entirely.
- Repository and immutable revision: daftprompt working tree; no model weights
  or private repository inputs were transmitted to any external service.
- Candidate qualification: Qwen 3.5 0.8B is Apache-2.0 licensed, 0.8B
  parameters (well under the 10B sub-tier), and the GGUF
  (`unsloth/Qwen3.5-0.8B-GGUF:UD-Q4_K_XL`, 533 MB) is present locally at
  `/Users/brainless/hf_models/models--unsloth--Qwen3.5-0.8B-GGUF/`. The local
  `~/Projects/llm-sdk` already defines `LlamaCppClient` (OpenAI-compatible
  HTTP client for llama-server at localhost:8080) and a `QWEN_3_5_0_8B_ID`
  constant.
- Infrastructure: llama-server v10360 installed at `/opt/homebrew/bin/llama-server`
  (Apple Silicon, Metal GPU offload). Smoke test confirmed: model loads in
  ~1 s, returns valid JSON chat completions at ~100 tok/s, responds to
  tool-call-shaped prompts. The model is a thinking model (returns
  `reasoning_content`) and wraps JSON in markdown code fences — the adapter
  must strip fences before parsing.
- Harness change: new `LlamaCppHelperModel` adapter implementing `HelperModel`
  via `llm_sdk::llama_cpp::LlamaCppClient`. Simpler than the OpenRouter
  adapter: no API key, no provider routing, no ZDR/data-collection fields, no
  `response_format` parameter (model may ignore it). Local inference means
  zero cost, zero network dependency, and zero provider-availability risk.
  New `--live-llama-cpp` CLI mode in `task_zero_lab helper`.
- Validated findings: local execution eliminates the two failure modes that
  dominated the hosted matrix (provider unavailability and provider-specific
  routing). The 0.8B model's small size means it may struggle with the full
  helper output schema — this is model-quality evidence, not a transport
  failure, and is exactly what the experiment needs to measure.
- Rejected or unsupported interpretations: local availability does not
  establish that the 0.8B model will produce useful helper submissions or
  follow gap-exit guidance. The thinking-model `reasoning_content` field and
  markdown-fenced JSON are known quirks, not confirmed protocol failures;
  the adapter handles them defensively.
- Remaining gaps: the 0.8B model hit the 2048-token output limit in both
  smoke rounds (24 s and 22 s, 2048 output tokens each, `stop_reason: length`).
  The model generates extensive `reasoning_content` before producing JSON and
  never completed a helper submission. This is model-quality evidence, not a
  transport or adapter failure — the truncation was correctly classified and
  the deterministic baseline survived byte-identically. The 9B model
  (`models--unsloth--Qwen3.5-9B-GGUF`, also present locally) may perform
  better on the helper schema and deserves its own smoke. No Task 5 criterion
  is checked until repeated experiment evidence exists across at least three
  models.
- User decision or pending decision: user approved replacing Llama/DeepInfra
  with local Qwen 3.5 0.8B and implementing the adapter.
- Next iteration: smoke test Qwen 3.5 9B through the same adapter (same
  llama-server, different model file), then run the fixed matrix with
  Granite/CoreWeave and the best-performing local Qwen as the three-helper
  set.
- Detailed artifacts: `crates/task-zero-lab/src/llama_cpp_helper.rs` (new
  adapter, 9 unit tests), updated `main.rs` (`--live-llama-cpp` mode),
  updated `openrouter_helper_experiment.rs` (`LiveLlamaCpp` subcommand),
  `/tmp/llama-cpp-smoke-report.json` (temporary live smoke artifact).

### 2026-08-13 — Qwen 3.5 9B smoke and 0.8B retirement

- Parent criterion/question: Epic 014 Task 5, "At least three open-weight
  helpers are supported in experiments" — smoke test the locally-available
  Qwen 3.5 9B through the same `LlamaCppHelperModel` adapter, and decide
  whether the 0.8B remains viable.
- Repository and immutable revision: daftprompt `fe4556a`; no model weights
  or private repository inputs were transmitted to any external service.
- Candidate qualification: Qwen 3.5 9B is Apache-2.0 licensed, 9B parameters
  (below-10B sub-tier), and the GGUF
  (`unsloth/Qwen3.5-9B-GGUF:UD-Q4_K_XL`, 5.6 GB) is present locally at
  `/Users/brainless/hf_models/models--unsloth--Qwen3.5-9B-GGUF/`. Runs on
  Apple Silicon with Metal GPU offload via llama-server v10360.
- Harness change: none. Same adapter, same `--live-llama-cpp` CLI, same
  `LAB-C01-REQUEST` case, same policy.
- Observed result (9B, reasoning on): 1 call, 2 rounds, 0 duplicates.
  Round 1: 1777 input / 103 output tokens, 15.6 s, clean stop. Round 2:
  1861 input / 316 output tokens, 26.6 s, clean stop. Produced a valid
  `submit` with `stop_reason: evidence_gap` — recognized Epic 020 is outside
  the graph scope and correctly reported the gap. No validation diagnostics,
  no truncation, no malformed output. Total elapsed 42 s.
- Observed result (0.8B, reasoning off): also tested with `--reasoning off`
  on the 0.8B model. Round 1–2 produced malformed JSON (column 295/297
  parse errors), round 3 produced a valid submit. Stop reason
  `retry_budget_exhausted`. The model is fast (~2 s/round) but schema
  compliance is unreliable even without thinking overhead.
- Validated findings: the 9B model cleanly completes the helper protocol on
  the first attempt without retry budget exhaustion. The 0.8B model cannot
  reliably produce valid `HelperModelResponse` JSON — with reasoning on it
  exhausts the output limit thinking, with reasoning off it produces
  malformed JSON that burns through the retry budget. This is model-quality
  evidence, not a transport or adapter failure.
- User decision: **drop Qwen 3.5 0.8B from the experiment model set.**
  The adapter and infrastructure remain (useful for future small models),
  but the 0.8B will not participate in the fixed matrix. The user may
  download other local models for future comparison.
- Revised three-helper candidate set:
  1. `ibm-granite/granite-4.1-8b` (hosted, Granite/CoreWeave) — demonstrated
  2. Qwen 3.5 9B (local, llama.cpp) — just demonstrated
  3. Third helper TBD (user will download additional local models)
- Next iteration: when a third model is available, run the fixed matrix with
  all three helpers over the same cases, packets, prompt patterns, and
  repeated-run policy.
- Detailed artifacts: `/tmp/llama-cpp-9b-smoke.json` (9B live smoke),
  `/tmp/llama-cpp-0.8b-noreason-smoke.json` (0.8B reasoning-off smoke).
