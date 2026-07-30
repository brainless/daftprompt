# Epic 012: Deterministic Task Planner and Prompt Generator

## Introduction

Epic 011 creates an evidence-backed provenance graph connecting repository
documents, code, tests, configuration, files, and history. This epic adds a
reusable `daftprompt-planner` crate that maps an incoming user request and a
relevant graph slice into small, typed, dependency-aware actions.

The planner is deterministic. Project detectors establish facts such as
the language, package manager, test runner, linter, type checker, build system,
and repository correctness gate. Planning rules combine those facts with
request intent and graph evidence. The durable plan is a typed intermediate
representation rather than an unvalidated list of model-generated shell
commands or relationships. Epic 013 consumes these plans and packets in a
mandatory helper-orchestrated conversation with tool-free capable models.

The crate plans work; it does not execute commands, call models, mutate files,
spawn agents, or assume that parallel execution is available. Host integrations
decide how authorized actions run.

## Dependency

Requires Epic 011, including its blocking thought experiments, stable graph
identity, explain queries, commit changed-file provenance, and conservative
evidence rules.

The planner may depend on `daftprompt-graph`. The graph crate must not depend on
the planner. Epic 013 may consume both crates; neither may depend on its
orchestrator. None of the reusable core crates may depend on akar or the
daftprompt UI.

## Blocking Thought-Experiment Gate

**Task 0 is a hard blocker. No planner implementation task may begin until Task
0 is complete.**

Reuse and extend the real prompts collected for Epic 011. The graph thought
experiment asks whether useful context can be retrieved; this gate asks whether
that context can produce a better bounded plan.

Use at least eight real prompts across at least three repositories. At least six
must come from actual prior prompting experience rather than examples invented
only for this epic. Include:

- requirement/PRD implementation;
- bug diagnosis and bug fix;
- focused refactor;
- test creation or test repair;
- documentation/research request;
- history/rationale request;
- an ambiguous request that should cause investigation before mutation;
- a request spanning more than one language, package, or subsystem.

For each prompt, manually construct:

1. Detected request intent and uncertainty.
2. Project facts available deterministically from repository files.
3. The relevant graph query and evidence bundle.
4. Typed actions with inputs and expected outputs.
5. Dependencies and legal parallel groups.
6. Commands that can be derived exactly from configuration.
7. Decisions that require agent reasoning rather than a deterministic rule.
8. The final synthesis/implementation context.
9. What would happen without the graph or planner.
10. Failure modes: missing evidence, misleading matches, unsafe commands,
    excessive fragmentation, redundant actions, or summaries that omit
    provenance.
11. The initial context packet's source coverage, omissions, unsupported
    capabilities, and unresolved gap kinds.
12. Whether a deterministic gap trigger justifies a bounded small/tiny-model
    investigation, which tools and budgets it receives, what it discovers, and
    which observations can be independently validated.

Write the results under
`epics/research/012-planner-thought-experiments.md` (or an equivalently named
file referenced here). Revise the action model, detector facts, rule catalog,
and task boundaries from the findings.

Implementation remains blocked unless at least six of the eight prompts produce
a plan that:

- has independently executable investigation actions where appropriate;
- avoids redundant repository-wide discovery;
- keeps every supplied context item linked to graph evidence;
- identifies focused verification commands or explicitly explains why it
  cannot;
- is materially smaller and clearer than handing the raw prompt plus broad
  repository access to one agent.

## Goals

- Add a reusable `daftprompt-planner` workspace crate.
- Represent plans as typed, serializable data before rendering prompts.
- Detect project capabilities deterministically from repository configuration.
- Classify common request intents conservatively.
- Retrieve bounded, provenance-linked context from Epic 011.
- Produce row/task evidence packets with explicit coverage, omissions,
  candidates, and unresolved context gaps.
- Produce small actions with explicit inputs, dependencies, execution policy,
  and output contracts.
- Identify independent action groups without requiring parallel execution.
- Derive focused commands from observed project facts rather than language-name
  guesses.
- Render generic prompts and structured JSON for multiple host integrations.
- Preserve uncertainty, missing evidence, and rejected candidate actions.
- Emit typed context gaps and validation contracts that the mandatory Epic 013
  helper orchestrator can consume without promoting model interpretation to
  repository truth.
- Plan evidence-bounded review and cross-review comparison without treating
  model agreement as repository truth.
- Provide deterministic prompt material and structured packets for the Epic 013
  helper orchestrator.
- Evaluate plan quality using real prompts and deterministic fixtures.

## Non-goals

- Execute shell commands or tools.
- Modify repository files.
- Spawn, supervise, or communicate with sub-agents.
- Choose or invoke helper or capable models; that belongs to Epic 013.
- Encode Codex-, Claude Code-, or opencode-specific protocols in the core crate.
- Grant command authorization or bypass host safety policy.
- Guarantee that every task benefits from parallelism.
- Generate a complete implementation patch from a user request.
- Use an LLM as the source of truth for project tooling.
- Use a model-assisted context finding as authoritative graph evidence without
  independent deterministic validation.
