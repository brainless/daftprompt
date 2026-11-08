# HTTP API

The API is a stateless wrapper around Scribe. One `POST /parse` request returns
one complete, validated graph fragment; each submission is independent and
nothing is stored. The service binds to loopback by default and has no
authentication, session, or history endpoints.

## Start the service

The API process builds the provider clients itself (through
`daftprompt_core::llm::build_model`) and reads their credentials from the
environment. The default provider is OpenAI, and
`OPENAI_BASE_URL` selects an OpenAI-compatible endpoint. Neuralwatt and Cerebras
are also supported (see below).

On startup the process first loads a `.env` file from the working directory (or
a parent) with `dotenvy::dotenv()` semantics: a missing file is fine, variables
already set in the real environment take precedence over the file, and a
malformed file stops startup with an error. `.env` is git-ignored; copy
[`.env.example`](../.env.example) and fill it in.

```sh
rustup toolchain install nightly-2026-10-25 \
  --component rustc-codegen-cranelift \
  --component rustfmt \
  --component clippy

OPENAI_API_KEY=... cargo run -p daftprompt-api
```

The default bind address is `127.0.0.1:8080`:

```text
daftprompt-api listening on http://127.0.0.1:8080 (provider openai, model gpt-4o-mini, input limit 32768 bytes / 1000 segments, 3 attempt(s), timeout 60s)
```

The running service calls the model. Ordinary tests do not need credentials;
this command does.

| variable | default | meaning |
| --- | --- | --- |
| `OPENAI_API_KEY` | required for `openai` | Read by `OpenAI::from_env()`, never by daftprompt source |
| `OPENAI_BASE_URL` | provider default | Optional OpenAI-compatible endpoint |
| `NEURALWATT_API_KEY` | required for `neuralwatt` | Bearer key for Neuralwatt |
| `NEURALWATT_BASE_URL` | `https://api.neuralwatt.com/v1` | Optional override of Neuralwatt's OpenAI-compatible base URL |
| `CEREBRAS_API_KEY` | required for `cerebras` | Bearer key for Cerebras |
| `CEREBRAS_BASE_URL` | `https://api.cerebras.ai/v1` | Optional override of Cerebras's OpenAI-compatible base URL |
| `GROQ_API_KEY` | required for `groq` | Bearer key for Groq |
| `GROQ_BASE_URL` | `https://api.groq.com/openai/v1` | Optional override |
| `DAFTPROMPT_API_ADDR` | `127.0.0.1:8080` | Bind address (`IP:port`). Keep it on loopback unless you add your own access control |
| `DAFTPROMPT_API_PROVIDER` | `openai` | `openai`, `neuralwatt`, `cerebras`, or `groq`; recorded as `run.provider` |
| `DAFTPROMPT_API_MODEL` | `gpt-4o-mini` (`glm-5.3-flash` for `neuralwatt`, `gpt-oss-120b` for `cerebras`, `openai/gpt-oss-20b` for `groq`) | Model identifier recorded as `run.model` |
| `DAFTPROMPT_API_MAX_INPUT_BYTES` | `32768` | UTF-8 byte limit for the submitted text; oversized text is rejected, never truncated |
| `DAFTPROMPT_API_MAX_SEGMENTS` | `1000` | `\n`-separated segment limit |
| `DAFTPROMPT_API_MAX_ATTEMPTS` | `3` | Total attempts for invalid model output; provider errors and timeouts are never retried |
| `DAFTPROMPT_API_TIMEOUT_SECS` | `60` | Per-attempt model timeout |

### Neuralwatt

