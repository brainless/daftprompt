# Epic 011: Deterministic Repository Provenance Graph

## Introduction

daftprompt already indexes three complementary views of a repository:

1. Git commit messages and metadata
2. Code symbols extracted with Tree-sitter
3. Document sections and text chunks

Those indexes can be searched together, but their results are still independent
records. This epic adds a reusable `daftprompt-graph` crate that connects those
records with explainable, evidence-backed relationships in the same per-repo
SQLite database.

The graph is not an LLM-generated knowledge graph. Its first responsibility is
to capture relationships that can be reproduced from repository evidence:
changed files, exact paths, canonical symbols, document headings, test naming,
configuration files, and carefully scored lexical overlap. Semantic search may
propose candidates, but deterministic checks decide whether a durable edge is
stored.

## Background and Product Direction

This epic begins a larger effort based on recurring prompting experience across
Codex, Claude Code, opencode, and other coding agents.

Most substantial user requests can be split into smaller pieces. Researching a
PRD can be divided by relevant sections, with semantic search locating candidate
sections and separate workers cross-checking each section against the codebase,
tests, commands, and history. Coding work can likewise separate context
collection, test discovery, focused test creation, implementation, verification,
and synthesis.

daftprompt already has much of the raw context required for this workflow, but
it lacks provenance. A requirement such as “Section 6: Review Queue for Admins”
and “6.13: Admin can see columns…” may correspond to code symbols, tests, and
commits even when their wording is not identical. The product needs to preserve
those connections so an agent does not rediscover them through repeated broad
tool calls.

The long-term direction is:

1. Bring repository context into an explainable graph.
2. Break work into small, focused actions or prompts.
3. Combine deterministic packets with a mandatory small/tiny helper model.
4. Let that helper use only tightly bounded graph operations to gather context.
5. Validate model-discovered observations deterministically when possible.
6. Let the helper shape tool-free conversations with capable models.
7. Collect bounded, source-linked results with complete orchestration
   provenance.

The graph-building process should itself be incremental and decomposable.
Deterministic rules remain explicit tuning points as the project learns from
more repositories. Epic 013 makes a small/tiny helper model a co-foundation of
the interaction runtime, but not of graph truth. A model may propose resources
and relationships, but it must not author authoritative graph facts. Exact or
structural observations become graph evidence only when the corresponding
deterministic validator reproduces them; unsupported interpretations remain
attributed candidates.

This epic builds the provenance foundation. Epic 012 consumes it to produce
plans and packets; Epic 013 gives a mandatory helper a closed graph-tool facade
and keeps capable models tool-free.

A note on the real prompts collected for the thought-experiment gate below: they
are not disposable test inputs. The author already has a large corpus of real
prompts on this machine, used across Codex, Claude Code, opencode, and other
agents. Sanitized and normalized, that corpus becomes the seed of a versioned
**prompt-pattern library** that the Epic 013 helper draws on to shape better
capable-model prompts (Epic 013, Design Decision 14), later broadened with
curated web-sourced patterns. The same prompts therefore serve three roles:
proving the graph supplies useful evidence (Epic 011), proving the planner
produces a better bounded plan (Epic 012), and seeding the pattern corpus the
helper reasons with (Epic 013). Patterns inform prompt *shape* only; they never
become authoritative graph facts.

## Dependency

Requires Epics 003, 004, 007, 009, and 010: commits, code, and documents must
already share the per-repository `items` source of truth and preserve stable
source-specific identifiers.

Epics 012 and 013 depend on this epic's public graph types and query API, but
the graph crate must not depend on the planner or orchestrator crates.

## Blocking Thought-Experiment Gate

**Task 0 is a hard blocker. No implementation task in this epic may begin until
Task 0 is complete.**

Before choosing a final schema or API, collect real prompts previously used in
other projects and work through them against those repositories. The exercise
must establish whether the proposed graph can supply useful evidence rather
than merely sounding plausible.

Use at least six prompts across at least three repositories, including:

- a requirement or PRD implementation request;
- a bug investigation;
- a refactor spanning more than one file;
- a request whose answer depends on tests or project tooling;
- a request whose rationale is best explained by Git history;
- a request containing a language/tool keyword such as Python, pytest, Rust,
  Cargo, TypeScript, or npm.

For every prompt, write down:

1. The expected small investigation or implementation steps.
2. Which graph nodes each step would need.
3. Which edges would retrieve those nodes.
4. The exact deterministic evidence that could create each edge.
5. Which required connections cannot be created from current indexed data.
6. Where semantic candidate generation is useful and what deterministic check
   would accept or reject the candidate.
7. What an agent would still have to rediscover with broad search or repeated
   tool calls.
8. Whether the proposed graph reduces work enough to justify persisting the
   relationship.
9. Whether relevant files or document sections changed over time, which source
   timestamps are trustworthy, and whether Git/non-Git reconciliation preserves
   the intended logical identity.
10. Whether the initial result can state its source coverage, omissions, and
    unresolved gaps rather than implying completeness.
11. Whether a deterministic gap trigger plus a bounded, read-only small-model
    investigation finds useful context that graph/search retrieval missed, and
    which returned observations can be validated independently.

The results must be committed as a design artifact under
`epics/research/011-provenance-thought-experiments.md` (or an equivalently named
file referenced from this epic). Update this epic's data model, detectors, and
task boundaries in response to the findings before marking Task 0 complete.

If fewer than four of the six prompts obtain materially useful, explainable
context from the proposed graph, implementation remains blocked. “Materially
useful” means the graph identifies at least two focused artifacts or one
high-value artifact plus its rationale/test/history without a repository-wide
search.

## Goals

- Add a reusable `daftprompt-graph` workspace crate with no UI dependency.
- Persist graph data in the existing per-repository SQLite database.
- Give every durable edge reproducible evidence, detector identity, detector
  version, and confidence.
