# Epic 013: Graph-Tool Helper Orchestrator

## Introduction

Epics 011 and 012 provide two complementary deterministic foundations:

1. an explainable provenance graph with explicit evidence, coverage, and gaps;
2. typed plans and context packets derived from repository facts and graph
   evidence.

This epic adds the model foundation that operates alongside them. A deliberately
selected small/tiny language model (the **helper**) sits directly above the
graph and between the human and one or more capable models. Together, the
helper and graph act as a graph-grounded prompt engineer. The helper turns an
informal, incomplete, or poorly structured human request into the precise
question, context, constraints, evidence, and output contract that a capable
model needs. It owns the conversation loop, uses a closed set of graph tools to
gather context, shapes the prompts sent to capable models, and inspects their
responses to decide whether another graph investigation, human clarification,
or prompt revision is required.

The helper path is mandatory. Capable models receive no tools and no direct
repository access. From a capable model's perspective, it is participating in a
normal conversation with a well-informed human collaborator. The helper makes
that illusion useful by supplying focused evidence, asking follow-up questions,
and relaying results while deterministic code enforces graph access, budgets,
provenance, validation, and loop termination.

This is intentionally an experiment. Whether the architecture or the emergent
helper+graph behavior is novel is an open question that cannot be answered
confidently before it is built and observed. The product hypothesis is that
humans are often poor at expressing complex requests systematically, while
models below 20B parameters may be good enough to identify what is missing,
interrogate a structured graph, and ask a more capable model a substantially
better question. Prior experiments make that hypothesis plausible, but not
proven. The helper-to-graph and helper-to-capable-model protocols are expected
to change repeatedly as evidence accumulates. The implementation and its
recorded interaction traces should make it possible to compare the resulting
behavior with known agent, retrieval, and prompt-optimization patterns. If the
approach fails, the architecture must make the failure measurable and permit
the helper model, prompts, protocol, or orchestration policy to be replaced
without weakening the graph's trust boundary.

## Product Hypothesis

The main gap addressed by this epic is not raw model intelligence or repository
search. It is the translation between human intent and a high-quality model
request.

A human may provide:

- an underspecified goal;
- relevant but unstructured background;
- implicit success criteria;
- conflicting constraints;
- the wrong technical vocabulary;
- too much irrelevant detail;
- no awareness of which repository evidence is available.

The helper+graph system should turn that input into:

- a clarified objective without silently changing intent;
- explicit known constraints and unresolved decisions;
- the smallest useful evidence-backed context;
- precise questions for the capable model;
- a suitable output contract;
- follow-up prompts when the first response reveals missing context;
- a final response whose claims and limitations remain traceable.

The helper is therefore evaluated as a prompt engineer and conversational
intermediary, not merely as a retrieval agent. Graph tool use is one mechanism
for building better prompts.

## Dependencies

Requires:

- Epic 011 graph queries, evidence packets, coverage reports, candidate
  validation, and stable repository/version identity;
- Epic 012 project facts, intent constraints, typed plans, context budgets, and
  deterministic prompt material.

The dependency direction is:

```text
daftprompt-graph       daftprompt-planner
         \                 /
          \               /
           helper-orchestrator
                    |
             capable model(s)
```

The graph and planner crates must not depend on the orchestrator. The
orchestrator may consume both. Model/provider adapters remain outside all three
core crates.

## Blocking Experiment Gate

**Task 0 is a hard blocker. No implementation task may begin until Task 0 is
complete.**

Use real conversations and prompts from prior experiments with models below 20B
parameters. Include at least twelve cases across at least three repositories and
at least three helper models. The set must cover:

- sufficient initial deterministic context requiring no tool call;
- missing implementation, test, configuration, decision, or history context;
- ambiguous requests requiring a human clarification;
- a capable-model response that exposes a new context gap;
- a capable-model response that incorrectly asks for unavailable or unnecessary
  tooling;
