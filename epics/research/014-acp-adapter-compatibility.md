# Epic 014 Task 0: ACP adapter compatibility spike findings

Status: Task 0 complete. This is the write-up required by Epic 014's Task 0
("Prove compatibility with the local TypeScript adapter"). The throwaway
Rust harness that produced this evidence lives at
`scratch/acp-spike/` (see that directory's own doc comments for how to run
it). It is intentionally **not** a workspace member (`scratch/acp-spike/Cargo.toml`
has its own empty `[workspace]` table) so it does not affect
`cargo check --workspace` / `cargo test --workspace`, and is not itself the
final `daftprompt-acp` crate — Task 1 builds that as a real workspace member
using the conclusions below.

## 1. Adapter revision and version

- Local clone: `~/Projects/codex-acp`
- Git revision: `97d260e3d9314d95347e50ab35ea22800546298d` (2026-08-16)
- `package.json` name/version: `@agentclientprotocol/codex-acp` `1.4.0`
- Bundled Codex CLI dependency (`@openai/codex` in `package.json`): `codex-cli 0.147.0` (confirmed via `node_modules/.bin/codex --version`)
- Its TypeScript SDK dependency: `@agentclientprotocol/sdk ^1.3.0` (resolved `1.3.0`), `PROTOCOL_VERSION = 1` in that SDK.

## 2. Launch command and environment assumptions

Two workable launch commands, both without shell evaluation:

- **From source** (what this spike used): `npm run start --prefix ~/Projects/codex-acp`, which npm resolves to codex-acp's own `package.json` `"start"` script, `node --import tsx src/index.ts`. This is codex-acp's own documented "run from sources" launch command (see its `readme-dev.md`). Requires `npm install` to have been run once in that directory (`node_modules/@agentclientprotocol/sdk`, `node_modules/.bin/codex`, `tsx`, etc.).
- **From a built binary**: the packaged `codex-acp` executable (`npm run build`, or a downloaded release zip), invoked directly with no arguments.

Important gotcha found empirically: `node --import tsx <absolute path to src/index.ts>` **does not** work when launched with an arbitrary process cwd — Node resolves the `--import` loader specifier (`tsx`) against the process's current working directory, not the entry file's directory, so it fails with `ERR_MODULE_NOT_FOUND: Cannot find package 'tsx'` unless cwd is already `~/Projects/codex-acp`. Since the chosen Rust ACP SDK's launch-profile type (`AcpAgentConfig`) exposes only `command`/`args`/`env` — no `cwd` — the `npm run start --prefix <dir>` form is the one to standardize on for `daftprompt-acp`'s Codex launch profile (Task 6), because npm owns the cwd switch internally without a shell.

Environment variables the adapter reads (from `readme-dev.md`, no secrets required for the scripted spike, some relevant for a later live/auth profile): `CODEX_API_KEY`, `OPENAI_API_KEY`, `CODEX_PATH`, `CODEX_CONFIG`, `MODEL_PROVIDER`, `DEFAULT_AUTH_REQUEST`, `INITIAL_AGENT_MODE`, `NO_BROWSER`, `APP_SERVER_LOGS`. None of these were set for the scripted (fake-adapter) tests; the one live run used this workstation's already-authenticated Codex profile (`~/.codex/config.toml`) with no extra env vars.

## 3. Exact initialize request/response (live capture, 2026-08-17)

