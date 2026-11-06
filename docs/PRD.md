# daftprompt: Product Requirements

## Problem

When people use LLM agents for research, brainstorming, or software work, a single prompt often comes back with a plan, open questions, and pushback. Today's chat and terminal interfaces force all of that into one linear conversation:

- You can't respond to one part of a reply without continuing the whole thread.
- Asking what a term or statement means interrupts the original chat, even when the explanation is only for your understanding.
- Answering questions one or two at a time fills the agent's context with small exchanges and repeated digging. Quality drops as the thread grows.
- Some answers come right away, some need a colleague, some need time. A linear chat has no place for that.
- Edits to a plan, answers to questions, and new findings can contradict each other, and nothing points that out.

## Vision

An orchestrator built on the **text** agents produce, not the chat. Every reply is broken into parts the user can act on separately. Each action (answering, editing, delegating, clarifying) sends a focused agent off with just the relevant text and access to project sources as needed. Task conclusions are woven back into a growing, traceable record; clarification exchanges remain in linked side branches. Conflicts and stale conclusions are surfaced, not silently merged.

## Users

Individuals who use LLM agents for thinking-heavy work: developers on existing projects, researchers, product people. Secondary: the colleagues they pull in to answer specific questions.

## Core concepts

- **Root prompt**: the request that starts a piece of work.
- **Nodes**: every piece of text (prompts, replies, parts of replies, file excerpts) is addressable and can be responded to on its own.
- **Labels**: parts of the record are marked as questions, answers, conflicts, superseded, or stale.
- **Assignee**: an optional handle identifying who is expected to respond to an open question, such as `@username` or `@legal-team`. A team assignment means anyone in that team can respond. Handles are recorded as text; daftprompt does not manage people or team membership.
- **Branches**: each response the user gives sends a fresh agent off with only the relevant text, not the whole history.
- **Clarification**: a branch labelled `clarification` for understanding a specific term or statement. The first version targets an extracted span. It links to that span and keeps its questions and explanations outside the original chat. It never blocks the main task and never changes the record's labels or statuses.
- **Done**: work is complete when no blocking questions, conflicts, or stale items remain.

## Key flows

1. **Start**: the user writes a root prompt. The agent researches and replies with a plan, questions, and pushback. These are separated into parts the user can act on.
2. **Partial answers**: the user answers some questions now. A branch agent takes the root prompt, those questions, and the answers, and returns updated conclusions.
3. **Delegate**: the user assigns a question to a person (`@username`) or a team (`@legal-team`). It remains open, or is marked as deferred if the answer must wait, possibly days later. Anyone in the assigned team can provide the answer.
4. **Edit the plan**: the user changes part of the plan. A branch agent analyses the change against the original request.
5. **Conflict**: a branch's conclusion contradicts something else in the record. The conflict is shown to the user or another agent to resolve. It is never silently overwritten.
6. **Staleness**: when something a conclusion relied on changes (an edited plan, a changed file), that conclusion is flagged as stale.
7. **Converge**: the user can always see what is still open. When nothing blocking remains, the work is ready to act on.
8. **Clarify**: the user picks an extracted span of an agent's reply, such as one containing an unfamiliar term, and asks about it. A separate agent opens a **Clarification** branch linked to that span, sees only the span, nearby source text, and the branch's earlier turns, and, when a project root is configured, reads files under that root read-only. It explains the span citing the sources it used, and says when it is working from the text alone. The user can continue asking questions in that branch without adding the exchange to the original chat or changing its conclusions. Arbitrary text selection and persistence of branches are later work.

## Goals

- Respond to any part of an agent's reply on its own.
- Get project-grounded explanations in clarification branches without interrupting the original chat.
- Keep each agent's context small and relevant.
- Allow asynchronous input (colleagues, delays) without losing track.
- Make conflicts and staleness visible.
- Full traceability: for any conclusion, show what it was based on and what the agent looked at.

## Parser expansion

[Epic 002](../epics/002-recommendations-tasks-and-identifiers.md) extends the extraction contract with standalone recommendations and offered options in plans or investigation replies, optional links to local questions, explicit alternative relationships, outstanding tasks with optional explicit assignees, and integer display identifiers. Recommendations alone leave questions open; open tasks will join the blocking items in the completion rule. The stateless contract preserves original text and generated graph identities; each extracted span also has a result-local integer display ID.

