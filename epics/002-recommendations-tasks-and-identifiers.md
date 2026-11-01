# Epic 002: Recommendations, tasks, and readable identifiers

## Outcome

Expand Scribe so readers can distinguish questions, suggested answers, actual answers, and outstanding actions. Give every extracted source span a short integer identifier, assigned by Rust, and show related questions and explicitly presented alternative approaches. Recommendations in plans or investigation replies are captured even when the submitted text contains no question.

**Status: implemented.** Shared contracts/core validation, Scribe v3, API/GUI rendering, deterministic tests, and expanded evaluation reporting are implemented. Live Neuralwatt results and model limitations are recorded in [the evaluation report](../docs/evaluations/scribe-v3-neuralwatt-qwen3.6-35b-fast.md). This epic does not introduce persistence or conversation history.

References: [PRD](../docs/PRD.md), [data model](../docs/data-model.md), [Epic 001](001-text-parsing.md), and [Epic 003: Parsing refinements](003-parsing-refinements.md).

## Scope and decisions

- Add `recommendation` and `task` annotations, plus `recommendation_for` and `alternative_to` graph relationships, to the documented vocabulary and extraction contract. Use `recommendation` for both recommendations and suggestions; use `task` for both tasks and action items.
- Preserve the Epic 001 architecture: typed extraction through Rig's single `submit` tool, validation and graph construction in core, a stateless API, and a GUI that renders original source text.
- Preserve immutable payloads, source provenance, segment-and-quote anchoring, Unicode boundaries, and disjoint spans. A single span can carry multiple annotations when each is independently justified.
- Relationships refer only to spans in the same submission. Do not infer missing questions, answers, decisions, assignees, or task completion.
- Keep generated string IDs as graph identities. Add integer display IDs for extracted spans; do not migrate graph primary keys to integers.
- Continue to accept unlabelled text and empty extractions. No database, task execution, status-editing workflow, recommendation acceptance workflow, or cross-submission identity matching is included.

## Label semantics

| kind | shape | status | meaning |
|------|-------|--------|---------|
| `question` | unary | Rust-derived `open` or `resolved` | A question the reader may need to answer |
| `answer` | binary: source → question | NULL | An actual answer present in the submitted text |
| `recommendation` | unary | NULL | A suggested approach or an offered option, without implying acceptance or preference |
| `recommendation_for` | binary: source → question | NULL | A recommendation addresses an identified local question |
| `alternative_to` | binary: source ↔ destination | NULL | Two recommendations are explicitly offered as alternatives |
| `task` | unary | Initially `open` | An explicit outstanding requested or committed action |

### Recommendations and answers

- Extract recommendations from plans, investigations, and ordinary replies, with or without a local question. A suggested approach must be supported by the source text; do not invent the earlier user request or a synthetic question to attach it to.
- Emit one unary `recommendation` label per recommendation span. Its optional `questions` list produces separate `recommendation_for` links. One recommendation may target several questions; one question may have several recommendations. The unary label exists whether or not links exist.
- A recommendation does **not** resolve its question. Tighten the answer prompt so advice such as “I suggest a five-minute cache” is not also labelled as an answer solely because it proposes a possible response.
- Preserve the existing rule that a validated answer link resolves its target question. For a decision question, a proposed option is a recommendation; an explicit selection or decision is an answer. For a factual question, an actual factual response is an answer. Do not infer acceptance from a recommendation alone.
- If the text explicitly asks for advice, advice can itself answer that request. Include examples distinguishing “What do you recommend?” from “Which option shall we choose?” so the model does not treat every advisory response as an unresolved decision.
- Do not automatically add an answer annotation to a recommendation or vice versa. Multiple annotations on one span require independently supported meanings.

### Plans and alternative options

- Capture each independently actionable suggestion or offered option as its own source span and `recommendation` annotation. “Option A: cache locally” and “Option B: use a shared cache” are two entities with two display IDs. The label means an offered approach, not necessarily an endorsed or preferred one.
- Preserve each option's supporting rationale or tradeoffs when they form a contiguous part of that option. Keep shared introductions and background in the original unlabelled text. Do not copy shared rationale into several nodes or create an overlapping whole-plan span.
- A list of suggested plan steps is not automatically a list of alternatives: steps may be complementary or sequential. Add `alternative_to` only when the text explicitly presents a choice between approaches, including clearly introduced option lists. Alternatives do not imply a `conflict`, and a shared question alone does not establish alternatives.
- For an explicit set of alternatives, connect every pair of member spans with `alternative_to`. This represents the set without introducing synthetic group nodes or a new group store. Do not infer additional alternatives by transitivity across separately stated pairs.
- Represent alternatives in extraction with an optional `alternatives` list of local recommendation span IDs on a recommendation annotation. Submit each unordered pair once; either direction is accepted. Rust canonicalizes the stored direction by source order, so rendering and scoring treat the relationship as symmetric.
- Retain wording such as “recommended”, “preferred”, or “not recommended” in source text. Do not infer rankings or acceptance. A rejected approach mentioned only to explain why it should not be used is not an offered option. Machine-readable ranking and explicit plan/group entities are outside this epic.
- Proposed plan steps remain recommendations until the text explicitly requests or commits to their execution. A heading such as “Suggested plan” matters: imperative wording inside it does not by itself create open tasks.

