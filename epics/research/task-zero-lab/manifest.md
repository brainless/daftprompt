# Task Zero Lab Case Manifest

Epic 014, Task 0 deliverable. This manifest maps the real cases already
required by Epics 011–013 to immutable repository revisions, sanitized human
requests, known practical relevance sets, expected graph facts, and parent
acceptance criteria. It is the concise index; full narrative detail for each
case remains in `epics/research/011-provenance-thought-experiments.md` and
`epics/research/012-planner-thought-experiments.md`, which this file
references rather than duplicates.

This manifest is itself a research/documentation artifact. It authorizes no
implementation work in Epics 011–013 (all Task 1+ boxes there remain
unchecked; `crates/` contains only `daftprompt-indexer`, confirmed
2026-08-02) and no Task 1+ work in this epic beyond what is explicitly listed
as done elsewhere.

## 1. Repository/revision manifest

All revisions below were re-resolved against the local clones on 2026-08-02
with `git cat-file -e <rev>`; every one exists and is immutable. Five
repositories are represented (more than the required minimum of three).

| Repository | Local path | Role in corpus |
|---|---|---|
| `akar` | `~/Projects/akar` | Cross-model epic review (C01), task-by-task epic implementation (C09), commit-mechanics root/deletion/modification fixtures |
| `Keystone` | `~/Projects/Keystone` | Client testing-sheet row retrieval and bounded-helper run (C02) — private, sanitized |
| `ReporGo` | `~/Projects/ReporGo` | Multi-file task-split epic implementation (C03) — private domain, sanitized |
| `dwata` | `~/Projects/dwata` | Requirement implementation, Git-rationale removal, bug investigation x2 (C04–C07) |
| `nocodo` | `~/Projects/nocodo` | Multi-file Tree-sitter/index implementation (C08), rename commit-mechanics fixture |

Non-daftprompt repositories are used as read-only evidence sources for this
research artifact; no code in this repository depends on their contents.

## 2. Case index

Case IDs are new to this manifest; each links back to its originating
research-artifact experiment so provenance is traceable in both directions.

| Case | Repository | Immutable revision(s) | Source experiment | Real or reconstructed prompt |
|---|---|---|---|---|
| C01 | akar | `febfa42e747ee6f5b64f7c2f0549f9b82d1babaa` (original), `9334431bbb37dd0db20f4b80c660387c62cf34bc` (revised) | 011 Experiment 1 | Real |
| C02 | Keystone | `5f4562688fd7dcf2101b88cf05c7d6e4bf0d22a4` | 011 Experiment 2 (LOG-019 replay) | Real, sanitized row |
| C03 | ReporGo | `ee38009^` (planning input state), implementation commits `ee38009`…`14e6601` | 011 Experiment 3 | Reconstructed (epic text back-dated to `ee38009^` as documented; implementation history is real) |
| C04 | dwata | reconstructed pre-session state; post-work `2e504f8`, `33a3882`, `d0db807` | 011/012 Experiment 4A | Real |
| C05 | dwata | reconstructed pre-session state; post-work `1928d38`, `6d1d2fc` | 011 Experiment 4B | Real |
| C06 | dwata | reconstructed pre-session state; post-work `79dc8bc` | 011 Experiment 4C | Real |
| C07 | dwata | reconstructed pre-session state; post-work `8165cd2` | 011 Experiment 4D | Real |
| C08 | nocodo | reconstructed pre-session state (parent `72b5070`, inferred); post-work `8c320c3` | 011 Experiment 4E | Real |
| C09 | akar | checkout `0a381f8ebd434b5308fe7d73ea068dd10ed8e841`; task commits `99233d8`, `e85c639`, `8b0ca1a`, `2de8be8` | 011 Experiment 4F | Real, exact session metadata |

Cases 4A–4E have no recorded checkout SHA in the original OpenCode sessions;
their pre-versions are reconstructed from the last pre-session commit and the
parent of the matching post-work commit, as already disclosed in
`011-provenance-thought-experiments.md`. This is a known provenance gap, not
a defect introduced by this manifest.

Additional real-history fixtures exist for narrower, non-prompt-driven
acceptance criteria (Git changed-file mechanics, document/section identity)
and are listed separately in §6 because they do not carry a human request and
therefore do not count toward Epic 013's twelve-case corpus.

## 3. Per-case detail

