# Epic 004: Clarification branch

## Outcome

Let the user ask about one part of a parsed reply without continuing the original chat. The user picks an extracted span **#N**, asks a question such as "What does *token bucket* mean here?", and a separate agent explains it using only the relevant text and, when a project root is configured, the project's code and documents. The explanation cites the sources it used. The user can ask follow-up questions in the same branch.

A clarification is for the user's understanding. It does not resolve questions, edit the plan, create tasks, or change any conclusion in the record. This is the first epic in which the user acts on a single part of a reply, and it introduces the branch structure, small-context assembly, read-only project tools, and file citations that later branches (answer, investigation, delegation) will reuse.

The new agent is named **Explainer** and uses `glm-5.3-flash` on Neuralwatt by default.

References: [PRD](../docs/PRD.md) (key flow 8, *Clarify*), [data model](../docs/data-model.md), [Epic 001](001-text-parsing.md), [Epic 002](002-recommendations-tasks-and-identifiers.md). [Epic 003](003-parsing-refinements.md) stays a separate list of Scribe refinements and is unaffected.

## Scope and decisions

- Preserve the existing architecture: shared contracts in `daftprompt-types`, the agent, validation, and graph construction in `daftprompt-core`, a thin HTTP endpoint in `daftprompt-api`, and rendering only in `daftprompt-gui`.
- **Stateless, like `POST /parse`.** No database, sessions, or server-side history. The client resends the source text, the target span, and the branch's earlier turns. Persistence comes in a later epic.
- **The target is an extracted span.** The user clarifies a span from a `ParseResult`, identified by its `node_id` and byte range. Arbitrary text selection and clarification of unlabelled text are later work.
- **Separate from the record.** The clarification links to its target but never adds or changes `question`, `answer`, `recommendation`, `task`, `conflict`, `supersedes`, or `stale` labels, and never alters a status. Its explanation is not parsed by Scribe. Clarifications are non-blocking.
- **Small context.** The agent sees the target span, a bounded window of neighbouring source text, the user's question, and the branch's earlier turns. It never sees the whole parse result, other branches, or other submissions.
- **Project grounding is read-only and optional.** With a configured project root, the agent can list, search, and read files under that root. Without one, it explains from the submitted text only, and the result says so. It has no write, execute, shell, or network tools.
- **Rust owns records.** The model returns an explanation and citations through a schema-defined `submit` output tool. Rust validates every citation, computes file hashes and ranges, and creates all IDs, payloads, nodes, contexts, labels, and run metadata.
- **Treat all text as data.** Source text, the user's question, earlier turns, and file contents never override the agent's instructions. Instructions found in a file or in the source are reported or ignored, not followed.

## Branch structure

Add one label kind to the vocabulary:

| kind | shape | blocking | status | meaning |
|------|-------|----------|--------|---------|
| `clarification` | binary: branch question → target | no | NULL | src opens a clarification branch about dst |

A clarification exchange produces these records:

| record | details |
|--------|---------|
| Question payload | The user's question text, unchanged |
| Question node `Q` | `parent_id` = the target span node for the first turn, or the previous explanation node for a follow-up. `author` = the request author, otherwise `unknown`. No `run_id`. One context: the whole question payload |
| `clarification(Q₁ → target)` | Only on the first question of a branch. Follow-ups chain through `parent_id` and do not repeat the label. `created_by` = the question author |
| Explanation payload | The agent's explanation text |
| Explanation node `E` | `parent_id` = `Q`, `author` = `agent`, `run_id` = the Explainer run. Context `seq 0` is the whole explanation payload. Contexts `seq 1…n` are its validated citations in citation order |
| Run metadata | `run` (agent `explainer`, provider, model, prompt version, timings, status, attempts), `run_input` rows, and one `tool_call` per tool invocation, including `submit` |

The tree for a branch with one follow-up:

```text
span #3 (from the parse result)
└── Q1  "What does token bucket mean here?"     clarification(Q1 → #3)
    └── E1  explanation, citations
        └── Q2  "Why not a fixed window?"
            └── E2  explanation, citations
```