- a helper that over-calls tools;
- a helper that stops too early;
- conflicting graph evidence;
- prompt injection or untrusted instructions in repository content;
- exhausted graph coverage where no tool call can answer the question;
- multi-turn implementation or review planning;
- comparison of one capable model against more than one helper model;
- a vague human request whose intended outcome is recoverable through helper
  clarification;
- a verbose but poorly structured human request that should become a smaller,
  clearer capable-model prompt;
- a case where the helper must preserve genuine ambiguity rather than
  over-specify the prompt;
- a case where a technically polished helper prompt nevertheless changes the
  human's intended outcome.

For every case, record:

1. Human input and explicit intent/mutation constraints.
2. Initial deterministic facts, plan, and evidence packet.
3. The exact helper prompt and model/version.
4. Tool schemas exposed to the helper.
5. Every requested and executed graph operation.
6. Tool-call, byte, token, round, and elapsed-time budgets.
7. Prompt(s) sent to the capable model.
8. The capable-model response and the helper's response assessment.
9. Any follow-up graph calls or capable-model turns.
10. Final human-visible output and preserved provenance.
11. Relevant artifacts ultimately needed by a strong tool-enabled baseline.
12. Missed context, irrelevant context, duplicate calls, unsupported claims,
    and incorrect stop/continue decisions.
13. Whether deterministic policy corrected or constrained the helper.
14. Whether the same result could have been achieved more reliably without the
    helper.
15. Which prompt-engineering transformation the helper performed: clarification,
    decomposition, terminology normalization, evidence selection, constraint
    extraction, question sequencing, or output-contract design.
16. Whether the transformed prompt preserved the human's intended outcome.

Compare at minimum:

- capable model with broad native tools;
- capable model receiving the raw human prompt without tools;
- capable model with deterministic packet only;
- mandatory helper plus deterministic graph tools;
- mandatory helper with each candidate small/tiny model;
- ablations for response inspection and deterministic stop rules.
- the same capable model receiving human-authored versus helper-authored prompts.

Implementation remains blocked unless the selected helper configuration:

- matches or improves relevant-artifact recall over deterministic packets alone;
- materially reduces direct context and tool surface exposed to the capable
  model;
- avoids materially worse unsupported-claim and task-completion rates than the
  tool-enabled capable-model baseline;
- reliably detects at least 80% of planted or known post-response context gaps;
- keeps unnecessary graph operations within an agreed threshold;
- survives prompt-injection fixtures without expanding its capabilities;
- terminates every fixture within explicit round and budget limits.
- preserves intended outcome and explicit constraints at an agreed high rate;
- materially improves capable-model answer quality or decision usefulness over
  raw human prompts on the selected corpus.

Write the results under
`epics/research/013-helper-orchestrator-experiments.md`. Document thresholds,
failed models, prompt variants, and rejected policies rather than reporting only
the winning setup.

## Goals

- Make the helper a foundational, mandatory participant in every capable-model
  conversation.
- Make helper+graph operate as a prompt-engineering system above repository
  evidence.
- Translate informal human requests into systematic capable-model prompts
  without erasing ambiguity or changing intent.
- Use dialogue with the human when missing product intent cannot be recovered
  from graph evidence.
- Combine helper reasoning and deterministic graph/planner code as one
  orchestration system with an explicit division of authority.
- Give only the helper tool-calling ability.
- Restrict helper tools to typed, read-only graph operations.
- Give capable models no tools, filesystem access, command execution, or graph
  API.
- Let the helper assemble, reshape, and sequence prompts sent to capable models.
- Inspect every capable-model response for context needs, unsupported claims,
  unresolved ambiguity, and useful follow-up questions.
- Allow response-driven graph investigation without allowing free-form tool
  execution.
- Preserve human intent and negative constraints across every rewritten prompt.
- Preserve evidence provenance through helper calls, capable-model turns, and
  human-visible output.
- Support deliberately verbose helper prompts and a closed, empirically selected
  set of open-weight helper models.