Each entry gives: sanitized intent, mutation constraints, expert/non-expert
renderings, known practical relevance set with its evidence, and expected
graph facts. "Non-expert rendering" is newly authored for this manifest
(marked NEW) where the original corpus only captured the expert-level
real-world phrasing; it is a deliberately underspecified version of the same
underlying request, for the accessibility comparisons required by Epic 013
Task 0 and Epic 014 Task 6.

### C01 — akar: cross-model review of Epic 020

- **Intent:** read-only planning review of a model-authored implementation
  epic, followed by an explicit request to revise and commit it.
- **Mutation constraints:** review phase is read-only; revision phase is
  explicitly authorized by a follow-up request, not implied by the review.
- **Expert rendering (real):** "Analyze Epic 020 and suggest changes before
  implementation."
- **Non-expert rendering (NEW):** "Can someone check epic 020 is okay before
  we build it?"
- **Known practical relevance set:** Epic 020 original blob
  `git-sha1:cff16996884d251b8d4ddc3bbae54b1cc75d506e` and revised blob
  `git-sha1:49878280500dfdbb61a707ae4879723369c13848`; mentioned symbols
  `glyphon::FamilyOwned`, `akar_button`/`akar_badge`/`akar_navbar` C wrappers,
  paragraph text-measurement code, Epic 019 prerequisite. **Evidence:** the
  committed document diff between the two blobs incorporates exactly these
  review findings (011 Experiment 1, "Deterministic evidence review").
- **Expected graph facts:** `contains(repo, Epic020)`; two `graph_node_versions`
  for one logical document with `predecessor_version_id` linking revised →
  original; `changes` edge from commit `9334431` to the document; exact
  symbol/path mentions inside the original version resolving to component
  source files. Review authorship (MiMo/Codex) and the review text itself are
  **not** establishable from the graph — an explicit expected gap.
- **Coverage properties:** sufficient initial deterministic context (no tool
  call needed to find the review-relevant symbols); multi-turn
  implementation/review planning; history dependence (predecessor version
  chain).
