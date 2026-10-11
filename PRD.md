# daftprompt — Product Requirements (high level)

## Problem

Agent-driven development is mostly about managing text and chats. I use Claude
Code, Codex, opencode, and others, and I no longer read or write code directly.
What the work actually needs is chat manipulation:

1. **Split.** The agent raises several questions at once, so I split them, dig
   into each one in its own chat, merge the results, and resume.
2. **Fork and join.** I have a question about the current chat, so I fork it,
   dig in, join the result back to the parent chat, and resume.
3. **Fan out.** There are several clear items (tasks in an epic or in a chat),
   so I fan out one agent per item (implementation, review) and merge the
   results.

Today I do this by hand. I copy parts of a TUI chat into a new agent instance,
work there (dig into an old-data issue, fix code, write a plan, run
migrations), and then paste a summary back into the original chat ("I dug into
this separately; here is what I did…").

Digging into a long chat inside the same session is expensive. Every detour
fills the context window and makes the main thread worse.

## Product idea

daftprompt sits on top of the agent CLIs I already use. It turns their output
into small, typed, linked pieces, and it manages forking, merging, and fan-out
of agent work for me. The workflow is **map-reduce all the way down**:

- **Map:** split a response or document into items, and give each item a
  focused agent run (or a fork) with a shared preamble.
- **Reduce:** summarize the results and join them back into the parent
  context.

A live example: asking an agent to "use a subagent to read and summarize the
epics" should be the default way daftprompt works, not a special request.

## Principles

- **Wrap the agents, don't replace them.** Call Claude Code, Codex, or opencode
  headless, with the model I choose and no tool restrictions.
- **Log everything.** Keep every prompt, response, and git change, so nothing
  depends on scrolling back through a terminal.
- **Plain code first.** Parse, split, and link with deterministic code. Use an
  LLM to review, judge, or summarize, not to do work that code can do.
- **Typed records.** Every piece of a response becomes a Rust type stored in
  SQLite and linked to the prompt that produced it.
- **Split as much as possible.** Small pieces are what make forking, merging,
  and fan-out cheap.
- **CLI first; earn every feature.** Try each step on live examples before
  building the next one. Don't chase features.

## Core concepts

| Concept | Meaning |
|---|---|
| Prompt | Text I (or daftprompt) sent to an agent, with its system preamble |
| Run | One headless agent invocation: agent, model, repo, output, git state before and after |
| Part | A typed piece of a response: Overview, Thoughts, Open Question, Task, … |
| Fork | A new run seeded from one part, for example "dig into this open question" |
| Join | A summary of a fork's result, fed back into its parent context |
| Fan-out | One run per item in a list (tasks in an epic or response), with a shared preamble, run sequentially |

## Roadmap

| Stage | What | Where |
|---|---|---|
| 1 | CLI that prompts an external agent with a section format and logs each run (stdout, git status) | Epic 016 (done) |
| 2 | Split responses with plain code, review the split with an internal Rig agent, store typed parts in `.projects/<project>/state.sqlite` | Epic 017 |
| Later | Fork a part into a new run, and join its summary back to the parent | — |
| Later | Fan out tasks from a response or an epic to sequential agent runs | — |
| Later | Relate parts to the codebase, documents, and git history | — |
| Later | Run agents in a loop, internal (Rig) or CLI, that take parts, act on them, and feed results back | — |

The "Later" rows are direction, not commitments. Each one becomes an epic only
after the previous stage has been used on real work.

## Existing foundations

Earlier epics (001–011) built a per-repository SQLite indexer with hybrid
search over git history, code (Rust, TypeScript/TSX, JavaScript/JSX), and
documents (FTS5 + sqlite-vec). They also built heading-aware Markdown chunking,
an LLM adapter with typed validation, and an infinite-canvas GUI on akar.
Relating parts to code and documents will likely reuse the indexer. The GUI is
not on the near-term path.

## Non-goals (for now)

- Building our own coding agent, or restricting the tools the wrapped agents
  can use.
- A GUI or TUI. Plain CLI prompting comes first.
- Parallel fan-out. Sequential runs come first.