- Make helper/model/prompt/policy replacement possible behind typed boundaries.
- Measure context yield, orchestration quality, latency, token use, and failure
  modes against strong baselines.
- Measure intent preservation, clarification quality, prompt quality, and the
  capable model's improvement over receiving the raw human request.

## Non-goals

- Give the helper shell, filesystem, network, editor, test-runner, or mutation
  tools.
- Give capable models hidden or direct tools.
- Let either model create authoritative graph evidence.
- Let the helper override explicit human intent, authorization, or negative
  constraints.
- Treat a capable-model statement as proof that a repository fact exists.
- Implement repository mutations or command execution in this epic.
- Pretend that a small model is reliable without deterministic policy,
  validation, budgets, and evaluation.
- Claim or dismiss novelty before the system is built, observed, and compared
  with relevant prior work.
- Hide from the human that an orchestrated model system produced the answer.
- Support arbitrary provider/model strings in the first-party configuration.
- Permit unbounded recursive conversations between helper and capable models.

## Design Decisions

### 1. The helper is a co-foundation, not an optional fallback

The runtime always constructs a helper session before communicating with a
capable model. There is no production mode that silently bypasses the helper.
If the helper cannot be loaded or invoked, the orchestration request fails
clearly with the deterministic packet still available for diagnostics or manual
inspection.

This differs from graceful degradation inside indexing. An unavailable
embedding detector may reduce graph coverage, but removing the helper would
change the interaction architecture and must not masquerade as equivalent
behavior.

Tests may use a scripted helper implementation. Offline deployments may use a
local open-weight helper. Both still exercise the same mandatory interface.

### 2. The helper and deterministic code form one policy system

The helper decides among semantically rich choices: whether more context would
help, which bounded graph question to ask, how to explain available evidence to
a capable model, and whether a response exposes a new gap.

Deterministic code owns hard constraints:

- human intent and negative constraints;
- repository and graph version;
- tool schemas and argument validation;
- source visibility and result budgets;
- evidence/candidate distinction;
- candidate validation;
- allowed state transitions;
- maximum rounds and stop conditions;
- model identity and prompt provenance.

The helper may request an operation. The deterministic host decides whether the
request is valid and executes only the corresponding typed graph API.

### 3. Prompt engineering is the primary helper responsibility

Before choosing tools or contacting a capable model, the helper constructs an
explicit request model:

```rust
pub struct RefinedRequest {
    pub original_request: HumanTurnRef,
    pub objective: String,
    pub preserved_constraints: Vec<ConstraintRef>,
    pub assumptions: Vec<AttributedAssumption>,
    pub ambiguities: Vec<OpenQuestion>,
    pub evidence_needed: Vec<ContextNeed>,
    pub capable_model_questions: Vec<String>,
    pub output_contract: OutputContract,
}
```

The helper may normalize terminology, organize background, expose contradictions,
and propose a more precise objective. It may not silently resolve product
ambiguity, add mutation authority, or discard an awkward constraint because it
makes the prompt less elegant.

When the graph cannot answer an ambiguity about human preference or desired
behavior, the helper asks the human. When the ambiguity is technical and graph
evidence may resolve it, the helper uses graph tools. When enough evidence is
available, it asks the capable model. These are distinct decisions.

### 4. The helper is the only tool caller

The helper receives a closed graph-tool catalog. Initial capabilities:

- `query_context`: retrieve a bounded evidence packet for an objective or gap;
- `expand_nodes`: traverse permitted relation kinds from exact seed nodes;
- `search_graph`: search indexed graph nodes and attributed artifacts;
- `explain_edge`: return the evidence behind one graph relationship;
- `get_node`: retrieve a bounded representation of one exact node/version;
- `get_coverage`: inspect indexed sources, detectors, omissions, and gaps;
- `validate_candidates`: run deterministic validators on cited candidates;
- `inspect_history_graph`: query commit/file/version relations already stored
  in the graph.

