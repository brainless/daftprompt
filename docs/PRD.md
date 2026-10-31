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
- **Clarification**: a branch labelled `clarification` for understanding a specific term or statement. It links to the source text and keeps its questions and answers outside the original chat. It does not block the main task by default.
- **Done**: work is complete when no blocking questions, conflicts, or stale items remain.

## Key flows

1. **Start**: the user writes a root prompt. The agent researches and replies with a plan, questions, and pushback. These are separated into parts the user can act on.
2. **Partial answers**: the user answers some questions now. A branch agent takes the root prompt, those questions, and the answers, and returns updated conclusions.
3. **Delegate**: the user assigns a question to a person (`@username`) or a team (`@legal-team`). It remains open, or is marked as deferred if the answer must wait, possibly days later. Anyone in the assigned team can provide the answer.
4. **Edit the plan**: the user changes part of the plan. A branch agent analyses the change against the original request.
5. **Conflict**: a branch's conclusion contradicts something else in the record. The conflict is shown to the user or another agent to resolve. It is never silently overwritten.
6. **Staleness**: when something a conclusion relied on changes (an edited plan, a changed file), that conclusion is flagged as stale.
7. **Converge**: the user can always see what is still open. When nothing blocking remains, the work is ready to act on.
8. **Clarify**: in an ongoing chat, the user asks about a specific part of an agent's reply, such as an unfamiliar term. A separate agent opens a **Clarification** branch linked to that text, consults code or documents anywhere in the project as needed, and explains it with references to the sources it used. The user can continue asking questions in that branch without adding the exchange to the original chat or automatically changing its conclusions.

## Goals

- Respond to any part of an agent's reply on its own.
- Get project-grounded explanations in clarification branches without interrupting the original chat.
- Keep each agent's context small and relevant.
- Allow asynchronous input (colleagues, delays) without losing track.
- Make conflicts and staleness visible.
- Full traceability: for any conclusion, show what it was based on and what the agent looked at.

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
