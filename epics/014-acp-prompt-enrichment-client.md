# Epic 014: ACP Prompt Enrichment Client

## Introduction

daftprompt already searches repository code, documents, and git history through
one hybrid retrieval API. This epic turns that capability into an observable
prompt-enrichment client for coding agents. The user types a request in
daftprompt; daftprompt preserves the original request, retrieves relevant
repository context with `search_all_hybrid`, builds a deterministic enriched
prompt, and sends that prompt to an existing coding-agent adapter over the
Agent Client Protocol (ACP). The adapter continues to own Codex execution,
authentication, model behavior, tool calls, and native configuration.

The first supported adapter is the TypeScript
`@agentclientprotocol/codex-acp` server from
`https://github.com/agentclientprotocol/codex-acp`. The locally cloned source at
`~/Projects/codex-acp` is the implementation reference for this epic. At the
time of writing, the clone identifies itself as version 1.4.0, uses
`@agentclientprotocol/sdk`, communicates over newline-delimited JSON on stdio,
starts Codex App Server internally, and advertises authentication, session,
prompt, embedded-context, permission, configuration, and other capabilities at
initialization. daftprompt is an ACP client of that process; it does not call
Codex App Server directly and does not copy adapter-specific translation logic.

This is deliberately a retrieval-first MVP. Every ordinary user prompt sent
through the new conversation surface is enriched, but no local builder LLM is
introduced yet. The deterministic formatter and complete interaction record
make retrieval choices visible: what the user asked, what search returned,
which excerpts were selected or omitted, the exact prompt sent to Codex, what
the adapter streamed back, and which permissions the user chose. That evidence
must exist before a local LLM is allowed to rewrite prompts, assess responses,
or initiate follow-up turns.

Epic 014 is compatible with the direction of Epics 012 and 013 but does not
implement their planner, graph, mandatory helper, or tool-free capable-model
architecture. Codex remains a normal tool-enabled coding agent behind its ACP
adapter. A later epic may place the Epic 013 helper policy above the transport
and evidence foundation built here.

## User Flow

```text
user types original request
          |
          v
daftprompt records original request
          |
          v
search_all_hybrid(original request, retrieval limit)
          |
          v
deterministic selection + bounded formatting
          |
          v
daftprompt records retrieval run and exact enriched prompt
          |
          v
ACP session/prompt over codex-acp stdio
          |
          v
session/update stream + permission requests + prompt response
          |
          v
daftprompt transcript and inspection UI
```

There is exactly one production submission path for ordinary conversation
prompts. UI code must not be able to call `session/prompt` without first
creating a recorded enrichment result. Protocol-level smoke tests may send a
raw prompt through an explicitly named test-only or diagnostic path.

## Dependencies

Requires:

- the existing `daftprompt-indexer` unified hybrid search API and its graceful
  FTS-only degradation;
- the existing per-repository identity and database behavior;
- a locally runnable `codex-acp` command, initially either the installed
  `codex-acp` binary or a user-configured executable and arguments;
- an authenticated Codex configuration supported by the adapter.

The implementation must read `~/Projects/codex-acp` when resolving adapter
behavior. In particular, the following files define the current integration
contract:

- `src/index.ts` for registered ACP methods and stdio process lifecycle;
- `src/StdUtils.ts` for newline-delimited JSON transport behavior;
- `src/CodexAcpServer.ts` for initialization, sessions, prompting,
  cancellation, and advertised capabilities;
- `src/ACPSessionConnection.ts` and `src/CodexEventHandler.ts` for
  `session/update` notifications;
- `src/CodexApprovalHandler.ts` for client-side permission requests and option
  responses.

Do not depend on files inside the local clone at build time. The clone is a
reference; the runtime boundary is the ACP protocol and a configured adapter
process.

## Goals

- Make daftprompt an ACP client that can launch and supervise one
  `codex-acp` process.
- Implement initialization, new sessions, prompts, streamed session updates,
  cancellation, permission responses, and clean session/process shutdown.
- Enrich every ordinary outgoing user prompt with bounded results from
  `search_all_hybrid`.
- Preserve the original prompt exactly and show the exact enriched prompt sent
  to the agent.
- Record retrieval candidates, selection decisions, prompt versions, ACP
  traffic, permission decisions, and turn outcomes.
- Keep retrieval, prompt construction, ACP transport, durable records,
  application coordination, and akar rendering behind separate boundaries.
- Keep the winit/render thread responsive while retrieval, adapter IO, and
  Codex turns run asynchronously.
