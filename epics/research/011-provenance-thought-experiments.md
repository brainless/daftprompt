# Epic 011 Provenance Thought Experiments

This artifact records real repository/prompt cases used to revise Epic 011.
It is intentionally cumulative. Task 0 remains incomplete until the full
required corpus and acceptance criteria in the epic are satisfied.

## Experiment 1: Cross-model review of an implementation epic

### Repository and real workflow

Repository: `/Users/brainless/Projects/akar`

Workflow:

1. MiMo authored `epics/020-component-webpage-sample.md`.
2. The original epic was committed.
3. Codex was asked to analyze the epic and suggest planning changes.
4. The review initially existed only in chat.
5. Codex revised the epic and the revision was committed.

This is a recurring planning workflow: use different models to cross-check a
plan before implementation so omissions and incorrect assumptions surface
early.

### Exact observed versions

Logical document:

```text
epics/020-component-webpage-sample.md
```

Original version:

```text
commit: febfa42e747ee6f5b64f7c2f0549f9b82d1babaa
blob:   git-sha1:cff16996884d251b8d4ddc3bbae54b1cc75d506e
time:   2026-07-29T14:37:03+05:30
subject: epic(020): component-based webpage sample — rebuild using akar components
```

Revised version:

```text
commit: 9334431bbb37dd0db20f4b80c660387c62cf34bc
blob:   git-sha1:49878280500dfdbb61a707ae4879723369c13848
time:   2026-07-29T15:39:10+05:30
subject: docs: revise component webpage epic
```

Git proves that these blobs are two immutable versions of one path-level
logical document. It records Sumit Datta as Git author/committer. It does not
prove that MiMo generated the first version or Codex generated the review and
revision; that attribution came from the user conversation and was not stored
in the repository.

### Prompt intent

```text
Analyze Epic 020 and suggest changes before implementation.
```

Intent: read-only planning review followed, in the actual workflow, by an
explicit request to revise and commit the epic.

### Required nodes

- repository `akar`;
- Epic 020 logical document;
- original and revised Epic 020 versions;
- individual Epic 020 sections/tasks;
- Epic 019 prerequisite and its implementation commits;
- mentioned Rust component symbols and files;
- affected C ABI functions and generated header declarations;
- component isolation and screenshot tooling;
- saved Codex review, if exported;
- review request/prompt, if exported.

### Required edges and evidence

```text
repository --contains(structural)--> Epic 020
Epic 020 graph_node_versions includes original blob (git tree)
Epic 020 graph_node_versions includes revised blob (git tree)
revised version predecessor_version_id --> original version
review --reviews(exact input metadata)--> Epic 020
review graph_artifact_inputs.input_version_id --> original version
review --derived_from(explicit host metadata)--> review prompt
review finding --mentions(exact span/symbol)--> component or C ABI symbol
commit 9334431 --changes(git diff)--> Epic 020
```

The initial Epic 011 relation catalog lacked `reviews`, `derived_from`,
`revises`, and `supersedes`. This experiment added them and added a normalized
artifact-to-exact-input-version boundary.

### Deterministic evidence review

The original epic mentioned exact symbols and paths sufficient to select a
small review corpus without repository-wide discovery:

- `glyphon::FamilyOwned` does not implement `Copy`, contradicting the proposed
  `TextStyle: Copy`.
- Appending `Option` arguments changes Rust function signatures and therefore
  breaks existing call sites.
- Existing `akar_button`, `akar_badge`, and `akar_navbar` C wrappers lacked the
  proposed styling inputs.
- Existing navbar combined slot construction, root-style mutation, and paint,
  exposing a construct/compute/paint lifecycle problem for the proposed card.
- Paragraph wrapping required intrinsic text measurement so wrapped height
  could participate in Taffy layout.
- New components required explicit demo isolation registry work.
- Pixel-identical MiMo verification required a fixed viewport and saved
  baseline, not a qualitative screenshot assertion.