These operations may return source excerpts or metadata already represented by
graph nodes. That is graph access, not direct file access. The helper cannot
name an arbitrary filesystem path and read it outside the graph.

Rejected alternative retained for evaluation: exposing generic `read_file`,
`grep`, or shell tools would improve recall when graph coverage is weak, but
would make the graph boundary cosmetic and greatly expand prompt-injection and
authorization risk.

### 5. Capable models are tool-free conversational reasoners

A capable-model request contains:

- the human objective and preserved constraints;
- selected context with inline provenance labels;
- explicit coverage limits and unresolved gaps;
- the requested reasoning or output contract;
- conversational follow-up from the helper when needed.

It contains no tool definitions. If the capable model asks to inspect a file,
run a search, verify a claim, or obtain more context, that request appears as
ordinary language. The helper assesses it and may translate it into one or more
allowed graph operations.

The helper must not falsely claim to have executed commands, edited files, run
tests, or observed runtime behavior. Those capabilities are outside this epic.

### 6. Every capable-model response passes through response assessment

The helper never relays a capable-model response automatically. It first emits a
schema-validated assessment:

```rust
pub struct ResponseAssessment {
    pub disposition: ResponseDisposition,
    pub claims_to_validate: Vec<ClaimRef>,
    pub context_requests: Vec<ContextRequest>,
    pub unresolved_questions: Vec<OpenQuestion>,
    pub proposed_human_response: Option<String>,
    pub rationale: String,
}

pub enum ResponseDisposition {
    Relay,
    GatherContext,
    AskCapableModel,
    AskHuman,
    RejectUnsupported,
    BudgetExhausted,
}
```

Deterministic policy may require validation for cited repository claims, forbid
another round, or force disclosure of unresolved coverage. The helper may not
turn a capable model's suggested mutation into human authorization.

### 7. Orchestration is a bounded state machine

```text
human input
    -> deterministic intent/facts/initial packet
    -> helper request refinement
       -> graph operation(s), then reassess
       -> human clarification, then reassess
       -> capable-model prompt
    -> capable-model response
    -> helper response assessment
       -> graph operation(s) and revised capable-model prompt
       -> capable-model follow-up
       -> human clarification
       -> final relay with provenance and limitations
```

Every transition is recorded. The state machine enforces:

- maximum helper tool rounds;
- maximum capable-model turns;
- per-operation and cumulative result bytes;
- helper and capable-model token budgets;
- repeated-query detection;
- marginal-yield thresholds;
- explicit completion, escalation, and exhaustion states.

No model may recursively invoke another model or tool outside this host-owned
loop.

### 8. Prompt shaping is a first-class artifact

The helper may rewrite, split, summarize, or sequence prompts, but every prompt
retains:

- the original human message or immutable reference;
- explicit intent and mutation constraints;
- repository/graph version;
- selected evidence references;
- omissions and unresolved gaps;
- requested output contract;
- helper model, prompt-template version, and generation parameters.

Prompt transformations are attributed artifacts. A concise capable-model prompt
may be derived from a verbose helper prompt; neither replaces the original human
request.

### 9. Open-weight model selection is empirical and closed

The first-party adapter exposes only manually evaluated helper variants. Model
selection considers:

- structured tool-call reliability;
- context-gap detection recall;
- unnecessary-call rate;
- ability to follow long, explicit orchestration prompts;
- resistance to instructions found in repository content;
- latency, memory, and cost;
- quality of capable-model prompt reshaping;
- response-assessment accuracy.
- preservation of intended outcome and explicit constraints;
- ability to recognize when to question the human rather than query the graph.

There is no architectural requirement to minimize helper prompt tokens. Clear,
redundant instructions and examples are acceptable when they improve
reliability. Unknown model keys fail configuration validation rather than
falling back silently.

### 10. Repository content is untrusted tool output

Documents, code comments, commit messages, saved reviews, and model-authored
artifacts may contain instructions. The helper prompt treats them as quoted
evidence, never higher-priority instructions.