- Degrade clearly when retrieval fails without preventing the user from using
  the coding agent.
- Provide enough inspection UI and structured logs for manual comparison of
  raw requests, context, enriched prompts, and agent behavior.
- Pin and report the ACP SDK/protocol assumptions and detected adapter version
  so compatibility failures are diagnosable.

## Non-goals

- Add a local prompt-builder LLM.
- Automatically inspect an agent response and generate another prompt.
- Automatically continue, branch, or replace an ACP session.
- Implement Epic 013's helper orchestration policy.
- Support Claude Code, OpenCode, or more than one adapter in the MVP.
- Reimplement Codex App Server or depend directly on its protocol.
- Implement every capability advertised by `codex-acp`.
- Implement session list, load, resume, delete, steering, goals, MCP server
  injection, images, resource links, elicitation, providers, or authentication
  UI in the first slice.
- Automatically approve commands, file changes, network access, or broader
  filesystem access.
- Place ACP or prompt-building concerns in `daftprompt-indexer`.
- Treat retrieved repository text as trusted instructions.
- Guarantee that retrieved context improves every coding task.

## Design Decisions

### 1. daftprompt is the ACP client, not an ACP proxy

daftprompt launches `codex-acp` as a child process and communicates with it over
stdin/stdout. `codex-acp` launches and translates Codex App Server internally.
The MVP has one daftprompt conversation runtime, one adapter process, and one or
more sequential ACP sessions owned by that runtime.

The boundary is intentionally adapter-neutral even though only Codex is
supported. Provider-specific executable discovery and environment configuration
belong in an adapter launch profile, not in retrieval or session state.

### 2. Use an ACP SDK when compatible, isolate it when not

Prefer a maintained Rust ACP SDK that implements the protocol version used by
the adapter. Pin the selected crate/version. All SDK types must remain behind a
small `daftprompt-acp` boundary so protocol upgrades do not spread through UI or
prompt-building code.

Before committing to a crate, complete a compatibility spike against the local
TypeScript adapter. If no Rust SDK interoperates with the adapter's negotiated
protocol and newline-delimited stdio transport, implement the minimum typed
JSON-RPC messages locally inside `daftprompt-acp`. Do not add loosely typed
`serde_json::Value` handling throughout the application.

The adapter's local `StdUtils.ts` shows that its daftprompt-facing transport
uses the SDK's standard `acp.ndJsonStream` through `createJsonStream`. Its
TODO-marked `createJSONRPCWriter`/`createJSONRPCReader` strip-and-reinject
`jsonrpc` behavior is used only for codex-acp's internal Codex App Server
connection and must not be reproduced by daftprompt. The client must tolerate
the adapter's current wire behavior while keeping applicable compatibility
workarounds documented and tested.

### 3. Capability negotiation controls behavior

Send `clientInfo` and conservative client capabilities during `initialize`.
Persist the full initialization request and response. Use the returned
`protocolVersion`, `agentInfo`, `agentCapabilities`, and `authMethods` rather
than assuming support from the adapter name.

The MVP requires new session and prompt support. Cancellation is an ACP
notification. `session/update` is sent by the agent and consumed by daftprompt.
Permission handling is a reverse request from the agent to the client. Session
close should be used when advertised; otherwise the runtime may close the
adapter process during application shutdown.

Unknown notification variants and unknown `_meta` data are preserved in the
event log and surfaced as generic transcript events. They must not crash the
session.

### 4. One mandatory deterministic enrichment boundary

The application exposes one method conceptually equivalent to:

```rust
async fn submit_user_prompt(
    session: SessionRef,
    original: String,
) -> Result<TurnRef>;
```

It performs retrieval, selection, formatting, recording, and only then calls
the ACP transport. The original prompt is immutable. The enriched prompt is a
derived, versioned artifact. A failed retrieval run records its error and
produces a deterministic no-context envelope rather than silently taking an
unrecorded bypass.

Slash commands and future structured ACP content need an explicit policy. For
the MVP, daftprompt supports ordinary text requests only; adapter slash
commands are rejected with a clear message or sent through a separately labeled
diagnostic control that does not pretend to be enriched.

### 5. Retrieved content is bounded and untrusted

The formatter labels code, documents, and commit text as automatically
retrieved, potentially incomplete, and untrusted as instructions. The original
request appears in a separate, clearly delimited section. Retrieved text must
not be allowed to escape its section through delimiter-like source content;
formatting must safely encode or length-prefix excerpts.