The committed revision incorporated these findings and expanded them into
component lifecycle, text measurement, akar-owned typography types, additive
styled APIs, C ABI coverage, deterministic capture, isolation, and explicit
deferrals.

### What the graph could and could not establish

Could establish:

- exact original/revised file content;
- commit times and Git authors;
- document diff and section changes;
- exact symbol/path references;
- repository evidence supporting many review findings.

Could not establish:

- MiMo or Codex authorship;
- the exact Codex review text;
- the review prompt and conversation identifier;
- rejected suggestions that did not enter the revised epic;
- whether matching conclusions from two models were independent.

The correct degraded result is: committed revision lineage is known; review
authorship and unseen claim provenance are missing.

### Design conclusions

1. Git blob IDs are exact version identity, not logical document identity.
2. A review must point to the exact version it examined.
3. Saved reviews are attributed artifacts and their findings are claims.
4. Cross-model agreement is not independent repository evidence.
5. Git author metadata must not be repurposed as model attribution.
6. Chat-only reviews require explicit export or host ingestion.
7. A separate review artifact preserves reasoning, rejected suggestions, and
   disagreements that disappear from a revised document.
8. Useful optional commit trailers include:

```text
Generated-With: Codex
Reviewed-Version: git-sha1:cff16996884d251b8d4ddc3bbae54b1cc75d506e
Review-Conversation: <opaque ID>
```

### Material usefulness

This case meets the Epic 011 usefulness threshold. Exact/structural retrieval
identified multiple high-value planning defects and their source evidence
without broad repository exploration.

It also produced concrete schema, relation, failure-mode, task, and test changes
in Epic 011. It should be reused by Epic 012 as a cross-review planning fixture.

## Experiment 2: Per-row context retrieval from a client testing sheet

### Repository and real workflow

Repository: `/Users/brainless/Projects/Keystone`

The repository contains a `PRD.md`, implementation epics and other generated
documents, and local Markdown/CSV snapshots downloaded from client-maintained
Google Docs and Sheets. The exact folder names are not semantically important.

The recurring workflow is:

1. Select one row from a downloaded software-testing sheet.
2. Determine whether it describes a defect, requested change, duplicate,
   misunderstanding, stale report, or unresolved product question.
3. Cross-check the row against PRD sections, implementation epics, prior
   decisions, code, tests, configuration, and Git history.
4. Decide whether to reject, defer, clarify, or implement the request.
5. If accepted, add appropriate unit/end-to-end coverage, modify code, and
   verify the result.

The row extraction problem is assumed to be deterministic: CSV records,
Markdown table rows, headers, and cells can be imported as versioned artifacts.
This experiment focuses on whether the graph can retrieve the context needed to
examine one row.

Because the actual client sheet is private, the representative row text in this
artifact is sanitized:

```text
When an admin filters the review queue, exporting produces an empty file.
Expected the export to contain the filtered rows.
```

### Expected focused retrieval

An initial row evidence packet should attempt to surface:

- the exact table snapshot, row, cells, and column meanings;
- duplicate or related testing rows;
- PRD sections governing queue filters, exports, and admin permissions,
  including parent constraints;
- prior epics and decision documents that refine or supersede those sections;
- candidate implementation symbols and focused source spans;
- candidate unit/end-to-end tests and repository verification commands;
- commits and focused diffs changing the candidate files, tests, or documents;
- provenance, confidence, coverage, omissions, and unresolved retrieval gaps.

The desired result is not a claim that every returned resource is relevant or
that all relevant resources were found. It is a high-recall, bounded,
source-linked corpus for a capable model.

### Required nodes and identity

- external table snapshot;
- table and optional sheet/tab;
- row and cells;
- PRD/epic/decision document files and stable sections;
- code symbols, focused code regions, files, tests, configuration, and commits;
- exact versions of every document and table snapshot used;
- optional attributed context-investigation artifacts;
- candidate findings produced by a model-assisted investigation.

Row number is a locator, not stable logical identity. Sorting, insertion, and
deletion can move a row. When the upstream sheet supplies no stable record ID,
row reconciliation must use the table version, canonical columns, content
fingerprint, and neighboring records conservatively. Ambiguous continuity
creates a new row node.