## Structure-aware parsing

[Epic 005](../epics/005-structure-aware-parsing.md) makes Scribe split text along the boundaries a reader sees. Each parse has a content type, `plain` or `markdown`, declared by the client or detected in core and shown in the GUI. Rust builds a block structure per content type (Markdown sections, list items, paragraphs, code; plain-text paragraphs), and the model selects whole blocks instead of guessing where an item ends. One agent and one extraction contract remain; only the structurer and one prompt section vary by content type.

The shared contracts accept optional `ParseRequest.content_type` and expose the chosen `content_type` and its `content_type_source` (`declared` or `detected`) in `ParseResult`. Unknown formats are rejected. Extraction spans select either a whole local block or a segment range with an optional exact quote; mixed references are invalid. Content type records a run's interpretation, independently of source provenance and the immutable payload. Core detection, block resolution, the shared-plus-format Scribe v4 prompts, API metadata and GUI display are implemented. The evaluation suite reports detection accuracy and extraction metrics separately per format. Live evaluation preserves aggregate plain-text span quality and detects all fixtures correctly, but the retained prompt extracts only 8 of 11 Epic 003 recommendation units; complete extraction-quality acceptance remains open. See the [recorded comparison](development.md#recorded-structure-aware-results-2026-11-04).

For Markdown, a section with subheadings supplies context rather than one labelable item. Validation rejects any extracted range that crosses a heading section boundary, regardless of whether the model used a block, segment range, or quote. A leaf section can still be selected with its heading and supporting paragraphs or bullets. This keeps labels on reader-visible units without imposing a length limit on legitimate leaf sections.

## Clarification branch (planned)

[Epic 004](../epics/004-clarification-branch.md) plans the first branch: the user clarifies one extracted span, and a separate agent (**Explainer**) explains it from the surrounding text and, when a project root is configured, read-only project files, citing its sources. Follow-up questions stay in the branch. The branch links to its target with a non-blocking `clarification` label and never changes the record's labels or statuses. The shared contracts (`ClarifyRequest`, `ClarifyResult`, the `clarification` label kind) are defined; the agent, endpoint, and GUI panel follow in later tasks.

Implemented scope is stateless and targets extracted spans only. Later work: clarifying an arbitrary text selection or unlabelled text, persisting branches and verifying client-supplied node IDs, and promoting a clarification finding into the main record.

## Project briefing (planned)

[Epic 006](../epics/006-project-briefing.md) plans three read-only agents that brief the user when a project opens: **Chronicler** (git history), **Surveyor** (files and documentation) and **Operator** (the commands the project supports, discovered and never run). Each accepts an optional prompt in place of its default task and cites evidence that Rust validates. Task A (the shared agent runner, the git tools, `GET /agents`, and the Chronicler behind `POST /projects/{id}/history`) is implemented; the rest follows.

## Projects and storage

A project is a named local folder registered in a single per-user SQLite database. The registry (add, list, open, rename, remove) is implemented and exposed over the API; it never modifies the folder. Graph persistence is a later step.

Agents receive a project per call: `/parse` and `/clarify` take a `project_id`, never a filesystem path. The server resolves it through the registry and an access policy (`daftprompt_core::access`) before an agent sees any folder. Locally every registered project is allowed; the same seam is where a hosted server's per-user, team, and per-agent folder rules would plug in. `DAFTPROMPT_PROJECT_ROOT` remains the fallback when a request names no project.

## Non-goals (for now)

- Non-text modalities (images, audio).
- Replacing the agents themselves. daftprompt orchestrates them.
- Automatic conflict resolution without a human or agent reviewing it.
- People directories, team management, or membership verification for assignee handles.

## Success signals

- Users resolve multi-question replies faster than in a linear chat.
- Branch agents produce better conclusions than the same follow-up in a long thread.
- Users trust the record: they can see why each conclusion exists and whether it still holds.

## Open questions

- How should the record be presented: as a document, a tree, or both?
- How do colleagues take part without adopting the full tool?
- Who resolves conflicts by default: the user, or a reconciling agent that proposes a resolution?