- Index files changed by each commit so commits can connect structurally to code
  and documents.
- Import commit, code, document, file, test, tool, and repository concepts as
  stable graph nodes without duplicating indexed text.
- Distinguish stable logical identity from immutable content-version identity,
  using Git object IDs when available and deterministic content hashes when Git
  is unavailable.
- Preserve source-authored/source-modified time when it is available and record
  when daftprompt first and most recently observed nodes, versions, edges, and
  evidence.
- Ingest saved reviews and other external analyses as attributed artifacts
  linked to the exact repository versions they examined.
- Preserve implementation epics, their shared context, split tasks, and
  normative requested-change statements as versioned attributed artifacts
  without pretending that requested future code already exists.
- Build exact path, exact symbol, containment, commit-change, test-target,
  configuration, and conservative lexical relationships.
- Use semantic search only for candidate generation in v1.
- Reconcile changed and deleted inputs incrementally.
- Query and explain relevant subgraphs through a library API and CLI.
- Return evidence packets that distinguish established graph paths, semantic
  candidates, externally proposed candidates, coverage, omissions, and
  unresolved gaps.
- Accept bounded external candidate proposals through a validation boundary
  without treating model output as authoritative evidence.
- Surface strong document-code-history clusters only when multiple independent
  evidence paths support them.
- Degrade safely when Git history, embeddings, or an individual detector is
  unavailable.

## Non-goals

- Let an LLM author authoritative graph edges.
- Execute or orchestrate small/tiny models or their tools; the graph only
  exposes coverage/gaps and validates externally supplied candidate
  observations.
- Build a complete compiler-grade call graph or resolve dynamic dispatch.
- Infer runtime behavior by executing indexed code.
- Replace `items`, `items_fts`, or the existing vector partitions.
- Store complete copies of item text in graph tables.
- Support cross-repository graph edges in v1.
- Implement task planning, prompt rendering, helper orchestration, capable-model
  conversations, agent execution, or sub-agent scheduling; planning belongs to
  Epic 012 and model orchestration belongs to Epic 013.
- Treat semantic similarity alone as proof that a requirement is implemented.
- Add graph visualization to the canvas in this epic.

## Design Decisions

### 1. Separate provenance from planning

`daftprompt-graph` answers:

- What repository artifacts exist?
- How are they connected?
- What evidence created each connection?
- How strong and current is that evidence?

It does not answer:

- What should an agent do next?
- Which actions can run concurrently?
- How should a prompt be worded?
- Is a shell command authorized?

This keeps the graph reusable by daftprompt, CLI tools, editors, and planners
with different execution policies.

### 2. Stable logical node identity

Graph identity is `(source_type, identifier)`, not the numeric `items.id`.
Reindex operations may delete and recreate item rows. A code symbol such as
`src/review.rs::ReviewQueue::columns` must retain its logical identity even when
its backing row ID changes.

`graph_nodes.item_id` may cache the current `items.id` for efficient joins, but
it is nullable and repairable. The stable source and identifier remain the
unique key.

Virtual structural nodes such as files, repositories, tests, and tools need not
have an `items` row. They use the same logical key model.

Content identity is separate from logical identity. In a Git repository, the
blob object ID is an excellent immutable identifier for one exact file version,
but it cannot be the file node's stable identity because every edit produces a
new blob. Treat Git object IDs as opaque, algorithm-qualified version IDs; do
not assume that every repository uses SHA-1. The stable file node remains keyed
by its normalized repository-relative path, with rename reconciliation using
observed Git history when available.

For repositories without Git, use an xxh3 content hash as the immutable
file-version fingerprint. Filesystem `mtime` may help avoid unnecessary reads,
but it is neither content identity nor authored time. Git and non-Git inputs
must expose the same logical-node/version boundary to detectors and callers.

Documents may be addressed below file level:

- a document file is a stable structural node;
- a heading or section may be a stable logical node when it has a canonical
  heading anchor within the document;
- an indexed document chunk is an item-backed node with file/line and heading
  provenance;
- a paragraph or sub-paragraph is normally an evidence span rather than a
  durable node, but may be promoted when independently referenced or linked.

Chunk ordinals and line numbers are locators, not sufficient stable identity:
inserting text earlier in a document can renumber both. Prefer a normalized
file path plus heading anchor for section identity. Reconcile unheaded or
duplicate-heading chunks conservatively using their containing section,
content fingerprint, neighboring anchors, and source span. If continuity is
ambiguous, create a new logical node rather than silently attaching history to
the wrong text.

### 3. Typed relationships with first-class evidence

Initial relation kinds:

- `contains`
- `defines`
- `mentions`
- `documents`
- `implements`
- `tests`
- `changes`
- `configures`
- `depends_on`
- `similar_to`
- `reviews`
- `derived_from`
- `revises`
- `supersedes`

An edge stores an aggregate confidence, but the individual evidence records are
the source of truth. Each evidence record contains:

- detector name;
- detector version;
- method;
- score;
- human-readable detail;
- optional source span or structured metadata.

Callers must be able to explain an edge without reconstructing the detector.
Multiple detectors may contribute evidence to the same typed edge.

### 4. Evidence strength is explicit

Evidence methods are classified broadly as:

1. **Structural:** a commit changed a path; a symbol is defined in a file; a
   document chunk belongs to a file.
2. **Exact:** text contains a canonical path, symbol, heading anchor, test
   target, or configuration key.
3. **Normalized lexical:** case splitting, snake/camel/kebab normalization,
   rare-token overlap, or a documented alias.
4. **Semantic candidate:** vector similarity suggested the pair.

Structural and exact evidence can create high-confidence edges directly.
Normalized lexical evidence requires conservative thresholds and collision
checks. Semantic evidence cannot create a durable `documents`, `implements`,
`tests`, or `changes` edge without corroboration; it may create a low-confidence
`similar_to` edge or feed a deterministic validator.

