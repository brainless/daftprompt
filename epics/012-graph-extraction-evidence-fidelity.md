# Epic 012: Graph Extraction Evidence-Fidelity Hardening

## Introduction

Epic 011 shipped the document provenance graph CLI (`graph build`/`graph
search`) with fixture-driven tests and marked itself Complete. Fixture tests
validate the *validation logic* (evidence checking, re-anchoring, strict
fail-after-one-correction) but cannot exercise real model behavior. The first
live attempt at the epic's own documented workflow —

```bash
cargo run --bin graph -- build --repo . --source 'epics/*.md'
```

— failed immediately and kept failing in new ways as each failure was fixed.
This epic documents that debugging process as a reference for continuing it,
and breaks the remaining, still-unresolved failure modes into tasks.

**Status:** In progress — not complete. A prior agent session spent several
hours on this before the user asked it to stop and hand off via
`GRAPH_CLI_HANDOFF.md` (superseded by this epic; the handoff file predates
several of the fixes described below and should not be treated as current
state). A second session (this one) continued the same delegate-a-fix,
retest-live loop and made substantial further progress, documented below,
before the user asked to stop again and capture the remaining work here.

## Goals

- Get the exact documented command (`graph build --repo . --source
  'epics/*.md'`, and eventually including `DEVELOP.md`, `README.md`,
  `AGENTS.md` per the AGENTS.md-documented workflow) to complete successfully
  against this repository's real content, using the real Xiaomi MiMo V2.5 Pro
  model — not a fixture.
- Do this without relaxing Epic 011's strict validation rules: evidence must
  match the snapshot exactly (whitespace-normalized), wrong line numbers may
  only be corrected by unambiguous re-anchoring to a real contiguous span, and
  a claim that is still invalid after one model correction must fail the
  build rather than being discarded or silently omitted.
- Once a full strict build succeeds, run `graph search` against it and verify
  citations resolve correctly (Epic 011's acceptance criteria describe this,
  but it has never actually been exercised against a genuinely complete
  strict wildcard build — the one 95-claim build that completed early in this
  effort did so under a since-removed partial-claim salvage path that
  violated the strict rule, and does not count).

## Non-goals

- Relaxing `evidence_text_matches_snapshot` or `validate_entity`'s allowed
  vocabulary to make failures go away. Every fix so far has made the model
  more likely to produce *correct* output, not made incorrect output pass.
- General-purpose fuzzy/approximate evidence matching. Where a safer,
  narrowly-scoped re-anchoring extension is proposed below (Task 1), it must
  remain unambiguous — extending an already-correctly-identified line, never
  searching for a best-effort match.
- Re-litigating the `llm-sdk` dependency. That was a separate, now-resolved
  issue (see "Related work already completed" below) — this epic is only
  about the extraction/validation loop itself.
- A graph visualization, GUI, or any change to the SQLite schema.

## Related work already completed (not part of this epic's remaining tasks)

During this session, a prior mistake was found and corrected: an earlier
agent had vendored a full private copy of the `llm-sdk` crate into
`vendor/llm-sdk/` just to add a `thinking: {"type":"disabled"}` request field
for Xiaomi, instead of fixing it in the actual upstream repository
(`github.com/brainless/llm-sdk`, which the user owns). This was corrected:
the `disable_thinking()` builder method was added across every provider in
`llm-sdk` that supports disabling extended thinking (Xiaomi, MixLayer,
Ollama, Groq, GLM/zAI, Cerebras, OpenRouter — Claude, Grok, Gemini, and
OpenAI were skipped with documented reasons), released as `llm-sdk` commit
`9d44871870ef177d12d704ad7c6841709e4f0d0a` (crate version 0.1.13), and
`daftprompt`'s dependency was repointed to a real `git` dependency at that
commit. `vendor/llm-sdk/` was deleted. No call-site changes were needed since
the wire-level `XiaomiThinking`/`XiaomiChatCompletionRequest` types were
unchanged. This is mentioned here only so a future agent doesn't rediscover
or redo it.

## Process reference: how each fix in this session was found and verified

This is the loop that produced every fix below. It worked well and should be
repeated for the remaining tasks rather than switching approach:

1. **Reproduce live.** Run the focused or full build directly:
   ```bash
   env RUSTC_WRAPPER= DAFTPROMPT_GRAPH_BATCH_CONCURRENCY=4 \
     cargo run --bin graph -- build --repo . --source <one failing file>
   ```
   Add `DAFTPROMPT_GRAPH_DEBUG_EVIDENCE=1` for evidence-mismatch byte detail.
   Live model calls and `.env`/`XIAOMI_API_KEY` work fine directly in this
   sandbox with plain `cargo run` — no special flags needed, contrary to an
   earlier assumption recorded in `GRAPH_CLI_HANDOFF.md`.
2. **Read the actual source lines** the error cites with `awk 'NR>=X &&
   NR<=Y{print NR": ["$0"]"}' <file>` to see exact byte content — do not trust
   a description of the mismatch, read the real snapshot.
3. **Diagnose the root cause** by reading the relevant code
   (`crates/daftprompt-indexer/src/graph.rs` for validation/re-anchoring,
   `src/graph/model.rs` for the extraction/correction prompts) before
   proposing a fix. Several apparent duplicates of "the same bug" turned out
   to be distinct root causes (see the list below) — do not assume a new
   failure is the old one recurring without checking source lines and code.
4. **Delegate the fix to a subagent** (this repo's owner explicitly wants
   each fix delegated, with the parent agent waiting and retesting rather
   than fixing inline). Give the subagent: `AGENTS.md`, `DEVELOP.md`, this
   epic file, exact error text, exact source lines (already looked up),
   precise `file:line` pointers into `graph.rs`/`model.rs`, and explicit
   constraints (don't weaken strict validation; add tests; don't run the
   full wildcard build itself, only the one failing source, since the parent
   agent retests the full build afterward).
5. **Verify independently — do not trust a subagent's self-reported
   `cargo check`/`cargo test` pass.** A subagent's `cargo check` can report
   success in well under a second after an edit because Cargo skipped
   recompilation of a file it considered unchanged mid-edit; this happened at
   least once in this session and the IDE/LSP diagnostics caught real
   compile errors that a stale `cargo check` had missed. Always `touch` the
   changed file(s) and rerun `cargo check --workspace` yourself before
   trusting a green result.
6. **Retest the one previously-failing source**, then **re-run the full
   `'epics/*.md'` wildcard build** in the background (it can run past a
   single tool-call timeout; use `run_in_background` + a `Monitor`/polling
   loop watching for a line matching `^completed graph build|^Error:`).
7. **Report the new failure point precisely** (which source, which batch,
   exact error, exact source lines) and decide whether to delegate the next
   fix or stop.

## Failure modes found and fixed, in order

Each was a genuinely distinct root cause, not a recurrence of the same bug —
worth reading before assuming a new failure matches an old one:

1. **Line-counting hallucination.** The model was given raw passage text with
   only the passage's overall `start_line`/`end_line` and had to count lines
   itself to cite a claim's exact range; it sometimes cited a real quote for
   a completely unrelated line number. Fixed by rendering each passage line
   with an explicit absolute line-number prefix (`"90: <line>"`) in the
   outbound request only (`render_numbered_passage_text`,
   `NumberedExtractionRequest` in `model.rs`), leaving the stored
   `SourcePassage.text` and all validation untouched.
2. **False-ambiguity bug in re-anchoring.** `unique_evidence_span_in_passage`
   (`graph.rs`) compares whitespace-normalized word sequences, so a blank
   Markdown paragraph-separator line contributed zero words and let the
   search "find" the same wording at multiple `(start, end)` pairs that
   differed only by an adjacent blank line — the function then wrongly
   treated this as real ambiguity and refused to re-anchor a genuinely
   unique, exact multi-line quote. Fixed by requiring a candidate span to
   start on non-blank content and stop at the minimal matching end.
3. **Undisclosed entity-kind vocabulary.** The extraction contract disclaimed
   `project`/`epic` as invalid entity kinds but never stated the complete
   valid set, so the model guessed `"task"` as an entity kind (it's only
   valid as a claim *scope*). Fixed by stating the exact allowed list in the
   prompt, cross-referenced with a test against the actual `ENTITY_KINDS`
   constant so the two can't silently drift apart.
4. **Correction regressing an unrelated field.** The one-shot correction
   request was a fresh 2-message exchange with no assistant turn carrying the
   model's actual prior (rejected) response, so fixing one reported problem
   could silently reintroduce a different one on the same claim. Fixed by
   threading the model's previous response back in as a prior assistant turn
   in the correction request, with an explicit "change only what's needed,
   copy everything else byte-for-byte" instruction.