- **Maps to unchecked criteria:** Epic 011 Task 0 bullets "schema, detector
  list, task boundaries revised from findings" (partially — `reviews`,
  `derived_from`, `revises`, `supersedes` relations were added, but the
  research artifact still needs final review/commit) and "research artifact
  reviewed and committed"; Epic 012 Task 0 bullets "at least one real
  cross-model plan review experiment" (satisfied in content, unchecked
  pending overall Task 0 closure) and "8 real prompts... documented" (this is
  1 of the required 8); Epic 013 Task 0 (all bullets unchecked — this is 1 of
  the required 12 cases and covers "multi-turn implementation or review
  planning").

### C02 — Keystone: LOG-019 testing-sheet row (private, sanitized)

- **Intent:** examine one client-reported testing-sheet row; decide reject,
  defer, clarify, or implement; cross-check against PRD, code, tests, and
  history.
- **Mutation constraints:** read-only investigation; no mutation is
  authorized until a human/product decision selects an implementation path.
- **Expert rendering (sanitized, real structure):** "Examine LOG-019: when an
  admin filters the review queue, exporting produces an empty file. Expected
  the export to contain the filtered rows. Cross-check the PRD and earlier
  decisions, determine relevant implementation and tests, and decide whether
  to implement."
- **Non-expert rendering (NEW):** "Export doesn't work right on the filtered
  queue, can you look into LOG-019?"
- **Privacy:** the source CSV is Git-ignored and never committed raw. Its
  identity is recorded only as content hashes: local path
  `misc_documents/Sri_Testing_Test_Log.csv`, size 11,498 bytes, Git blob hash
  `99a1f135b4de4a9d537ee7e9df56115f412dfd07`, SHA-256
  `2871d581df184d001f0aa38629181eee18d3f636817094036b38e47aa6338d46`, row key
  `LOG-019 / 2026-07-23`. The row text used in prompts is a sanitized
  behavioral abstraction, never the private original (011 Experiment 2,
  "Real Keystone LOG-019 replay").
- **Known practical relevance set:** the row; `PRD.md` §5.4
  (blob `00a009f82728930d7dabafd921b8eee720123937`); the global route
  (blob `cb0e3d1ed0771920cb46dab6b82a5119e38ad282`); `OverdueCertBanner`
  (blob `2d3bfbb4e043cb7cf020b739d29412bdbd6cfa1a`); history commits
  `91ab196`, `c06a7ad`, `ce96e3a`; cross-check blob
  `3e7608182aad6e708e4add0736323a34058fa917`; candidate test
  `e2e/tests/dashboard/overdue-banner.spec.ts`
  (blob `1444d2c08b57ccb5eb78cde3d921ec116b9a7a2a`). **Evidence:** this set is
  the historical capable-agent cross-check result at Keystone commit
  `5f4562688fd7dcf2101b88cf05c7d6e4bf0d22a4`, already independently
  classified per-artifact in the source experiment (validated/rejected table).
- **Expected graph facts:** row → PRD section (mention); PRD section → route
  symbol (mention, contested — the actual PRD requirement was later judged
  narrower than the route/banner pairing); route → `OverdueCertBanner`
  (definition/containment); commits `91ab196`/`c06a7ad`/`ce96e3a` → changed
  files (structural); candidate test → banner (test-target, but scope
  mismatch: unit dashboard coverage, not the global-page workflow).
- **Coverage properties:** missing implementation/test/history context;
  conflicting decision sections (PRD §5.4 vs. actual behavior); exhausted
  graph coverage (no upstream Google Sheets lineage, no history tool call
  made by the helper even though Git could answer it); helper stops too
  early (GPT-OSS run stopped after 3 of 5 allowed calls and wrongly reported
  zero gaps).
- **Maps to unchecked criteria:** Epic 011 Task 0 "research artifact reviewed
  and committed"; Epic 012 Task 0 "at least one real per-row testing-sheet
  experiment" is `[x]` already, "at least one experiment records
  deterministic context-gap triggers... investigation yield, stop/escalation
  behavior" is `[x]` already — this case is the evidentiary basis for both,
  and remains needed to close the still-open Epic 012 Task 0 bullets ("8
  prompts... documented", "models/rules/tasks revised", "research artifact
  reviewed and committed"); Epic 013 Task 0 (all bullets unchecked) — covers
  "missing... context", "exhausted graph coverage", and "a helper that stops
  too early".

### C03 — ReporGo: multi-file task-split epic (private domain, sanitized)

- **Intent:** implement Epic 001 (contact-ownership refactor) task by task,
  from a deterministically split shared introduction plus ten tasks.
- **Mutation constraints:** none stated beyond the epic's own explicit
  non-goals (parser, prompt, SDK, authentication, and frontend-redesign
  scope are out of bounds).
- **Expert rendering (reconstructed, matches epic structure):** "Implement
  Epic 001. For the shared introduction and each task, retrieve the focused
  repository evidence needed to implement it."
- **Non-expert rendering (NEW):** "We need contacts to own the messages and
  reports instead of matching phone numbers to conversations — can you make
  that change across the backend and frontend?"
- **Privacy:** the domain is a WhatsApp/medical-reports product; no raw
  patient, phone-number, or message content is reproduced here or in the
  source experiment — only schema/table/field names and commit identities.
- **Known practical relevance set:** the actual per-task implementation
  commits and their changed-file lists, already enumerated per task in 011
  Experiment 3 (e.g. Task 1 `ee38009` touched 36 migration files replacing
  the chain with one schema file and four seed files; Task 3 `497b9be`
  changed seven backend files beyond the two headline paths; Tasks 7/8 have
  no standalone commit — substantially absorbed into the baseline and Task 3
  changes). **Evidence:** direct `git diff`/`git show --stat` comparison of
  each task commit against the epic's task text, at parent revision
  `ee38009^`.
- **Expected graph facts:** epic → task containment; task → exact
  mentioned tables/fields/paths (mention, requested-change claim, **not**
  authoritative `changes`); commit → changed files (structural); no
  compiler-grade reference graph, so downstream SQL/type consumers not
  literally named in the task text are an expected miss (documented per-task
  in the source experiment for Tasks 2, 3, 9, 10).
- **Coverage properties:** multi-file work; history dependence (already-landed
  prerequisite tasks change what remains relevant to later tasks, e.g. Task 5
  only touched `messaging.rs` because schema/type work had landed earlier);
  runtime-only discoveries (SQLx offline metadata and a medical-query fix
  only appeared in a later, unplanned commit — Task 10).
- **Maps to unchecked criteria:** Epic 011 Task 0 "at least one real
  multi-file implementation epic... queried at an exact pre-implementation
  repository version, and compared with files/symbols actually changed" and
  "the multi-file experiment distinguishes requested future changes from
  observed graph facts, overlapping task scope from exclusive ownership, and
  static retrieval from runtime/build-only discoveries" — both currently
  unchecked despite substantial prose evidence, because no executable fixture
  exists yet (Tasks 1–8 blocked); Epic 012 Task 0 "at least one multi-file
  epic experiment" (not yet performed in the 012 research artifact — this is
  the identified gap feeding it, see §5); Epic 013 Task 0 — covers "multi-file
  work" and "history dependence".

### C04 — dwata: financial-data-extractor requirement implementation

- **Intent:** implement the financial-data-extractor task from the
  development guide: register `dwata-agents`, implement agent, storage,
  tools, API/config/routes, shared types, and tests.
- **Mutation constraints:** none stated; standard implement-feature intent.
- **Expert rendering (real):** "Implement the financial-data-extractor task
  from the development guide."
- **Non-expert rendering (NEW):** "Add the thing that pulls numbers out of
  financial documents automatically."
- **Known practical relevance set:** commits `2e504f8` (19 files, 889
  insertions), `33a3882`, `d0db807`. **Evidence:** post-work diff against the
  reconstructed pre-session state (last pre-session commit / parent of
  `2e504f8`).
- **Expected graph facts:** task document → existing financial-pattern
  storage (mention); workspace manifest → new `dwata-agents` member
  (configuration); handler/route → configuration key (structural). The task
  text was not committed at prompt time, so it reconciles to a later Git
  blob rather than an earlier one — an explicit non-Git-to-Git identity
  reconciliation case.
- **Expected graph facts (negative control):** generic `Agent`/`Session`/
  `Message`/`execute` lexical matches must not create high-confidence edges
  (false-positive control already documented in 011 Experiment 4A).
- **Coverage properties:** sufficient initial deterministic context (task,
  existing pattern implementation, manifest, handler form a focused packet);
  positive retrieval baseline case.
- **Maps to unchecked criteria:** Epic 012 Task 0 "8 real prompts... at
  least six from prior prompting experience" (this is one of the six-plus
  real ones) and "at least six plans meet every quality threshold"; Epic 013
  Task 0 — covers "sufficient initial deterministic context requiring no
  tool call".

