# Epic 012 Planner Thought Experiments

This artifact records real planning cases used to revise Epic 012. It is
intentionally cumulative. Task 0 remains incomplete until the full required
corpus and acceptance criteria in the epic are satisfied.

## Cross-cutting helper configuration conclusion

The bounded small/tiny-model investigation is optional in capability but
enabled by default in host-facing library configuration. Callers can set
`enabled = false`; that path performs no model or network call and still
returns the deterministic initial packet, coverage, and gap diagnostics.

The shipped adapter will use `~/Projects/llm-sdk/` through an OpenAI-compatible
API. Model choice is a closed supported-model value rather than an arbitrary
string. The initial supported set and default remain intentionally unsettled
until manual testing measures retrieval quality, correct bounded tool use,
latency, and cost. Recorded fixtures exercise planner core without credentials
or network access.

## Experiment 1: Cross-model review of akar Epic 020

The repository facts, commits, blob IDs, evidence, and provenance gaps are
recorded in `011-provenance-thought-experiments.md`, Experiment 1.

### Intent and uncertainty

Intent: review a model-authored implementation epic before implementation and
compare findings from another model.

Uncertainty:

- the original and revised Git versions are known;
- model authorship is user-supplied but not repository-observed;
- the Codex review text was initially chat-only;
- the revised epic does not preserve rejected suggestions.

### Deterministic project facts

- akar is a Rust workspace.
- Public component capabilities span Rust and a generated C ABI.
- Repository instructions require local-source inspection and visual
  verification.
- Canonical gates include `cargo fmt --check`, `cargo test --workspace`, and
  `cargo clippy --workspace -- -D warnings`.
- The repository provides deterministic screenshot, frame-dump, component
  isolation, scripted-input, and pixel-diff tools.

### Relevant graph query

1. Resolve the original Epic 020 blob.
2. Select exact epic sections mentioning text style, card, navbar, button,
   badge, C ABI, screenshot, and isolation requirements.
3. Follow exact symbol/path links to component implementations, C wrappers,
   generated header declarations, tests, Epic 019, and debug tooling.
4. Resolve the revised blob and document diff.
5. Retrieve each saved review linked to the exact original version.

### Typed actions and dependencies

```text
Parallel evidence group
  A. read_document_section: exact original Epic 020 sections
  B. read_symbol: mentioned component and glyphon types
  C. read_symbol: affected C ABI wrappers/header declarations
  D. inspect_history: Epic 019 and Epic 020 commits
  E. inspect_configuration: canonical gates and visual tooling

Barrier
  F. summarize_evidence: evidence-linked planning findings

Optional review group
  G. read_document_section: persisted MiMo-generation metadata
  H. read_document_section: persisted Codex review

Comparison
  I. compare_reviews: agreement, conflict, unique findings, unsupported claims,
     stale-version claims, and missing provenance

Final
  J. synthesize: bounded revision recommendations
```

A-E are independently executable and read-only. F depends on A-E. G-H can run
in parallel only when their artifacts exist. I depends on F-H; a missing G or H
produces a diagnostic rather than blocking evidence review. J depends on I.

### Output contracts

Evidence action:

```text
exact source version
file/span or symbol
finding
evidence method
confidence
maximum bounded excerpt
```

Comparison action:

```text
claim identity
review author/model provenance
reviewed version
agreement/disagreement classification
independent supporting evidence
unsupported or stale status
missing provenance
```

### Commands

No mutation or build command is required to review the plan. If the user later
authorizes implementation, canonical verification commands come from
`AGENTS.md`/`DEVELOP.md`, not from language-name guessing.

### Decisions requiring reasoning

- whether a proposed API tradeoff is desirable after factual contradictions
  are established;
- whether to preserve an existing API or accept a documented pre-alpha break;
- how much C ABI scope belongs in the epic;
- whether a review finding should revise the epic or become an explicit
  deferral.

### Failure modes observed

- attributing a Git commit to Codex because the user account committed it;
- comparing a review against the current epic instead of the exact reviewed
  blob;
- treating two models agreeing as two independent evidence methods;
- reconstructing missing chat review content from the resulting document diff;
- losing rejected suggestions when only the revised epic is saved;
- returning entire implementation files instead of focused symbols/spans.

### Result

The plan is materially smaller than giving the prompt and whole repository to
one agent. Exact mentions bounded the relevant corpus, while parallel API,
C ABI, history, and verification inspection exposed concrete defects.

This experiment added `review_plan` and `compare_reviews` actions,
review-version diagnostics, persisted optional-model provenance, and
claim-versus-evidence rules to Epic 012.

## Experiment 2: Plan one client testing-sheet row

The real Keystone workflow and sanitized representative row are recorded in
`011-provenance-thought-experiments.md`, Experiment 2.

### Intent and uncertainty

Initial intent: examine one reported testing item and decide whether it should
be rejected, deferred, clarified, or implemented. Mutation is conditional, not
yet authorized merely because the row contains change-oriented language.

Uncertainty:

- the row may be a defect, requested change, misunderstanding, duplicate, stale
  report, or unresolved product question;
- expected behavior may differ across PRD and later decision versions;
- the implementation and tests may use terminology unrelated to the row;
- graph coverage may be incomplete even when returned connections look strong;
- runtime reproduction may be necessary before a decision;
- the downloaded table may lack upstream revision and authorship metadata.

### Deterministic project and corpus facts

- repository instructions and configuration identify the packages, test
  runners, end-to-end facilities, and correctness gates;
