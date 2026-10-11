# Epic 017: Split, Review, and Store Agent Responses

## Introduction

Epic 016 sends prompts to external agents and logs each run as files. This
epic turns those runs into typed records:

1. Plain code splits each response into sections and items.
2. An internal API-based agent built with Rig reviews the split.
3. Everything is stored in a per-project SQLite database, linked to the prompt
   that produced it.

Splitting into small, typed, linked pieces is the foundation for the later
fork, merge, and fan-out work described in `PRD.md`.

**Status:** Not Started

## Scope (and only this)

7. Plain code parses the captured stdout into Overview, Thoughts, Open
   Questions, and Tasks.
8. An internal Rig agent reviews the split that the plain code produced.
9. Prompts, runs, the parsed parts, and reviews are stored in
   `.projects/<project-name>/state.sqlite`. The file sits in this repository
   but outside Git, next to the Epic 016 run folders.
10. Every part is a Rust type: an overview has a summary, a `created_at`, and
    so on. Every part links to the source prompt, which is also stored.

## Non-goals

- Relating parts to the code, document, or git-log indexes.
- Fork, merge, fan-out, or an agent loop.
- Changing the Epic 016 file logs. They stay as the raw record.
- The GUI.

## Lessons from Epic 016 live runs

- `claude --model haiku` ignored the format instruction and replied in free
  Markdown with no section headings (run `20261011T085143.187Z-claude`). The
  parser must accept that, and the review must flag it rather than fail.

## Parsing contract (plain code, no LLM)

- Split on level-2 headings (`## `). Match the four known names
  case-insensitively, ignoring surrounding whitespace and trailing colons.
- Text before the first known heading, and any section with an unknown
  heading, is kept as an `Unsectioned` part with its heading text. Nothing
  from stdout is dropped.
- Open Questions and Tasks are split further, into one item per top-level
  bullet or numbered entry. Nested lines stay with their parent item.
- Overview and Thoughts are stored as whole sections. Thoughts may be split
  later if live examples show they should be.
- Each part records its byte range in `stdout.md`, so it can be traced back to
  the raw output.
- A response with no known headings is valid. It produces a single
  `Unsectioned` part.

## Rust types

These are sketches; the field names may change during implementation.

```rust
struct Prompt   { id, text, system_prompt, created_at }
struct Run      { id, prompt_id, agent, model, repo, log_dir, exit_code,
                  head_before, head_after, git_status_before, git_status_after,
                  started_at, finished_at }
struct Overview     { id, run_id, prompt_id, summary, span, created_at }
struct Thoughts     { id, run_id, prompt_id, text, span, created_at }
struct OpenQuestion { id, run_id, prompt_id, ordinal, text, span, created_at }
struct Task         { id, run_id, prompt_id, ordinal, text, span, created_at }
struct Unsectioned  { id, run_id, prompt_id, heading: Option<String>, text, span, created_at }
struct SplitReview  { id, run_id, reviewer_model, verdict, issues, created_at }
```

## SQLite

- One database per project, at `.projects/<project-name>/state.sqlite`.
- It is separate from the indexer cache database, and it does not touch
  `items` or `items_fts`.
- The schema is versioned, and creating it is safe to rerun.
- One transaction per run, so a failure while parsing or reviewing one run
  does not roll back earlier runs.

## Review contract (Rig agent)

- **Input:** the raw stdout and the parsed parts as JSON.
- **Output:** a typed verdict, `ok` or `issues`. Each issue says what is
  wrong, for example:
  - content that belongs in a different section
  - one bullet that holds two tasks
  - two bullets that are one task
  - a missing section
  - the format was ignored
- The review is advisory. It is stored next to the split and never rewrites
  it.
- If the review fails (no API key, network error, invalid JSON), the run and
  its split are still stored. The failure is recorded as the review's status.

## Open questions

- Which provider and model should the Rig reviewer use? Should the Xiaomi key
  that `graph` already uses be reused?
- Should the existing Epic 016 run folders be imported into the database with
  a one-off command?

## Tasks

### Task 1: Response parser

**Status:** Not Started

- [ ] A pure function turns stdout into typed parts, following the parsing
      contract.
- [ ] Unit tests cover:
  - all four sections
  - optional sections that are missing
  - a reply with no headings (like the haiku run)
  - unknown headings
  - nested bullets
  - numbered tasks

### Task 2: Per-project state database

**Status:** Not Started

- [ ] Create `.projects/<project-name>/state.sqlite` with the versioned schema
      for prompts, runs, parts, and reviews.
- [ ] Every part links to its `run_id` and `prompt_id`.

### Task 3: Store each chat run

**Status:** Not Started

- [ ] After the run log is written, `chat` saves the prompt, the run, and the
      parsed parts in one transaction.
- [ ] A database error is reported, but the file log stays complete.

### Task 4: Rig split reviewer

**Status:** Not Started

- [ ] A Rig agent reviews each split and returns a typed verdict, following
      the review contract.
- [ ] The verdict is stored.
- [ ] A failed review never loses the run.

### Task 5: Try live examples

**Status:** Not Started

- [ ] Run real prompts through each agent.
- [ ] Check the stored parts and reviews against the raw stdout.
- [ ] Record the lessons in this epic.