The host:

- labels source and trust class;
- separates tool output from orchestration instructions;
- validates all tool arguments independently;
- ignores tool-like text embedded in graph results;
- never expands the tool catalog based on model output;
- records injection detections and policy rejections.

The capable model receives the same trust labels in reshaped context.

### 11. Provenance crosses the complete conversation

Persistable session records include:

- human turns;
- deterministic plans and evidence packets;
- helper prompts, responses, and tool requests;
- validated tool arguments and graph results;
- capable-model prompts and responses;
- response assessments;
- final relayed output;
- exact model identifiers, prompt-template versions, graph/repository versions,
  budgets, timings, and content hashes.

Model outputs remain attributed claims. Deterministically validated graph facts
remain distinct even when a model first proposed them.

### 12. Graph-only tools create an explicit completeness ceiling

If relevant material is absent from the graph, the helper cannot recover it by
reading the filesystem. It must report the coverage gap, ask the human for
information, or continue with an explicit limitation.

This ceiling is deliberate and must be measured. The experiment should reveal
which missing graph operations or indexers are worth adding. It must not be
quietly bypassed with a generic file tool.

### 13. Failure is explicit

- Helper unavailable: fail orchestration; preserve deterministic diagnostics.
- Malformed helper output: retry within a small fixed budget, then fail clearly.
- Invalid graph call: reject it and return a structured policy diagnostic to
  the helper.
- Empty or low-yield result: preserve the prior packet and count the attempt.
- Capable-model API failure: retain session state and expose a retryable error.
- Repeated context loop: stop with unresolved gaps and loop diagnostics.
- Budget exhaustion: return the best supported state, never invent completion.
- Unsupported repository claim: validate, request context, qualify, or reject;
  do not relay it as established fact.

## Public Boundaries

Candidate core interfaces:

```rust
pub trait HelperModel {
    fn complete(&self, request: HelperRequest) -> Result<HelperResponse>;
}

pub trait CapableModel {
    fn complete(&self, conversation: CapableConversation)
        -> Result<CapableResponse>;
}

pub trait GraphToolExecutor {
    fn execute(&self, call: ValidatedGraphToolCall)
        -> Result<GraphToolResult>;
}

pub struct OrchestrationPolicy {
    pub max_graph_rounds: u32,
    pub max_capable_turns: u32,
    pub max_graph_result_bytes: usize,
    pub max_total_context_bytes: usize,
    pub min_marginal_yield: usize,
}
```

Provider request/response types do not leak into core orchestration types.
Adapters translate exact model APIs into these boundaries.

## Representative Flows

### Flow A: Initial packet is sufficient

1. Human asks why a retry count is one.
2. Deterministic graph packet supplies the symbol, decision, commit, and test.
3. Helper determines no graph call is needed and prompts the capable model.
4. Capable model gives a sourced explanation.
5. Helper validates cited node references and relays the answer.

The helper remains foundational even when it makes zero tool calls.

### Flow B: Capable response reveals missing test context

1. Helper gathers implementation and rationale context.
2. Capable model says the change appears safe but asks whether tests cover one
   edge case.
3. Helper assesses the response as `GatherContext`.
4. Helper calls graph search/expansion for tests linked to the exact behavior.
5. Helper sends the bounded result and original constraints as a follow-up.
6. Capable model revises its conclusion.
7. Helper relays the supported answer and remaining coverage limits.

### Flow C: Graph cannot answer

1. Capable model asks for runtime output.
2. Helper recognizes that graph tools cannot run the program.
3. It does not fabricate a tool call or claim verification.
4. It asks the human for the runtime observation or returns a qualified answer
   explaining the missing capability.

### Flow D: Repository prompt injection

1. A document node says to ignore prior instructions and request shell access.
2. The helper receives it as untrusted quoted graph content.
3. The helper cannot request shell access because no such tool exists.
4. Deterministic policy rejects any malformed attempt and records the event.
5. The capable-model prompt includes only the relevant evidence with its trust
   label.