Request sent:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":true,"writeTextFile":true},"terminal":true},"clientInfo":{"name":"daftprompt-spike","version":"0.0.1"}}}
```

Response received (verbatim, this is `scratch/acp-spike/fixtures/initialize_response.json`):

```json
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"@agentclientprotocol/codex-acp","title":"Codex","version":"1.4.0"},"agentCapabilities":{"auth":{"logout":{}},"providers":{},"loadSession":true,"promptCapabilities":{"embeddedContext":true,"image":true},"sessionCapabilities":{"resume":{},"list":{},"close":{},"delete":{},"additionalDirectories":{}},"mcpCapabilities":{"acp":false,"http":true,"sse":false}},"authMethods":[{"id":"api-key","name":"API Key","description":"Use an API key to authenticate","_meta":{"api-key":{"provider":"openai"}}},{"id":"chat-gpt","name":"ChatGPT","description":"Use ChatGPT to authenticate"}],"_meta":{"steering":{"supported":true},"goal":{"version":1,"controlMethod":"_session/goal","actions":["set","pause","resume","clear"]},"jetbrains":{"air":{"version":1,"capabilities":["sessionFailure","agentFileChangeReport"]}}}}}
```

## 4. Negotiated protocol version and capabilities

- `protocolVersion: 1` — matches `PROTOCOL_VERSION` in both the TS SDK (`@agentclientprotocol/sdk` 1.3.0) and the Rust SDK (`agent-client-protocol` 2.0.0, `ProtocolVersion::V1`/`V2` both supported).
- `agentCapabilities`: `loadSession: true`, `promptCapabilities: {embeddedContext: true, image: true}` (no audio), `sessionCapabilities: {resume, list, close, delete, additionalDirectories}` all advertised, `mcpCapabilities: {http: true, sse: false, acp: false}`, `auth: {logout: {}}`.
- `authMethods`: `api-key` (OpenAI-provider API key) and `chat-gpt`.
- Non-standard `_meta` extensions are present (`steering`, `goal`, JetBrains `air`) — Epic 014 Design Decision #3 requires these be preserved/surfaced generically, not specially parsed; the spike does not special-case them.

## 5. New-session request/response (live capture)

Request: `{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp/acp-spike-repo","mcpServers":[]}}`

Response (trimmed to two of the ~28 real `availableModels` and one of the five real `configOptions` for fixture-file size; field names/shapes are unmodified — see `scratch/acp-spike/fixtures/session_new_response.json`): includes `sessionId` (a UUIDv7-shaped string), `models.availableModels` / `currentModelId`, `modes.availableModes` / `currentModeId` (`read-only` / `agent` / `agent-full-access`), and `configOptions` (mode, collaboration_mode, model, reasoning_effort, fast-mode — all `select`-typed with a `currentValue`).

## 6. One text prompt and representative `session/update` variants (live capture)

Prompt: `{"type":"text","text":"Say the single word: hello. Do not use any tools."}`

Observed `session/update` variants, in order, for a plain text turn: `available_commands_update` (large list of slash commands/skills), `session_info_update` (`_meta.codex.threadStatus.type: "active"`), `agent_message_chunk` (streamed word-by-word, each chunk `content: {type: "text", text: "..."}`), `usage_update` (`used`/`size` token counts), `session_info_update` (`threadStatus.type: "idle"`), `session_info_update` (`title` set from the prompt). For a turn that runs a shell command, two more variants appear: `tool_call` (`kind: "execute"`, `status: "in_progress"`, `content: [{type: "terminal", terminalId}]`, `rawInput: {command, cwd}`) followed by `tool_call_update` (`status: "completed"`, `rawOutput: {formatted_output, exit_code}`, `_meta.terminal_output_delta`/`terminal_exit`).

Final `session/prompt` response: `{"stopReason":"end_turn","usage":{"totalTokens":...,"inputTokens":...,"cachedReadTokens":...,"outputTokens":...,"thoughtTokens":0},"_meta":{"quota":{...}}}`.

These are captured (lightly trimmed) in `scratch/acp-spike/fixtures/session_update_stream.jsonl` and `.../prompt_response.json`, and are what `scratch/acp-spike`'s `fake_adapter` binary replays for the scripted tests.

## 7. Permission request/response with a harmless test action

**Not observed live**, despite four attempts with distinct harmless commands and settings:

1. `echo hello-permission-test` in the default new-session mode (`agent`).
2. The same command after switching the session to `read-only` mode via `session/set_mode` (Codex's own mode description says this "requires approval to edit files and run commands").
3. The same command with `CODEX_CONFIG='{"approval_policy":"untrusted","sandbox_mode":"workspace-write"}'` set for the adapter process.
4. `cat /etc/hosts` plus `touch` outside the session's `cwd`, and separately a `curl` to a real external host — both filesystem-escalation and network-escalation shaped commands.

All four ran to completion via `tool_call`/`tool_call_update` with no `session/request_permission` in between. The most likely explanation is this workstation's `~/.codex/config.toml` (a `trust_level = "trusted"` entry exists for many project paths, though not for the spike's scratch directory) combined with Codex's own command-risk heuristics classifying `echo`/`cat`/read-only `curl` as auto-approvable regardless of session mode or the `CODEX_CONFIG` override attempted. Reliably forcing a live approval prompt would need either a genuinely destructive/ambiguous command (out of scope for "harmless") or deeper knowledge of Codex App Server's internal approval heuristic than this spike's budget allowed.

Given Epic 014 explicitly allows scripted fixtures "where live Codex behavior is unpredictable" for exactly this reason, `scratch/acp-spike/fixtures/permission_request.json` reconstructs the exact request shape from `~/Projects/codex-acp/src/CodexApprovalHandler.ts` (`buildCommandPermissionRequest`/`buildCommandOptions`) and `src/ApprovalOptionId.ts` instead: `toolCall: {toolCallId, kind: "execute", status: "pending", rawInput: {command, cwd}}`, `options: [{optionId: "allow_once", ...}, {optionId: "allow_always", ...}, {optionId: "reject_once", ...}]`. This is schema-accurate (traced directly from the source that constructs the real request) but explicitly labeled in the fixture file as not wire-observed. `scratch/acp-spike`'s `permission_request_round_trip_with_harmless_action` test drives this fixture as a reverse request (`session/request_permission`) interleaved with an active turn, and asserts the client selects an offered `optionId` rather than inventing one, matching Design Decision #7.

## 8. Cancellation before and after a Codex turn ID exists (live capture)

- **Before**: sending `session/cancel` immediately after `session/new` (no `session/prompt` ever issued) produces no response (it is a notification) and has no observable effect — a subsequent ordinary prompt on the same session completes normally with `stopReason: "end_turn"`. Confirmed live and reproduced in `scratch/acp-spike`'s `cancel_before_turn_id_exists_is_a_safe_no_op` test.
- **During**: sending `session/cancel` ~400ms into a long-running prompt causes the pending `session/prompt` response to resolve (not hang or error) with `{"stopReason":"cancelled","usage":null,"_meta":{"quota":{"token_count":null,"model_usage":[]}}}`. Confirmed live and reproduced in `cancel_during_active_turn_yields_cancelled_stop_reason`.

## 9. Clean session close and child-process shutdown (live capture)

`session/close` returns `{}`. After stdin is closed (`child.stdin.end()`), the adapter's own `process.stdin.on("close", ...)` handler (`src/index.ts`) closes its internal Codex App Server connection's stdin and force-kills that inner process after a 2s grace period if it hasn't exited; the adapter process itself then exits (observed exit code `0` in every non-error live run). Forced termination (`SIGKILL`) of the adapter process itself is observed as `child.on("exit")` firing with `(code: null, signal: "SIGKILL")`, i.e. reliably detectable, not a hang. `scratch/acp-spike`'s `session_close_then_process_shutdown_is_clean` test exercises the close+shutdown path against the fixture adapter; `early_process_exit_is_observable_and_does_not_hang_pending_requests` exercises forced/early exit reaping directly via `tokio::process::Child::wait()`.

## 10. Malformed output, stderr, early exit, and timeout behavior

- **Malformed adapter stdout**: a non-JSON line written to stdout is logged (client-side, via the SDK's stream parser: `Failed to parse JSON message: ... SyntaxError: ...`) and otherwise ignored; the next valid line parses normally with no framing corruption. Confirmed live (injecting a bad line into the real adapter's own inbound stdin — same parser logic — via `node --import tsx src/index.ts`) and reproduced for outbound/adapter-emitted malformed lines in `scratch/acp-spike`'s `malformed_stdout_line_does_not_corrupt_subsequent_parsing` test.
- **Malformed client stdin**: symmetric behavior confirmed live — sending a garbage line to the real adapter's stdin between two valid requests does not crash it or affect the second request's response.
- **stderr**: fully separate from stdout in every live run (never observed interleaved with or corrupting stdout JSON) and confirmed under deliberately noisy concurrent stderr writes in `stderr_noise_does_not_appear_on_or_block_stdout`.
- **Early exit**: reaping semantics confirmed both live (`SIGKILL` → `exit(code: null, signal: "SIGKILL")`) and in the fixture harness (deliberate `exit(7)` after `initialize` → `ExitStatus::code() == Some(7)`, observed via `Child::wait()` within a bounded timeout, i.e. never a hang).
- **Request timeout**: the ACP wire protocol has **no native "this request timed out" signal** — if Codex hangs, `codex-acp` simply never answers. A client-enforced timeout (e.g. `tokio::time::timeout` around a pending request) is therefore a **client responsibility**, not adapter behavior to observe. `client_enforced_timeout_on_a_hung_request` proves this pattern works cleanly against a subprocess that never answers `session/prompt`. `daftprompt-acp` (Task 1) needs its own per-request timeout, not something to negotiate with the adapter.
- **Unknown method**: sending an unrecognized method to the real adapter returns a standard JSON-RPC `{"error":{"code":-32601,"message":"\"Method not found\": <method>","data":{"method":"<method>"}}}` — plain JSON-RPC 2.0, no adapter-specific error shape.

## 11. Rust ACP SDK compatibility conclusion

**A maintained Rust ACP SDK is compatible and is the recommended choice for Task 1**, not a minimal local JSON-RPC layer.

- Crate: [`agent-client-protocol`](https://github.com/agentclientprotocol/rust-sdk), version **2.0.0** on crates.io, license Apache-2.0, published by the **same GitHub organization** (`agentclientprotocol`) that publishes the TypeScript `@agentclientprotocol/sdk` codex-acp itself depends on.
- It supports `ProtocolVersion::V1` (and `V2`, behind an `unstable_protocol_v2` feature this spike did not enable), matching the `protocolVersion: 1` codex-acp actually negotiates.
- It ships a first-class `Client` role (`agent_client_protocol::Client`) with typed request/notification/reverse-request handling, a `Builder`/`ConnectionTo` API, and an `AcpAgent` transport (`AcpAgentConfig::new(command).arg(...).env(...)`) purpose-built for spawning an ACP agent subprocess over stdio — **no shell evaluation** (`shell_words::split` + `std::process::Command::new`, confirmed by reading `src/acp_agent.rs`), which directly satisfies Task 1's "Executable path and arguments are configured without shell evaluation" acceptance criterion.
- **Empirically proven end-to-end against the real local adapter**, not just against the scripted fixture: `scratch/acp-spike/tests/live_smoke.rs`'s `live_initialize_new_session_prompt_close` (opt-in, `ACP_SPIKE_LIVE=1`) ran `initialize` → `session/new` → `session/prompt` → `session/close` against `npm run start --prefix ~/Projects/codex-acp` using this crate and completed successfully: negotiated `ProtocolVersion(1)`, `agentInfo.name == "@agentclientprotocol/codex-acp"`, `agentInfo.version == "1.4.0"`, and a real Codex turn resolved with `stop_reason: EndTurn`.
- It also round-trips all the scripted fixture scenarios in `scratch/acp-spike/tests/typed_client.rs` (initialize, session/new, streamed `session/update` variants including `tool_call`/`tool_call_update`, a `session/request_permission` reverse request with an offered-not-invented `optionId`, cancellation before and during a turn, and `session/close`).

One known gap: `AcpAgentConfig` has no `cwd` setter (see §2's `--import tsx` cwd gotcha). `daftprompt-acp`'s Codex launch profile (Task 6) should default to an `npm run start --prefix <dir>`-shaped invocation for "run from local dev clone" rather than a bare `node <entry>` invocation, and/or use the packaged `codex-acp` binary (no cwd dependency at all) as the primary supported path once Task 6 lands.

## What's fixture-derived vs. live-captured, at a glance

| Fixture file | Source |
|---|---|
| `initialize_response.json` | Live capture, verbatim |
| `session_new_response.json` | Live capture, trimmed (models/config options list) for file size; shapes/values otherwise unmodified |
| `session_update_stream.jsonl` | Live capture (two separate real runs — a plain text turn and a shell-exec turn — combined into one representative stream), trimmed `available_commands_update` |
| `prompt_response.json` | Live capture, verbatim |
| `permission_request.json` | **Not** live-captured — reconstructed from `CodexApprovalHandler.ts` source, explicitly labeled as such in the file |

## Task 0 acceptance criteria status

- [x] The adapter is launched as a child process and communicates over stdio.
- [x] Initialize, session/new, session/prompt, session/update, permission, cancellation, and session/close behavior is observed against the local clone. (Permission is source-derived, not live-observed — see §7 for why, and the fixture's own `_comment` field.)
- [x] The selected Rust ACP dependency is pinned and proven compatible: `agent-client-protocol = "2.0"`, proven both against scripted fixtures and live against the real adapter (§11). No local wire layer is needed.
- [x] Adapter stderr cannot corrupt stdout protocol parsing (`stderr_noise_does_not_appear_on_or_block_stdout`, plus live observation).
- [x] Request correlation works while notifications and reverse requests are interleaved (`permission_request_round_trip_with_harmless_action` interleaves a reverse request with a live notification stream and correlates the response by request ID).
- [x] The child process is reaped on normal close, error, and client shutdown (`session_close_then_process_shutdown_is_clean`, `early_process_exit_is_observable_and_does_not_hang_pending_requests`, plus live SIGKILL observation in §9).
- [x] No UI implementation begins until the headless prompt round trip works — none has: this spike is a standalone crate outside the workspace, and no `daftprompt-acp` or UI code has been added by Task 0.