Rejected alternative retained for tuning: persisting every top-k vector match
would make the graph dense and easy to build, but would convert retrieval
similarity into false provenance and make explanations misleading.

### 5. Commit changed-file indexing is foundational

The existing commit index stores messages but not changed paths. Extend commit
indexing so each indexed commit has its changed-file list and change kind
(added, modified, deleted, renamed when available).

The representation must support:

```text
commit SHA --changes--> file path --defines/contains--> code or document item
```

Store commit-file facts in a normalized table rather than burying the only copy
inside commit metadata JSON. Metadata may include a summary for display, but
graph building and reconciliation require queryable rows.

Root commits compare against the empty tree. Merge commits retain a separate
change set for every named parent; the first-parent set is the default query
view, not the only stored truth. Callers may request another parent or all
parent-relative sets. Renames should retain old and new paths when gitoxide
exposes them reliably; otherwise represent the observed delete/add pair
honestly.

### 6. Graph schema lives beside, not inside, the text indexes

Proposed schema:

```sql
CREATE TABLE IF NOT EXISTS commit_files (
    commit_sha TEXT NOT NULL,
    parent_sha TEXT,
    parent_index INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    previous_path TEXT,
    change_kind TEXT NOT NULL,
    PRIMARY KEY (commit_sha, parent_index, file_path, change_kind)
);

CREATE TABLE IF NOT EXISTS graph_builds (
    id INTEGER PRIMARY KEY,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    graph_version INTEGER NOT NULL,
    status TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS graph_nodes (
    id INTEGER PRIMARY KEY,
    source_type TEXT NOT NULL,
    identifier TEXT NOT NULL,
    item_id INTEGER REFERENCES items(id) ON DELETE SET NULL,
    content_hash TEXT,
    version_id TEXT,
    authored_at TEXT,
    source_modified_at TEXT,
    first_observed_at TEXT NOT NULL,
    last_observed_at TEXT NOT NULL,
    current_content_since TEXT NOT NULL,
    metadata TEXT,
    UNIQUE(source_type, identifier)
);

CREATE TABLE IF NOT EXISTS graph_node_versions (
    id INTEGER PRIMARY KEY,
    node_id INTEGER NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    version_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    authored_at TEXT,
    source_modified_at TEXT,
    first_observed_at TEXT NOT NULL,
    last_observed_at TEXT NOT NULL,
    valid_from_build_id INTEGER NOT NULL REFERENCES graph_builds(id),
    valid_to_build_id INTEGER REFERENCES graph_builds(id),
    predecessor_version_id INTEGER
        REFERENCES graph_node_versions(id) ON DELETE SET NULL,
    metadata TEXT,
    UNIQUE(node_id, version_id)
);

CREATE TABLE IF NOT EXISTS graph_artifact_inputs (
    artifact_node_id INTEGER NOT NULL
        REFERENCES graph_nodes(id) ON DELETE CASCADE,
    input_node_id INTEGER NOT NULL
        REFERENCES graph_nodes(id) ON DELETE CASCADE,
    input_version_id INTEGER
        REFERENCES graph_node_versions(id) ON DELETE SET NULL,
    role TEXT NOT NULL,
    first_observed_at TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (artifact_node_id, input_node_id, role)
);

CREATE TABLE IF NOT EXISTS graph_edges (
    id INTEGER PRIMARY KEY,
    from_node_id INTEGER NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    to_node_id INTEGER NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    relation TEXT NOT NULL,
    confidence REAL NOT NULL,
    first_build_id INTEGER NOT NULL REFERENCES graph_builds(id),
    last_build_id INTEGER NOT NULL REFERENCES graph_builds(id),
    first_observed_at TEXT NOT NULL,
    last_observed_at TEXT NOT NULL,
    UNIQUE(from_node_id, to_node_id, relation)
);

CREATE TABLE IF NOT EXISTS graph_edge_evidence (
    edge_id INTEGER NOT NULL REFERENCES graph_edges(id) ON DELETE CASCADE,
    detector TEXT NOT NULL,
    detector_version INTEGER NOT NULL,
    method TEXT NOT NULL,
    score REAL NOT NULL,
    detail TEXT NOT NULL,
    first_build_id INTEGER NOT NULL REFERENCES graph_builds(id),
    last_build_id INTEGER NOT NULL REFERENCES graph_builds(id),
    first_observed_at TEXT NOT NULL,
    last_observed_at TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (edge_id, detector, detector_version, method, detail)
);

CREATE TABLE IF NOT EXISTS graph_detector_state (
    detector TEXT NOT NULL,
    detector_version INTEGER NOT NULL,
    input_key TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    last_build_id INTEGER NOT NULL,
    PRIMARY KEY (detector, input_key)
);
```

For a root commit, `parent_sha` is NULL and `parent_index` is 0. Merge parents
use their Git order beginning at 0. This preserves facts that differ by parent
without inventing a parent-independent change.

The exact schema may change during Task 0. Foreign keys and cascade behavior
must be tested with SQLite foreign-key enforcement enabled. Graph migrations
must not write directly to `items_fts` or merge the existing source-specific
vector tables.

`version_id` identifies exact source content. For a Git-backed file version it
is the algorithm-qualified blob object ID; for non-Git content it is an
algorithm-qualified xxh3 digest. `content_hash` remains a deterministic digest
usable by detector incrementality even when the source supplies a different
version identifier. Document-section and chunk versions derive from their
exact text plus structural context rather than pretending the containing
file's blob ID identifies an individual section.

Time fields have distinct meanings:

- `authored_at` is a nullable time explicitly supplied by the artifact or its
  trustworthy source metadata; it must retain provenance in `metadata`;
- `source_modified_at` is a nullable filesystem, VCS, or external-source
  modification time and must not be presented as authored time;
- `first_observed_at` is when daftprompt first stored the logical fact or
  version;
- `last_observed_at` is the most recent completed scan/build in which it was
  observed;