### C05 — dwata: Git-history-based obsolete-binary removal

- **Intent:** use Git history to compare two inspection binaries and remove
  the redundant one if history confirms redundancy.
- **Mutation constraints:** removal is conditional on the history comparison,
  not a blanket delete-on-request instruction.
- **Expert rendering (real):** "Use Git history to compare two inspection
  paths and remove the obsolete binary if redundant."
- **Non-expert rendering (NEW):** "We probably have two copies of the same
  inspector tool, can you check and clean it up?"
- **Known practical relevance set:** commits `1928d38`, `6d1d2fc` (the
  259-line binary plus its `[[bin]]` manifest entry deleted).
  **Evidence:** `git status`/similarity records plus the `6d1d2fc` diff.
- **Expected graph facts:** commit-changed-file `delete` on the binary and
  `modified` on `Cargo.toml`; rename/similarity evidence is explicitly
  low-confidence and must not create a durable `duplicate_of` edge (the
  commit message is attributed rationale, not independent proof of
  equivalence — an explicit rejected-alternative case).
- **Coverage properties:** history dependence (primary driver of the whole
  decision); conflicting-evidence-adjacent (filename/import similarity
  proposes duplication but is not sufficient alone).
- **Maps to unchecked criteria:** Epic 011 Task 0 "commit changed-file
  semantics are tested against at least one merge, root commit, rename,
  deletion, and ordinary modification" (this case supplies the deletion
  instance's human-request context, complementing the pure-mechanics fixture
  in §6); Epic 012 Task 0 "at least one experiment demonstrates missing graph
  evidence and feeds a concrete follow-up" (semantic similarity was
  insufficient by itself — feeds Epic 011 Design Decision 4's corroboration
  requirement); Epic 013 Task 0 — covers "history dependence" and partially
  "conflicting graph evidence" (candidate).

### C06 — dwata: email-content normalization defect

- **Intent:** reproduce an email-cleaning defect with a focused test and
  reconcile CLI/API behavior.
- **Mutation constraints:** reproduce-then-fix; the report explicitly
  preserves a first attempted fix that was reverted, so over-eager mutation
  claims must not be trusted at face value.
- **Expert rendering (real):** "Reproduce an email-cleaning defect with a
  focused test and reconcile CLI/API behavior."