- Ask a retrieval helper to make the final product, implementation, or
  verification decision.
- Hard-code `python` to mean `pytest` or `javascript` to mean `npm test` without
  configuration evidence.
- Replace agent reasoning for ambiguous product or architecture decisions.

## Design Decisions

### 1. Typed plans precede prose prompts

Core output:

```rust
pub struct TaskPlan {
    pub objective: String,
    pub intents: Vec<IntentAssessment>,
    pub facts: Vec<DetectedFact>,
    pub context: Vec<ContextRef>,
    pub actions: Vec<ActionSpec>,
    pub diagnostics: Vec<PlanDiagnostic>,
}

pub struct ActionSpec {
    pub id: ActionId,
    pub kind: ActionKind,
    pub inputs: Vec<ResourceRef>,
    pub depends_on: Vec<ActionId>,
    pub execution: ExecutionPolicy,
    pub expected_output: OutputContract,
    pub rationale: String,
}
```

The plan can be validated, serialized, scheduled, reviewed, and rendered for
different agents. Prompt text is a view of this model, not the durable planning
format.

Rejected alternative retained for tuning: asking a model to emit a list of
prompts directly is quick to prototype, but makes dependency validation,
command safety, reproducibility, and cross-agent adapters fragile.

### 2. Planning is separate from execution

Initial action kinds:

- `search_graph`
- `search_text`
- `read_document_section`
- `read_symbol`
- `inspect_history`
- `inspect_configuration`
- `locate_tests`
- `run_command`
- `run_tests`
- `add_tests`
- `modify_code`
- `review_diff`
- `review_plan`
- `compare_reviews`
- `assemble_context_packet`
- `investigate_context_gap`
- `validate_context_candidates`
- `summarize_evidence`
- `synthesize`

Execution policies:

- `deterministic`: a host may execute with ordinary authorization checks;
- `agent`: requires reasoning or code generation;
- `optional`: useful but not required for plan completion;
- `manual`: requires a user/product decision.

The planner never interprets an execution policy as permission.

### 3. Output contracts bound context

Every action specifies what it should return:

```rust
pub struct OutputContract {
    pub format: OutputFormat,
    pub required_fields: Vec<String>,
    pub max_bytes: Option<usize>,
    pub include_provenance: bool,
}
```

Examples:

- a document action returns heading, relevant excerpt, file/line range, and
  node key;
- a history action returns commit, changed paths, rationale summary, and graph
  evidence;
- a test action returns command, exit status, failing test names, and bounded
  diagnostics;
- an implementation action returns changed files, decisions, and unresolved
  questions.

This prevents independent workers from returning entire files or unbounded logs
when the synthesis step needs only focused evidence.

### 4. Project detectors emit facts, not actions

```rust
pub trait ProjectDetector {
    fn name(&self) -> &'static str;
    fn version(&self) -> u32;
    fn detect(&self, repository: &RepositoryView)
        -> anyhow::Result<Vec<DetectedFact>>;
}
```

Fact categories include:

- language and language version;
- workspace/package boundaries;
- package manager;
- build system;
- test runner;
- formatter;
- linter;
- type checker;
- documentation set;
- CI correctness gates;
- repository-specific agent instructions;
- canonical commands and their source.

For Python, the word `python` triggers inspection for `pyproject.toml`,
`uv.lock`, `poetry.lock`, requirements files, `.python-version`, pytest/tox/nox
configuration, Ruff, mypy/pyright, Make targets, and CI workflows. It does not
immediately emit `pytest` or create a virtual environment.

Equivalent detectors inspect Cargo manifests/configuration for Rust and package
manifests/lockfiles/scripts for JavaScript and TypeScript. Generic planning
rules consume capabilities such as `TestRunner`; they should not branch on a
language when the capability is sufficient.

### 5. Facts retain provenance and confidence

```rust
pub struct DetectedFact {
    pub kind: FactKind,
    pub value: FactValue,
    pub confidence: Confidence,
    pub evidence: Vec<ResourceRef>,
    pub detector: DetectorIdentity,
}
```

An exact `cargo check --workspace` instruction from `AGENTS.md` is stronger than
guessing `cargo check` from the presence of `Cargo.toml`. Conflicting facts are
preserved as diagnostics and may produce an investigation or manual-decision
action.

### 6. Intent classification is conservative

Initial intents:

- explain/research;
- diagnose;
- implement feature;
- fix bug;
- refactor;
- add/update tests;
- update documentation;
- review;
- verify;
- ambiguous.

Keyword rules alone must not turn “explain how to fix the parser” into
authorization to edit code. When confidence is insufficient, the plan contains
read-only investigation and a decision boundary rather than mutation.

Ambiguous intent remains explicit in the plan and is handed to the capable
model or user as a decision boundary. The small/tiny context helper does not
classify intent. Deterministic constraints and explicit user verbs remain
inspectable inputs to the final assessment.

### 7. Rules consume capabilities and graph patterns