- `current_content_since` is when daftprompt first observed the node's current
  version.

All persisted times use UTC RFC 3339 text. Build IDs provide deterministic
ordering when wall-clock times collide. Re-observing unchanged input updates
`last_observed_at` without changing `first_observed_at`,
`current_content_since`, or version history.

Saved reviews, research notes, generated plans, and other analyses are ordinary
attributed graph artifacts, not authoritative edges merely because a model
produced them. Their node metadata may record:

- `author_kind` such as `human`, `model`, or `mixed`;
- author/model/provider identity when known;
- an opaque conversation or external artifact ID when available;
- the source-authored time and daftprompt ingestion time;
- the prompt/request content hash or a reference to a separately ingested
  prompt artifact.

`graph_artifact_inputs` links an analysis to the exact node version it examined.
For example, a Codex review of a MiMo-authored epic records the original epic's
Git blob-backed `graph_node_versions.id`, not merely its current document node.
If the reviewed version is unknown, retain the node-level input with
`input_version_id = NULL` and report the provenance gap.

Version succession for one logical node is stored through
`graph_node_versions.predecessor_version_id`. The `revises` and `supersedes`
graph relations are reserved for distinct logical artifacts or nodes; do not
create a meaningless self-edge merely because one document node acquired a new
version.

Git commit author metadata must not be used to infer model authorship. A commit
can prove that an account stored a revision and that one blob superseded
another; model attribution requires an explicit trailer, saved review,
conversation metadata, or another observed source.

### 7. Generic crate boundary

The graph crate exposes repository-neutral types:

```rust
pub struct NodeKey {
    pub source: SourceKind,
    pub identifier: String,
}

pub struct Edge {
    pub from: NodeKey,
    pub to: NodeKey,
    pub relation: RelationKind,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
}

pub struct Evidence {
    pub detector: String,
    pub detector_version: u32,
    pub method: EvidenceMethod,
    pub score: f32,
    pub detail: String,
    pub metadata: Option<serde_json::Value>,
}
```

It must not depend on `daftprompt` or akar. Avoid making it depend on
`daftprompt-indexer` merely to read private internals. Integration should use a
small public corpus/store boundary or shared `rusqlite::Connection` operations
whose ownership and transaction behavior are explicit.

The crate must be usable with an in-memory SQLite connection in tests and by a
different project that supplies equivalent normalized artifacts.

### 8. Detectors are independent, versioned, and incremental

```rust
pub trait Detector {
    fn name(&self) -> &'static str;
    fn version(&self) -> u32;

    fn detect(
        &self,
        corpus: &dyn Corpus,
        changes: &ChangeSet,
    ) -> anyhow::Result<Vec<EdgeCandidate>>;
}
```

Initial detectors:

- `ContainmentDetector`
- `CommitChangedPathDetector`
- `ExactPathMentionDetector`
- `ExactSymbolMentionDetector`
- `HeadingSymbolDetector`
- `TestTargetDetector`
- `ConfigurationDetector`
- `LexicalOverlapDetector`
- `SemanticCandidateDetector`

Changing detector logic increments its version. A detector rebuild removes or
reconciles evidence produced by older versions without deleting evidence from
other detectors. One detector failure logs and continues; it must not roll back
successful independent detector work.

### 9. Candidate generation is bounded

Do not compare every item with every other item. Candidate generation uses:

- shared or mentioned paths;
- canonical symbol tokens and aliases;
- uncommon normalized tokens;
- source-type constraints;
- same-directory or test/source naming conventions;
- commit changed paths;
- a bounded semantic top-k when embeddings are available.

Detectors publish candidate and accepted counts so density and false-positive
rates can be reviewed.

### 10. Strong provenance clusters require corroboration

A strong document-code-history cluster requires:

- document, code, and commit nodes;
- at least two independent evidence methods;
- at least one structural or exact edge;
- no edge supported only by semantic similarity;
- an explainable path connecting all three source families.

Example:

```text
Document "6.13 Admin can see columns"
    --mentions(normalized symbol)--> AdminReviewQueue::columns

Commit abc123
    --changes(git diff)------------> src/admin/review_queue.rs

src/admin/review_queue.rs
    --defines(tree-sitter)---------> AdminReviewQueue::columns
```

The first implementation should return clusters through a query API; UI
visualization is deferred.

### 11. Reconciliation preserves versions but favors correct active state

When an item, file, or commit disappears:

- repair or remove its current graph node;
- remove evidence whose input no longer exists;
- delete edges with no remaining evidence;
- close the active `graph_node_versions` interval instead of overwriting the
  only record of previously observed content;
- retain completed `graph_builds` and closed node versions as audit metadata,
  not stale active graph content.

Use xxh3 for stable detector input hashes. Touching a file without changing its
content must not force unrelated detector work.

Git history improves reconciliation but does not make identity automatic. A
blob object ID proves that two observations contain the same bytes. A Git
rename can provide structural evidence that a logical file moved, but heuristic
rename detection and delete/add pairs must be represented honestly. Copying a
file must not merge two logical nodes merely because their blob IDs match.

For document sections, compare parsed structural anchors and section hashes
between file versions. Exact heading-anchor continuity is strong evidence;
content similarity alone is only a reconciliation candidate. Preserve the
previous version and start a new logical section when matching is ambiguous.

The first implementation preserves node content-version history and observation
intervals. Edges and evidence remain current-state facts with first/last
observation metadata; complete historical reconstruction of every prior edge
set is deferred unless Task 0 demonstrates that it is required.

### 12. Reviews are attributed claims, not independent truth

A common planning workflow asks multiple humans or models to review the same
requirement or epic before implementation. Preserve each saved review as its
own artifact and, when useful, address individual findings as child document
sections or chunks.

The graph may connect:

```text
review --reviews--> exact epic version
review --derived_from--> review request or prompt
revised epic version --revises--> reviewed epic version
new document version --supersedes--> previous document version
review finding --mentions--> code symbol or configuration
```