- **Non-expert rendering (NEW):** "Email 1479 looks wrong when I clean it,
  the CLI and the API don't agree — can you fix that?"
- **Known practical relevance set:** commit `79dc8bc` (6 files changed).
  **Evidence:** `git diff` against the reconstructed pre-session state; the
  set includes normalization/cleaning/HTML-fallback helpers, API and
  template/financial callers, and value extraction, per 011 Experiment 4C.
- **Expected graph facts:** CLI entry point → normalization/cleaning
  helpers (definition/reference); helpers → API and financial callers
  (reference); test target → helper (test). Email content and command output
  are runtime evidence, not repository facts, and must not be represented as
  graph nodes.
- **Coverage properties:** bug investigation with tests/tooling dependency;
  a case with a documented failed hypothesis and reverted fix (useful
  negative-result fixture per Design Constraint on retaining rejected
  experiments).
- **Maps to unchecked criteria:** Epic 012 Task 0 "at least two experiments
  demonstrate that an initially attractive action should be removed,
  delayed, or made manual" (the reverted first fix is one instance; C05's
  rejected `duplicate_of` edge is the other); "8 real prompts... documented".

### C07 — dwata: Unicode byte-boundary panic (negative/borderline control)

- **Intent:** fix a Unicode byte-boundary panic in email ranking, given an
  already-located file/line from the panic message.
- **Mutation constraints:** fix plus regression test; scope explicitly
  excludes an unrelated GUI date-request change that shares the same commit.
- **Expert rendering (real):** "Fix the Unicode byte-boundary panic in email
  ranking at `email_ranking/mod.rs::contains_date`."
- **Non-expert rendering (NEW):** "The email ranking thing crashes on some
  emails, can you fix it?"
- **Known practical relevance set:** commit `8165cd2`. **Evidence:** the
  panic already names the exact function; the diff replaces byte slicing
  with character iteration at the reported byte offsets (50 and 100, with
  U+200C crossing the second boundary).
- **Expected graph facts:** the prompt-supplied file/span already resolves
  the fault, so this case is explicitly retained as a **borderline control**
  — it "passes" the material-usefulness bar only if graph/plan retrieval adds
  a focused test or caller beyond the already-known line (011/012 corpus
  conclusion). A later, unrelated GUI date-request change lands in the same
  commit and must not be treated as evidence that the two are related
  (co-change ≠ dependency, explicit false-positive control).
- **Coverage properties:** bug investigation; negative/borderline control for
  "how much does the graph actually help when the human already found the
  bug" — useful for honestly reporting cases where the harness adds little.
- **Maps to unchecked criteria:** Epic 012 Task 0 "8 real prompts...
  documented" and "at least six plans meet every quality threshold" (C07 is
  the corpus's intentional failure/borderline case, so the "six of eight"
  threshold is evaluated with C07 as a likely non-passing case, not silently
  dropped); Epic 013 Task 0 — this is a candidate for "a capable-model
  response that exposes a new context gap" once run through the harness,
  because the co-change/GUI-date confound is exactly the kind of thing a
  capable model might wrongly fold into scope.

### C08 — nocodo: Tree-sitter extractor and identifier index

- **Intent:** design and implement Tree-sitter-based code extraction and a
  persisted identifier index for coding agents: queries, extraction,
  index/reindex, lookup, tests, docs.
- **Mutation constraints:** none stated; standard multi-file implement
  intent, informed by design documents that must be read first.
- **Expert rendering (real):** "Build Tree-sitter extraction and a persisted
  identifier index for coding agents."
- **Non-expert rendering (NEW):** "Can we make the code index remember where
  functions and types are defined so agents don't have to re-search every
  time?"
- **Known practical relevance set:** commit `8c320c3` against reconstructed
  parent `72b5070`. **Evidence:** exact `git diff`/`git show --name-status`;
  the set includes design documents, existing scanner/source discovery,
  manifests, DB/schema precedents, and new extractor/index modules and tests
  (011 Experiment 4E).
- **Expected graph facts:** design doc → existing scanner (mention);
  manifest → new dependency (configuration); new extractor module → SQLite
  DDL (definition). A cross-repository `rustysolid` reference is an explicit
  coverage gap — no cross-repository edge may be created (Epic 011 v1
  non-goal).
- **Coverage properties:** multi-file implementation; runtime-only discovery
  (a build-time Tree-sitter incompatibility and an SQL insertion failure were
  only found by actually building/running the code, not by static
  retrieval).
