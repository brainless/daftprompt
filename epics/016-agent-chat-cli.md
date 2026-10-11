# Epic 016: Agent Chat CLI

## Introduction

Daily agent work is mostly chat manipulation: forking a chat to dig into one
part, merging the result back, and fanning out tasks. This epic only builds the
first step toward that workflow. It adds a CLI that sends my prompts to an
external coding agent (Claude Code, Codex, or opencode). It asks the agent to
reply in a simple Markdown format and records everything the run produced.

The goal is to try the flow on live examples before building anything else.

**Status:** Task 1 done; Task 2 (live examples) in progress

## Scope (and only this)

1. A CLI, not the GUI.
2. Prompting works the way it does in other agent apps: a single prompt
   argument, or an interactive prompt loop.
3. All inputs and outputs are logged.
4. `--agent claude|codex|opencode` and `--model <model>` choose the external
   agent CLI and its model.
5. The agent gets a custom system prompt where its CLI allows one. The prompt
   asks for Markdown with optional sections: Overview, Thoughts, Open
   Questions, Tasks. Tools are not restricted.
6. Each run records stdout, stderr, and `git status` before and after the run,
   in case the agent changed files.

## Non-goals (deferred until live examples justify them)

- Parsing responses into sections, review by an internal Rig agent, and SQLite
  storage (steps 7–10, now Epic 017).
- Session resume, fork, merge, and fan-out.
- Relating output to the code, document, or git-log indexes.

## CLI contract

```bash
cargo run --bin chat -- --agent claude --model opus "Explain the indexer"
cargo run --bin chat -- --agent codex --model gpt-5.5 --repo ~/Projects/other
cargo run --bin chat -- --agent opencode --model anthropic/claude-sonnet-5-5
```

- `--repo` is the directory the agent runs in. It defaults to the current
  directory.
- With a prompt argument, the CLI makes one run and exits. Without one, it
  starts a loop: type or paste a prompt, then press Enter on an empty line to
  send it. Ctrl-D exits.
- Every prompt is an independent agent run. Nothing carries over from one run
  to the next.
- Agent stdout and stderr stream to the terminal live and are captured.

| Agent | Command | System prompt | All tools |
|---|---|---|---|
| claude | `claude -p` | `--append-system-prompt` | `--dangerously-skip-permissions` |
| codex | `codex exec` | `-c developer_instructions=…` | `--dangerously-bypass-approvals-and-sandbox` |
| opencode | `opencode run` | no flag; the preamble is prepended to the message | `--auto` |

## Run log

Logs live in this repository, outside Git, at
`.projects/<project-name>/runs/<UTC-timestamp>-<agent>/`. `<project-name>` is
the base name of `--repo`.

| File | Contents |
|---|---|
| `prompt.md` | the user prompt |
| `system_prompt.md` | the format preamble that was sent |
| `stdout.md` | the agent's stdout |
| `stderr.txt` | the agent's stderr |
| `git_status_before.txt`, `git_status_after.txt` | `git status --porcelain=v1` |
| `run.json` | agent, model, repo, argv, start and end times (UTC), exit code, `HEAD` before and after |

## Tasks

### Task 1: Agent chat CLI with run logging

**Status:** Done

**Acceptance criteria**

- [x] `src/bin/chat.rs` implements the CLI contract for all three agents.
- [x] The `/.projects/` directory is Git-ignored.
- [x] Every run writes the full run log, even when the agent exits non-zero.
- [x] `cargo check --workspace` passes.

### Task 2: Try live examples

**Status:** Not Started

Use the CLI for real work with each agent. Note where the response format,
the logging, or the flow falls short, and decide what to build next from that.