- CSV and Markdown-table ingestion can expose a versioned row and cells;
- document/code/test/commit indexes report their supported source families and
  current versions;
- exact paths, symbols, headings, test conventions, changed files, and
  configuration can provide deterministic graph evidence;
- unavailable reference, language, cross-repository, external-revision, or
  runtime coverage must be reported rather than inferred.

### Initial graph query and evidence bundle

1. Resolve the exact table snapshot and row.
2. Search every relevant source partition using exact, lexical, and semantic
   row facets.
3. Include parent requirement constraints and document versions.
4. Expand strong seeds to symbols, files, tests, configuration, commits, and
   focused diffs.
5. Preserve semantic-only candidates separately from established graph paths.
6. Produce a coverage manifest, omitted candidates, and unresolved gaps.

### Typed actions and dependencies

```text
Initial retrieval group
  A. read_table_row: exact row, cells, headers, snapshot provenance
  B. search_graph/search_text: PRD, epic, and decision candidates
  C. search_graph/search_text: implementation and test candidates
  D. inspect_history: commits for any exact or strong candidate files
  E. inspect_configuration: focused and broad verification capabilities

Barrier
  F. assemble_context_packet: evidence, candidates, coverage, omissions, gaps

Conditional bounded enrichment group
  G. investigate_context_gap: locate an unlinked implementation path
  H. investigate_context_gap: find or reject candidate tests
  I. investigate_context_gap: compare conflicting PRD/decision sections
  J. investigate_context_gap: inspect focused rationale-bearing history

Validation and packet rebuild
  K. validate_context_candidates: reproduce cited exact/structural observations
  L. assemble_context_packet: add validated evidence, attributed candidates,
     investigation attempts, yield, remaining gaps

Capable-model handoff
  M. synthesize: classify the row, compare expected/current behavior, recommend
     reject/defer/clarify/investigate-runtime/implement, and explain evidence

Conditional implementation sequence
  N. reproduce or establish the behavior contract
  O. add or update focused tests
  P. modify the smallest supported code region
  Q. run focused verification and repository gates
  R. review the diff against the row and governing decisions
```

A-E may run independently when their inputs are bounded. G-J are emitted only
for deterministic gap kinds present in F; adjacent small gaps should be
coalesced. A host executes them, and planner core remains model- and
network-independent. K validates only reproducible observations. L retains
unsupported interpretations as attributed candidates. N-R are absent until M
or a user/product decision selects an implementation path.

### Bounded context-investigation contract

Each tiny-model investigation receives:

```text
one explicit gap question
graph-selected seed resources and evidence paths
allowed read-only tool capabilities
maximum calls, bytes, and traversal depth
required structured output
```

It returns:

```text
exact resources and spans observed
tool observation supporting each fact
candidate relationships and their inference status
queries attempted, including zero-result searches
unresolved ambiguity
budget exhaustion
```

An initial evaluation policy may allow at most three independent
investigations, five read-only calls each, 12 KiB per result, and one packet
rebuild. These are named evaluation parameters, not hard-coded product truths.
Repeated offline evaluation may revise them.

### Output contract for the capable-model packet

```text
row and exact snapshot provenance
established requirement, decision, code, test, configuration, and history
candidate context separated by semantic/model provenance
complete graph paths and focused source spans
source/index/detector coverage
context-budget omissions
investigation attempts and marginal yield
unresolved retrieval and runtime gaps
explicit decision alternatives and mutation boundary
```

The packet does not claim completeness. It tells the capable model what was
searched, what was found, how each item was selected, and what remains unknown.

### Commands

Initial retrieval and tiny-model enrichment are read-only. Commands for
reproduction or verification are derived from exact project facts only after
the decision boundary. The planner does not treat a model request to run a
command as authorization.

### Decisions requiring capable-model or human reasoning

- whether reported and specified behavior actually conflict;
- whether a later decision supersedes the PRD or merely specializes it;
- whether the request should be accepted as product behavior;
- whether runtime reproduction is required;
- what constitutes an adequate unit/end-to-end test oracle;
- whether unresolved graph coverage makes implementation unsafe.

The tiny model is not asked to make these decisions.

### What would happen without graph-assisted enrichment

A capable agent receives the row plus broad repository access and repeats
repository orientation, PRD search, source tracing, test discovery, and Git
history inspection. With graph-only retrieval, it may receive a precise but
incomplete neighborhood. The bounded enrichment group targets only the named
gaps between those outcomes.

### Failure modes

- interpreting row language as immediate mutation intent;
- using only graph neighbors and hiding missing coverage;
- spawning a model investigation without a deterministic gap trigger;
- giving the tiny model general mutation or shell capabilities;
- treating its interpretation as graph truth;
- promoting cited observations without deterministic reproduction;
- recursive enrichment with no attempt/yield budget;
- fragmented prompts whose coordination costs exceed their retrieval value;
- allowing enrichment failure to block the capable-model handoff instead of
  reporting the remaining gap;
- asking the tiny model to make the final product decision.

### Result and required epic changes

This case adds a row evidence packet and coverage contract, deterministic gap
types, `investigate_context_gap`, `validate_context_candidates`, and
`assemble_context_packet` actions, bounded read-only investigator interfaces,
candidate-versus-observation validation, attempt/yield diagnostics, and
conditional mutation boundaries.

Its value must be established empirically by replaying historical rows and
comparing search-only, graph-only, search-plus-graph, and bounded-enrichment
retrieval against the artifacts the capable agent ultimately used.