A downloaded file proves only the locally observed snapshot. Google document
IDs, revision history, comments, edit authors, formulas, and prior values are
unknown unless export or host metadata supplies them.

### Retrieval channels and graph edges

Graph traversal alone is insufficient. Initial candidates should be the union
of:

- exact paths, symbols, issue IDs, headings, and explicit cross-references;
- source-stratified FTS and semantic retrieval over rows, document sections,
  code, tests, and commits;
- graph expansion from accepted seeds through containment, definition,
  mentions, tests, changes, versions, and configuration;
- bounded co-change/history expansion;
- deterministic project and test conventions.

Potential graph paths include:

```text
row --mentions(exact/lexical)--> PRD section
PRD section --mentions(exact symbol)--> code symbol
code symbol <--defines(structural)-- file
commit --changes(structural)--> file
test --tests(exact/convention/import evidence)--> symbol or file
decision version --supersedes(version evidence)--> earlier decision
```

A semantic match may seed traversal and remain a candidate in the evidence
packet, but it does not become an authoritative `implements`, `tests`, or
`documents` edge by itself.

### Gaps that a bounded model-assisted investigation can fill

Even after multi-channel retrieval, important links may use unrelated
vocabulary. For example, the row may say “filtered export,” the PRD may say
“active review-queue predicate,” the implementation may use
`build_download_dataset`, and the test may mention “relation constraints.”

The deterministic system can classify explicit coverage gaps such as:

- no implementation path connected to a strong requirement candidate;
- multiple colliding symbols with no deterministic winner;
- no test linked to a candidate behavior;
- a committed code change with no linked decision/rationale document;
- conflicting decision and PRD sections;
- semantic candidates that need focused source inspection.

A host may respond by giving a small/tiny model a short, read-only,
gap-specific investigation with graph-selected seeds and strict tool/output
budgets. Suitable tools include graph query, bounded text/symbol/reference
search, focused file reads, and focused Git log/diff inspection.

Example:

```text
Starting from the supplied review-queue and export candidates, find the code
that turns the active queue filter into export rows. Use at most five read-only
tool calls. Return exact files, symbols, spans, observed references, candidate
relationships, and unresolved ambiguity.
```

The model may propose new resources and relationships. Its output is an
attributed claim artifact, not authoritative graph evidence. Exact cited paths,
symbols, spans, references, and changed-file facts may be promoted only after
the corresponding deterministic validator reproduces them. Interpretations
that cannot be validated remain labelled candidates for the capable model.

### Completeness and degraded behavior

The row evidence packet needs an explicit coverage contract:

```text
source families queried
index/detector versions and availability
established resources and complete evidence paths
semantic/model-proposed candidates
context budget and omitted candidates
unsupported languages or artifact kinds
missing reference/call/runtime evidence
unresolved gaps and exhausted investigations
```

The graph can enumerate its indexed universe, but cannot prove that the selected
packet contains every relevant artifact. An empty edge neighborhood is not
evidence that no implementation or test exists.

Runtime reproduction, product desirability, test-oracle design, implementation,
and verification remain work for the capable model or execution host. The
small/tiny model is strictly a context-retrieval helper.

### What an agent would otherwise rediscover

Without the graph and bounded context enrichment, a capable coding agent
typically performs broad searches for row terminology, reads large PRD/epic
regions, traces code and tests, inspects project commands, and searches Git
history. It may repeat much of this orientation for each row.

The proposed graph reduces that work when one high-confidence seed can expand
to several focused artifacts. The tiny-model helper is valuable when the graph
has useful seeds but deterministic edges stop before the implementation, test,
decision, or rationale needed for the final prompt.

### Failure modes

- treating row position as stable identity;
- silently merging successive downloaded sheets without upstream lineage;
- returning a precise but incomplete graph neighborhood as complete context;
- excluding useful semantic candidates because they cannot become durable
  provenance edges;
