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