- **Maps to unchecked criteria:** Epic 011 Task 0 (feeds "schema, detector
  list, task boundaries revised from findings" via the explicit
  cross-repository-evidence non-goal reaffirmation); Epic 012 Task 0 "8 real
  prompts... documented"; Epic 013 Task 0 — covers "multi-file work" and
  contributes to "runtime-only discoveries" alongside C03/C07.

### C09 — akar: Epic 018 task-by-task implementation

- **Intent:** load Epic 018 and its shared context, split into tasks,
  retrieve affected input/core/widget/C-ABI/example/test nodes per task,
  implement and review each task, run focused tests, update status, commit
  separately.
- **Mutation constraints:** explicit per-task commit discipline; review
  gates between tasks are part of the requested workflow, not optional.
- **Expert rendering (real):** "Implement Epic 018 task by task, reviewing,
  testing, updating status, and committing each task."
- **Non-expert rendering (NEW):** "Work through epic 018 a piece at a time
  and commit as you go."
- **Known practical relevance set:** task commits `99233d8`, `e85c639`,
  `8b0ca1a`, `2de8be8` at checkout `0a381f8ebd434b5308fe7d73ea068dd10ed8e841`.
  **Evidence:** exact Codex session metadata (checkout SHA is recorded
  directly, unlike C04–C08) plus per-task `git diff`.
- **Expected graph facts:** epic → task containment; task → winit
  translation, core input/context types, `text_input`/`textarea`, shared edit
  engine, C bridge (mention); task commits → changed files (structural).
  Compilation exposed initially-missed `textarea` and caller work — an
  explicit example of retrieval not being complete on the first pass.
- **Coverage properties:** multi-turn implementation planning (this is the
  strongest available real analogue to Epic 013's "multi-turn implementation
  or review planning" requirement, since it has the most exact provenance of
  any case); overlapping task scope (task boundaries do not imply exclusive
  file ownership); history dependence.
- **Maps to unchecked criteria:** Epic 011 Task 0 (feeds task-boundary
  overlap findings); Epic 012 Task 0 "8 real prompts... documented"; Epic 013
  Task 0 — covers "multi-turn implementation or review planning" most
  directly among the nine cases.

## 4. Coverage matrix against Epic 013's required properties

