# Data Model

Bare minimum. Everything lives in one SQLite database. The vocabulary is fixed and will grow slowly.

## Concepts

- **Payload**: immutable text from a chat (user prompt, agent reply, user answer). It behaves like a file: it has content and a hash. It is stored in the database, not on disk.
- **Node**: relationships only. A Node never carries text. Its content is defined by one or more NodeContexts.
- **NodeContext**: points at content. It is either a Payload or a file on disk, with optional line/column ranges. A sub-part of a text is a Node whose NodeContext points at a range of an existing Payload. Text is never copied.
- **Label**: a fixed-vocabulary annotation on the graph. It is either unary (describes one node) or binary (relates two nodes).
- **Assignee**: an optional text handle on a label indicating who is expected to respond, particularly to an open question. It is `@username` or `@team-name`, with no reference to a people or teams table.
- **Run log**: what agents did (runs, inputs, tool calls). It is stored alongside the graph but is not part of it.

The graph is a forest. Each tree's root is the Node for the user's root prompt (`parent_id IS NULL`). `parent_id` records "said in response to / is a part of". Labels add cross-links.

## Schema

```sql
-- Immutable text. Content-addressed: id is the SHA-256 hex of content.
CREATE TABLE payload (
  hash        TEXT PRIMARY KEY,
  content     TEXT NOT NULL,
  created_at  TEXT NOT NULL
);

CREATE TABLE node (
  id          TEXT PRIMARY KEY,
  parent_id   TEXT REFERENCES node(id),   -- NULL only for a root prompt
  author      TEXT NOT NULL,              -- human identity, or 'agent'
  run_id      TEXT REFERENCES run(id),    -- set when an agent produced this node
  created_at  TEXT NOT NULL
);

-- A Node has one or more contexts, ordered by seq.
-- Exactly one of payload_hash / path is set.
-- Range NULL = whole payload/file. Lines and columns 1-based; end exclusive.
CREATE TABLE node_context (
  id            TEXT PRIMARY KEY,
  node_id       TEXT NOT NULL REFERENCES node(id),
  seq           INTEGER NOT NULL,
  payload_hash  TEXT REFERENCES payload(hash),
  path          TEXT,                     -- relative to project root
  content_hash  TEXT,                     -- file hash at capture time (files only)
  start_line    INTEGER, start_col INTEGER,
  end_line      INTEGER, end_col   INTEGER,
  CHECK ((payload_hash IS NULL) != (path IS NULL))
);

CREATE TABLE label (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL,            -- see vocabulary
  src_node_id   TEXT NOT NULL REFERENCES node(id),
  dst_node_id   TEXT REFERENCES node(id), -- NULL for unary kinds
  status        TEXT,                     -- open | deferred | resolved | dismissed; NULL for non-blocking kinds
  assignee      TEXT,                     -- @username or @team-name; NULL = unassigned
  note_node_id  TEXT REFERENCES node(id), -- optional explanation
  created_by    TEXT NOT NULL,
  run_id        TEXT REFERENCES run(id),
  created_at    TEXT NOT NULL
);

-- Run log: outside the graph.
CREATE TABLE run (
  id          TEXT PRIMARY KEY,
  agent       TEXT NOT NULL,              -- which agent / mode
  started_at  TEXT NOT NULL,
  ended_at    TEXT,
  status      TEXT NOT NULL               -- running | done | failed
);

CREATE TABLE run_input (
  run_id   TEXT NOT NULL REFERENCES run(id),
  node_id  TEXT NOT NULL REFERENCES node(id),
  PRIMARY KEY (run_id, node_id)
);

CREATE TABLE tool_call (
  id           TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL REFERENCES run(id),
  seq          INTEGER NOT NULL,
  tool         TEXT NOT NULL,
  input        TEXT NOT NULL,             -- JSON
  output_hash  TEXT REFERENCES payload(hash),
  created_at   TEXT NOT NULL
);
```

A run's outputs are the nodes and labels whose `run_id` points to it.

## Vocabulary

| kind         | shape                 | blocking | meaning                                   |
|--------------|-----------------------|----------|-------------------------------------------|
| `question`   | unary                 | yes      | src is an open question                   |
| `answer`     | binary: src → dst     | no       | src answers question dst                  |
| `conflict`   | binary: src ↔ dst     | yes      | src and dst cannot both hold              |
| `supersedes` | binary: src → dst     | no       | src replaces dst (edits, resolutions)     |
| `stale`      | unary                 | yes      | src was built on superseded or changed content |

Many-to-many falls out of binary labels: one answer can resolve several questions (several `answer` labels), and a question can have several answers. A question that raises more questions has them as child nodes, each labelled `question`.

## Rules

- **Immutability.** Payloads and nodes are never edited. An edit is a new node plus `supersedes(new → old)`. Only `label.status` and `label.assignee` change.
- **Assignment.** A label has at most one assignee handle, which can be set, reassigned, or cleared independently of its status. A handle starts with `@` and has a non-empty name with no whitespace. Handles are opaque: the model does not distinguish people from teams or resolve membership. For example, a question assigned to `@legal-team` invites an answer from anyone in that team; assignment records responsibility and does not enforce access permissions.
- **Conflicts are not auto-materialized.** A `conflict` label just records the conflict. Resolving it means creating a new node, adding `supersedes` labels, and marking the conflict `resolved`.
- **Staleness detection.**
  - A node from run R is stale if any of R's input nodes has since been superseded.
  - A file NodeContext is stale if the file's current hash ≠ `content_hash`. File ranges can drift. The hash detects this but cannot re-anchor the range.
  - Detected staleness is written as a `stale` label so it can be resolved or dismissed.
- **Done.** A tree is done when no blocking label in it has status `open` or `deferred`.

## Worked example

| step | what happens | records |
|------|--------------|---------|
| 1 | User: "Add rate limiting to the public API" | payload P1; node N1 (root) → ctx P1 |
| 2 | Run R1 replies with a plan and two questions | payload P2; node N2 (parent N1, run R1) → ctx P2 |
| 3 | Questions are spans of the reply | N3 → ctx P2 L8–L9, N4 → ctx P2 L10–L11; `question(N3)`, `question(N4)` open |
| 4 | Plan step 3 is a span | N5 → ctx P2 L5–L6 |
| 5 | User edits step 3 | payload P3; N6 (parent N2) → ctx P3; `supersedes(N6 → N5)` |
| 6 | User answers both questions in one reply, pointing at a file | payload P4; N7 (parent N2) → ctx P4 + ctx `src/api/middleware.ts` L20–L45 (hash h1); `answer(N7 → N3)`, `answer(N7 → N4)`; both questions resolved |
| 7 | Branch run R2 takes N1, N3, N4, N7 | `run_input` rows; tool calls logged; reply payload P5; N8 (parent N7, run R2) |
| 8 | N8 contradicts the user's edit N6 | `conflict(N8 ↔ N6)` open. Shown to the user, not merged |
| 9 | `middleware.ts` changes | N7's file ctx hash ≠ h1 → `stale(N7)` open |

At step 9 the tree is not done: one open `conflict` and one open `stale`.