Selection is deterministic and configuration is recorded. At minimum it has:

- a total character or estimated-token budget;
- a per-excerpt limit;
- a per-source maximum or reserved quota;
- stable-identifier deduplication;
- deterministic ordering based on the unified combined ranking;
- explicit truncation markers;
- file paths and line ranges for code;
- identifiers and source labels for every excerpt.

Scores and match types are recorded for evaluation. They need not appear in
the prompt unless manual tests demonstrate that they help.

### 6. Conversation records are durable data, not rebuildable index state

Do not place durable conversations in tables that are dropped during
`--reindex`. Use a separate SQLite database or a clearly separate durable
database lifecycle. Search result snapshots must be stored because repository
files, commits, ranking behavior, and indexes can change after a turn.

The minimum record includes:

- application session ID and ACP session ID;
- adapter command, reported name/version, and negotiated protocol/capabilities;
- original and enriched prompt;
- formatter and retrieval-policy versions;
- retrieval query, latency, results, ranks, scores, match types, inclusion,
  truncation, and omission reasons;
- ordered inbound and outbound ACP messages with timestamps and correlation
  IDs;
- permission request, displayed options, chosen option, and outcome;
- streamed response events, final prompt response, cancellation, errors, and
  stop reason where available.

Secrets and inherited environment variables must never be written to the event
record. Large or sensitive protocol payloads require an explicit redaction and
size policy.

### 7. Permissions default to explicit user choice

When `codex-acp` sends `session/request_permission`, daftprompt displays the
tool-call description and the exact options supplied by the adapter. It returns
only an offered `optionId`, or the ACP cancelled outcome. The client must not
invent, rename semantically, or automatically choose an allow option.

Closing the permission dialog, cancelling the turn, losing the adapter, or
shutting down produces a conservative cancelled/rejected result. Persisted
"allow always" behavior is not implemented by daftprompt; if the adapter offers
an allow-always option and the user explicitly chooses it, the response and its
scope are recorded.

### 8. ACP IO never blocks rendering

Run process supervision, stdout parsing, request correlation, retrieval, and
prompt completion outside the winit event callback. Convert protocol activity
into typed application events delivered through a channel. `AppState` owns a
renderable projection, not child-process handles or pending JSON-RPC response
maps.

Each session has at most one active prompt in the MVP. A second submission is
disabled until the turn completes or is cancelled. Late updates after
cancellation are recorded and ignored or rendered according to their session
and turn identity; they cannot complete a newer turn accidentally.

### 9. Local LLM work begins only after retrieval is observable

The next prompt-builder phase must use the records and manual test corpus from
this epic. It may propose a follow-up, continue a session, or start a session,
but each generated turn must have a distinct origin and explicit user control.
No such behavior is included here.

## Tasks

### Task 0: Prove compatibility with the local TypeScript adapter — DONE