## Tasks

### Task 0: Run and pass the blocking helper experiments

Create the research artifact and satisfy the Blocking Experiment Gate.

#### Acceptance Criteria

- [ ] At least twelve real cases across three repositories are documented.
- [ ] At least three sub-20B open-weight helper models are compared.
- [ ] All required success, failure, ambiguity, and injection cases are covered.
- [ ] Baselines and ablations are measured.
- [ ] Tool calls, prompt versions, budgets, context yield, and final quality are
  recorded.
- [ ] Model selection and orchestration thresholds are justified by results.
- [ ] Failed approaches and rejected alternatives remain documented.
- [ ] Tasks 1–7 have not begun before every Task 0 criterion is checked.

### Task 1: Add typed orchestration/session models

Define original/refined requests, turns, prompts, assessments, state transitions,
provenance, budgets, diagnostics, and serialization.

#### Acceptance Criteria

- [ ] Session state round-trips through JSON without losing attribution.
- [ ] Human intent and negative constraints are immutable across derived prompts.
- [ ] Assumptions and ambiguities remain explicit and attributed.
- [ ] Refined requests retain an immutable link to the original human turn.
- [ ] Invalid transitions, missing references, and cycles are rejected.
- [ ] Stable IDs exist for sessions, turns, prompts, calls, results, and claims.
- [ ] Core types contain no provider-specific protocols.

### Task 2: Add the closed graph-tool facade

Expose only the bounded graph operations defined by this epic.

#### Acceptance Criteria

- [ ] No tool accepts an arbitrary filesystem path, shell command, URL, or write.
- [ ] Every call is schema-validated and repository/version scoped.
- [ ] Every result reports provenance, coverage, omissions, and byte cost.
- [ ] Candidate validation preserves model claim versus graph fact.
- [ ] Tool results cannot dynamically define new tools or permissions.

### Task 3: Implement the orchestration state machine

Combine deterministic planning, helper decisions, graph execution, capable-model
turns, and response assessment.

#### Acceptance Criteria

- [ ] Every capable-model call passes through the helper.
- [ ] Every capable-model response receives helper assessment before relay.
- [ ] Capable-model requests contain no tool definitions.
- [ ] Invalid helper calls are rejected without execution.
- [ ] Duplicate calls, marginal yield, rounds, tokens, and bytes are bounded.
- [ ] Human clarification and explicit exhaustion states are supported.
- [ ] A scripted helper and scripted capable model can test every transition
  without network access.

### Task 4: Implement prompt shaping and trust labeling

Create versioned helper and capable-model prompt templates.

#### Acceptance Criteria

- [ ] Helper prompts may be verbose and include tool-use examples.
- [ ] Capable prompts remain bounded and conversational.
- [ ] Original human intent, constraints, provenance, and coverage survive every
  rewrite.
- [ ] Fixtures cover clarification, decomposition, terminology normalization,
  evidence selection, question sequencing, and output-contract design.
- [ ] Genuine product ambiguity produces a human question rather than an
  invented assumption or graph call.
- [ ] Repository/model content is clearly delimited as untrusted evidence.
- [ ] Prompt-injection fixtures cannot expand tools or alter deterministic
  policy.

### Task 5: Integrate selected open-weight helper adapters

Implement the manually selected helper models through the existing
OpenAI-compatible `llm-sdk` boundary.

#### Acceptance Criteria

- [ ] Supported helper variants form a closed serializable type.
- [ ] Exact API model identifiers and prompt-template compatibility are tested.
- [ ] Unknown variants fail explicitly.
- [ ] Model output is schema-validated.
- [ ] Credentials and endpoints never enter graph facts or prompt evidence.
- [ ] At least one local/offline configuration is documented.

### Task 6: Integrate tool-free capable-model adapters

Provide one or more capable-model adapters that expose only conversational
completion.