5. **Feature-vs-index subject-kind confusion.** Epic prose that describes an
   indexing pipeline and a search feature in the same sentence (e.g. "a code
   indexer... exposed via a separate search API") led the model to use
   `subject.kind: "feature"` for `indexes_language`/`indexes_extension`
   claims, which require `subject.kind: "index"`. Fixed with a concrete
   worked example in the prompt distinguishing the two.
6. **Partial-quote truncation at a sentence boundary within one physical
   line — found twice, in two shapes.** Markdown hard-wraps a paragraph so
   one physical source line sometimes contains the end of one sentence and
   the start of an unrelated one. The model correctly identifies the right
   line(s) but truncates its quote right before the irrelevant trailing
   sentence, producing a partial-line quote — forbidden by the strict rule.
   - First seen at the **tail of a multi-line span** (lines 18-20 of
     `epics/009-typescript-tsx-code-indexer.md`, quote correctly extended
     across 18-19 but truncated mid-line-20). Fixed by explicitly extending
     the "whole-line" prompt rule (previously only stated for a one-line
     range) to every line in a multi-line range, including the last one.
   - **Recurred afterward in its simplest form**, a plain single-line
     truncation (`epics/009-typescript-tsx-code-indexer.md` line 15: model
     quoted `"Epic 004 established the Rust pipeline."` and dropped `" Epic
     008 established the evidence,"` from the same line) — **and the model
     reproduced the identical wrong text on its one correction attempt, even
     though the correction message already echoed the exact expected
     snapshot text to it.** This is the current, unresolved blocker (see
     Task 1). It indicates prompt wording alone is not a reliable enough fix
     for this specific failure shape, unlike failures 1-5 above.

## Current state

As of the last full wildcard run, the strict build clears **sources 1-8 of
11 cleanly (62 valid claims)** and reaches source 9
(`epics/009-typescript-tsx-code-indexer.md` — the file that was the
*original* blocker documented in `GRAPH_CLI_HANDOFF.md`) before failing on a
new claim with failure mode 6's single-line variant. Sources 10-11
(`epics/010-javascript-jsx-code-indexer.md`,
`epics/011-document-provenance-graph-cli.md`) and the root `DEVELOP.md`/
`README.md`/`AGENTS.md` sources have not yet been reached by a passing build,
so they may surface further, still-undiscovered failure modes once source 9
is resolved. `cargo check --workspace` and `cargo test --workspace --quiet`
pass (210 tests) as of this epic's writing.

## Tasks

### Task 1: Safely auto-extend a verified single-line prefix quote

**Status:** Complete

**Depends on:** none (extends existing code in `graph.rs`).

**Change:** When a claim's declared `start_line == end_line` and the
model's proposed evidence text is an exact, whitespace-normalized **prefix**
of that exact declared line's snapshot content, auto-extend the stored
evidence text to the complete physical line, the same way re-anchoring
already corrects a wrong line number. This is narrower and safer than
general fuzzy matching: the line is already unambiguously and correctly
identified by the model (both `start_line` and `end_line` are already
right, and the mismatch is confirmed to be a strict prefix relationship, not
a guess) — nothing is searched for. Consider whether the identical, narrower
case also applies to the *last* line of an already-correct multi-line
declared range (i.e., `start_line`/`end_line` are right, all lines up to the
second-to-last match exactly, and only the last line is a prefix) — if so,
extend the fix to cover both single-line and multi-line-tail cases with one
mechanism, since failure mode 6 above has now appeared in both shapes. Do
NOT extend this to search across multiple lines or multiple candidate
locations — a prior fix in this codebase deliberately declined a more general
prefix-extension inside `unique_evidence_span_in_passage` as too risky (see
failure mode 2's fix and its test
`reanchors_multiline_claim_across_a_trailing_blank_paragraph_line` for the
established caution level); this task is intentionally narrower than that
declined approach because it never searches, it only extends a
range the model already got right.

**Acceptance criteria:**

- [x] A claim whose evidence text is an exact prefix of its declared line's
      snapshot content is accepted and its stored `evidence.text` is
      corrected to the full line, without a model correction round.
- [x] A claim whose evidence text is NOT a genuine prefix (differs in
      wording, not just a truncation point) still fails validation exactly
      as before — this must not become a backdoor for accepting wrong text.
- [x] A new unit test reproduces the exact real-world case (source line
      containing two sentences, model quoting only the first) and asserts
      correct auto-extension.
- [x] `cargo check --workspace` and `cargo test --workspace` pass (verified
      with a forced rebuild, not a possibly-stale cached check).
- [x] Live spot-check: `graph build --repo . --source
      epics/009-typescript-tsx-code-indexer.md` completes without the
      previously-failing claim needing a correction round.

### Task 2: Resume the full wildcard build to completion

**Status:** Not Started

**Depends on:** Task 1 (or whatever else blocks source 9 by the time this is
picked up).

**Change:** Continue the process-reference loop above: run the full
`graph build --repo . --source 'epics/*.md'` wildcard build, diagnose and fix
whatever new failure surfaces next (sources 10-11 have not yet been reached
live and may contain undiscovered failure modes), delegate each fix to a
subagent per the established pattern, and repeat until the build completes
successfully end-to-end.

**Acceptance criteria:**

- [ ] `cargo run --bin graph -- build --repo . --source 'epics/*.md'`
      completes successfully with no evidence, structural, or vocabulary
      errors, and without any weakening of strict validation.
- [ ] The completed build's claim count and per-source breakdown are recorded
      in this file or a follow-up note for future reference.
- [ ] `cargo check --workspace` and `cargo test --workspace` pass.

### Task 3: Extend the wildcard build to the full documented source set

**Status:** Not Started

**Depends on:** Task 2.

**Change:** AGENTS.md and README.md document the full workflow as including
`DEVELOP.md`, `README.md`, and `AGENTS.md` alongside `epics/*.md`. Run:
```bash
cargo run --bin graph -- build --repo . \
  --source DEVELOP.md --source README.md --source AGENTS.md \
  --source 'epics/*.md'
```
and resolve whatever new failures these three additional, differently
structured documents surface (they are not epic files and may have different
Markdown shapes — tables, code fences, nested lists — not yet exercised by
the epics-only build).

**Acceptance criteria:**

- [ ] The full documented build command completes successfully.
- [ ] `cargo check --workspace` and `cargo test --workspace` pass.

### Task 4: Verify `graph search` against a genuinely complete strict build

**Status:** Not Started

**Depends on:** Task 3.

**Change:** Epic 011 documents `graph search` acceptance criteria, but no
search has ever been run against a build that completed under the *current*
strict rules with the *full* documented source set (the one prior 95-claim
build used a since-removed partial-claim salvage path and doesn't count).
Run the documented example searches (see `AGENTS.md`'s "Document provenance
graph CLI" section) against the Task 3 build and inspect answers, citations,
and claim IDs for correctness.

**Acceptance criteria:**

- [ ] `graph search --repo . "Which languages can code search find?"` and the
      other AGENTS.md-documented example searches return plausible answers
      with citations that resolve to real, correct snapshot text.
- [ ] At least one `--json` search is inspected for `outcome`, `answer`,
      `citations`, `claim_ids`, and `build_id` correctness.
- [ ] Any search-side bug found is diagnosed and fixed using the same
      process-reference loop (delegate, verify independently, retest).

### Task 5: Full workspace verification outside the sandbox

**Status:** Not Started

**Depends on:** Task 4.

**Change:** Per `GRAPH_CLI_HANDOFF.md`'s original note (still applicable):
the `llm-sdk` mock-server unit tests abort inside this sandbox because local
sockets/system configuration are blocked. Run `cargo test --workspace`
outside the sandbox (or with appropriate execution approval) at least once
before considering this epic complete, plus `git diff --check` for
whitespace/line-ending issues across all files touched during this epic.

**Acceptance criteria:**

- [ ] `cargo test --workspace` passes outside the sandbox, including any
      tests that abort inside it.
- [ ] `git diff --check` is clean.
- [ ] This epic's `**Status:**` is updated to Complete, and each task above
      is marked complete.

## File-change summary

| Path | Intended change |
|---|---|
| `crates/daftprompt-indexer/src/graph.rs` | Evidence-fidelity fixes (Task 1); whatever Tasks 2-4 surface. |
| `crates/daftprompt-indexer/src/graph/model.rs` | Extraction/correction prompt refinements as needed. |
| `epics/012-graph-extraction-evidence-fidelity.md` | This file — keep task statuses current as work proceeds. |
| `GRAPH_CLI_HANDOFF.md` | Superseded by this epic; safe to delete once this epic is picked up, or leave as historical record. |