- allowing a model-proposed relation to masquerade as deterministic evidence;
- providing a tiny model broad shell access or an unbounded investigation;
- repeatedly investigating the same gap without recording attempts or yield;
- using model agreement as corroboration;
- omitting candidate resources or coverage gaps from the capable-model prompt;
- assuming a nearby test proves the reported behavior.

### Design conclusions

1. Table rows and cells need logical identity, exact snapshot versions, and
   source spans equivalent to document sections.
2. Per-row retrieval must combine search with graph expansion; neither is
   sufficient alone.
3. Query results need an explicit completeness/coverage contract.
4. Candidate context is useful even when it cannot become a durable edge.
5. Deterministic coverage gaps may trigger bounded model-assisted
   investigations.
6. Model-assisted findings remain attributed claims until deterministic
   validators reproduce their observed evidence.
7. The graph API must keep established evidence, semantic candidates,
   model-proposed candidates, omissions, and unresolved gaps distinct.
8. Investigation attempts and yield must be measurable so repeated iteration
   can improve detectors, prompts, tools, and budgets.
9. The capable model, not the graph or tiny helper, performs product judgment,
   behavioral reasoning, implementation, and verification.
10. Whether the helper runs is host/planner policy, not a graph fact. The
    host-facing path is enabled by default but explicitly disableable; either
    path preserves the same graph trust boundary and completeness contract.

### Evaluation requirement

Replay historical rows and record the document sections, code regions, tests,
configuration, and commits that the capable agent ultimately used. Compare
those practical relevance sets with:

1. search-only retrieval;
2. graph-only retrieval;
3. search plus graph expansion;
4. search plus graph expansion plus bounded model-assisted investigation.

Measure artifact recall, irrelevant context, first-packet sufficiency,
additional tool calls, context bytes, time to first justified decision, and
which channel found each useful artifact. Classify every miss as unindexed
source, missing seed, missing edge, insufficient expansion, budget omission,
cross-repository evidence, runtime-only evidence, or later-discovered need.

## Experiment 3: Multi-file ownership refactor from a task-split epic

### Repository and hypothetical workflow

Repository: `/Users/brainless/Projects/ReporGo`

Epic:

```text
epics/001-unify-chat-ownership-and-collapse-migrations.md
```

Prompt:

```text
Implement Epic 001. Assume deterministic code has split the epic introduction
and its ten tasks. For the shared introduction and each task, retrieve the
focused repository evidence needed to implement it.
```

The epic was actually added to the repository after its implementation. For
this thought experiment, treat its text as if it existed immediately before
commit `ee38009` and query repository state at `ee38009^`. The later commits
provide comparison evidence, not evidence that would have been available to
the hypothetical planner.

The implementation history is unusually useful ground truth:

```text
ee38009  Task 1: Collapse migrations into fresh baseline
5d41305  Task 2: Update shared-types and regenerate TS
497b9be  Task 3: Rewrite WhatsApp inbound path
3fa049c  Task 4: Add web demo endpoint
ff9a899  Task 5: Agent tasks rework
7e6bdb1  Task 6: Patient resolution at message level
21b51aa  Task 9: Frontend type/query updates
14e6601  Task 10: Test sweep and formatting
```

Tasks 7 and 8 have no separate named implementation commit. Their report
rewiring and `chat_conversations` removal were substantially absorbed into the
baseline and Task 3 changes. This prevents the experiment from treating the
epic's task boundaries as observed code ownership.

### Shared epic context

The introduction supplies a durable, attributed requirement artifact:

- replace string equality between
  `chat_conversations.platform_conversation_id` and
  `whatsapp_contacts.phone_number` with foreign keys;
- make the contact own messages, reports, and task scope;
- resolve the patient per message;
- replace a task's single source message with an N:M association;
- model anonymous web-demo sessions as synthetic contacts;
- collapse migration history because the repository is pre-launch;
- preserve explicit non-goals around parser, prompt, SDK, authentication, and
  frontend redesign.

A graph packet can retrieve strong exact/structural support for much of this:

- schema definitions for `chat_conversations`, `chat_messages`,
  `whatsapp_contacts`, `medical_reports`, and `agent_tasks`;
- SQL joins and UPSERTs using `conversation_id`,
  `platform_conversation_id`, or phone strings;
- Rust fields and functions carrying `conversation_id`;
- generated TypeScript types derived from shared Rust types;
- handlers and GUI pages consuming those response fields;
- historical commits that introduced contacts, patient context, task
  deduplication, and report/conversation links;
- `DEVELOP.md` commands and workspace structure.

It cannot establish from structure alone that:

- WhatsApp will remain the universal channel;
- web-demo sessions should be anonymous and ephemeral;
- a contact may speak for several household members;
- production data can be discarded safely;
- the abstraction is no longer worth preserving;
- the proposed ownership model is the right product decision.

Those are authored requirements and assumptions. The packet must quote or
reference the exact epic version rather than recast them as repository-observed
facts.

### Per-task retrieval

#### Task 1: Collapse migrations

High-value graph context:

- every file under `backend/migrations`;
- structural SQL definitions, FKs, indexes, triggers, and seed inserts grouped
  by affected table;
- migrations introducing the exact old/new columns named by the epic;
- project evidence for SQLx and the migration source path;
- `DEVELOP.md` database setup and verification commands;
- Git history showing schema evolution and seed-file lineage.

The graph can focus the migration corpus and preserve which definitions came
from which immutable file versions. It cannot prove that deleting migration
history is operationally safe, that no deployed database depends on it, or
that the composed baseline is semantically equivalent. Those require the
epic's pre-launch assertion plus database execution and schema comparison.

Actual comparison: `ee38009` touched 36 migration files, replacing the chain
with one schema file and four seed files. Exact path and schema-table retrieval
would have found essentially the whole change surface.

#### Task 2: Update shared types and regenerate TypeScript

High-value graph context:

- exact paths named by the task:
  `shared-types/src/agent.rs`, `communication.rs`, and `medical.rs`;
- definitions and references for `AgentTask`, `ChatConversation`,
  `ChatMessage`, `MedicalReport`, `conversation_id`, and
  `source_message_id`;
- generator code and generated `gui/src/types/api.ts`;
- Cargo/workspace and documentation evidence for the generation command.

Exact symbol and field references can identify downstream Rust consumers.
Generated-file metadata or a configured convention can connect Rust source
types to TypeScript output. The graph cannot decide whether
`AgentTaskMessage` is useful, invent its final API shape, or guarantee that a
generated diff is correct.

Actual comparison: the implementation also changed generator/re-export files
not explicitly listed by the task. Definition/reference expansion would likely
find them; path-only retrieval would miss them.

#### Task 3: Rewrite the WhatsApp inbound path

High-value graph context:

- the explicitly named messaging service and WhatsApp handler;
- the `chat_conversations` UPSERT and `chat_messages` insert;
- contact UPSERT code, message persistence helpers, and callers passing
  `conversation_id`;
- chat, media, report, route, and test-chat queries joining through
  `chat_conversations`;
- schema FKs and response types used by those queries.

This is a strong case for exact-token seed expansion. At the parent revision,
`conversation_id` and `chat_conversations` occurred throughout messaging,
handlers, shared types, and GUI code. The graph could return focused symbols
and query spans rather than the entire large `messaging.rs`.

Actual comparison: `497b9be` changed seven backend files, including chat,
medical, routing, test-chat, messaging, and text-extraction files beyond the
two headline paths. A reference-aware graph would materially improve recall.
The proposed v1 lexical/exact graph may still miss dynamically related SQL
query/result-shape consumers because it is not a Rust type checker or SQL data
flow engine.

#### Task 4: Add the web-demo endpoint

High-value graph context:

- route-registration conventions in `backend/src/handlers/mod.rs`;
- neighboring handler request/response and error-handling patterns;
- the normalized inbound message structure and common processing entry point;
- `whatsapp_contacts` constraints and contact UPSERT behavior;
- UUID/random-ID dependencies and existing test-chat analogues.