```rust
pub trait PlanningRule {
    fn name(&self) -> &'static str;
    fn version(&self) -> u32;
    fn applies(&self, state: &PlanningState) -> bool;
    fn expand(&self, state: &PlanningState)
        -> anyhow::Result<Vec<ActionCandidate>>;
}
```

Representative generic rules:

```text
Implementation intent + linked requirement
    -> read exact requirement and parent constraints

Code mutation intent + TestRunner fact
    -> locate focused tests
    -> run/reproduce focused target
    -> modify code
    -> rerun focused target
    -> run repository correctness gate

Bug intent + linked history
    -> inspect current symbol
    -> inspect relevant tests
    -> inspect rationale-bearing commits
    -> summarize likely cause before mutation

Refactor intent
    -> establish behavior contract
    -> locate callers/tests where evidence exists
    -> modify bounded region
    -> verify behavior and review diff
```

Rules may propose candidates that a plan normalizer deduplicates. A rule must
state which fact or graph evidence caused it to apply.

### 8. Parallelism is derived from dependencies

The plan stores a directed acyclic action graph. Independent actions can be
grouped for concurrent execution, but “use sub-agents” is not itself an action.

For example, reading a PRD section, inspecting linked code, locating tests, and
inspecting history may run independently. Implementation must wait for the
evidence summary; verification must wait for code changes.

The planner must:

- reject dependency cycles;
- use deterministic ordering within a parallel group;
- allow a sequential host to execute the same plan;
- avoid splitting tiny adjacent reads whose coordination cost exceeds their
  benefit;
- avoid concurrent actions that would mutate overlapping files.

### 9. Context bundles are bounded and provenance-linked

```rust
pub struct ContextBundle {
    pub objective: String,
    pub shared_context: Vec<ContextResource>,
    pub requested_changes: Vec<RequestedChangeClaimRef>,
    pub resources: Vec<ContextResource>,
    pub facts: Vec<DetectedFact>,
    pub unresolved: Vec<OpenQuestion>,
    pub budget: ContextBudget,
}
```

Every resource includes its `NodeKey` or file/span provenance and the graph path
that selected it. Deduplicate overlapping document chunks and nested code
symbols. Prefer exact/structural paths before lexical/semantic candidates.

For a deterministically split implementation epic, `shared_context` retains the
bounded goals, assumptions, non-goals, target design, and verification contract
needed to interpret each task. `requested_changes` preserves what the exact
task version explicitly asks to add, remove, replace, or verify. These are
attributed requirement claims, not detected facts that the future code already
exists and not authorization to mutate.

For reviews and other derived analyses, a resource also retains the exact
`graph_node_versions` input it examined when available. Comparing a review
against a later document version without disclosing the version mismatch is a
planning error. A node-level review link with no exact version remains usable
only with a provenance-gap diagnostic.

Budget overflow is explicit: the planner records omitted candidates and why,
rather than silently truncating a requirement or source block.

For task-oriented retrieval, including one testing-sheet row, the planner wraps
the bundle in an evidence packet with an explicit completeness contract:

```rust
pub struct TaskEvidencePacket {
    pub objective: String,
    pub primary_input: ResourceRef,
    pub repository_version: RepositoryVersionRef,
    pub shared_context: Vec<ContextResource>,
    pub requested_changes: Vec<RequestedChangeClaimRef>,
    pub established: Vec<ContextResource>,
    pub candidates: Vec<ContextCandidate>,
    pub coverage: Vec<SourceCoverage>,
    pub omissions: Vec<ContextOmission>,
    pub unresolved_gaps: Vec<ContextGap>,
    pub investigation_attempts: Vec<InvestigationAttemptRef>,
}
```

The packet states which source partitions, index/detector versions, languages,
artifact kinds, and reference/runtime capabilities were available. It keeps
semantic-only and externally proposed candidates separate from established
graph paths. Zero-result searches, unsupported capabilities, and budget
omissions are observations, not proof that no relevant artifact exists.

Task packets may overlap in affected resources. The planner uses that overlap
to create dependencies or serialize mutations; it must not infer exclusive
file ownership from task headings. Querying the same task after a prerequisite
lands may legitimately return a smaller remaining change surface, so packet
identity includes the graph/repository version.

### 10. Commands are structured and evidence-derived

```rust
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub source: ResourceRef,
    pub scope: CommandScope,
}
```

Never represent a generated command only as a shell string. Do not interpolate
user text into command arguments without validation. The host remains
responsible for sandboxing, approval, and execution.

A focused command may be derived from:

- an exact repository instruction;
- a manifest script;
- test configuration and a known test target;
- an established generic adapter with explicit confidence.

When only a broad correctness gate is known, the plan says so rather than
inventing a focused invocation.

### 11. Planner output is the deterministic substrate for orchestration

This epic defines the facts, constraints, plans, evidence packets, gaps,
budgets, and output contracts consumed by Epic 013. It does not implement or
configure a helper model, capable model, model adapter, tool-calling loop, or
response assessment.

The planner must preserve enough structure for the mandatory helper to:

- distinguish human intent from detected repository facts;
- see which context is established, candidate-only, omitted, or unavailable;
- ask bounded graph questions without inventing filesystem access;
- retain exact repository and graph versions across prompt rewrites;
- identify decisions that require human clarification;
- construct a capable-model output contract without losing constraints.

Core planner tests require no model or network. A host may inspect planner
output independently for diagnostics, but production capable-model
conversations use the mandatory Epic 013 helper path.

### 12. Context-gap investigations are deterministic, bounded, and read-only

The planner may emit `investigate_context_gap` only from a recognized gap in a
`TaskEvidencePacket`. Epic 013 decides how its mandatory helper reasons over the
gap and whether it requests a permitted graph operation. Model execution,
credentials, tool schemas, and orchestration policy remain outside planner
core.

```rust
pub struct InvestigationSpec {
    pub gap: ContextGap,
    pub question: String,
    pub seeds: Vec<ResourceRef>,
    pub allowed_tools: Vec<ToolCapability>,
    pub budget: InvestigationBudget,
    pub expected_output: InvestigationOutputContract,
}

pub struct InvestigationResult {
    pub observations: Vec<ObservedFinding>,
    pub candidates: Vec<CandidateFinding>,
    pub attempted_queries: Vec<ToolObservation>,
    pub unresolved: Vec<OpenQuestion>,
    pub budget_exhausted: bool,
}
```

Triggers are inspectable rules, not model decisions. Examples include:

- a strong requirement candidate with no implementation path;
- an ambiguous symbol collision;
- no test connected to a candidate behavior;
- a committed code change with no linked decision or rationale document;
- conflicting PRD and later decision sections;
- semantic candidates requiring focused source inspection.

Allowed capabilities initially cover graph query, bounded text/symbol/reference
search, focused file-range reads, and focused Git log/diff inspection. They do
not grant general shell access, mutation, dependency installation, or test
execution. Behavioral reproduction remains a separate authorized action for a
capable agent or execution host.

Every returned finding distinguishes an observed tool fact from model
interpretation and cites exact resources/spans. `validate_context_candidates`
reproduces eligible paths, symbols, spans, references, configuration keys, and
changed-file facts using deterministic validators. The validator's evidence,
not the model assertion, may strengthen the graph. Unsupported interpretations
remain attributed candidates in the capable-model packet.

Investigation is iterative but bounded. Plans record stable attempt identity,
tool-call/byte/depth limits, duplicate findings, new validated resources, new
candidate resources, remaining gaps, and budget exhaustion. Stop conditions
and maximum enrichment rounds are explicit evaluation parameters. Failure or
low yield does not block handoff of the initial packet.

### 13. Plan diagnostics are product data

Record:

- missing project facts;
- conflicting commands;
- graph evidence gaps;
- discarded or deduplicated actions;
- unsafe/unresolved command candidates;
- context budget omissions;
- reasons an action cannot run in parallel;
- manual decisions required before mutation.
- unavailable chat-only reviews or missing review attribution;
- reviews targeting stale or unknown document versions;
- cross-review agreement unsupported by independent repository evidence.
- unsupported source/index/reference/runtime coverage;
- context-gap trigger and investigation budget;
- malformed or unvalidated model-proposed findings;
- duplicate investigations and marginal context yield;
- exhausted enrichment rounds and remaining gaps.

These diagnostics drive future detector and graph improvements and must be
available to thought experiments and evaluation tooling.

### 14. Cross-review planning separates claims from evidence

For a request to cross-check an epic or plan with multiple models, the planner
first produces a bounded evidence review of the exact source version. Saved
human/model reviews can then be compared as attributed claim sets.

Representative flow:

```text
Parallel evidence group
  A. Read the exact epic sections under review.
  B. Inspect mentioned component/code APIs.
  C. Inspect affected C ABI/configuration/public contracts.
  D. Inspect relevant history, tests, and verification facilities.

Synthesis
  E. Produce evidence-linked findings against the reviewed version.

Optional cross-review group
  F. Ingest or read review 1.
  G. Ingest or read review 2.

Comparison
  H. Classify agreement, disagreement, unique findings, unsupported claims,
     stale-version findings, and missing provenance.
```

The planner may emit a review action and its evidence contract, but Epic 013
owns all helper and capable-model turns. The planner does not execute a model
call or assume that a chat-only result was persisted. Two reviews reaching the
same conclusion count as claim agreement. Confidence increases only when their
claims point to independent structural, exact, test, configuration, or history
evidence.

## Representative Plans

### Plan A: Requirement implementation

Input:

> Implement Section 6.13: Admin can see the required columns in the review
> queue.

Expected plan:

```text
Parallel investigation group
  A. Read Section 6.13 and its parent requirement constraints.
  B. Inspect graph-linked review queue symbols and containing files.
  C. Locate tests linked to the review queue behavior.
  D. Inspect commits connected to those files/symbols.

Barrier
  E. Summarize required behavior, current behavior, rationale, test gaps,
     uncertainties, and provenance.

Implementation group
  F. Add or update focused tests.
  G. Modify the smallest supported code region.

Verification group
  H. Run the focused test target.
  I. Run the repository correctness gate.
  J. Review the diff against Section 6.13 and report unresolved requirements.
```