For an investigation reply with no question:

```text
Two alternatives for reducing latency:
Option A: Cache locally. This avoids a network hop.
Option B: Use a shared cache. This shares entries across workers.
Separately, I recommend adding latency metrics.
```

Extract three recommendation spans: **#1** and **#2** are alternatives; **#3** is an independent recommendation. None needs a question target, none creates a task, and none is automatically preferred. The introduction remains unlabelled. The model annotations are:

```json
[
  { "kind": "recommendation", "span": "s1", "alternatives": ["s2"] },
  { "kind": "recommendation", "span": "s2" },
  { "kind": "recommendation", "span": "s3" }
]
```

Rust emits three unary recommendation labels and one symmetric `alternative_to` relationship. If the same submission also contains a question these options address, their `questions` lists additionally produce `recommendation_for` links to it.

### Tasks and assignees

- Extract concrete outstanding requests, instructions, commitments, and explicit TODOs: “Please add a migration”, “I will update the docs”, or “TODO: benchmark the cache”.
- Do not turn hypothetical options, tentative suggestions, descriptions of capabilities, or completed-work reports into open tasks. “We could add a cache” is not a commitment; “We added a cache yesterday” is not outstanding work.
- A polite action request phrased as a question, such as “@alex, can you update the docs?”, is a task when it requests the action. A genuine capability or decision question remains a question. Do not add both labels merely because a request ends with a question mark.
- Allow explicit `@username` or `@team-name` assignees on tasks as well as questions. A bare mention is insufficient. Do not resolve “I”, names, or pronouns to invented handles; an unassigned task is valid.
- Tasks start `open`. Do not infer `resolved` or `deferred`, accept model-proposed statuses, or derive task completion from answer links.
- Tasks are blocking: the planned “done” rule includes open or deferred tasks alongside questions, conflicts, and stale items. Task completion and status editing are later work.

## Extraction and relationship mapping

Keep model-local span IDs such as `s1`. Extend annotations with optional `questions` and `alternatives` lists on recommendations, alongside the existing `answers` list for answer targets. Omitted lists default to empty:

```json
{
  "spans": [
    { "id": "s1", "start_segment": 1, "end_segment": 1 },
    { "id": "s2", "start_segment": 2, "end_segment": 2 },
    { "id": "s3", "start_segment": 3, "end_segment": 3 }
  ],
  "annotations": [
    { "kind": "question", "span": "s1" },
    { "kind": "recommendation", "span": "s2", "questions": ["s1"] },
    { "kind": "task", "span": "s3", "assignee": "@alex" }
  ]
}
```

For this input:

```text
Should we cache tokens?
I suggest a five-minute cache.
@alex, please benchmark the cache.
```

Rust validates the complete extraction before building records:

1. Anchor spans and validate unique local IDs, ranges, and non-overlap.
2. Resolve annotation references and identify question and recommendation spans regardless of annotation order.
3. Require non-empty `answers` on answers and an empty `answers` list on other kinds. Allow `questions` and `alternatives` only on recommendations; either or both may be empty. Assignees are allowed only on questions and tasks.
4. Require answer and recommendation-question targets to be existing question spans, and alternative targets to be existing recommendation spans. Reject self-links, unknown targets, duplicate links of the same kind (including reversed alternative pairs), and duplicate unary labels of the same kind on a span. Different relationship kinds are distinct; structural validation cannot establish whether the model's semantic interpretation is correct.
5. Require every span to be annotated. Allocate generated node IDs and map each model-local span ID to its node ID, then construct labels using those identities.
6. Derive question status from answer links only and task status as `open`.

The model supplies neither graph IDs nor display IDs. Relation targets are node IDs, not label IDs or copied text. Labels retain their own generated IDs so multiple annotations and links remain separately addressable.

## Integer display IDs

- Add `display_id: u32` to the API's `SpanInfo`, assigned in core after validation.
- Number extracted spans consecutively from 1 in source order, using the existing byte-range ordering. Root nodes and unlabelled gaps do not consume a number.
- A span with several labels has one display ID. Do not create separate question, answer, recommendation, or task numbering sequences.
- Maintain the existing graph references through `node_id`, `src_node_id`, and `dst_node_id`. Clients obtain a relationship's display target by looking up its node ID in the returned spans.
- Display IDs are unique only within one parse result. They are stable for that result, not across reparses or changed extractions. A reference retained outside the result needs its run identity as well; the integer alone is insufficient.
- The API is authoritative for numbering. The GUI must not independently renumber spans or expose model-local IDs as user-facing identities.