Agreement between two model reviews is agreement between claims, not a second
independent structural or exact repository evidence method. A finding becomes
stronger when it cites code, configuration, history, tests, or exact document
spans that independently support it. Unsupported and conflicting review claims
remain queryable with explicit confidence and provenance.

Chat-only output is outside the graph until a host exports it or an ingestion
integration supplies it. The graph must identify the missing artifact rather
than reconstructing or attributing unseen review content.

### 13. Requested changes are attributed claims, not future-code edges

An implementation epic can state valuable future changes precisely: remove a
field, add a join table, replace an UPSERT, update a named type, or run a
repository command. Those statements answer what the document requests, not
what the repository currently contains.

When a deterministic document parser identifies shared epic context and task
sections, retain:

- the exact epic version and task/source spans;
- the task's parent epic and ordered task identity;
- shared goals, assumptions, non-goals, target design, and verification text
  needed to interpret the task;
- normalized requested-change claims where the syntax is sufficiently
  explicit;
- exact mentioned paths, symbols, fields, tables, configuration keys, and
  commands;
- ambiguity or unsupported syntax without inventing a structured operation.

Requested-change claims are attributed document artifacts. They may seed exact,
lexical, semantic, and graph expansion against the current repository version,
but they do not create authoritative `changes`, `implements`, `defines`, or
future-code edges. A statement such as “add `agent_task_messages`” does not
prove that the table exists. A task-to-resource mention means the task names or
describes that resource, not that the resource is exclusively owned by the
task.

Task-oriented context queries include both the task-local section and bounded
shared epic context. They query a specified graph/repository version so
already-completed prerequisite work can change the returned change surface.
Overlapping tasks may return the same file or symbol; the graph does not infer
exclusive ownership, implementation ordering, or patch boundaries.

Rejected alternative retained for tuning: converting imperative task prose
directly into durable future relations would make requested intent
indistinguishable from observed repository state and would encode a planner
inside the provenance graph.

### 14. Context packets expose coverage, candidates, and gaps

A graph query can enumerate its indexed universe, but cannot prove that a
bounded result contains every artifact relevant to a task. An empty graph
neighborhood is not evidence that no implementation, test, decision, or
rationale exists.

Context-oriented queries therefore return a completeness contract alongside
resources:

```rust
pub struct ContextEvidencePacket {
    pub established: Vec<ExplainedResource>,
    pub candidates: Vec<ContextCandidate>,
    pub coverage: Vec<SourceCoverage>,
    pub omissions: Vec<ContextOmission>,
    pub unresolved_gaps: Vec<ContextGap>,
}
```

The exact types are settled during Task 0 and Task 1, but the boundary must
preserve:

- source families queried and their index/detector versions;
- established resources with complete evidence paths;
- semantic-only candidates;
- externally proposed candidates and their attribution;
- unsupported artifact kinds, languages, or reference/runtime capabilities;
- context-budget omissions;
- zero-result searches as observations, not proof of absence;
- unresolved gap kinds suitable for deterministic planner rules.

Initial gap kinds should cover at least an unlinked requirement, ambiguous
symbol collision, missing implementation path, missing test, unknown
configuration, a committed code change with no linked decision/rationale,
conflicting decision/PRD sections, unsupported source coverage, and exhausted
candidate budget.

### 15. Model-assisted context discovery remains outside the trust boundary

A helper orchestrator may initiate a bounded investigation before or after a
capable-model turn. The graph does not choose a model, render its prompt,
execute model calls, or decide whether an investigation is required.

Epic 013 makes the helper path mandatory at the orchestration layer. That
requirement does not turn the helper into graph authority. The helper receives
only a closed set of read-only graph operations, while capable models receive
no tools. Every path retains the same validation and provenance boundary.

An external investigation may submit candidate findings containing:

- investigator/model attribution and optional prompt/action identity;
- seed graph resources and exact versions;
- tool observations and queries attempted;
- cited paths, symbols, spans, commits, and candidate relationships;
- observed-versus-inferred classification;
- confidence, unresolved ambiguity, and budget exhaustion.

Model output is an attributed claim artifact. Deterministic validators may
reproduce exact paths, symbols, source spans, references, configuration keys,
and changed-file facts. Reproduced observations contribute the validator's
normal evidence method, while retaining the investigation artifact as
provenance. A model interpretation such as “implements filtered export” remains
a candidate unless independent deterministic evidence supports that relation.

Established evidence, semantic candidates, externally proposed candidates,
validated observations, and unsupported claims must remain distinguishable in
storage and query output. Model agreement is not an independent evidence
method.

Repeated investigations need stable attempt identity and measurable yield:
new validated resources, new attributed candidates, duplicate findings,
tool-call count, returned bytes, elapsed time when supplied by the host, and
remaining gaps. This enables iterative evaluation without allowing recursive
investigation to imply increasing confidence.

### 16. Failures degrade, never panic

- Missing embeddings skip semantic candidates.
- A non-git folder omits commit and changed-file evidence.
- A non-Git folder uses xxh3 version IDs and source `mtime` where available,
  while retaining the same logical identity and observation-time semantics.
- An unavailable review conversation leaves the committed document revisions
  queryable but reports missing review authorship, prompt, and claim provenance.
- A malformed metadata row is logged and skipped.
- One detector failure does not discard other detector output.
- An incomplete graph remains queryable and identifies which detectors ran.
- A failed, unavailable, or exhausted external context investigation leaves the
  initial packet usable and reports the unresolved gap.
- Malformed model-proposed candidates are rejected diagnostically and cannot
  affect established graph evidence.

## Representative Graph Queries

### Requirement implementation

Input:

> Implement Section 6.13: Admin can see the required columns in the review
> queue.

Expected graph contribution:

1. Locate the exact document chunk and its parent heading.
2. Follow exact/normalized mentions to review-queue symbols.
3. Follow symbol containment to source files.
4. Find tests linked by naming, imports, or changed-file history.
5. Find commits that changed those files.
6. Explain every returned connection.

### Regression rationale

Input:

> Why does checkout retry only once, and which tests protect that behavior?

Expected graph contribution:

1. Match `checkout`, `retry`, and the exact configuration or symbol.
2. Find tests targeting the symbol/module.
3. Find commits that changed the implementation and tests together.
4. Retrieve commit messages and relevant document decisions.
5. Distinguish direct evidence from merely similar text.

### Cross-file refactor

Input:

> Move token validation out of the HTTP handler without changing behavior.

Expected graph contribution:

1. Identify the handler and validation symbols.
2. Identify their containing files and directly linked tests.
3. Show commits that previously changed both areas.
4. Avoid claiming a call relationship unless a future reference detector
   provides evidence for it.

### Cross-model planning review

Input:

> A model authored this epic, then another model reviewed and revised it.
> Show what changed, which suggestions have repository evidence, and which
> provenance is missing.

Expected graph contribution:

1. Resolve the original and revised document blobs as versions of one logical
   document.
2. Link the saved review to the exact original version it examined.
3. Link review findings to exact epic sections and supporting code,
   configuration, tests, or history.
4. Distinguish the human Git author/committer from the model that generated a
   review or revision.
5. Treat cross-model agreement as claim agreement, not independent proof.
6. Report a chat-only review as unavailable rather than inventing its contents.

### Testing-sheet row context

Input:

> Examine this testing-sheet row. Cross-check the PRD and earlier decisions,
> determine the relevant implementation and tests, and assemble the initial
> context needed to decide whether to implement it.

Expected graph contribution:

1. Resolve the exact table snapshot, row, cells, and column meanings.
2. Search document, code, test, and commit partitions independently.
3. Expand strong seeds through containment, mentions, definitions, tests,
   configuration, changes, and version history.
4. Preserve parent requirement constraints and potentially superseding
   decisions.
5. Return established resources and semantic candidates separately.
6. Report source coverage, budget omissions, missing reference/runtime
   capabilities, and recognized context gaps.
7. Accept externally proposed resources from a bounded investigation and
   validate cited exact/structural observations deterministically.
8. Never claim that the packet is complete or that a linked test proves the
   reported behavior.

## Tasks

### Task 0: Run and pass the blocking thought experiments

Collect real prompts and repository evidence, execute the process defined in
“Blocking Thought-Experiment Gate,” and revise this epic accordingly.

#### Acceptance Criteria

- [x] At least six real prompts from at least three other repositories are
  documented.
- [x] The required prompt categories are represented.
- [x] Every prompt includes desired steps, required nodes, required edges,
  deterministic evidence, gaps, and expected graph benefit.
- [x] At least four prompts meet the “materially useful” threshold.
- [x] False-positive and missing-evidence cases are documented, not removed from
  the report.
- [ ] Commit changed-file semantics are tested against at least one merge, root
  commit, rename, deletion, and ordinary modification.
- [ ] At least one Git-backed document edit and one non-Git document edit test
  logical identity, immutable version identity, and observation timestamps.
- [ ] At least one heading rename, duplicate heading, or ambiguous section edit
  demonstrates when section continuity must not be inferred automatically.
- [x] At least one real cross-model review records original/revised document
  versions, review attribution, supported findings, and missing chat provenance.
- [x] At least one real per-row testing-sheet workflow compares graph-only,
  search-plus-graph, and bounded model-assisted context retrieval.
- [ ] At least one real multi-file implementation epic is split into shared
  context and tasks, queried at an exact pre-implementation repository version,
  and compared with the files/symbols actually changed.
- [ ] The multi-file experiment distinguishes requested future changes from
  observed graph facts, overlapping task scope from exclusive ownership, and
  static retrieval from runtime/build discoveries.
- [x] The per-row experiment records a completeness contract, deterministic gap
  triggers, model-proposed candidates, independently validated observations,
  investigation budgets, and unresolved gaps.
- [ ] This epic's schema, detector list, and task boundaries are revised from
  the findings.
- [ ] The research artifact is reviewed and committed.
- [x] Tasks 1–8 have not begun before every Task 0 criterion is checked.

### Task 1: Add the graph crate and stable domain model

Create `crates/daftprompt-graph`, add it to the workspace, and implement stable
node, relation, evidence, candidate, build-report, and query-result types.

#### Acceptance Criteria

- [ ] The crate has no UI or agent-specific dependency.
- [ ] Stable identity is `(source_type, identifier)`.
- [ ] Logical node identity is distinct from algorithm-qualified content
  version identity.
- [ ] File, document-section, chunk, and evidence-span addressing rules are
  represented without treating chunk ordinals or line numbers as stable IDs.
- [ ] Review/analysis artifacts retain author kind, optional model identity,
  prompt provenance, and exact reviewed-version references when available.
- [ ] Split epics/tasks and requested-change claims retain exact document
  versions, source spans, parent/shared context, mentioned resources, and
  unsupported or ambiguous statement diagnostics.
- [ ] Context packets distinguish established evidence, semantic candidates,
  externally proposed candidates, coverage, omissions, and unresolved gaps.
- [ ] External investigation attempts and findings retain attribution, seeds,
  exact input versions, observed/inferred status, budget use, and yield.
- [ ] Relation and evidence types serialize to stable string keys.
- [ ] Unknown stored relation/evidence keys can be surfaced diagnostically
  rather than silently mapped to an unrelated variant.
- [ ] Unit tests cover type round trips and evidence aggregation.
- [ ] `cargo check --workspace` passes.

### Task 2: Add graph persistence and migrations

Implement the graph tables, repository/store abstraction, transactional writes,
build lifecycle, and explain queries.

#### Acceptance Criteria