Actions A–D are parallel only if their inputs are already resolved and bounded.
F and G may be sequential when using test-driven development. H–J depend on the
implementation outputs.

### Plan B: Python bug fix

Input:

> Fix the Python import cache bug and add a regression test.

Detected facts might establish:

```text
Python 3.12 from .python-version
uv from uv.lock
pytest from pyproject.toml
Ruff from pyproject.toml
canonical full gate from AGENTS.md
```

Expected plan:

```text
Parallel investigation group
  A. Inspect linked import-cache symbols and their callers/evidence.
  B. Locate focused cache/import tests.
  C. Inspect commits and documents explaining cache invalidation.
  D. Confirm exact test/lint commands from configuration.

Barrier
  E. State the reproducible failure hypothesis and focused test target.

Implementation sequence
  F. Run the focused test to reproduce.
  G. Add a failing regression test if none exists.
  H. Modify the linked implementation.
  I. Rerun the focused test.
  J. Run configured lint/type/full test gates.
```

The presence of “Python” initiates capability inspection; it does not justify a
guessed `python -m pytest` command when the repository uses `uv run pytest`,
tox, nox, or a wrapper.

### Plan C: History-based explanation

Input:

> Why is retry limited to one attempt? Do not change anything.

Expected plan:

```text
Parallel read-only group
  A. Read the exact retry configuration/symbol.
  B. Read linked design-document sections.
  C. Inspect commits that changed the symbol or containing file.
  D. Read linked tests that encode the one-attempt behavior.

Synthesis
  E. Explain current behavior, likely rationale, confidence, conflicting
     evidence, and source links.
```

No mutation or test-writing action is allowed because the request is
explanatory.

### Plan D: Behavior-preserving refactor

Input:

> Move token validation out of the HTTP handler without changing behavior.

Expected plan:

```text
Parallel investigation group
  A. Inspect the handler and validation evidence.
  B. Locate focused behavior tests.
  C. Inspect documented constraints and relevant history.
  D. Determine package/module boundaries and verification commands.

Barrier
  E. Summarize the behavior contract and unresolved reference/call-graph gaps.

Implementation sequence
  F. Strengthen focused tests only if the contract is not already protected.
  G. Extract validation with the smallest public API change.
  H. Run focused tests and the correctness gate.
  I. Review the diff for behavior or dependency-direction changes.
```

The planner must not claim it has found every caller unless the graph contains
the corresponding evidence.

### Plan E: Cross-model epic review

Input:

> MiMo authored this epic. Codex reviewed and revised it. Compare the reviews
> and show which planning changes are supported by the repository.

Expected plan:

```text
Parallel evidence group
  A. Read the exact original epic version by Git blob-backed provenance.
  B. Inspect exact symbols, APIs, C ABI surfaces, tests, and commands mentioned
     by the epic.
  C. Inspect the original-to-revised document diff and relevant history.
  D. Read each persisted review and its prompt/model/version metadata.

Barrier
  E. Normalize review findings as attributed claims linked to exact source
     sections and evidence.

Comparison
  F. Report agreements, disagreements, unique findings, unsupported claims,
     stale-version claims, and provenance gaps.
```

If a review exists only in chat and was not ingested, D returns a missing
artifact diagnostic. The planner can still compare the committed document
versions, but must not infer who generated the revision from Git author
metadata.

### Plan F: Examine one client testing-sheet row

Input:

> Examine this testing-sheet row against the PRD, earlier decisions, code,
> tests, and history. Decide whether it should be implemented.

Expected plan:

```text
Parallel initial retrieval
  A. Read the exact versioned row, cells, and table headers.
  B. Retrieve requirement and decision candidates with parent constraints.
  C. Retrieve implementation and test candidates across source partitions.
  D. Inspect configuration and history for strong candidate files.

Barrier
  E. Assemble an evidence packet with established paths, candidates, coverage,
     omissions, and deterministic context gaps.

Conditional bounded enrichment
  F. Investigate only recognized gaps using short prompts, graph-selected
     seeds, specific read-only tools, and explicit call/byte/depth budgets.
  G. Validate cited exact/structural observations independently.
  H. Rebuild the packet with investigation provenance, yield, and remaining
     gaps.

Capable-model decision
  I. Classify the row and recommend reject, defer, clarify, reproduce, or
     implement.

Conditional implementation
  J. Establish behavior or add a focused failing test.
  K. Modify the smallest supported code region.
  L. Run focused and repository verification.
  M. Review the result against the row and governing decision versions.
```

F-H are absent when E reports sufficient coverage or no supported investigation
can improve it. The small/tiny model is strictly a context-retrieval helper; it
does not make the product decision or authorize J-M.

### Plan G: Implement one task from a multi-file epic

Input:

> Implement Task 5 from the contact-ownership refactor epic.

Expected plan:

```text
Versioned task input
  A. Read the exact Task 5 section and bounded shared epic goals, assumptions,
     non-goals, target schema, and verification contract.
  B. Normalize explicit requested changes as attributed claims.

Parallel current-state retrieval
  C. Resolve named tables, fields, types, functions, migrations, and commands
     against the selected repository version.
  D. Expand exact/structural seeds to candidate consumers and focused tests.
  E. Inspect history for the current deduplication and ownership behavior.

Barrier
  F. Assemble established facts, requested changes, candidate change surface,
     cross-task overlap, unsupported reference/data-flow coverage, and open
     design questions.

Implementation
  G. Decide concrete signatures, transaction behavior, and compatibility where
     the epic does not specify them.
  H. Modify the evidence-supported regions without assuming Task 5 exclusively
     owns shared files.
  I. Run focused and repository gates, then perform exact old-identifier
     absence checks within declared coverage.
  J. Review the diff against both Task 5 and the shared epic constraints.
```

If Tasks 1–4 have already landed, C-F are recomputed against that newer
repository version rather than replaying a stale pre-epic change surface.
Requested-change claims say what the epic asks for; they do not prove that a
table exists, authorize mutation, or determine a concrete patch. Runtime SQLx,
database, compiler, and smoke-test discoveries remain execution results.

## Tasks

### Task 0: Run and pass the blocking planner thought experiments

Perform the process in “Blocking Thought-Experiment Gate,” compare proposed
plans with the actual work previously done, and revise this epic.

#### Acceptance Criteria

- [ ] At least eight real prompts across at least three repositories are
  documented; at least six are from prior prompting experience.
- [ ] All required request categories are represented.
- [ ] Each experiment records facts, graph queries, actions, dependencies,
  parallel groups, output contracts, commands, uncertainty, and failure modes.
- [ ] At least six plans meet every quality threshold in the gate.
- [ ] At least two experiments demonstrate that an initially attractive action
  should be removed, delayed, or made manual.
- [ ] At least one experiment demonstrates missing graph evidence and feeds a
  concrete follow-up into Epic 011 or a later epic.
- [x] At least one real per-row testing-sheet experiment compares search-only,
  graph-only, search-plus-graph, and bounded model-assisted retrieval against
  the artifacts a capable agent ultimately used.
- [x] At least one experiment records deterministic context-gap triggers,
  read-only tool/budget contracts, validated versus candidate findings,
  investigation yield, and stop/escalation behavior.
- [ ] At least one experiment covers a real cross-model plan review and
  distinguishes exact document versions, attributed claims, repository
  evidence, and an unavailable chat-only artifact.
- [ ] At least one multi-file epic experiment plans per-task implementation
  using bounded shared epic context and requested-change claims, compares the
  candidate change surface with actual diffs, and records cross-task overlap
  plus runtime/build-only misses.
- [ ] Plan size and expected tool/context savings are compared with the
  original prompting workflow.
- [ ] This epic's models, rules, and tasks are revised from the results.
- [ ] The research artifact is reviewed and committed.
- [ ] Tasks 1–8 have not begun before every Task 0 criterion is checked.

### Task 1: Add the planner crate and typed plan model

Create `crates/daftprompt-planner`, add it to the workspace, and implement
serializable intents, facts, resources, actions, dependencies, output contracts,
diagnostics, and plans.

#### Acceptance Criteria

- [ ] The crate depends on graph abstractions but not the daftprompt binary/UI.
- [ ] Plans round-trip through JSON without losing provenance or unknown
  diagnostic values.
- [ ] Review actions retain exact reviewed-version references and attributed
  claim sources.
- [ ] Requested-change claims round-trip with exact epic/task version, source
  span, shared-context references, normalized operation when known, and
  ambiguity diagnostics.
- [ ] Task evidence packets and investigation specs/results round-trip without
  losing coverage, gaps, attribution, observed/inferred status, or budgets.
- [ ] Action IDs and ordering are deterministic for identical inputs.
- [ ] Dependency cycles are rejected with a useful error.
- [ ] A sequential host and a parallel host can interpret the same plan.
- [ ] Unit tests cover validation, serialization, and normalization.
- [ ] `cargo check --workspace` passes.

### Task 2: Implement project fact detectors

Build generic repository inspection plus initial Rust, Python, and
JavaScript/TypeScript capability detectors.

#### Acceptance Criteria

- [ ] Rust detects workspace/package boundaries, Cargo test/check commands, and
  explicit repository instructions.
- [ ] Python inspects the configuration/lock/tool files listed in Design
  Decision 4 without assuming pytest or a venv.
- [ ] JavaScript/TypeScript detects package manager from lockfiles and commands
  from manifest scripts/configuration.
- [ ] Conflicting package managers or commands produce diagnostics.
- [ ] Every fact links to its source file/span and detector version.
- [ ] Repository-specific instructions outrank generic guesses while remaining
  visible as evidence.
- [ ] Detectors perform no network access or tool installation.

### Task 3: Implement conservative request intent classification

Classify explicit user intent, scope, mutation constraints, and uncertainty.

#### Acceptance Criteria