The example above displays as question **#1**, recommendation **#2 → #1**, and task **#3**, assigned to `@alex`. Question #1 and task #3 remain open. A later span containing an actual answer to #1 receives its own number and an answer link to #1.

## Tasks

### 1. Extend shared contracts and graph construction

- Add annotation and label kinds, optional recommendation question/alternative targets, and `SpanInfo.display_id` in `daftprompt-types`.
- Update label shape and blocking helpers: recommendations are unary and non-blocking; `recommendation_for` and symmetric `alternative_to` are binary and non-blocking; tasks are unary and blocking. Materialize relationship labels separately from the unary recommendation label.
- Extend core validation and graph construction using the rules above. Preserve current answer behavior and the separation between source entities and label records.
- Document the additive API fields and new enum values. Update all workspace consumers together; older clients with exhaustive enums may reject the new kinds.
- Update the data model vocabulary and PRD from the planned notes to the implemented semantics, keeping Epic 001 as the historical initial scope. No SQLite schema implementation is needed.

### 2. Update Scribe's prompt

- Introduce a new prompt version based on Scribe v2, retaining the exact-copy and whitespace-tolerant anchoring rules.
- Add examples of a question with advice but no decision, a selected option, a direct request for advice, multiple recommendations, and one recommendation targeting multiple questions. Include investigation replies without questions, suggested plans, explicit alternative sets, independent suggestions, rejected approaches, and complementary steps that are not alternatives.
- Add assigned and unassigned tasks, polite action requests, commitments, explicit TODOs, tentative options, completed actions, and incidental mentions.
- Show how to reuse a single span for multiple justified annotations without overlapping spans. Keep text reconstruction and paraphrasing out of the output contract.

### 3. Expose and render the results

- Continue using `POST /parse`; add no new endpoints. Document the expanded result and an example mapping recommendation and answer targets to display IDs.
- Show `#N` on each extracted span, alongside its labels, status, and assignee where applicable.
- Show answer and recommendation-question targets using their question numbers and short source previews. On questions, show incoming recommendations separately from incoming answers.
- Show standalone recommendations without an empty or fabricated question target. Show explicit alternative peers by display ID with source previews; do not imply that every recommendation in a reply is an alternative or is preferred.
- Render tasks with their open status and optional assignee. Preserve original source order and unlabelled text between blocks.
- Resolve GUI relationships through node IDs. Preserve defensive handling of malformed API results without silently assigning replacement numbers.

### 4. Verify structure and extraction quality

- Add deterministic tests with stub extractions for new label shapes, invalid fields and targets, duplicate and self-links, task assignees, many-to-many recommendation links, and question status with recommendation-only versus actual-answer links.
- Test standalone recommendations, empty target lists, wrong-kind alternative targets, symmetric canonicalization, duplicate reversed pairs, and explicit sets with three or more alternatives. Verify complementary plan steps receive no alternative links.
- Test source-order numbering when model spans and annotations arrive out of order, contiguous numbering, empty output, one ID for multiple labels, and correctly mapped targets after sorting.
- Extend API and GUI tests for serialization, display IDs, new badges, incoming/outgoing links, and unchanged source text.
- Extend fixtures and evaluation scoring to report unary recommendation labels, recommendation-question links, symmetric alternative links, and task labels/assignees separately. Score alternative pairs without regard to stored direction; do not count relationship labels as extra recommendation entities. Generalize the existing question-only assignee scoring. Keep exact and lenient source-span metrics and report invalid outputs, attempts, and latency.
- Include semantic fixtures for advice mistaken for a decision, tasks mistaken for questions, suggestions mistaken for commitments, completed work, and unsupported assignment. Compare the existing question/answer fixtures before and after the prompt change.
- Run formatting, relevant deterministic tests, and workspace checks. Ordinary tests require no LLM credentials. Run the expanded live evaluation opt-in with a configured provider and record model, prompt version, results, and limitations; if unavailable, record it as not run.

## Acceptance criteria

- The worked example produces three numbered spans, a recommendation link to question #1, and an assigned task; the recommendation leaves the question open.
- The investigation example produces three standalone numbered recommendations and exactly one alternative relationship, with no invented question, preference, or task. Suggested multi-step plans preserve independent steps without treating them as alternatives.
- Actual local answers still resolve questions, and task labels start open without inventing completion or assignees.
- Graph links remain correct regardless of model output order, and every rendered extracted entity has exactly one API-assigned display integer.
- The GUI distinguishes answers, recommendations, and tasks while displaying the original source text.
- Deterministic verification passes, and evaluation reporting covers the new labels, relationships, and assignees without dropping the existing question/answer metrics.
