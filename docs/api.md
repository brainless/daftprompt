# HTTP API

The API is a stateless wrapper around Scribe. One `POST /parse` request returns
one complete, validated graph fragment; each submission is independent and
nothing is stored. The service binds to loopback by default and has no
authentication, session, or history endpoints.

## Start the service

The API process builds the provider client itself and reads its credentials
from the environment. The default provider is OpenAI, and
`OPENAI_BASE_URL` selects an OpenAI-compatible endpoint. Neuralwatt is also
supported (see below).

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
| `DAFTPROMPT_API_ADDR` | `127.0.0.1:8080` | Bind address (`IP:port`). Keep it on loopback unless you add your own access control |
| `DAFTPROMPT_API_PROVIDER` | `openai` | `openai` or `neuralwatt`; recorded as `run.provider` |
| `DAFTPROMPT_API_MODEL` | `gpt-4o-mini` (`qwen3.6-35b-fast` for `neuralwatt`) | Model identifier recorded as `run.model` |
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
# DAFTPROMPT_API_MODEL defaults to qwen3.6-35b-fast
cargo run -p daftprompt-api
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
  "run": { "id": "run-1", "agent": "scribe", "provider": "openai", "model": "gpt-4o-mini", "prompt_version": "scribe-v4+plain-v1", "started_at": "…", "ended_at": "…", "status": "done", "attempts": 1 },
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
| `DAFTPROMPT_EXPLAINER_PROVIDER` | `neuralwatt` | `openai` or `neuralwatt`; recorded as `run.provider`. Independent of `DAFTPROMPT_API_PROVIDER` |
| `DAFTPROMPT_EXPLAINER_MODEL` | `glm-5.3-flash` | Model identifier recorded as `run.model` |
| `DAFTPROMPT_PROJECT_ROOT` | unset | Directory the Explainer may read. Must exist and be a directory or startup fails. Unset means text-only mode |

Credentials are the provider's own variables (`NEURALWATT_API_KEY`,
`NEURALWATT_BASE_URL`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`), shared with Scribe.
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