Neuralwatt is OpenAI-API-compatible. Its base URL is
`https://api.neuralwatt.com/v1`
([docs](https://docs.neuralwatt.com/api/overview.md)); Chat Completions is
served at `/v1/chat/completions`. The OpenAI provider in Rig defaults to the
Responses API, which compatible providers generally do not serve (Neuralwatt
limits `/v1/responses` to enrolled accounts), so the API builds the Neuralwatt
model with Rig's explicit Chat Completions wire
(`OpenAIConfig::new(key).with_base_url(url).client().chat(model)`).

```sh
# .env
DAFTPROMPT_API_PROVIDER=neuralwatt
NEURALWATT_API_KEY=...
# DAFTPROMPT_API_MODEL defaults to glm-5.3-flash
cargo run -p daftprompt-api
```

### Cerebras

Cerebras's [OpenAI compatibility](https://inference-docs.cerebras.ai/resources/openai)
uses `https://api.cerebras.ai/v1`. Its
[tool-calling guide](https://inference-docs.cerebras.ai/capabilities/tool-use)
lists `gpt-oss-120b` with required tool choice and multi-turn tool calls. Scribe's
schema-defined `submit` tool and Explainer's project/output tools use Rig's
Chat Completions wire. The local `.env` can select Cerebras for both agents:

```sh
DAFTPROMPT_API_PROVIDER=cerebras
DAFTPROMPT_API_MODEL=gpt-oss-120b
DAFTPROMPT_EXPLAINER_PROVIDER=cerebras
DAFTPROMPT_EXPLAINER_MODEL=gpt-oss-120b
CEREBRAS_API_KEY=... # provide your own key
cargo build --release -p daftprompt-api
./target/release/daftprompt-api
```

The JSON request envelope is capped at six times the text limit plus 16 KiB, so
JSON escaping and optional source metadata have room without hiding an
oversized submission from the parser.

## Text format

`ParseRequest.content_type` is optional: `plain` or `markdown`. Omit it (or use
`null`) for deterministic detection in core; unknown values return HTTP 400 before
calling Scribe. Source provenance is independent: a file or email can use either
format. A `.md` source reference alone does not establish Markdown.

Every successful result includes top-level `content_type` (the format used) and
`content_type_source` (`declared` or `detected`). These describe this parsing run,
not the immutable payload. For example, `{"text":"# Plan","content_type":"plain"}`
returns `"content_type":"plain","content_type_source":"declared"`; omitting the
declaration returns `"content_type":"markdown","content_type_source":"detected"`.
Scribe records `scribe-v4+plain-v1` or `scribe-v4+markdown-v1` in
`run.prompt_version`. Rust computes paragraph or Markdown block boundaries; the
model selects whole blocks or narrower segment ranges, and the result retains
original byte ranges. The GUI displays the returned format and its source and
offers no content-type override.

## Sample request

```sh
curl -sS -X POST http://127.0.0.1:8080/parse \
  -H 'content-type: application/json' \
  --data-binary @- <<'JSON'
{
  "text": "Plan\nShould we cache tokens? @infra-team\nYes, cache them.\n",
  "source": {
    "kind": "chat",
    "reference": "thread-1",
    "author": "ada"
  }
}
JSON
```

`text` is required and is preserved unchanged. `source` is optional provenance:
`kind` is one of `chat`, `file`, `clipboard`, `email`, or `other`; `reference`
and `author` are opaque strings. The root node's author is the supplied author,
otherwise `unknown`; it is never derived from the text.

`project_id` is optional: the id of a project registered with `POST /projects`
(see "Projects and storage"). The server resolves it against the registry and
its access policy; Scribe validates it the same way as the Explainer, so an
unknown id is `404 not_found` and a refused one `403 forbidden` before any
model call. Scribe has no file tools, so it never reads the folder; the
granted id is recorded as `run.project_id` in the result. Requests never carry
a filesystem path.

```sh
curl -sS -X POST http://127.0.0.1:8080/parse \
  -H 'content-type: application/json' \
  -d '{"text": "Should we cache tokens?\nYes.", "project_id": "project-18f3a2-1"}'
```

## Response

The response is a `ParseResult` as JSON. Its important fields:

| field | meaning |
| --- | --- |
| `payload.content` | The complete original text, stored once |
| `payload.hash` | SHA-256 of the payload, its identity |
| `spans` | `{ node_id, display_id, start_byte, end_byte }` for every span node. Half-open byte ranges into `payload.content`, always on `char` boundaries |
| `nodes` | One root node (`parent_id` absent) plus one child node per span |
| `node_contexts` | Each span node's ranged context pointing at the payload (1-based lines/columns, exclusive end) |
| `labels` | `question`, `answer`, `recommendation`, `recommendation_for`, `alternative_to`, and `task` records, with Rust-derived statuses and optional question/task `assignee` handles |
| `source` | The provenance from the request, unchanged |
| `run`, `run_inputs`, `tool_calls` | In-memory run metadata for the Scribe call |

A client renders source order from `payload.content` and `spans`: text outside
every range is unlabelled, and each span's labels, status, assignee, and answer
targets come from `labels` keyed by `node_id`. No coordinate recomputation is
needed.

Abbreviated example (IDs, hashes, and timestamps vary):

```json
{
  "content_type": "plain",
  "content_type_source": "detected",
  "payload": {
    "hash": "6b6f…",
    "content": "Plan\nShould we cache tokens? @infra-team\nYes, cache them.\n",
    "created_at": "2026-10-31T12:00:00Z"
  },
  "source": { "kind": "chat", "reference": "thread-1", "author": "ada" },
  "nodes": [
    { "id": "node-root", "parent_id": null, "author": "ada", "run_id": null, "created_at": "…" },
    { "id": "node-s1", "parent_id": "node-root", "author": "agent", "run_id": "run-1", "created_at": "…" },
    { "id": "node-s2", "parent_id": "node-root", "author": "agent", "run_id": "run-1", "created_at": "…" }
  ],
  "node_contexts": [
    { "id": "ctx-root", "node_id": "node-root", "seq": 0, "target": { "kind": "payload", "payload_hash": "6b6f…" }, "range": null },
    { "id": "ctx-s1", "node_id": "node-s1", "seq": 0, "target": { "kind": "payload", "payload_hash": "6b6f…" }, "range": { "start_line": 2, "start_col": 1, "end_line": 2, "end_col": 24 } },
    { "id": "ctx-s2", "node_id": "node-s2", "seq": 0, "target": { "kind": "payload", "payload_hash": "6b6f…" }, "range": { "start_line": 3, "start_col": 1, "end_line": 3, "end_col": 17 } }
  ],
  "labels": [
    { "id": "label-q", "kind": "question", "src_node_id": "node-s1", "dst_node_id": null, "status": "resolved", "assignee": "@infra-team", "note_node_id": null, "created_by": "agent", "run_id": "run-1", "created_at": "…" },
    { "id": "label-a", "kind": "answer", "src_node_id": "node-s2", "dst_node_id": "node-s1", "status": null, "assignee": null, "note_node_id": null, "created_by": "agent", "run_id": "run-1", "created_at": "…" }
  ],
  "spans": [
    { "node_id": "node-s1", "display_id": 1, "start_byte": 5, "end_byte": 28 },
    { "node_id": "node-s2", "display_id": 2, "start_byte": 42, "end_byte": 58 }
  ],
  "run": { "id": "run-1", "agent": "scribe", "provider": "openai", "model": "gpt-4o-mini", "prompt_version": "scribe-v4+plain-v1", "started_at": "…", "ended_at": "…", "status": "done", "attempts": 1, "consistency_followup": false, "consistency_used": false },
  "run_inputs": [{ "run_id": "run-1", "node_id": "node-root" }],
  "tool_calls": [{ "id": "tool-1", "run_id": "run-1", "seq": 0, "tool": "submit", "input": "{...}", "output_hash": null, "created_at": "…" }]
}
```

## Errors

Every failure is a serialized `ParseError` with a machine-readable `kind`, a
human-readable `detail`, and `attempts` where that is meaningful. A failed
request never returns a partly valid graph.

| status | `kind` | when |
| --- | --- | --- |
| 400 | `invalid_request` | Malformed JSON, missing `text`, or empty text |
| 413 | `oversized_input` | Text exceeds the byte or segment limit, or the JSON envelope is too large |
| 422 | `invalid_output` | The model did not submit a valid extraction within the attempt budget |
| 403 | `forbidden` | `project_id` names a project the access policy refuses for this caller and agent |
| 404 | `not_found` | `project_id` names no registered project |
| 422 | `project_unavailable` | `project_id` is registered and allowed, but its folder no longer exists or is not a directory |
| 500 | `internal_error` | A configuration or adapter failure unrelated to model output |
| 502 | `provider_error` | The provider request failed |
| 504 | `timeout` | A model attempt exceeded the configured timeout |

```json
{
  "kind": "invalid_output",
  "detail": "spans 1 and 2 overlap",
  "attempts": 3
}
```

Oversized input is checked before the first model call. Invalid output
(deserialization or validation failures) is retried from scratch, without
feedback, until the attempt budget is exhausted; the 422 body reports how many
attempts were used. Provider errors (502) and timeouts (504) are not retried:
they are returned immediately, so the worst-case latency is one timeout.

## Epic 002 contract additions

`SpanInfo.display_id` is an API-assigned integer, consecutive from 1 in source order within one result. Multiple annotations on a span share its number. Clients resolve targets through `node_id` and must not renumber spans. Retain the run identity when saving a display reference outside its result.

The new label enum values are `recommendation` (unary), `recommendation_for` (binary to a question), `alternative_to` (symmetric binary between recommendations, stored in source order), and `task` (unary). Recommendations and relationships have null status; tasks start open and may carry an explicit assignee. Only answer links resolve questions. Standalone recommendations need no question. These fields and enum values are additive; clients using exhaustive enum decoding must update together.

The model contract adds optional `questions` and `alternatives` lists to recommendations; omitted lists default to empty. These local model IDs never become API identities. Rust rejects inappropriate fields, invalid targets, duplicate unary labels or links, reversed alternative duplicates, and self-links before constructing any graph records.

### Mapping relationship targets to display identifiers

For `Should we cache tokens?\nI suggest a five-minute cache.\n@alex, please benchmark the cache.`, the response has these span identities
(abbreviated; generated IDs vary):

| node ID | display ID | label/status |
| --- | --- | --- |
| `node-q` | 1 | `question`, `open` |
| `node-r` | 2 | `recommendation`, null |
| `node-t` | 3 | `task`, `open`, assignee `@alex` |

The separate `recommendation_for` label has `src_node_id = node-r` and
`dst_node_id = node-q`. Looking up those node IDs in `spans` yields **#2 → #1**.
No answer link exists, so #1 remains open. If an actual answer appears later in
the same input, its span receives **#4** and its `answer` label points to `node-q`:
the client displays **#4 answers #1**, and #1 is resolved. A standalone
recommendation emits no fabricated `recommendation_for` link. Explicit alternatives
use the same node-ID lookup at both ends and display symmetrically, irrespective
of stored direction. The API remains `POST /parse`; there are no new endpoints.

## `POST /clarify` (Epic 004)

`POST /clarify` explains one extracted span for one question. It is stateless
like `/parse`: the client sends the original text, the target span, and any
earlier turns of the branch; the response is one validated graph fragment
(a question node, an explanation node, citations, run metadata). It never
changes the parse result, adds labels other than `clarification`, or stores
anything. The Explainer agent is Rig-backed, with its own provider and model.

### Request

```sh
curl -sS -X POST http://127.0.0.1:8080/clarify \
  -H 'content-type: application/json' \
  --data-binary @- <<'JSON'
{
  "source": {
    "text": "We use a token bucket.\nIt refills slowly.\nThe limit is per user.",
    "payload_hash": "<payload.hash from the parse result>",
    "metadata": { "kind": "chat", "reference": "thread-1", "author": "ada" }
  },
  "target": { "node_id": "node-s1", "display_id": 1, "start_byte": 7, "end_byte": 19 },
  "turns": [
    {
      "question_node_id": "node-q1",
      "question": "What is it?",
      "explanation_node_id": "node-e1",
      "explanation": "A rate limiter."
    }
  ],
  "question": "Why not a fixed window?",
  "author": "ada"
}
JSON
```

| field | meaning |
| --- | --- |
| `source.text` | The complete original text |
| `source.payload_hash` | `payload.hash` from the parse result. Rust recomputes it from `text`; a mismatch is `400 payload_mismatch` |
| `source.metadata` | Optional provenance (`kind`, `reference`, `author`), preserved |
| `target` | The span asked about: opaque `node_id`, display-only `display_id`, and a half-open, non-empty byte range on `char` boundaries (`400 invalid_target` otherwise) |
| `turns` | Earlier `{question_node_id, question, explanation_node_id, explanation}` entries, oldest first. Omit on the first turn. Node IDs are recorded as given, not verified |
| `question` | The user's question, preserved unchanged |
| `author` | Optional human identity; the question node author, otherwise `unknown` |
| `project_id` | Optional id of a registered project (`POST /projects`). The Explainer then reads that project's folder for this call only, grounding is `project`, and `run.project_id` records it. It takes precedence over `DAFTPROMPT_PROJECT_ROOT`. Never a filesystem path |

```sh
curl -sS -X POST http://127.0.0.1:8080/clarify \
  -H 'content-type: application/json' \
  -d '{"source": {"text": "We use a token bucket.", "payload_hash": "<payload.hash>"},
       "target": {"node_id": "node-s1", "display_id": 1, "start_byte": 7, "end_byte": 19},
       "question": "What is a token bucket?",
       "project_id": "project-18f3a2-1"}'
```

### Response

The response is a `ClarifyResult`:

| field | meaning |
| --- | --- |
| `question_payload`, `explanation_payload` | Payloads holding the question and the explanation (`hash`, `content`, `created_at`) |
| `tool_payloads` | Payloads holding tool outputs, referenced by `tool_calls[].output_hash` |
| `question_node_id`, `explanation_node_id` | IDs of the two new nodes in `nodes` |
| `nodes` | Question node (`parent_id` = target node, or the previous explanation node on a follow-up) and explanation node (`author` `agent`) |
| `node_contexts` | Question context, explanation context (`seq 0`), then one context per citation (`seq 1..n`) |
| `labels` | One `clarification` label on a first turn; empty on a follow-up |
| `citations` | `{ index, context_id, location }` in citation order; `[n]` markers in the explanation refer to `index`. `location` is `{ "kind": "file", "path", "start_line", "end_line" }` (1-based, inclusive) or `{ "kind": "source", "start_byte", "end_byte" }` (half-open bytes of `source.text`) |
| `grounding` | `project` or `text_only` |
| `source_window` | `{ start_segment, end_segment, start_byte, end_byte }`: the 1-based inclusive `\n`-separated segments the agent was given, and the byte range they cover. Omitted only in results produced before the field existed |
| `run`, `run_inputs`, `tool_calls` | In-memory run metadata (agent `explainer`), including every tool call and `submit` |

### Configuration

| variable | default | meaning |
| --- | --- | --- |
| `DAFTPROMPT_EXPLAINER_PROVIDER` | `neuralwatt` | `openai`, `neuralwatt`, `cerebras`, or `groq`; recorded as `run.provider`. Independent of `DAFTPROMPT_API_PROVIDER` |
| `DAFTPROMPT_EXPLAINER_MODEL` | `glm-5.3-flash` | Model identifier recorded as `run.model`; select `gpt-oss-120b` for Cerebras |
| `DAFTPROMPT_PROJECT_ROOT` | unset | Fallback directory for requests without `project_id`. Must exist and be a directory or startup fails. A request's `project_id` takes precedence over it. Unset means such requests are text-only; a hosted deployment would leave it unset so every folder goes through the registry and policy |

Credentials are the provider's own variables (`NEURALWATT_API_KEY`,
`NEURALWATT_BASE_URL`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `CEREBRAS_API_KEY`,
`CEREBRAS_BASE_URL`, `GROQ_API_KEY`, `GROQ_BASE_URL`), shared with Scribe.
Startup builds both clients, so the credentials for both providers must be
present; with the defaults that is `NEURALWATT_API_KEY` for the Explainer.
Input limits (`DAFTPROMPT_API_MAX_INPUT_BYTES`, `DAFTPROMPT_API_MAX_SEGMENTS`)
apply to `source.text` too. Startup logs the grounding mode:

```text
daftprompt-api POST /clarify: explainer provider neuralwatt, model glm-5.3-flash, grounding project (root /path/to/repo)
```

or `grounding text_only`. In project mode the model gets read-only
`list_files`, `search`, and `read_file` tools. Every path is resolved against
the canonical root; paths that escape it (including through symlinks) and
ignored or sensitive files are refused. A file citation is valid only for lines
a `read_file` call returned in the same run.

### Limits and defaults

| limit | default |
| --- | --- |
| Question | 2048 bytes |
| Earlier turns | 8 turns, 16 KiB total |
| Source text | 32768 bytes / 1000 segments (shared with `/parse`) |
| Source window given to the agent | 20 segments around the target |
| Agent model calls per attempt | 12 |
| Tool calls per attempt | 20 |
| Citations | 20 |
| Attempts | 3 (invalid output only) |
| Timeout | 120 s **per attempt**, covering the whole tool loop |
| JSON envelope | twice the `/parse` envelope (`2 * (6 * text limit + 16 KiB)`) |

Oversized fields are rejected, never truncated.

### Errors

Same shape and statuses as `/parse`, with two more `400` kinds:

| status | `kind` | when |
| --- | --- | --- |
| 400 | `invalid_request` | Malformed JSON, missing fields, or empty question |
| 400 | `payload_mismatch` | `source.payload_hash` is not the SHA-256 of `source.text` |
| 400 | `invalid_target` | The target range is empty, out of bounds, or off a `char` boundary |
| 413 | `oversized_input` | Question, history, source, or JSON envelope over its limit |
| 422 | `invalid_output` | No valid submission within the attempt budget (bad schema, unread file citation, bad `[n]` marker, ...) |
| 403 | `forbidden` | The access policy refuses `project_id` for this caller |
| 404 | `not_found` | `project_id` names no registered project |
| 422 | `project_unavailable` | The project's folder is gone or not a directory |
| 500 | `internal_error` | Adapter or configuration failure |
| 502 | `provider_error` | The provider request failed (not retried) |
| 504 | `timeout` | One attempt exceeded the timeout (not retried). The worst case is one timeout |

### Security

The API binds to loopback and has no authentication. With
`DAFTPROMPT_PROJECT_ROOT` set, anything that can reach the port can make the
model read files under that root and receive their contents in explanations and
citations. Files outside the root are never exposed, but do not point the root
at a directory holding secrets, and do not bind the service to a non-loopback
address without your own access control.

## Agents and project briefings (Epic 006, Tasks A to C)

[Epic 006](../epics/006-project-briefing.md) adds read-only agents that brief the user on a registered project. Task A provides `GET /agents` and the **Chronicler**, which reads git history. Task B adds `GET /projects/{id}/profile` and the **Surveyor** (`POST /projects/{id}/overview`). Task C adds the **Operator** (`POST /projects/{id}/commands`), which lists the commands the project supports. Commands are discovered, never run.

Each successful history, overview, or commands run for a registered project is
also appended to the SQLite `project_agent_response` table (see
[data-model](data-model.md)); failed runs write nothing. If the finished run
cannot be written, the request fails with `500 internal_error` naming the run id
and no result is returned.

**Reuse ([Epic 007](../epics/007-project-paths-and-agent-storage.md), Task C1).**
Only the Chronicler's responses are served from storage so far. A default-task
history request without `"refresh": true`, whose granted scope is the whole
registered path, first looks for a stored response of the same project, agent,
request and configuration at the same full `HEAD` commit and repository-relative
subpath: the requested path's own responses first, then other paths of that
project, newest first, at most three candidates. Each candidate is verified
against the *requested* path (the source path's folder is never read): the
recorded `git_log` call is repeated with the same arguments and limits and its
output hash must match, and `HEAD` and subpath must be unchanged before and
after the check. A working tree with uncommitted changes does not matter to
this history-only agent. A verified hit calls no model, creates no run, and
inserts no row; the original result is returned with its original run ids. Any
unknown input (not a repository, unborn `HEAD`, no `git`, a probe error or
timeout, an unusable stored row, a failed check) is a miss and the agent runs;
hit and miss reasons are logged to stderr without source text. Custom prompts,
`refresh: true`, and a policy-narrowed scope always run fresh.

Overview (Surveyor) and commands (Operator) requests use the same lookup with a
stricter check, because their output depends on the working tree. A dirty
checkout does not block capture or reuse; equivalence comes from replaying the
recorded observations. Against the requested path only: the project profile is
recomputed and compared; the common-file preload is rerun and its complete ledger compared with
the saved fingerprint (so a newly selected file, such as a new ignored document,
is a miss); for Operator the command candidates, their manifests, and the stored
working directories are recomputed and compared; and the model's own
`list_files`, `search`, and `read_file` calls are replayed under their original
limits and their results (errors and truncation included, plus each read file's
whole-file hash) compared. Cited-line hashes alone never decide a hit. The
`submit` call is not replayed, no discovered command is ever executed, and no
model is called; an observation that was not completely recorded makes the
response a miss. A hit returns the stored commands with their citation indexes
and working directories unchanged, interpreted against the requested path.

The history, overview, and commands bodies accept `"refresh": true` (default
`false`): it skips the lookup but the new result is still stored.

**Delivery metadata.** These three routes return the result with two extra
top-level fields (`Delivered<T>`); every existing field stays top level,
including the `BriefingResult` fields flattened into a `CommandsResult`.

```json
{
  "...every BriefingResult / CommandsResult field...": "...",
  "project_path_id": "path-...",
  "cache": {
    "requested_path_id": "path-...",
    "source_path_id": "path-...",
    "response_id": "response-...",
    "captured_at": "2026-11-08T10:00:00Z",
    "kind": "fresh"
  }
}
```

`kind` is `fresh` (a new run, stored just now), `same_path` (a verified earlier
response of the requested path), or `other_path` (a verified response owned by
another path of the project; `source_path_id` names it and the result keeps that
run's provenance). `response_id` is the stored row's id, not a run id, and
`captured_at` is when it was stored. A result that was not stored has
`cache: null`: the unregistered `DAFTPROMPT_PROJECT_ROOT` fallback (which also
has `project_path_id: null`, having no path record) or a service without a
database. Clients that predate the wrapper ignore the two extra fields.

The GUI (see [gui.md](gui.md#project-briefing-epic-006)) sends `project_path_id`
for the path selected in its path selector on these routes, on `/parse` and
`/clarify`, with `refresh` false for ordinary opens and `true` only from a
section's Re-run control, and drops any reply whose project and path no longer
match the selection. It shows `Reused from <path> · captured <time>` for
`same_path` and `other_path` and nothing for `fresh`. It does not call
`GET /projects/{id}/profile` or register paths.

### `GET /agents`

Lists every configured agent as `AgentInfo[]`: `{ "kind", "provider", "model", "prompt_version" }`. `provider` and `model` are configuration (the values recorded on runs), never credentials. An agent that is not configured on this service is not listed.

```sh
curl -sS http://127.0.0.1:8080/agents
```

```json
[
  { "kind": "scribe", "provider": "openai", "model": "gpt-4o-mini", "prompt_version": "scribe-v4" },
  { "kind": "explainer", "provider": "neuralwatt", "model": "glm-5.3-flash", "prompt_version": "explainer-v1" },
  { "kind": "chronicler", "provider": "neuralwatt", "model": "glm-5.3-flash", "prompt_version": "chronicler-summary-v1" }
]
```

### `POST /projects/{id}/history` (Chronicler)

Summarizes recent history supplied by Rust. Before the model call, Rust reads the latest `DAFTPROMPT_CHRONICLER_COMMIT_COUNT` commits (default **10**, newest first). The model receives that bounded log and returns **plain text**, with no tools, JSON schema, or output-submission tool. The existing `git_log`/`git_show` agent is retained as `Chronicler::brief_with_tools`, but no API route invokes it.

An empty body or `{}` runs the default task: summarize themes, notable changes, and areas in progress. Optional `prompt` replaces that task but uses the same supplied history; it cannot request additional history. Blank prompts mean absent; the limit is 2048 bytes. Optional `author` records the prompt author (default `unknown`, at most 200 bytes).

```sh
curl -sS -X POST http://127.0.0.1:8080/projects/$PROJECT_ID/history
curl -sS -X POST http://127.0.0.1:8080/projects/$PROJECT_ID/history \
  -H 'content-type: application/json' \
  -d '{"prompt":"What changed in auth?"}'
```

The HTTP response remains a `BriefingResult` JSON object. `summary_payload.content` contains the plain-text answer. `citations` is empty; the summary context references only its payload. `tool_calls` records the one **Rust-initiated** `git_log` read, with its output in `tool_payloads`; there is no model tool invocation or synthetic `submit` record. `run.prompt_version` is `chronicler-summary-v1`. Each request reads history afresh; retries reuse that same bounded history. Optional prompt nodes, run metadata, and `project_id` are unchanged.

Git reads run off the async worker. Existing git process/output caps apply, including at most 50 commits by default. The configured count is clamped to these limits; invalid or zero environment values fall back to 10. Missing git, non-repository folders, and truncation retain their notices. The model is instructed to acknowledge unavailable or insufficient history.

Empty or oversized summaries are invalid output and retry within the three-attempt budget. Provider HTTP 400 `output_parse_failed` also uses that budget. Exhaustion returns `422 invalid_output`. Other errors retain the existing shapes: `404 not_found`, `403 forbidden`, `422 project_unavailable`, `400 invalid_request`, `413 oversized_input`, `502 provider_error`, `504 timeout`, and `500 internal_error`.

### Git tools and their limits

The preserved, unused tool-based Chronicler path exposes `git_log` and `git_show` (`daftprompt_core::git_tools`). They are read-only and run `git` as a subprocess without a shell:

- Arguments come only from validated, typed parameters: hashes are 7 to 64 hex characters, `since` is a short date expression, `path` is a root-relative path with no `..`, absolute, hidden or `.env` component, and nothing starts with `-` (or `:`).
- Every invocation is `git -C <root> --no-pager -c core.pager=cat -c core.fsmonitor=false -c log.showSignature=false --literal-pathspecs`, with `--no-ext-diff --no-textconv --no-color`, and the environment sets `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0` and `GIT_CONFIG_NOSYSTEM=1`.
- Each call has a 10 s timeout and a 1 MiB read cap; model-visible output is capped at 32 KiB (`git_show` patches at 16 KiB), at most 50 commits per `git_log`, and at most 12 tool calls per attempt.
- A project root that is a subdirectory of a repository sees only history under it (`-- .`; `git_show` paths are root-relative). A root that is not in a repository, or a machine without `git`, yields a `not_a_git_repository` or `git_unavailable` notice.
- Commit subjects, messages and diffs are data; the system prompt tells the model never to follow instructions found in them.

### Configuration

| variable | default | meaning |
| --- | --- | --- |
| `DAFTPROMPT_CHRONICLER_PROVIDER` | `neuralwatt` | `openai`, `neuralwatt`, `cerebras`, or `groq`; recorded as `run.provider`. Same default as the Explainer, independent of the other agents |
| `DAFTPROMPT_CHRONICLER_COMMIT_COUNT` | `10` | Latest commits supplied by Rust; positive integer, clamped to git limits |
| `DAFTPROMPT_CHRONICLER_MODEL` | `glm-5.3-flash` | Model identifier recorded as `run.model` |

Credentials are the provider's own variables, shared with Scribe and Explainer. Startup builds the Chronicler's client too, so with the defaults `NEURALWATT_API_KEY` must be present. Startup logs its provider and model:

```text
daftprompt-api POST /projects/{id}/history: chronicler provider neuralwatt, model glm-5.3-flash
```

The Surveyor is configured the same way, independently:

| Variable | Default | Meaning |
| --- | --- | --- |
| `DAFTPROMPT_SURVEYOR_PROVIDER` | `neuralwatt` | Provider recorded as `run.provider` |
| `DAFTPROMPT_SURVEYOR_MODEL` | `glm-5.3-flash` | Model recorded as `run.model` |

```text
daftprompt-api POST /projects/{id}/overview: surveyor provider neuralwatt, model glm-5.3-flash
```

### `GET /projects/{id}/profile`

The deterministic `ProjectProfile` of the project folder, computed by Rust with no model call (so it works even when no Surveyor is configured). The project is resolved for `AgentKind::Surveyor`: `404 not_found`, `403 forbidden`, `422 project_unavailable`.

```bash
curl -sS http://127.0.0.1:8080/projects/project-18f3a2-1/profile
```

```json
{
  "languages": [
    {
      "language": "rust",
      "projects": [
        {
          "dir": "",
          "name": null,
          "package_manager": "cargo",
          "toolchain": "nightly-2026-10-25",
          "workspace": true,
          "packages": ["daftprompt-api", "daftprompt-core", "daftprompt-gui", "daftprompt-types"],
          "files": ["Cargo.toml", "rust-toolchain.toml"]
        }
      ]
    }
  ],
  "key_files": ["AGENTS.md", "docs/PRD.md", "docs/api.md", "Cargo.toml", "rust-toolchain.toml"]
}
```

- `languages[].language` is `rust`, `typescript` (also JavaScript) or `python`; a `truncated: true` flag appears when a search or the project cap hit a limit. A language that is not used is absent; an unrecognized folder gives `"languages": []`.
- Each `projects[]` entry is one manifest directory (`dir` is root-relative, `""` is the root), so a monorepo lists each project with its own `package_manager` (`cargo`, `npm`, `pnpm`, `yarn`, `bun`, `uv`, `poetry`, `pip`, or `null`). Members of a declared workspace are folded into the workspace root's `packages`. `toolchain` is the Rust channel, `typescript`/`javascript`, or Python's `requires-python`. `files` is the evidence (manifests and lockfiles).
- `key_files` are what Surveyor reads first: root README*/DEVELOP*/CONTRIBUTING*/ARCHITECTURE*, AGENTS.md, CLAUDE.md, then, in this priority order under the cap of 40: top-level docs, text files directly in `docs/`, each language's manifests (lockfiles excluded), and finally text files in nested `docs/**` folders. At most 20 files are taken from `docs/` in all.
- Detection uses the project tools' rules: hidden, ignored and binary files are invisible (so a binary `bun.lockb` is not seen; `bun.lock` and the `packageManager` field are), vendored directories (`node_modules`, `target`, `vendor`, `dist`, `build`, virtualenvs) are skipped, manifests deeper than 4 directories are ignored, and malformed manifests still detect the language with less detail.

### Initial file context (Surveyor and Operator)

Before the first model call, Rust supplies a bounded stack-aware snapshot on a
blocking worker. Generic documentation and task runner files are interleaved with
language-owned suggestions, round-robin across detected stacks. Rust contributes
Cargo/toolchain and workspace member manifests, source entrypoints and listings;
TypeScript contributes package/workspace/tsconfig manifests and src/app/pages;
Python contributes pyproject/setup/requirements/manage files and package/source
listings. Existing manifests are rediscovered with the language scanner, since
folded workspace package names are not paths. Lockfile contents are excluded.

At most 24 files and 12 shallow listings are supplied, with 50 paths per listing,
200 lines and 8 KiB output per file, and a 96 KiB combined serialized context cap.
Existing stricter tool limits and file protections still apply. Only records
actually included in the context seed the citation ledger. The snapshot is reused
across retries without consuming the model's tool budget; filenames alone are
not citable evidence. Tools remain available for omitted or truncated context.
Active prompt versions are `surveyor-v3` and `operator-v3`.

### `POST /projects/{id}/overview` (Surveyor)

Summarizes the project's files and documentation. The body and the errors are those of `/history`, resolved for `AgentKind::Surveyor` (`500 internal_error` when no Surveyor is configured). The default task is a **project summary**: what it is, the stack, the layout, and where to read further. The model gets the computed profile and key-file list as context and has `list_files`, `search` and `read_file`.

```bash
curl -sS -X POST http://127.0.0.1:8080/projects/project-18f3a2-1/overview
curl -sS -X POST http://127.0.0.1:8080/projects/project-18f3a2-1/overview \
  -H 'content-type: application/json' \
  -d '{"prompt": "how is it tested?", "author": "@ada"}'
```

The response is the same `BriefingResult` as `/history`, with these differences: `citations` are files only (`"location": { "kind": "file", "path", "start_line", "end_line" }`, which must lie within lines `read_file` returned in the same run; a commit citation is rejected and retried), the context target is `file` with the content hash and a whole-line range, `profile` holds the `ProjectProfile` above (it is `null` for `/history`), `run.agent` is `surveyor`, and `notices` may contain `truncated`.

### `POST /projects/{id}/commands` (Operator)

Lists the commands the project supports (setup, build, run, dev server, test, lint, format) with a short summary. **Commands are never run, copied or executed by the service.** The body and the errors are those of `/history`, resolved for `AgentKind::Operator` (`404 not_found`, `403 forbidden`, `422 project_unavailable`, `500 internal_error` when no Operator is configured). An empty body is the default task; a `prompt` replaces it.

The Operator is configured independently:

| Variable | Default | Meaning |
| --- | --- | --- |
| `DAFTPROMPT_OPERATOR_PROVIDER` | `neuralwatt` | Provider recorded as `run.provider` |
| `DAFTPROMPT_OPERATOR_MODEL` | `glm-5.3-flash` | Model recorded as `run.model` |

```text
daftprompt-api POST /projects/{id}/commands: operator provider neuralwatt, model glm-5.3-flash
```

```bash
curl -sS -X POST http://127.0.0.1:8080/projects/project-18f3a2-1/commands
curl -sS -X POST http://127.0.0.1:8080/projects/project-18f3a2-1/commands \
  -H 'content-type: application/json' \
  -d '{"prompt": "how do I run the tests?", "author": "@ada"}'
```

How it works: Rust parses the manifests into **command candidates** (no model call): Rust gets `cargo build`, `test`, `clippy` and `fmt` (with `--workspace` for a workspace) and `cargo run -p <pkg>` per binary package (`--bin` for named `[[bin]]` targets); TypeScript/JavaScript gets an install command and one command per `package.json` script, run with the detected package manager (`npm run x`, `pnpm x`, `yarn x`, `bun run x`); Python gets `uv sync` or `poetry install` or, for pip, `python3 -m venv .venv` and `.venv/bin/pip install ...` as separate commands, `[project.scripts]` entries run through the tool, `manage.py runserver` and `test`, and `pytest` when it is a dependency or configured. Each candidate has a `working_dir` (the project directory, or a workspace member's own directory for `package.json` scripts) and cites the manifest lines it came from. The model receives the profile and the numbered candidate list, has `list_files`, `search` and `read_file`, and submits the commands to offer. It picks and labels candidates (`candidate` is the 1-based number, and `command` and `working_dir` must equal the candidate's) and may add a documented command, such as a README "how to run" step, only with file citations.

The response is a `CommandsResult`: the `BriefingResult` fields of `/overview` (flattened) plus `commands`.

```json
{
  "project_id": "project-18f3a2-1",
  "summary_payload": { "...": "..." },
  "citations": [
    { "index": 1, "context_id": "ctx-...", "location": { "kind": "file", "path": "README.md", "start_line": 10, "end_line": 12 } },
    { "index": 2, "context_id": "ctx-...", "location": { "kind": "file", "path": "Cargo.toml", "start_line": 1, "end_line": 1 } }
  ],
  "commands": [
    {
      "id": "cmd-18f3a2c4-0",
      "label": "build",
      "command": "cargo build --workspace",
      "working_dir": "",
      "kind": "build",
      "language": "rust",
      "origin": "detected",
      "citation_indexes": [2]
    },
    {
      "id": "cmd-18f3a2c4-1",
      "label": "site",
      "command": "make site",
      "working_dir": "web",
      "kind": "run",
      "language": null,
      "origin": "documented",
      "citation_indexes": [1]
    }
  ]
}
```

- `kind` is `setup`, `build`, `run`, `dev`, `test`, `lint`, `format` or `other` (a type check is `lint`).
- `origin` is `detected` (a manifest candidate; `language` is set) or `documented` (backed by the model's file citations; `language` is `null`).
- `working_dir` is root-relative; `""` is the project root.
- `citation_indexes` are 1-based `index` values of the result's `citations`. They are never empty: a detected command points at a file citation for its manifest lines that Rust appended after the model's citations (computed from the manifest, with the content hash and a whole-line range); a documented command points at the model's own validated citations.
- Validation (invalid output is retried, then `422 invalid_output`): every command is a candidate or cites at least one file; `working_dir` is a directory inside the root (no absolute path, `..` or hidden component); labels are 24 characters or fewer and unique; no duplicate `(working_dir, command)` pair; at most 12 commands (`OperatorConfig::max_commands`); every `[n]` marker and citation index resolves; citations are files only and must lie within lines `read_file` returned in the same run. A command is one line, never chained with `&&`.
- The `run.agent` is `operator`; `profile` and `notices` are as for `/overview`.

The provider factory itself (`daftprompt_core::llm::build_model`) reads the provider variables listed above; the API no longer contains it.

## Groq configuration

Groq uses its [OpenAI-compatible endpoint](https://console.groq.com/docs/openai)
with Rig's Chat Completions client. The model
[`openai/gpt-oss-20b` supports tool calling](https://console.groq.com/docs/tool-use/overview),
including the local tools used by Scribe and Explainer. Set both agents explicitly:

```dotenv
DAFTPROMPT_API_PROVIDER=groq
DAFTPROMPT_API_MODEL=openai/gpt-oss-20b
DAFTPROMPT_EXPLAINER_PROVIDER=groq
DAFTPROMPT_EXPLAINER_MODEL=openai/gpt-oss-20b
GROQ_API_KEY=your-key
# GROQ_BASE_URL=https://api.groq.com/openai/v1
```

For the release GUI, build `cargo build --release -p daftprompt-api`, then restart
the GUI/API. Process environment variables override `.env`. No new dependencies
are required. Live model availability and extraction quality require a live test.

## Projects and storage

The API opens one SQLite database at startup and serves a registry of
*projects*: a project is a named local folder. Registering a project records
its location only; nothing inside the folder is read, written, or deleted, and
removing a project from the registry never touches files.

A request selects a project per call with `project_id` on `/parse` and
`/clarify`; clients register a folder here first (the GUI does this on a
folder pick or drop; it attaches no further paths itself). Resolution has three steps, in `daftprompt_core::access`:
the registry lookup, a `RootPolicy` decision for the caller (`Principal`) and
the agent (Scribe or Explainer), and a check that the folder still exists. The
default `LocalPolicy` allows every registered project for both agents and the
principal is always `Local`, because there is no authentication yet. A hosted
server would replace the principal extractor with its auth layer and install a
policy that applies per-user, per-team, and per-agent folder rules. When a
request has no `project_id`, `DAFTPROMPT_PROJECT_ROOT` (if set) is the
Explainer's default project.

| variable | default | meaning |
| --- | --- | --- |
| `DAFTPROMPT_DB_PATH` | `<OS user data dir>/daftprompt/daftprompt.db` | SQLite file. Missing parent directories are created. On macOS: `~/Library/Application Support/daftprompt/daftprompt.db`; Linux: `$XDG_DATA_HOME/daftprompt/daftprompt.db`. A database written by a newer build is refused at startup |

A project is `{ id, name, root_path, created_at, last_opened_at, paths,
default_path_id }`. `id` is a Rust-generated string
(`project-<nanos hex>-<counter hex>`, the same scheme as other records). A
project owns one or more registered local folders, `paths`, each
`{ id, project_id, root_path, is_default, created_at, last_opened_at }` with a
Rust-generated `path-...` id. A path's `root_path` is the absolute,
canonicalized folder and is unique across the whole registry; exactly one path
per project is the default (listed first), named by `default_path_id`. The
project's `root_path` is computed from the default path for older clients; it
is not stored or writable separately. Timestamps are RFC 3339 UTC. A folder
that disappears stays registered; using it fails with `project_unavailable`
and never switches to another path.

| route | body | success | notes |
| --- | --- | --- | --- |
| `GET /projects` | none | `200` array, most recently opened first | |
| `POST /projects` | `{ "path": "...", "name": "..."? }` | `201` project | The path must exist and be a directory; the name defaults to the folder name. If the folder is already registered the existing project is returned unchanged with `200` (the supplied name is ignored) |
| `GET /projects/{id}` | none | `200` project | Includes `paths` and `default_path_id` |
| `POST /projects/{id}/open` | none | `200` project | Sets `last_opened_at` to now on the project and its default path |
| `PATCH /projects/{id}` | `{ "name"?: "...", "default_path_id"?: "..." }` | `200` project | At least one field. Blank names are rejected; the default must be a path of this project (`404` otherwise). Both apply or neither |
| `DELETE /projects/{id}` | none | `204` | Registry only; removes its paths too |
| `GET /projects/{id}/paths` | none | `200` array of paths, default first | |
| `POST /projects/{id}/paths` | `{ "path": "..." }` | `201` path | The folder must exist and be a directory. Attaching a folder the project already has returns that path with `200`; a folder registered to another project is `400` |
| `DELETE /projects/{id}/paths/{path_id}` | none; query `replacement_default_path_id`? | `200` updated project | Registry only. Removing the default path requires the replacement, another path of the project, which becomes the default; removing the only path is `400` (delete the project instead). An unknown path id, or one owned by another project, is `404` |

`project_path_id` is optional wherever a project is named. It is a body field
on `POST /parse`, `POST /clarify`, and the history, overview, and commands
routes (which also require their `project_id` or URL id), and a query parameter
on `GET /projects/{id}/profile`. Omitted, the persisted default path is used,
so older bodies behave as before. An id that is unknown or belongs to another
project is `404 not_found`; `project_path_id` without a `project_id` is `400`.
The access policy receives the selected path, and the scope's root is that
path's folder (a policy may narrow it, never widen it).

Failures use the same `{ "kind", "detail" }` body as the other routes:
`invalid_request` (400: bad path, blank name, malformed JSON), `not_found`
(404: unknown project or path id), `internal_error` (500: storage failure, or no store
configured). On `/parse` and `/clarify`, `project_id` adds `not_found` (404),
`forbidden` (403), and `project_unavailable` (422: the request is well formed
but the registered folder cannot be used right now, which is not the client's
syntax error that 400 means). A `project_id` with no store configured is
`500 internal_error`.

```sh
curl -s -X POST http://127.0.0.1:8080/projects \
  -H 'content-type: application/json' \
  -d '{"path": "/Users/me/code/my-app"}'
curl -s http://127.0.0.1:8080/projects
curl -s http://127.0.0.1:8080/projects/$ID
curl -s -X POST http://127.0.0.1:8080/projects/$ID/open
curl -s -X PATCH http://127.0.0.1:8080/projects/$ID \
  -H 'content-type: application/json' -d '{"name": "My app"}'
curl -s -X POST http://127.0.0.1:8080/projects/$ID/paths \
  -H 'content-type: application/json' -d '{"path": "/Users/me/code/my-app-2"}'
curl -s -X PATCH http://127.0.0.1:8080/projects/$ID \
  -H 'content-type: application/json' -d '{"default_path_id": "'$PATH_ID'"}'
curl -s -X DELETE "http://127.0.0.1:8080/projects/$ID/paths/$PATH_ID?replacement_default_path_id=$OTHER"
curl -s -X DELETE http://127.0.0.1:8080/projects/$ID
```