The graph can retrieve patterns and constraints but cannot conclude the route
path, request/response shape, session trust boundary, or whether accepting a
client-supplied contact ID is secure. Those are design and security decisions.

Actual comparison: the implementation added `web_demo.rs` and modified route
registration, matching the likely structural packet closely.

#### Task 5: Rework agent tasks

High-value graph context:

- `agent_tasks` schema, Rust `AgentTask`, and all SQL inserts/selects;
- migrations `20260515` and `20260516` containing deduplication semantics;
- task-creation functions and filters using `conversation_id`;
- message IDs available at each creation site;
- existing task status, parent-task, and patient relationships.

The proposed join table is an authored target, not something the old graph can
discover as existing code. The graph can retrieve every place that must adapt
to it and historical rationale for current deduplication. It cannot choose
transaction boundaries, conflict behavior, role vocabulary, or whether every
message should be linked.

Actual comparison: the named Task 5 commit changed only `messaging.rs` because
schema/type work had landed earlier. This shows why a packet should distinguish
already-satisfied prerequisites from remaining task work at the selected
version.

#### Task 6: Resolve patients at message level

High-value graph context:

- `PatientContext`, `ContactContext`, and exact comments describing current
  keys;
- functions loading patient/contact context;
- message-to-contact and contact-to-user schema paths;
- task writes to `patient_user_id`;
- prior commits that introduced and later split patient/contact context.

The graph can show the current ownership chain and retrieve the default
contact-user candidate. It cannot infer which household member a message is
about or validate the product rule that unresolved subjects should remain
NULL. That is precisely the kind of semantic/runtime capability gap the packet
should report.

Actual comparison: the first implementation changed only `messaging.rs` and
used a deterministic message → contact → `user_id` fallback, consistent with
the epic's limited initial resolver.

#### Task 7: Rewire medical reports

High-value graph context:

- `MedicalReport` definitions and schema;
- report inserts in messaging and text extraction;
- `source_message_id`, `patient_user_id`, and conversation-filtered listing
  queries;
- handler routes and GUI consumers of report lists;
- the output of Task 6 as an explicit dependency.

The graph can retrieve insertion and listing surfaces, but it cannot prove that
all report creation paths are covered or that a report's resolved patient is
clinically correct. It also cannot infer the intended endpoint compatibility
policy when both patient and contact filters are possible.

Actual comparison: much of this task landed inside Task 3 rather than a
dedicated commit. Commit names alone would therefore under-report its change
surface; file/symbol evidence is more reliable than task-label history.

#### Task 8: Drop `chat_conversations`

High-value graph context:

- all active code, SQL, docs, and generated-type occurrences;
- migration/version evidence showing where the table is defined;
- callers and queries connected to the old identifiers;
- omissions for unsupported generated artifacts or unindexed file types.

This task is unusually deterministic: an exact absence query can verify the
specified directories after earlier tasks. A zero-result must still be
reported as “no indexed matches in these covered sources,” not universal proof
of absence.

Actual comparison: there was no standalone commit because the baseline and
Task 3 removed the active uses. This is a verification action over cumulative
state, not necessarily an implementation diff.

#### Task 9: Update frontend types and queries

High-value graph context:

- generated `api.ts` provenance;
- GUI references to `conversation_id`, `ChatConversation`, grouping keys, and
  affected API routes;
- pages calling chat/report endpoints;
- package scripts and frontend correctness gates where indexed.

The graph can find exact field consumers. It cannot reliably infer state-flow
or UI grouping semantics from TSX lexical edges alone, especially when values
are renamed during destructuring or passed through generic helpers.

Actual comparison: the named Task 9 commit changed only
`PatientChats.tsx`; later Task 10 compilation cleanup changed several other GUI
pages and regenerated a much larger `api.ts`. The initial packet would likely
be useful but not complete.

#### Task 10: Test sweep

High-value graph context:

- test-chat handlers and fixtures mentioning the removed table/fields;
- Cargo workspace members and documented commands;
- frontend package scripts;
- test/source naming conventions and prior commits that changed adjacent tests;
- every remaining exact occurrence of old identifiers after Tasks 1–9.

The graph can recommend observed repository gates and focused candidate tests.
It cannot know which builds fail, generate SQLx offline metadata, exercise
PostgreSQL behavior, or perform the WhatsApp/web-demo smoke test. Those are
execution results.

Actual comparison: the named commit mostly fixed GUI consumers and regenerated
types; a following backend-fix commit added SQLx metadata and repaired a
medical query. This is direct evidence that static retrieval alone would not
make the first implementation packet complete.

### Can the graph extract “what to implement”?

It can extract two different things that must not be conflated.

First, the exact task section is a versioned, attributed requirement artifact.
A deterministic task splitter can preserve normative statements such as:

```text
remove AgentTask.conversation_id
add AgentTask.whatsapp_contact_id
replace the conversation UPSERT with a contact UPSERT keyed by wa_id
add agent_task_messages(task_id, message_id, role)
```

Those statements can be represented as requested-change claims with exact
source spans. The graph can link their mentioned files, symbols, fields, tables,
and commands to current repository evidence. This is valuable extraction of
what the epic says to implement.

Second, a concrete patch recipe requires reasoning:

- the exact functions and query spans to edit;
- ordering and dependencies between edits;
- new API/type signatures;
- transaction and error behavior;
- compatibility choices;
- code not explicitly named but broken by the change;
- whether the implementation satisfies the product intent.

The graph should not manufacture those as authoritative relations. It can
return candidates, evidence paths, coverage, and gaps from which Epic 012 or a
capable model creates a plan. Deterministic validators can later confirm exact
observations and final absence checks, but not the semantic correctness of the
patch.

A useful boundary is:

```text
Epic text says what is requested.
Graph evidence says what exists, where it is connected, and what may be affected.
Planner/capable model decides how to change it.
Execution and tests show what actually works.
```

### Material usefulness and misses

This prompt meets the material-usefulness threshold. For every task, exact
paths, schema identifiers, Rust fields, or commands seed at least two focused
artifacts. For Tasks 2, 3, 5, 7, and 9, reference/containment expansion also
finds material consumers not fully enumerated by the task text.

Important misses and limitations:

- v1 has no compiler-grade Rust reference or SQL data-flow graph;
- generated-source provenance needs an explicit detector/convention;
- a task section's requested future relation is a claim, not an existing edge;
- overlapping tasks cannot be assigned exclusive file ownership;
- later task commits are evaluation evidence, not valid pre-implementation
  context;
- runtime database, SQLx, frontend compilation, and smoke-test failures remain
  undiscoverable until execution;
- product assumptions and security decisions cannot be validated from code
  structure;
- exact-zero queries prove absence only within reported indexed coverage.

### Design conclusions

1. A split task and its shared epic introduction must both be included in the
   evidence packet; task-local text alone loses goals, non-goals, assumptions,
   and the target schema.
2. Normative task statements need a distinct requested-change/claim
   representation linked to exact document versions and spans. They must not
   become authoritative `changes`, `implements`, or future-code edges.
3. Exact file, symbol, field, table, and command mentions provide strong seeds.
4. Reference and containment expansion materially improves the change-surface
   packet, but coverage must disclose that v1 is not compiler/data-flow
   complete.
5. Task boundaries overlap. The graph returns evidence per task but does not
   assert exclusive ownership or implementation order.
6. Querying at an exact repository version is necessary. Already-completed
   prerequisite work changes what remains relevant to later tasks.
7. Absence verification is valuable when source coverage and index version are
   explicit.
8. Historical task-labelled commits are useful evaluation ground truth, but
   file/symbol diffs are more reliable than commit subjects for determining
   what a task actually changed.
9. The graph can extract what the epic explicitly requests and connect it to
   current evidence; synthesis of a concrete implementation remains planner or
   capable-model work.