- [ ] Graph tables coexist with all current index tables.
- [ ] Schema initialization is idempotent.
- [ ] Tests use in-memory or temporary SQLite databases.
- [ ] Edge uniqueness and multi-detector evidence behavior are tested.
- [ ] `item_id` is a nullable `items.id` join cache with `ON DELETE SET NULL`;
  rebuilding repairs it from the stable node key.
- [ ] Node versions retain first/last observation, validity builds, source
  timestamps, and exact content identity without duplicating full item text.
- [ ] Artifact inputs reference exact node versions and degrade explicitly to a
  node-level link when the reviewed version is unavailable.
- [ ] Persisted context-investigation artifacts retain exact seed/input
  versions, attempt identity, observed/inferred findings, budgets, yield, and
  validation status without making candidate claims authoritative edges.
- [ ] Edge and evidence first/last observation fields survive unchanged rebuilds
  and advance only according to completed build semantics.
- [ ] All timestamps are UTC RFC 3339 and authored time is distinguishable from
  source-modified and observed time.
- [ ] Removing the last evidence removes or invalidates the edge.
- [ ] A failed build cannot be mistaken for a completed current build.
- [ ] No graph operation writes directly to `items_fts`.
- [ ] `cargo check --workspace` passes.

### Task 3: Index commit changed-file provenance

Extend Git indexing with normalized, queryable commit-file change facts and
incremental reconciliation.

#### Acceptance Criteria

- [ ] Ordinary additions, modifications, deletions, and available rename
  information are represented honestly.
- [ ] Git blob object IDs are stored as opaque, algorithm-qualified file-version
  IDs and are not used as the stable logical file key.
- [ ] Identical blobs at different paths do not merge copied files into one
  logical node.
- [ ] Root and merge commit comparison policy matches the Task 0 decision and
  is covered by repository fixtures, including the empty-tree root case and
  distinct parent-relative merge results with a first-parent default view.
- [ ] Paths are repository-relative with forward slashes.
- [ ] Reindexing removes stale `commit_files` rows.
- [ ] A commit node can connect through `changes` to file nodes.
- [ ] Existing commit search text/ranking behavior remains unchanged.
- [ ] One malformed/unreadable commit diff logs and continues where possible.
- [ ] `cargo check --workspace` passes.

### Task 4: Import corpus, tables, and structural nodes

Import item-backed nodes and create repository/file/test/tool virtual nodes.
Build containment, definition, and commit-change edges.

#### Acceptance Criteria

- [ ] Commit, code, and document items retain their stable identifiers.
- [ ] Code/document nodes link to normalized containing files.
- [ ] Document chunks link to their file and, when metadata permits, heading
  hierarchy.
- [ ] CSV records and Markdown-table rows/cells can enter through the normalized
  corpus boundary with exact snapshot versions, headers, source spans, and
  conservative logical identity.
- [ ] Stable heading anchors identify addressable document sections; line ranges
  and chunk ordinals remain locators.
- [ ] Paragraph/sub-paragraph evidence can carry precise source spans without
  requiring every span to become a durable node.
- [ ] Git-backed files use blob IDs and non-Git files use xxh3 for exact version
  identity through the same corpus API.
- [ ] Saved reviews are imported as distinct attributed nodes; Git author
  metadata alone never creates model-authorship evidence.
- [ ] Implementation epics import shared context and task sections without
  duplicating full text; explicit requested-change claims remain attributed
  claims and cannot masquerade as existing `changes` or `implements` edges.
- [ ] Imported table rows/cells retain exact snapshot provenance; row position
  is a locator rather than stable logical identity.
- [ ] Code symbols link to their defining file.
- [ ] File classification does not relabel all files containing “test” as tests;
  conventions are explicit and tested.
- [ ] Reindex/rebuild repairs cached `item_id` values.
- [ ] Deleted corpus items do not leave active orphan edges.

### Task 5: Implement exact and convention-based detectors

Implement containment, commit changed path, exact path mention, exact symbol
mention, test target, and configuration detectors.

#### Acceptance Criteria

- [ ] Exact path mentions handle backticks, forward-slash normalization, and
  unambiguous relative paths.
- [ ] Exact symbol matching handles canonical identifiers and conservative
  snake/camel/kebab normalization.
- [ ] Ambiguous short symbol names do not create high-confidence edges to every
  collision.
- [ ] Test/source conventions are configurable data, not scattered branches.
- [ ] Python/Rust/JavaScript tooling files can create `configures` evidence
  without introducing planner actions.
- [ ] Every accepted edge has human-readable evidence.
- [ ] Detector failures are isolated.

### Task 6: Implement bounded lexical and semantic candidate detection

Add rare-token/heading overlap and optional semantic candidate generation with
source constraints and conservative validation.

#### Acceptance Criteria

- [ ] Candidate generation is bounded and reports candidate/accepted counts.
- [ ] Common repository words alone cannot create high-confidence edges.
- [ ] Semantic-only evidence is labeled and cannot masquerade as structural or
  exact evidence.
- [ ] Externally proposed candidates cannot create durable authoritative edges
  without deterministic validation.
- [ ] Deterministic validators can reproduce cited exact paths, symbols, spans,
  references, configuration, and changed-file observations while retaining
  proposal provenance.
- [ ] FTS5-only operation remains useful when the embedder is unavailable.
- [ ] Fixture tests include compelling near matches and misleading false
  friends.
- [ ] Thresholds are named constants or configuration, with rejected
  alternatives retained as tuning comments.

### Task 7: Add incremental graph builds and explainable queries

Add change tracking, detector-version reconciliation, neighborhood queries, and
strong-cluster queries.

#### Acceptance Criteria

- [ ] An unchanged second build performs no material detector work.
- [ ] A content edit rebuilds only affected evidence plus required dependent
  edges.
- [ ] An unchanged observation preserves `first_observed_at` and
  `current_content_since` while advancing `last_observed_at`.