- [ ] Explain/diagnose requests do not acquire mutation actions.
- [ ] Implement/fix/refactor/test/review/verify intents are distinguishable.
- [ ] Negative constraints such as “do not change anything” are retained.
- [ ] Ambiguous prompts produce investigation plus a decision boundary.
- [ ] Small/tiny context-investigation results cannot alter explicit intent or
  mutation constraints.
- [ ] Fixture prompts include mixed and misleading verbs.

### Task 4: Implement graph context selection and budgets

Query Epic 011, choose evidence-backed context, deduplicate overlap, and enforce
explicit budgets.

#### Acceptance Criteria

- [ ] Structural/exact paths rank ahead of lexical/semantic-only paths.
- [ ] Selected resources retain complete graph evidence paths.
- [ ] Review resources target the exact document/node version where available;
  stale or unknown targets produce diagnostics.
- [ ] Parent requirement context is included without returning whole documents.
- [ ] Split-task selection includes bounded shared goals, assumptions,
  non-goals, target design, and verification requirements needed to interpret
  the task.
- [ ] Explicit requested-change claims seed current-code retrieval but remain
  distinct from established graph facts and mutation authorization.
- [ ] Nested/overlapping symbols and document chunks are deduplicated.
- [ ] Budget omissions and missing evidence appear in diagnostics.
- [ ] Coverage reports source/index/detector availability, unsupported
  artifact/reference/runtime capabilities, and zero-result observations.
- [ ] Established graph evidence, semantic candidates, and externally proposed
  candidates remain distinct.
- [ ] Recognized context gaps are stable typed values suitable for planning
  rules.
- [ ] Identical graph/request inputs produce identical context ordering.
- [ ] Packet identity includes the repository/graph version; overlapping tasks
  may select the same resources without implying exclusive ownership.
- [ ] Absence checks preserve exact indexed coverage and cannot become
  universal “no references exist” claims.
- [ ] An unavailable/incomplete graph falls back to bounded search actions
  rather than panicking.

### Task 5: Implement the initial generic planning rule catalog

Implement investigation, implementation, bug, refactor, test, documentation,
review, and explanation rules using capabilities and graph patterns.

#### Acceptance Criteria

- [ ] Every emitted action cites the rule and evidence that caused it.
- [ ] Review comparison rules separate agreement between claims from
  independent repository corroboration.
- [ ] An unavailable chat-only review produces an ingestion/read action or
  diagnostic rather than reconstructed content.
- [ ] Rules consume capabilities such as `TestRunner`, not unnecessary language
  branches.
- [ ] Investigation precedes mutation when required facts are missing.
- [ ] Context-gap rules emit bounded investigations only for explicit typed
  gaps and coalesce adjacent low-cost gaps where appropriate.
- [ ] Enrichment failure or low yield preserves the original packet and causes
  a diagnostic or capable-model escalation rather than recursive discovery.
- [ ] Focused verification precedes broad gates when a reliable target exists.
- [ ] Broad correctness gates come from project facts.
- [ ] Duplicate rule output is normalized without losing rationale.
- [ ] Rules may derive candidate edit scope from requested-change claims and
  current evidence, but concrete patch steps remain agent/manual actions when
  signatures, transaction behavior, compatibility, or product decisions
  require reasoning.
- [ ] Rule fixtures cover all representative plans in this epic.

### Task 6: Implement dependencies, parallel groups, and command specs

Build and validate the action DAG, derive safe concurrency groups, and represent
commands structurally.

#### Acceptance Criteria

- [ ] Dependency cycles and references to missing action IDs fail validation.
- [ ] Stable topological ordering and parallel groups are produced.
- [ ] Read-only independent investigations can share a group.
- [ ] Mutations with overlapping file/resource scope cannot be marked
  independent.
- [ ] Command program, args, cwd, source, and scope remain separate fields.
- [ ] No command is treated as authorized by the planner.
- [ ] Unknown focused-test syntax yields a diagnostic rather than an invented
  command.

### Task 7: Add deterministic rendering and orchestration handoff types

Render plans/actions to concise generic prompt material and JSON. Define the
typed packet and gap boundary consumed by Epic 013.

#### Acceptance Criteria

- [ ] Generic prompts include objective, bounded inputs, evidence, constraints,
  and output contract.
- [ ] Prompts do not reproduce unrelated files or unbounded prior outputs.
- [ ] JSON is sufficient for a host adapter without parsing prose.
- [ ] Core planning and tests require no model or network.
- [ ] Context-investigation prompts contain one typed gap, graph-selected seeds,
  proposed graph capabilities, explicit call/byte/depth budgets, and a
  structured observed-versus-inferred output contract.
- [ ] Planner core emits and consumes investigation specs/results but never
  chooses a model, defines the final tool catalog, calls tools, or treats a
  finding as authorized mutation.
- [ ] Eligible cited observations pass through deterministic validation;
  unvalidated interpretations remain attributed candidates.
- [ ] Attempt identity, duplicate findings, marginal yield, remaining gaps, and
  stop conditions are preserved.
- [ ] No agent brand or proprietary protocol is embedded in the core types.