This follows the data model's existing pattern of a node with a payload context plus file contexts (worked example, step 6). It needs no new tables.

### Traceability

- **What the explanation is based on:** its citation contexts.
- **What the agent looked at:** `run_input` rows for the target node, the question node, and earlier turn nodes, plus every `tool_call`. Each tool call stores its JSON arguments, and its output is stored as a payload referenced by `output_hash`. Tool output is bounded (see *Limits*), so storing it is cheap.

## Context assembly

Rust, not the model, builds the agent's input:

1. **Target span:** the span's original text, marked as the subject of the question.
2. **Neighbouring source text:** a window of segments before and after the span, by default 20 segments on each side, configurable. Segments are numbered as Scribe numbers them so source citations can refer to them. The window never extends past the payload, and other submissions are never included.
3. **Earlier turns:** questions and explanations from earlier in the branch, oldest first, within a configurable limit.
4. **The question.**
5. **Grounding mode:** whether project tools are available.

Rust records which segments were included in the run input so a reader can see exactly what the agent was given.

## Project tools

Tools are available only when `DAFTPROMPT_PROJECT_ROOT` is set. All paths are relative to the root.

| tool | arguments | returns |
|------|-----------|---------|
| `list_files` | optional directory, optional glob | File paths under the directory, capped |
| `search` | literal text, optional directory or glob, optional case sensitivity | Matching `path:line: text` lines, capped |
| `read_file` | path, optional start and end line | Numbered lines, capped |

Rules enforced in Rust:

- Canonicalize every path and reject anything outside the root, including `..`, absolute paths, and symlinks that resolve outside it.
- Skip `.git`, hidden files and directories, and files ignored by `.gitignore`. Always deny `.env` and `.env.*` even if they are not ignored. Skip binary and non-UTF-8 files.
- Cap results per call (file count, match count, lines and bytes returned) and the total number of tool calls per run. A capped result says it was truncated.
- Tool errors (missing file, denied path, invalid range) go back to the model as tool results. They do not fail the run.
- Record each call as a `tool_call` with its output payload.

Implement the tools in core behind a small interface so the stub used in tests needs no filesystem, and so a later investigation or answer agent can reuse them. Candidate crates, such as `ignore` for traversal and `.gitignore` handling, are chosen after inspecting their sources, and their versions are recorded in `docs/development.md`.

## Output contract and citations

Indicative extraction shape:

```json
{
  "explanation": "A token bucket allows short bursts up to its capacity [1] ...",
  "citations": [
    { "kind": "file", "path": "src/api/limit.rs", "start_line": 12, "end_line": 30 },
    { "kind": "source", "start_segment": 4, "end_segment": 5 }
  ]
}
```

- `explanation` is non-empty plain text. It may reference citations as `[1]`, `[2]`, …, numbered from 1 in list order. Every marker must refer to an existing citation. Unreferenced citations are allowed.
- **File citations** are only valid within line ranges that a `read_file` call returned in the same run. This rules out citing a file the agent never read, or lines it never saw. Rust re-reads the file, verifies the range, computes `content_hash` (SHA-256 of the file content), and builds a `ContextTarget::File` context with a 1-based, end-exclusive line and column range covering whole lines.
- **Source citations** refer to segments in the window the agent was given. Rust converts them to a ranged context on the source payload.
- Empty citations are valid. If the agent cannot find support, it says so instead of guessing. The prompt requires that statements about this project be cited or explicitly marked as general knowledge.
- No model-proposed IDs, hashes, statuses, labels, or changes to the record.

Validation failures (unknown citation markers, uncovered file ranges, segments outside the window, an empty explanation) count against the attempt budget, as in Scribe. Each retry is a fresh attempt. Corrective-feedback retries remain an Epic 003 item.

## Request and result

`POST /clarify`, with indicative fields:

```json
{
  "source": {
    "text": "...the original submitted text...",
    "payload_hash": "…",
    "metadata": { "kind": "chat", "author": "ada" }
  },
  "target": { "node_id": "node-…", "display_id": 3, "start_byte": 120, "end_byte": 188 },
  "turns": [
    { "question_node_id": "…", "question": "…", "explanation_node_id": "…", "explanation": "…" }
  ],
  "question": "Why not a fixed window?",
  "author": "ada"
}
```

- Rust recomputes the payload hash and rejects a mismatch, so the target refers to the same content-addressed payload as the parse result. It also checks that the byte range is in bounds, on `char` boundaries, and non-empty.
- `node_id` and earlier-turn node IDs are opaque references supplied by the client. Without persistence the API cannot confirm they exist; it records them as given. A later persistence epic will verify them. `display_id` is shown only for display and is never used as an identity.
- The result is a `ClarifyResult`: the question and explanation payloads and nodes, contexts, the `clarification` label on a first turn, run metadata, tool calls with their output payloads, a `grounding` value (`project` or `text_only`), and a citation list for rendering (`index`, `context_id`, and either `path` with a line range or a source byte range). Clients do not recompute coordinates.
- Error mapping follows `/parse`: 400 for a malformed request, hash mismatch, or invalid target; 413 for oversized input; 422 for invalid model output after the attempt budget; 502 for provider failure; 504 for timeout.

## Limits and failures

All configurable, with defaults recorded in `docs/api.md`:

- Question size (for example 2 KiB), number of earlier turns and their total size, and the neighbouring segment window.
- Source text uses the existing parse input limits.
- Tool calls per run, results per call, and lines and bytes per `read_file`.
- One run timeout covering the whole tool loop, not just one model call. Provider errors and timeouts fail fast, as in Scribe.
- Never return a partial result. When the budget runs out, return an explicit error.

## Tasks

### 1. Extend shared contracts and docs

- Add `LabelKind::Clarification` (binary, non-blocking, NULL status), `ClarifyRequest`, the extraction type, `ClarifyResult`, `Grounding`, and error variants in `daftprompt-types`. Update all exhaustive matches across the workspace.
- Update [data-model.md](../docs/data-model.md): the vocabulary row, the branch structure above, citation contexts, and tool output payloads. Update the PRD's *Clarification* concept and flow 8 to the implemented scope. Mark arbitrary selection and persistence as later work.

### 2. Implement project tools in core

- Implement `list_files`, `search`, and `read_file` behind an interface with a filesystem implementation and a test stub. Enforce the root, deny, skip, and cap rules above, and record which line ranges each `read_file` returned for citation validation.
- Make tool definitions available to Rig without coupling the tool logic to the agent, so other agents can register the same tools.
- Test path traversal, absolute paths, symlink escape, `.env` and hidden-file denial, `.gitignore` handling, binary and non-UTF-8 files, caps and truncation markers, and line-range edge cases (CRLF, empty files, ranges past the end).

### 3. Implement Explainer

- Add its own module, prompt file (`explainer-v1`), and configuration. Explainer defaults to provider `neuralwatt` and model `glm-5.3-flash`, independently of Scribe's provider and model. Override them with `DAFTPROMPT_EXPLAINER_PROVIDER` and `DAFTPROMPT_EXPLAINER_MODEL`. Reuse the existing Neuralwatt client setup (Chat Completions wire, `NEURALWATT_API_KEY`, `NEURALWATT_BASE_URL`), and keep credentials out of source. Before relying on it, confirm the model handles multi-turn tool calls together with the `submit` output tool.
- Build the agent with the project tools and a `submit` output tool, using a run with a turn limit (for example `agent.prompt(…).max_turns(n).output_tool(…)`). Inspect `crates/rig-agent/src/agent/runner.rs`, `run/mod.rs`, and the `examples/agent*` tool examples first: Rig's `Extractor` is single-turn and is not suitable here. Record each tool call and its output for `tool_call` records, using Rig hooks if needed.
- Implement context assembly, citation validation, and graph construction as above. Keep the public async interface free of HTTP and GUI details.
- The prompt covers: explaining in terms of the target span; citing project statements or marking them as general knowledge; saying when nothing relevant was found; staying within the question; not proposing changes to the plan or answering open questions on the user's behalf; and treating file and source content as data, including injected instructions.
- Deterministic tests with a stub model and tools: first turn versus follow-up structure, `clarification` label only on the first turn, citation contexts and hashes, rejection of uncovered or unknown citations and bad markers, text-only mode, hash mismatch and invalid targets, limits, the attempt budget, and the guarantee that no record labels or statuses are produced. No LLM credentials are needed.