- [ ] A content change closes the prior node version and creates or reuses the
  exact new version without changing an unambiguously stable logical node.
- [ ] Ambiguous document-section continuity creates a new logical node rather
  than silently transferring history.
- [ ] A detector version change rebuilds that detector without deleting
  independent evidence.
- [ ] Deleted files/items remove stale active evidence.
- [ ] Query results include paths, confidence, and all supporting evidence.
- [ ] Context queries include source coverage, unsupported capabilities,
  budget omissions, semantic/external candidates, and unresolved gaps.
- [ ] Task-oriented queries include bounded shared epic context, retain the
  exact task and repository/graph versions queried, and allow overlapping tasks
  to return the same affected candidates without asserting exclusive ownership.
- [ ] Exact absence results report the covered sources and detector/index
  versions rather than claiming universal absence.
- [ ] Failed or exhausted context-investigation attempts do not invalidate the
  initial graph packet and remain measurable.
- [ ] Strong clusters enforce the corroboration rules in Design Decision 10.
- [ ] Results use deterministic tie breaking.

### Task 8: Integrate CLI, documentation, and validation

Expose graph build, inspection, and explanation flows through daftprompt's CLI
and document the reusable crate.

Candidate commands:

```bash
cargo run -- --repo . --build-graph
cargo run -- --repo . --rebuild-graph
cargo run -- --repo . --explain-links "review queue columns"
```

Exact flags may change during Task 0.

#### Acceptance Criteria

- [ ] Users can build/rebuild the graph independently and after all-source
  indexing.
- [ ] Explain output distinguishes structural, exact, lexical, and semantic
  evidence.
- [ ] Partial detector failure is reported without losing successful results.
- [ ] `README.md`, `DEVELOP.md`, and `AGENTS.md` document the graph and commands.
- [ ] A focused review checks stable identity, stale-edge cleanup, false
  provenance, detector failure isolation, and transaction boundaries.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes.

## Test Matrix

| Area | Required evidence |
|---|---|
| Domain model | stable logical keys, version IDs, spans, enum/string round trips, unknown keys |
| Persistence | idempotent schema, nullable item cache, node versions, observation times, edge/evidence uniqueness, build states |
| Git changes | root, merge, add, modify, delete, rename/copy policy, opaque blob IDs |
| Non-Git changes | xxh3 versions, mtime distinction, edit/delete reconciliation |
| Structural graph | repository/file/item containment, definitions, document section/chunk hierarchy |
| Document identity | heading continuity, inserted text, duplicate/renamed headings, ambiguous matches |
| Review provenance | exact reviewed version, saved/chat-only review, prompt/model attribution, unsupported claims |
| Requested changes | exact task/epic version, shared context, normalized explicit operations, ambiguous prose, no future-code edges |
| Task change surface | exact seeds, reference candidates, overlapping tasks, pre/post-version comparison, generated consumers |
| Cross-model comparison | agreement versus independent repository evidence, conflicts, missing attribution |
| Table context | snapshot/row/cell identity, row reorder/edit ambiguity, source spans |
| Context completeness | source coverage, unsupported capabilities, omissions, zero results, unresolved gaps |
| External candidates | attribution, bounded attempts, observed/inferred split, deterministic validation, yield |
| Exact detectors | paths, symbols, collisions, quoted/backticked forms |
| Test/tool detection | conventions across Rust, Python, JS/TS |
| Lexical detection | rare overlap, common-token rejection |
| Semantic candidates | bounded top-k, corroboration, no-embed fallback |
| Incrementality | unchanged observation, touch, edit/version interval, delete, detector upgrade |
| Explanation | complete evidence paths and deterministic ordering |
| Strong clusters | three sources, independent methods, structural/exact edge |

## File-change Summary

| File | Change |
|---|---|
| `Cargo.toml` | Add `crates/daftprompt-graph` to the workspace and dependencies. |
| `crates/daftprompt-graph/Cargo.toml` | New reusable graph crate manifest. |
| `crates/daftprompt-graph/src/lib.rs` | Public graph domain and query API. |
| `crates/daftprompt-graph/src/store.rs` | SQLite persistence and build lifecycle. |
| `crates/daftprompt-graph/src/detectors/` | Independent deterministic detectors. |
| `crates/daftprompt-graph/src/schema.sql` | Graph tables and indexes. |
| `crates/daftprompt-indexer/src/schema.sql` | Add normalized commit-file facts if integration ownership remains here. |
| `crates/daftprompt-indexer/src/lib.rs` | Populate/reconcile commit changed-file data and expose corpus integration. |
| `crates/daftprompt-indexer/src/tables.rs` | Candidate deterministic CSV/Markdown-table extraction and normalized row/cell metadata, if owned by the indexer. |
| `src/main.rs` | Add graph CLI integration. |
| `epics/research/011-provenance-thought-experiments.md` | Blocking real-prompt design evidence. |
| `README.md`, `DEVELOP.md`, `AGENTS.md` | Document graph behavior and commands. |

The precise module split and ownership of shared schema initialization must be
settled by Task 0 and Task 1; the table above describes responsibilities rather
than requiring a premature file layout.

## Risks and Follow-ups

- Git rename detection can be expensive or ambiguous; preserve observed facts
  and document the configured policy.
- Common domain language may produce convincing false lexical matches.
- Symbol references are not a compiler call graph. A future reference indexer
  should add distinct evidence rather than strengthening lexical edges.
- Model-assisted retrieval can improve recall while quietly weakening
  provenance; externally proposed findings must remain candidates until
  independently validated.
- Repeated bounded investigations can still waste time if attempt identity,
  duplicate findings, marginal yield, and stop conditions are not measured.
- Very large repositories need detector work queues and bounded candidate sets.
- Framework-specific project detectors may eventually become plugins or data
  packs.
- Graph visualization, cross-repository nodes, issue trackers, code review data,
  and runtime traces are follow-up work.