### Task 8: Integrate CLI evaluation, documentation, and validation

Expose plan inspection without execution and add a repeatable evaluation
harness for the thought-experiment corpus.

Candidate command:

```bash
cargo run -- --repo . --plan "Implement Section 6.13"
cargo run -- --repo . --plan-json "Fix the import cache bug"
```

#### Acceptance Criteria

- [ ] CLI planning performs no repository mutation or command execution.
- [ ] Human output shows facts, context provenance, actions, dependencies,
  parallel groups, and diagnostics.
- [ ] JSON output is stable enough for experimental host adapters.
- [ ] The thought-experiment corpus can be replayed as evaluation fixtures
  without private repository contents being committed inadvertently.
- [ ] Evaluation compares search-only, graph-only, search-plus-graph, and
  bounded model-assisted retrieval using artifact recall, irrelevant context,
  packet sufficiency, tool calls, context bytes, and time to justified
  decision.
- [ ] `README.md`, `DEVELOP.md`, and `AGENTS.md` document planning scope and
  safety boundaries.
- [ ] A focused review checks intent safety, provenance loss, cycles, unsafe
  command construction, excessive fragmentation, and false parallelism.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes.

## Test Matrix

| Area | Required evidence |
|---|---|
| Plan model | JSON round trip, stable IDs, unknown values, validation |
| Intent | explicit, negative, mixed, and ambiguous requests |
| Rust facts | workspace, tests, check gate, repository instructions |
| Python facts | uv/poetry/pip, pytest/tox/nox, lint/type tools, conflicts |
| JS/TS facts | npm/pnpm/yarn/bun lockfiles and manifest scripts |
| Graph selection | evidence priority, parent context, deduplication |
| Epic task context | bounded shared context, requested-change claims, exact repository version, overlapping change surfaces |
| Context completeness | source/index coverage, unsupported capabilities, omissions, zero results, typed gaps |
| Context investigations | deterministic triggers, read-only tools, budgets, validation, attribution, failure fallback |
| Investigation iteration | stable attempts, duplicate findings, marginal yield, stop/escalation behavior |
| Review selection | exact reviewed version, stale/unknown version diagnostics, attributed claims |
| Cross-review | agreement, conflict, unique findings, unsupported claims, missing chat artifact |
| Budgets | bounded output, explicit omissions |
| Rules | research, diagnose, implement, fix, refactor, test, review |
| DAG | cycles, stable ordering, parallel and sequential interpretation |
| Mutations | overlapping-resource serialization |
| Commands | structured args, evidence source, no guessed focused command |
| Rendering | concise generic prompt, JSON, no-model fallback |
| Evaluation | replayed real-prompt cases and quality diagnostics |

## File-change Summary

| File | Change |
|---|---|
| `Cargo.toml` | Add `crates/daftprompt-planner` to the workspace and dependencies. |
| `crates/daftprompt-planner/Cargo.toml` | New reusable planner crate manifest. |
| `crates/daftprompt-planner/src/lib.rs` | Public planner API and orchestration. |
| `crates/daftprompt-planner/src/model.rs` | Typed intents, facts, actions, plans, and diagnostics. |
| `crates/daftprompt-planner/src/detectors/` | Generic and ecosystem project fact detectors. |
| `crates/daftprompt-planner/src/rules/` | Versioned generic planning rules. |
| `crates/daftprompt-planner/src/context.rs` | Graph selection, deduplication, and budgets. |
| `crates/daftprompt-planner/src/investigation.rs` | Typed context gaps, bounded investigation contracts, validation, and yield. |
| `crates/daftprompt-planner/src/render.rs` | Generic prompt and JSON rendering. |
| `src/main.rs` | Add read-only plan inspection CLI. |
| `epics/research/012-planner-thought-experiments.md` | Blocking real-prompt planning evidence. |
| `README.md`, `DEVELOP.md`, `AGENTS.md` | Document planner behavior and safety boundaries. |

The exact internal module split must follow Task 0 findings. The public typed
boundary and separation from execution are requirements.

## Risks and Follow-ups

- Over-decomposition can cost more coordination/context than it saves. Rules
  need minimum useful action sizes.
- Repository instructions may conflict with manifests or CI; diagnostics and
  precedence must remain visible.
- A provenance graph can be incomplete without being obviously wrong. Plans
  must communicate coverage limits.
- Small-model context enrichment can improve recall while obscuring provenance;
  observed facts, interpretations, deterministic validation, and final
  capable-model decisions must remain separate.
- Deterministic triggers can still cause wasteful repeated investigations;
  evaluation must tune coalescing, budgets, marginal-yield thresholds, and stop
  conditions from real rows.
- Deterministic rules can become a scattered language matrix. Capability facts
  and data-driven conventions should remain the shared boundary.
- Commands inferred from configuration still require host authorization.
- Real prompt fixtures may contain confidential project information; sanitize
  them while retaining the structural planning challenge.
- Model adapters, helper/capable-model communication, graph tool calling,
  response assessment, and prompt-engineering evaluation belong to Epic 013.
