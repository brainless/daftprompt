# Epic 014: Evaluation Workflow

## Overview

This document explains how to run manual evaluation sessions for Epic 014's
prompt enrichment feature. The goal is to produce evidence for the 12 required
data points listed in `014-acp-prompt-enrichment-evaluation-worksheet.md`.

## Prerequisites

- A Codex installation authenticated against OpenAI (or another supported
  provider). daftprompt does not read or store credentials; it delegates to the
  adapter's own auth flow.
- At least three indexed repositories with varied content (code, docs, git
  history).
- The `daftprompt` binary built in debug or release mode.

## Running daftprompt Against a Repository

```bash
# Index the repository first (code + documents + git log)
cargo run -- --repo /path/to/repo --index

# Launch the GUI with the ACP conversation panel
cargo run -- --repo /path/to/repo
```

Press Tab to toggle the conversation panel. The adapter status badge in the
header shows whether the ACP connection is live.

## Submitting a Test Prompt

1. Type a prompt in the editor at the bottom of the conversation panel.
2. Press Enter or click Send.
3. Observe the enrichment inspector (click the inspector toggle in the header):
   - **Original prompt**: exact text you typed.
   - **Enriched prompt**: the full prompt sent to the agent, including retrieved
     context sections.
   - **Retrieval status**: `ok`, `empty`, or `error`.
   - **Candidates**: list of retrieved items with rank, source, score, and
     match type.
4. Record all fields from the worksheet (data points 1-6).
5. Watch the transcript for agent text, tool calls, thoughts, plans, and
   permission requests (data point 7).
6. After the turn completes, inspect the durable record (see below).

## Inspecting the Durable Record

The `ConversationStore` persists every session, turn, retrieval run, candidate,
enriched prompt, ACP event, and permission decision to a SQLite database.

```bash
# Default location on macOS
ls ~/Library/Caches/daftprompt/conversations/

# The DB filename is derived from the repo path slug.
# Open with any SQLite viewer:
sqlite3 ~/Library/Caches/daftprompt/conversations/<slug>.db

# Useful queries:
-- List all sessions
SELECT * FROM sessions;

-- List turns with their enriched prompts
SELECT t.id, t.original_prompt, t.enriched_prompt, t.formatter_version, t.state
FROM turns t ORDER BY t.id;

-- List retrieval candidates for a turn
SELECT rc.rank, rc.source, rc.identifier, rc.score, rc.match_type, rc.included
FROM retrieval_candidates rc
JOIN retrieval_runs rr ON rc.run_id = rr.id
WHERE rr.turn_id = <turn_id>
ORDER BY rc.rank;

-- List ACP events for a session
SELECT e.sequence, e.direction, e.event_kind, e.method, e.payload_json
FROM events e
WHERE e.session_id = <session_id>
ORDER BY e.sequence;

-- List permission decisions for a turn
SELECT p.tool_call_json, p.offered_options_json, p.chosen_option_id, p.outcome
FROM permissions p
WHERE p.turn_id = <turn_id>;
```

## Comparing Enriched vs Raw Runs

For paired runs (worksheet data points for prompts 1, 6, 11):

1. **Enriched run**: Use daftprompt's conversation panel. Record all 13 data
   points from the worksheet.
2. **Raw run**: Open a separate terminal and run Codex directly:
   ```bash
   codex --model gpt-5.6-sol "Your exact prompt text here"
   ```
   Or use any other Codex client that sends raw text without enrichment.
   Record the same data points where applicable (skip retrieval-specific fields).
3. **Compare**: Did the agent behave differently? Did it ask clarifying
   questions? Did it search for context the enrichment already provided? Did it
   produce a better or worse answer?

Important: The raw run is manual and must not create a production bypass in
daftprompt. It is an external baseline only.

## Using the Worksheet

Fill in `014-acp-prompt-enrichment-evaluation-worksheet.md` as you go. Each
prompt gets its own row. The summary table at the bottom aggregates across all
prompts. The entry criteria checklist determines whether a local builder LLM
experiment is warranted.

## Where the Evidence Lives

- **Worksheet**: `epics/research/014-acp-prompt-enrichment-evaluation-worksheet.md`
- **Durable record**: `~/Library/Caches/daftprompt/conversations/<slug>.db`
- **Epic spec**: `epics/014-acp-prompt-enrichment-client.md` (Task 7 acceptance
  criteria)
- **Fixture tests**: `crates/daftprompt-acp/tests/typed_client.rs`
- **Coordinator tests**: `tests/coordinator.rs`

## Formatter Versioning

The prompt formatter version is recorded with every enriched prompt
(`formatter_version` column in the `turns` table). If the formatter or budget
changes, increment `FORMATTER_VERSION` in `crates/daftprompt-prompt-builder/src/lib.rs`.
This allows comparing records across formatter versions without losing earlier
evidence.
