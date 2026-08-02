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

- [ ] Git runs record the resolved commit, tree/blob identities, parents, and
  whether the working tree differs from the requested revision.
- [ ] Non-Git fixtures use deterministic xxh3 content identities and separate
  content identity from filesystem mtime and observation time.
- [ ] Index coverage reports available source partitions, supported languages,
  index/database identity, and any detectable staleness or version mismatch.
- [ ] Historic runs never silently read current file content as if it belonged
  to the pinned revision.
- [ ] Dirty or unindexed inputs are included only by explicit fixture policy and
  remain visibly labelled.
- [ ] Root, merge, rename, deletion, and ordinary modification fixtures expose
  honest Git evidence; uncertain rename detection degrades to delete/add.
- [ ] Snapshot inspection performs no mutation, checkout, network call, or
  dependency installation.
- [ ] Tests use local fixtures and pass without credentials.

### Task 2: Extract the minimal epic and repository evidence graph

Parse the selected project documents and repository/index evidence into the
experimental nodes, edges, candidates, provenance records, coverage, and gaps.

#### Acceptance Criteria

- [ ] Markdown headings, task nesting, checkboxes, acceptance criteria, explicit
  dependencies, paths, symbols, commands, and cross-epic references are
  extracted deterministically with source spans.
- [ ] Epic/task/criterion nodes retain the exact document version and stable
  logical locator; line number alone is not treated as stable identity.
- [ ] Unchecked criteria become explicit unresolved gaps without implying why
  they remain unchecked.
- [ ] Project rules from `AGENTS.md`, `DEVELOP.md`, and repository configuration
  retain source precedence and provenance.
- [ ] Existing index results reuse their canonical `(source_type, identifier)`
  identity and do not duplicate indexed text as graph truth.
- [ ] Git commit/file/version facts come from revision-pinned Git evidence.
- [ ] Established edges, lexical/semantic candidates, helper proposals, and
  human assumptions serialize into distinct fields.
- [ ] Every edge can be explained without rerunning its detector.
- [ ] Unknown relation/detector values survive fixture loading as diagnostics.
- [ ] Repeated extraction from identical inputs produces byte-stable normalized
  JSON apart from explicitly excluded run timing fields.

### Task 3: Implement bounded graph selection and context packets

Given a request and graph snapshot, select a small relevant subgraph and render
a machine-readable evidence packet with explicit coverage and gaps.

#### Acceptance Criteria

- [ ] Exact epic/task/criterion/path/symbol references seed retrieval before
  fuzzy channels.
- [ ] Search is source-stratified across instructions, epics/research,
  documents, code, and commits where available.
- [ ] Expansion uses an allowlist of relation kinds, maximum depth, item count,
  excerpt bytes, and total packet bytes.
- [ ] Parent/shared epic constraints are retained when selecting an individual
  task.
- [ ] Duplicate resources and overlapping excerpts are deduplicated without
  losing provenance paths.
- [ ] Established evidence and candidates remain separate in the packet.
- [ ] Coverage reports queried sources, unavailable sources, detector/index
  versions, budget omissions, and remaining gaps.
- [ ] A helper's proposed gap disposition cannot remove a deterministic host
  gap.
- [ ] Repeated identical queries over an identical graph produce an identical
  normalized packet.
- [ ] The LOG-019 fixture reproduces the known distinction between useful
  observations and rejected requirement/test/history interpretations.

### Task 4: Generate deterministic coding-agent prompts

Build the model-free baseline renderer that converts the original request and
context packet into a detailed Markdown prompt.

#### Acceptance Criteria

- [ ] A bare daftprompt request such as “continue closing the Task 0 blockers”
  resolves to the relevant Epics 011–013 criteria and research rather than all
  repository text.
- [ ] Original intent and negative constraints are copied verbatim into an
  immutable section before any refinement.
- [ ] The prompt follows the output contract in Design Constraint 5.
- [ ] Every repository claim in the prompt has a compact provenance reference;
  unsupported suggestions are labelled as suggestions.
- [ ] The renderer reports stale index/input mismatches and never hides missing
  coverage behind polished prose.
- [ ] Verification commands come from exact repository instructions or
  configuration, not from language-name guessing.
- [ ] Prompt length obeys a configured byte/token estimate budget and records
  omitted lower-ranked context.
- [ ] Identical request, graph, packet, and template versions produce identical
  prompt text.
- [ ] Golden fixtures cover implementation, diagnosis, review, research,
  ambiguity, and no-mutation requests.
- [ ] Generated prompts are exportable for manual handoff; automatic coding-
  agent dispatch is absent.

### Task 5: Add stateless helper refinement behind a closed interface

Evaluate whether selected small/tiny helpers improve the deterministic prompt.
Each helper decision is a fresh host-reconstructed request and may only request
typed, bounded, read-only lab operations.

#### Acceptance Criteria

- [ ] The deterministic baseline remains fully usable when helper execution is
  disabled or unavailable.
- [ ] The initial catalog contains only named graph/search/explain/coverage
  operations with validated repository/revision scope; no arbitrary path,
  shell, URL, network, or write tool is exposed.
- [ ] Helper output conforms to a schema that separates refinements,
  clarifications, selected evidence, candidate claims, and proposed gaps.
- [ ] Human intent, negative constraints, and deterministic gaps cannot be
  overwritten by model output.
- [ ] Each round reconstructs one self-contained prompt from host state rather
  than appending an unbounded transcript.
- [ ] Calls, rounds, tokens, result bytes, elapsed time, duplicate requests, and
  validated marginal yield are bounded and recorded.
- [ ] Prompt-pattern exemplars are trust-labelled, versioned reference material
  and cannot supply repository facts or expand capabilities.
- [ ] At least three open-weight helpers are supported in experiments,
  including at least two below 10B parameters and one sub-20B tier model.
- [ ] Model/provider code uses the local `~/Projects/llm-sdk` source boundary;
  recorded replay fixtures require no credentials or network access.
- [ ] Malformed output, prompt injection, early stop, over-calling, and false
  completion preserve the deterministic baseline and produce diagnostics.

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