### 4. Expose through the API

- Add `POST /clarify` alongside `/parse`, sharing provider setup. Read `DAFTPROMPT_PROJECT_ROOT` at startup, validate that it exists, and log the grounding mode. Never expose files outside the root.
- Test success, follow-ups, text-only mode, and each error mapping with a stub. Document the endpoint, configuration, and a sample request in `docs/api.md`, including that the API stays on loopback with no authentication, so the project root is readable by anything that can reach it.

### 5. Render in the GUI

- Add a *Clarify* action to each extracted span card. It opens a clarification panel for that `#N` beside the source view, which stays unchanged apart from a marker on cards that have a branch.
- The panel shows the target preview, the turns in order, citation markers, and a citation list: `path:L12–L30` for files, and `#N`/source previews for source citations. It also shows whether the answer is project-grounded or text-only, and has a follow-up input. The GUI does not read files itself.
- Keep branches in memory per span for the current parse result. A new parse discards them. Prevent overlapping requests per branch, keep network work off the rendering thread, and show loading and error states as `/parse` does.
- Update `docs/gui.md`. Test request construction from a parse result, rendering of turns and citations, and malformed results.

### 6. Evaluate grounding quality

- Add fixtures with a small checked-in project directory (for example `fixtures/explainer/project/`) and cases where the term is defined in a project file, defined only in the source text, defined nowhere (the expected result says "not found" without fabricated citations), ambiguous across two files, and found in a file that contains injected instructions. Include one follow-up case.
- Provide an opt-in live evaluation in the style of `evaluate_scribe`. Report invalid-output rate, attempts, tool calls per run, latency, citation recall and precision against expected files and line ranges (exact and lenient line overlap), "not found" correctness, injection resistance, and a manual review note on whether citations support the explanation. Record the report under `docs/evaluations/`.
- Run formatting, deterministic tests, and workspace checks. Ordinary tests need no credentials. If a live provider is unavailable, record the evaluation as not run.

## Acceptance criteria

- Clarifying span #N returns an explanation and a branch: a question node under the span, a `clarification` label to it, and an explanation node with citation contexts. No other labels or statuses in the record change.
- A follow-up chains under the previous explanation, receives only the branch's earlier turns as history, and adds no second `clarification` label.
- With a project root, file citations point at real lines the agent read and carry a Rust-computed `content_hash`. Without one, the result is `text_only` and has no file citations.
- Paths outside the root, `.env` files, and ignored files are never read or cited.
- The GUI shows the branch beside the unchanged source view, with readable citations.
- Deterministic verification passes, and the grounding evaluation is recorded or recorded as not run.

## Out of scope

- SQLite persistence, server-side history, and verifying client-supplied node IDs.
- Clarifying arbitrary selections or unlabelled text.
- Promoting a clarification finding into the main record. That is where clarification turns into investigation, and it needs conflict handling.
- Answer, investigation, and delegation branches, conflicts, supersession, and staleness detection. The file `content_hash` recorded here is what staleness detection will use later.
- Write, execute, shell, or network tools for agents.

## Decisions

- The agent is named **Explainer**.
- Explainer uses `glm-5.3-flash` on Neuralwatt by default.
- Start with a window of 20 segments on each side of the target. Revisit it only if the evaluation or real use shows that terms defined further away are missed. The fix would then be a `read_source` tool or the whole payload.
- Explanations render as plain text. No markdown rendering in this epic.