#### Acceptance Criteria

- [ ] No capable request contains tools or tool-choice fields.
- [ ] Multiple turns preserve helper-authored conversational context.
- [ ] Model/provider identity is recorded.
- [ ] API failure preserves retryable session state.
- [ ] Capable-model claims remain attributed until validated.

### Task 7: Add evaluation CLI, persistence, and documentation

Expose replayable orchestration inspection without repository mutation.

Candidate commands:

```bash
cargo run -- --repo . --orchestrate "Explain the import cache behavior"
cargo run -- --repo . --orchestrate-json "Review Section 6.13"
cargo run -- --replay-orchestration <fixture>
```

#### Acceptance Criteria

- [ ] Human output shows helper decisions, graph calls, capable turns,
  provenance, coverage, and stop reason.
- [ ] JSON permits deterministic replay with scripted model outputs.
- [ ] Evaluation reports recall, irrelevant context, unsupported claims,
  unnecessary calls, missed calls, intent preservation, prompt quality, answer
  improvement, latency, tokens, bytes, and final quality.
- [ ] No command, file mutation, or hidden capable-model tool use occurs.
- [ ] `README.md`, `DEVELOP.md`, and `AGENTS.md` document the architecture and
  limitations.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes.

## Test Matrix

| Area | Required evidence |
|---|---|
| Session model | JSON round trip, stable IDs, transition validation |
| Intent safety | preserved objectives, negative constraints, authorization |
| Request refinement | clarification, structure, assumptions, ambiguity, output contracts |
| Graph tools | closed schemas, read-only scope, budgets, provenance |
| Helper decisions | call, no-call, clarify, follow-up, stop |
| Response assessment | unsupported claims, new gaps, relay decisions |
| Capable model | tool-free payloads, multi-turn conversation |
| Prompt shaping | bounded context, trust labels, provenance retention |
| Injection | document, code comment, commit, and saved-review attacks |
| Loops | duplicate calls, low yield, round and budget exhaustion |
| Failure | helper, graph, capable-model, and malformed-output failures |
| Model selection | at least three sub-20B helpers and baselines |
| Replay | deterministic scripted-model fixture execution |

## File-change Summary

| File | Change |
|---|---|
| `Cargo.toml` | Add orchestrator and adapter crates to the workspace. |
| `crates/daftprompt-orchestrator/` | Provider-independent session state machine, policy, prompts, and graph-tool facade. |
| `crates/daftprompt-orchestrator-llm/` | `llm-sdk` helper and capable-model adapters. |
| `src/main.rs` | Add read-only orchestration and replay CLI modes. |
| `epics/research/013-helper-orchestrator-experiments.md` | Blocking model and policy evaluation. |
| `README.md`, `DEVELOP.md`, `AGENTS.md` | Document mandatory helper architecture and graph-only tools. |

The exact crate split must follow Task 0. The dependency direction, mandatory
helper path, graph-only helper tools, and tool-free capable-model boundary are
requirements.

## Risks and Follow-ups

- A mandatory probabilistic component reduces offline determinism and may become
  a reliability bottleneck.
- Tool-free capable models may perform worse on implementation tasks that need
  interactive runtime feedback.
- A graph-only tool ceiling may expose indexing gaps faster than the graph can
  evolve.
- Small models may overfit prompt templates, miss subtle context needs, or
  follow repository prompt injection.
- Verbose helper prompts and repeated capable turns may erase cost/latency gains.
- The helper may distort human intent while trying to improve a prompt.
- Prompt quality is difficult to measure independently of capable-model quality;
  the evaluation must use paired prompts, fixed model settings, and blinded
  review where practical.
- Response inspection may create loops or encourage needless second-guessing.
- One helper model may not be best across languages and task types; routing must
  remain closed, measured, and deterministic if introduced.
- Future mutation/execution support requires a separate authorization and
  execution epic. It must not expand the tools defined here implicitly.