Findings: `epics/research/014-acp-adapter-compatibility.md`. Harness:
`scratch/acp-spike/` (throwaway, not a workspace member — see that doc's
intro for why). Conclusion: the maintained `agent-client-protocol` crate
(v2.0.0, same GitHub org as codex-acp's TypeScript SDK) is compatible and
proven both against scripted fixtures and live against the real adapter; no
local wire layer is needed. Task 1 should depend on it directly.

Build a small headless spike or test harness before integrating akar UI. Launch
the local adapter through its documented development command or built entry
point and exercise the actual stdio wire contract.

Record:

1. the local adapter git revision and package version;
2. launch command and required environment assumptions, excluding secrets;
3. the exact initialization request and response;
4. negotiated protocol version and capabilities;
5. new-session request and response;
6. one text prompt and representative `session/update` variants;
7. one permission request and response using a harmless test action;
8. cancellation before and after a Codex turn ID exists;
9. clean session close and child-process shutdown;
10. malformed output, adapter stderr, early exit, and request-timeout behavior.

Use scripted fixtures for repeatability where live Codex behavior is
unpredictable. Keep one opt-in live smoke test for manual validation.

#### Acceptance Criteria

- [x] The adapter is launched as a child process and communicates over stdio.
- [x] Initialize, session/new, session/prompt, session/update, permission,
  cancellation, and session/close behavior is observed against the local clone.
- [x] The selected Rust ACP dependency is pinned and proven compatible, or the
  need for a minimal local wire layer is documented.
- [x] Adapter stderr cannot corrupt stdout protocol parsing.
- [x] Request correlation works while notifications and reverse requests are
  interleaved.
- [x] The child process is reaped on normal close, error, and client shutdown.
- [x] No UI implementation begins until the headless prompt round trip works.

### Task 1: Add the ACP runtime boundary

Add a reusable `daftprompt-acp` workspace crate. Implement adapter launch
profiles, process supervision, request IDs, pending-response correlation,
typed notifications, reverse requests, timeouts, cancellation, session close,
stderr tail capture, and graceful shutdown.

The public API must expose typed events and errors rather than raw process
objects. Raw protocol payload preservation may be available through a bounded
diagnostic type.

#### Acceptance Criteria

- [ ] Only `daftprompt-acp` depends on the ACP SDK or local wire schemas.
- [ ] Executable path and arguments are configured without shell evaluation.
- [ ] Initialization records and validates negotiated capabilities.
- [ ] New session, prompt, cancel, permission response, and close are supported.
- [ ] Concurrent reverse requests and streaming notifications do not block
  prompt-response correlation.
- [ ] Unknown updates are preserved as typed generic events.
- [ ] Timeout, malformed message, broken pipe, early exit, and stderr-tail
  errors are distinguishable.
- [ ] Scripted adapter tests run without network access or Codex credentials.

### Task 2: Add deterministic context selection and prompt formatting

Add a provider-independent prompt-enrichment crate or module above
`daftprompt-indexer`. Define immutable original prompts, retrieval snapshots,
selection decisions, budgets, enriched prompts, and formatter versions.

Use `AllSourceSearchResult::combined` as the primary ranking while retaining
the source-specific result data needed for metadata and quotas. Do not change
the index schema or ranking algorithm merely to build the first formatter.

#### Acceptance Criteria

- [ ] The original prompt is preserved exactly.
- [ ] Every included excerpt has source type, stable identifier, and location
  metadata where available.
- [ ] Total, excerpt, and source budgets are deterministic and recorded.
- [ ] Duplicate, truncated, and excluded results have explicit reasons.
- [ ] Repository text is safely delimited and labeled as untrusted evidence.
- [ ] Empty results and retrieval errors produce versioned deterministic
  envelopes.
- [ ] Golden tests cover code, document, git, mixed, duplicate, oversized,
  delimiter-injection, FTS-only, and no-result cases.
- [ ] The module has no ACP, akar, winit, or provider dependency.

### Task 3: Add durable conversation and trace storage

Create a durable schema and repository API for ACP sessions, turns, retrieval
runs, retrieval hits, protocol events, and permission decisions. Apply explicit
schema migrations. Do not couple this database's lifecycle to index rebuilding.

#### Acceptance Criteria

- [ ] Original and enriched prompts round-trip without normalization.
- [ ] Exact included context snapshots can reconstruct what Codex received.
- [ ] Retrieval candidates retain rank, score, match type, selection state,
  truncation, and exclusion reason.
- [ ] ACP events have stable sequence ordering, direction, correlation ID,
  method/update kind, and timestamp.
- [ ] A turn moves through explicit preparing, running, completed, cancelled,
  and failed states without impossible transitions.
- [ ] Permission options and the exact chosen outcome are durable.
- [ ] Reindexing does not delete conversation records.
- [ ] Secrets and full process environments are absent from fixtures and stored
  records.
- [ ] Migration and round-trip tests pass in a temporary directory.

### Task 4: Build the application conversation coordinator

Connect the indexer, formatter, storage, and ACP runtime through one asynchronous
submission service. Establish session and turn identities before dispatch so
all later events can be attributed.

#### Acceptance Criteria

- [ ] Every ordinary UI submission uses the mandatory enrichment path.
- [ ] The enriched prompt is persisted before `session/prompt` is sent.
- [ ] Retrieval failure is visible, recorded, and falls back to the deterministic
  no-context envelope.
- [ ] ACP dispatch failure retains a retryable failed turn and its evidence.
- [ ] Only one active prompt per session is allowed.
- [ ] Cancel targets the active session and leaves late events unable to finish
  a later turn.
- [ ] Permission requests suspend only the affected interaction and are resolved
  through typed application commands.
- [ ] No retrieval or ACP IO runs on the render thread.
- [ ] Scripted end-to-end tests exercise success, no-context, permission,
  cancellation, adapter exit, and restart flows.

### Task 5: Add a minimal conversation UI

Add a focused ACP conversation surface to daftprompt. It may coexist with the
canvas; it does not need to model messages as canvas cards in the MVP.

The surface includes:

- adapter state and reported version;
- new-session control;
- scrollable transcript for user text, agent text, thoughts when supported,
  plans, tool calls, tool-call updates, and generic events;
- prompt editor with send and cancel;
- permission dialog showing the adapter-provided tool call and options;
- turn status and actionable error state;
- an enrichment inspector showing the original prompt, exact enriched prompt,
  included excerpts, excluded candidates, budgets, match types, and retrieval
  fallback/error status.

#### Acceptance Criteria

- [ ] The UI remains responsive during retrieval and a live Codex turn.
- [ ] Streaming chunks appear in protocol order without duplicating completed
  messages.
- [ ] Unknown updates have a non-fatal diagnostic rendering.
- [ ] Send is disabled while a prompt is active; cancel remains available.
- [ ] Permission choices exactly match adapter option IDs and no option is
  pre-approved.
- [ ] Original and enriched prompts are separately inspectable and copyable.
- [ ] Retrieval omissions and truncation are visible, not silently discarded.
- [ ] Adapter launch, authentication-required, protocol, timeout, and process
  exit failures are distinguishable to the user.

### Task 6: Add configuration and lifecycle handling

Add explicit configuration for the adapter executable, arguments, permitted
environment overrides, request timeout, retrieval budgets, and trace location.
Provide a sensible Codex profile without assuming `npx` network installation at
application startup.

#### Acceptance Criteria

- [ ] daftprompt can launch an installed `codex-acp` or an explicitly configured
  local development command.
- [ ] Arguments are passed directly to the process and never through a shell.
- [ ] Missing executables fail with an actionable message.
- [ ] Adapter version and initialization metadata are displayed and recorded.
- [ ] Existing authentication may be used without daftprompt reading or storing
  credential files.
- [ ] Authentication-required is explained; interactive authentication UI is
  explicitly deferred.
- [ ] Closing a session uses `session/close` when advertised.
- [ ] Application exit cancels active work, closes stdin, waits for the child,
  and applies a bounded forced-termination fallback.

### Task 7: Add replayable fixtures and manual evaluation workflow

Create scripted ACP fixtures derived from observed local-adapter traffic and a
manual test worksheet for real Codex sessions. The worksheet is the evidence
base for deciding whether retrieval improves prompts and when a local builder
LLM should be introduced.

For at least twelve prompts across at least three repositories, record:

1. original user request;
2. retrieval policy and latency;
3. all retrieved and selected context;
4. exact enriched prompt;
5. agent/model/config identity where reported;
6. response, tool activity, permission decisions, and outcome;
7. relevant context missed by retrieval;
8. irrelevant or duplicated context included;
9. whether enrichment helped, harmed, or made no material difference;
10. whether the agent independently rediscovered the supplied context;
11. prompt size and turn latency;
12. any prompt-injection or trust-boundary failure.

Include paired manual runs of the same request as raw text in an external Codex
client and as an enriched daftprompt turn. This external baseline is manual and
must not create a production bypass in daftprompt.

#### Acceptance Criteria

- [ ] Scripted fixtures cover all supported update and permission paths without
  live credentials.
- [ ] At least twelve live prompt records across three repositories are
  reviewed manually.
- [ ] Raw versus enriched paired runs use the same agent/model/config as closely
  as practical.
- [ ] Missed, irrelevant, duplicate, stale, and injection-prone context is
  reported rather than reduced to one quality score.
- [ ] Formatter or budget changes create a new version and can be compared with
  earlier records.
- [ ] The evidence identifies concrete entry criteria for a later local builder
  LLM experiment.

### Task 8: Document and verify the MVP

Update project documentation only after the implementation behavior is proven.
Document adapter installation separately from development against the local
clone, the authentication assumption, supported ACP subset, retrieval format,
trace location, privacy implications, manual test flow, and known limitations.

#### Acceptance Criteria

- [ ] `README.md` explains how a user starts an ACP conversation and inspects
  enrichment.
- [ ] `DEVELOP.md` explains the crate boundaries, local adapter reference,
  fixture tests, and live manual test command.
- [ ] `AGENTS.md` records the mandatory enrichment invariant and ACP boundaries.
- [ ] The epic task status and acceptance criteria are updated as work is
  completed.
- [ ] `cargo check --workspace` passes after every implementation change.
- [ ] `cargo test --workspace` passes.
- [ ] A manual session demonstrates initialize, new session, enriched prompt,
  streamed response, permission choice, cancellation, and clean close.

## Suggested Manual Test Sequence

1. Start daftprompt against a small indexed repository with an already
   authenticated Codex installation.
2. Verify the displayed adapter name, adapter version, protocol version, and
   capabilities.
3. Create a session rooted at that repository.
4. Submit a question that should retrieve one known code symbol and inspect the
   original prompt, selected hit, and exact enriched prompt before judging the
   answer.
5. Submit a mixed question requiring code and documentation and verify source
   quotas and ordering.
6. Submit a nonsense query and verify the recorded no-result envelope.
7. Temporarily force embedding failure and verify FTS-only degradation is
   visible.
8. Ask for a harmless command that triggers a permission request; reject it and
   verify the turn continues or ends according to the adapter response.
9. Repeat and explicitly allow once; verify the chosen option ID is recorded.
10. Start a longer request, cancel it before completion, and verify the session
    can accept another prompt.
11. Close the session and application and verify no adapter or Codex child
    process remains.
12. Inspect the durable record and confirm it reconstructs the prompt and event
    sequence without containing credentials.

## Test Matrix

| Area | Required evidence |
|---|---|
| Wire compatibility | Local TypeScript adapter initialize and round trip |
| Process lifecycle | launch, stderr, early exit, close, forced fallback |
| Correlation | response, notifications, and reverse requests interleaved |
| Retrieval | mixed sources, FTS-only, empty, error, duplicate, oversized |
| Formatting | golden prompts, budgets, safe delimiters, versioning |
| Trust | code, document, and commit prompt-injection fixtures |
| Persistence | migrations, round trips, event ordering, no reindex loss |
| Session state | new, active, complete, cancelled, failed, closed |
| Updates | messages, thoughts, plans, tools, unknown variants |
| Permissions | allow once, offered persistent option, reject, cancelled |
| Cancellation | before turn start, active turn, late update isolation |
| UI | responsive streaming, inspector, errors, permission modal |
| Evaluation | paired raw/enriched prompts across real repositories |

## File-change Summary

The exact UI file split may evolve, but the dependency direction is required.

| File | Change |
|---|---|
| `Cargo.toml` | Add ACP and prompt-enrichment workspace crates/dependencies. |
| `crates/daftprompt-acp/` | Typed ACP client, process supervision, stdio transport, sessions, updates, permissions, and fixtures. |
| `crates/daftprompt-prompt-builder/` | Deterministic retrieval selection, budgets, trust labeling, and versioned prompt formatting. |
| `src/` application modules | Conversation coordinator, durable trace repository, async event bridge, and configuration. |
| `src/state.rs` | Renderable ACP conversation and enrichment-inspection state. |
| `src/ui/render.rs` or focused UI modules | Conversation surface, transcript, enrichment inspector, and permission dialog. |
| `epics/research/014-acp-prompt-enrichment-evaluation.md` | Manual raw-versus-enriched test records and conclusions. |
| `README.md`, `DEVELOP.md`, `AGENTS.md` | User workflow, architecture, adapter development, invariants, and limitations. |

`daftprompt-indexer` must not depend on either new crate. The prompt builder may
depend on indexer result types or receive a provider-independent projection.
The application may depend on all three. `daftprompt-acp` must not depend on the
indexer, prompt builder, akar, or winit.

## Risks and Follow-ups

- The ACP protocol and TypeScript adapter are evolving; pinned compatibility
  fixtures and recorded implementation versions are essential.
- The adapter exposes substantially more behavior than this MVP renders.
  Unknown-event preservation prevents silent loss but does not provide polished
  UX for every Codex feature.
- Authentication-required may block first-time users until a dedicated auth UI
  is implemented.
- Retrieval can add stale, irrelevant, redundant, or adversarial text. Trust
  labels reduce risk but do not prove model compliance.
- Character budgets only approximate model tokens. Token-aware accounting may
  follow after the first measurements.
- SQLite's synchronous connection model and the current indexer ownership need
  careful thread boundaries; moving an existing connection across threads must
  not be assumed safe.
- Full raw ACP traces may contain repository content or command details. Trace
  retention, export, redaction, and deletion need explicit product controls.
- A forced adapter shutdown can leave external work in an uncertain state; the
  UI and record must report that uncertainty.
- Comparing raw and enriched runs is noisy because agent behavior is
  nondeterministic. Paired runs and complete configuration records reduce but
  do not eliminate that limitation.
- A later epic may add Claude Code or OpenCode through additional launch
  profiles after the provider-neutral boundary is proven.
- A later local builder LLM must distinguish user-authored,
  builder-suggested, and builder-automatic turns, and must require explicit
  policy for continuing versus starting a new ACP session.