Epic 013 Task 0 (`epics/013-helper-orchestrator.md`, "Blocking Experiment
Gate") lists fifteen required coverage properties for its twelve-case corpus.
Status legend: **covered** = a documented real case already exhibits this
property from the corpus content alone; **partial** = a case exhibits a
related but not exact instance; **behavior-gap** = the property can only be
observed once a helper/capable-model run actually executes (Epic 014 Tasks
1–6), so no amount of manifest authorship can satisfy it in advance; **gap**
= no existing case exhibits this property and a new case must be sourced or
authored.

| Required property | Status | Case(s) |
|---|---|---|
| sufficient initial deterministic context requiring no tool call | covered | C01, C04 |
| missing implementation/test/config/decision/history context | covered | C02, C05 |
| ambiguous request requiring human clarification | gap | — |
| capable-model response exposes a new context gap | behavior-gap | candidate: C07 |
| capable-model incorrectly asks for unavailable/unnecessary tooling | behavior-gap | — |
| helper over-calls tools | behavior-gap | — |
| helper stops too early | covered | C02 (GPT-OSS run stopped after 3/5 calls, wrongly reported zero gaps) |
| conflicting graph evidence | partial | C02 (PRD §5.4 vs. actual behavior), C05 (similarity ≠ duplication) |
| prompt injection / untrusted repository content | gap | — |
| exhausted graph coverage, no tool call can answer | partial | C02 (upstream Sheets lineage unavailable) |
| multi-turn implementation or review planning | covered | C01, C09 |
| comparison of one capable model against >1 helper model | behavior-gap | — |
| vague request recoverable through helper clarification | gap | — |
| verbose but poorly structured request → smaller clearer prompt | gap | — |
| case preserving genuine ambiguity (must not over-specify) | gap | — |
| polished helper prompt that changes the human's intended outcome | behavior-gap | — |

Six properties are genuine content gaps (no case sourced yet); five are
behavior-gaps that cannot be pre-filled by this manifest because they are
properties of a helper's *conduct* during a run, not of a case's static
content — they become measurable only after Epic 014 Tasks 1–6 exist and are
executed against real or scripted helpers. Recording them here as open,
rather than fabricating a plausible-sounding outcome, is the honest result
required by Design Constraint 1 and the epic's guardrail against manufacturing
a passing conclusion.

## 5. Gap analysis: reaching the twelve-case corpus

Nine cases (C01–C09) are documented above, all real (C03 uses a
back-dated-epic reconstruction already disclosed in its source experiment;
the rest are directly real). Epic 013 Task 0 requires **at least twelve**
cases across at least three repositories with the coverage above. This
manifest therefore identifies, rather than invents, what is still needed:

1. **At least three more cases** purely to reach the numeric floor of
   twelve, ideally each also closing one of the six pure content gaps in §4
   (ambiguous request, injection, vague/clarification-recoverable request,
   verbose/poorly-structured request, genuine-ambiguity-preserving request).
   A single well-chosen case can plausibly satisfy more than one of these
   properties at once (e.g. a genuinely ambiguous, verbosely-phrased real
   request is both "ambiguous" and "verbose but poorly structured").
2. **Injection coverage is the highest-priority net-new gap.** None of the
   nine corpus repositories were searched in this pass for a real case where
   repository content (a comment, commit message, or document) contains
   text that could be misread as an instruction. This likely needs to be
   authored as a controlled fixture (e.g. an injected string inserted into a
   disposable worktree of an existing corpus repository) rather than found,
   since it is inherently an adversarial test case; Design Constraint and
   Epic 013 precedent do not require it to be organically real, only
   realistic.
3. **The five behavior-gap properties cannot be closed by this manifest.**
   They require Epic 014 Tasks 1–5 (harness, packet, prompt renderer, helper
   integration) to exist and be run at least once per case before they can be
   marked covered. This manifest records them as explicitly open rather than
   asserting they are satisfied.
4. Candidate additional repositories already available locally and not yet
   used: `admin-gui`, `pi`, `pixlie`, `rustysolid`, `SmartCrawler`, and
   others listed by `ls ~/Projects/`. None have been inspected for suitable
   real cases in this pass; doing so is the concrete next step recorded in
   the Experiment Notes entry below, consistent with the epic's iterative
   operating model (a later iteration, not this one, should mine them).

No case in this gap list has been fabricated with invented content; each gap
is stated as an open requirement, per Design Constraint 1's ban on
premature/plausible-sounding architecture and the epic's ban on manufacturing
a passing conclusion.

## 6. Non-prompt mechanics fixtures (not part of the twelve-case corpus)

These support Epic 011 Task 0's Git-mechanics and document-identity
acceptance criteria directly; they carry no human request and therefore do
not count toward Epic 013's twelve-case corpus, but they remain part of the
overall Task Zero lab corpus because Epic 011 Task 0 criteria depend on them.

| Kind | Repository/revision | Fact |
|---|---|---|
| root commit | akar `311e7d6` | no parent; adds `LICENSE`; must compare against the empty tree |
| merge commit | dwata `f14ac5c` | parents `90f8e7f9`, `070cbe83`; parent-relative change sets, first-parent default view |
| rename | nocodo `18bb8b4` | `DBDeveloperPage.tsx` → `DatabasePage.tsx`, 99% similarity |
| deletion | akar `3e17af8` | deletes `CLAUDE.md`; close current version, retain logical node/history |
| modification | akar `13d7692` | modifies `stat.rs` and demo `main.rs`; new immutable blob on stable path |
| Git-backed document identity | akar Epic 020 (see C01 blobs) | one logical document, two immutable blob versions |
| non-Git document identity | ephemeral local `guide.md` (not repository-pinned; xxh3 series recorded in 011 Experiment 6) | touch vs. content-change vs. content-restore observation semantics |
| ambiguous duplicate headings | synthetic scenario (011 Experiment 6) | two `## Configuration` sections plus an inserted third; no repository revision pin, deliberately synthetic |

The last two rows have no immutable repository revision to pin (one is
ephemeral local state, one is a synthetic scenario) and are flagged here
rather than silently included in §1's revision table.

## 7. Metrics defined for corpus evaluation

Epic 014 Task 0 requires the manifest to define, at minimum, the following
metrics for use across Tasks 4–6. None have been measured yet — no
extraction, packet, prompt, or helper code exists (Tasks 1–5 are unstarted;
this manifest is Task 0 only). Each metric's definition and the case field it
reads from are recorded so a later iteration can wire measurement without
re-deriving the taxonomy.

| Metric | Definition | Primary source |
|---|---|---|
| Artifact recall | fraction of a case's known practical relevance set (§3 per case) returned by a retrieval/prompt variant | §3 "known practical relevance set" |
| Irrelevant context | count/byte share of returned context not in the practical relevance set | packet output vs. §3 relevance set |
| Context bytes | total bytes of context included in a generated prompt/packet | packet/prompt output |
| Capable-model tokens | prompt + completion token count for the capable-model turn | model call record |
| Wall time | elapsed time from request to final output, per variant | run timing |
| Prompt quality | intent preservation, provenance labelling, coverage/gap disclosure, budget compliance — checkable from prompt text alone (Design Constraint 7) | prompt text |
| Intent preservation | whether original intent/mutation constraints (§3 per case) survive unchanged into the rendered prompt | §3 "intent"/"mutation constraints" vs. prompt text |
| Gap recall | fraction of the case's known/planted context gaps (§4 per-property table, §3 "expected graph facts" negative controls) that the packet/helper disclosed | §3/§4 documented gaps |
| Duplicate calls | count of repeated identical or near-identical tool/graph calls within one run | helper call trace |
| Unsupported claims | count of prompt or capable-model statements not traceable to established evidence or explicitly labelled as a suggestion | prompt/response text vs. established evidence |
| Task outcome | build/verification pass, artifact-set precision/recall against §3's relevance set, negative-constraint compliance, measured in a disposable worktree per Design Constraint 7 | downstream agent diff (Task 6 only) |

Per-case baseline values for "known practical relevance set" (needed for
artifact recall and task-outcome scoring) are already recorded in §3 for all
nine cases, since that evidence pre-dates this manifest and required no new
harness code to establish (it is direct `git diff`/`git show` inspection
against already-completed historical work).

## 8. Task 0 acceptance-criterion self-check

See the corresponding checkboxes in `epics/014-task-zero-prompt-lab.md`,
Task 0, for the authoritative status. Summary:

- Existing Epic 011/012 cases are included (§2–§3) and gaps to Epic 013's
  twelve-case corpus are identified (§4–§5), including which are pure
  content gaps versus behavior-gaps that cannot be pre-filled.
- Five repositories are represented by immutable, re-verified revisions
  (§1) — more than the required three.
- Private inputs (Keystone C02, ReporGo C03) are sanitized to content
  hashes/structural facts; no raw client or patient content appears here or
  in the referenced research artifacts.
- Each case records intent, mutation constraints, and both an expert and a
  newly-authored non-expert rendering (§3).
- Each case records its known practical relevance set and the evidence that
  established it (§3, mostly direct historical `git diff` comparison).
- Coverage of the nine required Epic 011 Task 0 categories (positive
  retrieval, missing evidence, ambiguity, conflict, injection, exhausted
  coverage, history dependence, multi-file work, runtime-only discoveries) is
  uneven: positive retrieval, missing evidence, history dependence,
  multi-file work, and runtime-only discoveries are covered; ambiguity and
  injection are explicit content gaps (§4); conflict and exhausted coverage
  are partial. This is recorded honestly rather than overstated.
- Metrics are defined for all eleven required measurements (§7), with
  explicit acknowledgment that none are yet measured because no harness code
  exists.
- Every case is mapped to specific unchecked Task 0 criteria in Epics
  011–013 (§3, "Maps to unchecked criteria" per case).
- No production implementation task in Epics 011–013 has begun: `crates/`
  contains only `daftprompt-indexer` (re-verified 2026-08-02), and every
  Task 1+ checkbox in Epics 011, 012, and 013 remains unchecked.

## 9. Hosted helper discovery artifacts

Task 5's credential-free OpenRouter discovery run and upstream model
verification are recorded separately from the prompt-case corpus because they
describe experimental model selection, not an additional human-request case:

- `openrouter-candidates-2026-08-11.json` archives the normalized public
  catalog result, exact filters, command, candidates, and explicitly
  non-authoritative parameter-size inferences.
- `openrouter-model-verification-2026-08-11.md` records authoritative upstream
  size evidence, public weight availability, and licenses for the selected
  two 8B helpers and one 12B helper.
- `openrouter-experiment-protocol-v1.md` freezes the cases, inputs, three-run
  repetition policy, sanitization boundary, replay admission, and measurements
  for the upcoming live comparison.

Neither artifact contains credentials, completion payloads, private repository
content, or evidence that a later OpenRouter route/provider will be available.
