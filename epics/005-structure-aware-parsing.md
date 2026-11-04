# Epic 005: Structure-aware parsing

## Outcome

Make Scribe split text along the boundaries a reader sees. Today the model receives a flat list of numbered lines and tends to label one line, or part of one, even when the actionable item is a whole Markdown section or list item. For example, pasting [Epic 003](003-parsing-refinements.md) produces a recommendation covering only `a high invalid-output rate, or many parses needing more than one attempt.`, where the item a reader acts on is the whole `### Retry with corrective feedback` section.

This epic adds a content type to each parse, a Rust-built block structure per content type, and a way for the model to select a whole block instead of a segment range. Plain text and Markdown are the two supported content types. Later formats (CSV, JSON, code) can be added as new structurers without changing the extraction contract.

References: [PRD](../docs/PRD.md), [data model](../docs/data-model.md), [Epic 001](001-text-parsing.md), [Epic 002](002-recommendations-tasks-and-identifiers.md), and [Epic 003](003-parsing-refinements.md). Epic 003's *Sub-line segmentation* item remains separate: it addresses spans smaller than a line, this epic addresses spans larger than one.

## Scope and decisions

- **Content type is separate from `SourceKind`.** `SourceKind` says where the text came from (chat, file, email); content type says how it is formatted. An email may be plain or Markdown; a file may be CSV.
- **Declared or detected.** A request may declare a content type. Otherwise Rust detects one with a deterministic heuristic. The result reports the content type used and whether it was declared or detected. Detection lives in `daftprompt-core`; the GUI only displays the result.
- **One agent, one contract.** Scribe remains a single agent with a single extraction schema. Only the structurer and one section of the system prompt vary by content type.
- **One prompt, assembled from parts.** A shared core prompt plus one format section per content type. The recorded prompt version names both parts.
- **An enum, not a plugin trait.** `ContentType { Plain, Markdown }` with `match` at each point that varies. Revisit only when a third format shows what really varies.
- **Rust owns structure.** Rust computes blocks and their byte ranges. The model selects blocks by local ID; it never produces offsets or decides where a block ends.
- **Unchanged guarantees.** The original text is preserved; spans stay disjoint; every span must be annotated; display IDs, graph construction, label semantics, and the stateless API are unchanged. Unlabelled text remains valid.
- **Eval per format.** Fixtures declare their content type, and metrics are reported per content type so a Markdown-focused change cannot silently degrade plain text.

## Content types and detection

| content type | structurer | typical sources |
|---|---|---|
| `markdown` | Markdown blocks and heading sections | LLM chat replies, `.md` files, docs |
| `plain` | Paragraphs separated by blank lines | Plain email, notes, logs, unknown text |

Detection, applied only when no content type is declared:

- `markdown` if the text has an ATX heading line (`#`–`######` followed by a space), a fenced code block (```` ``` ```` or `~~~`), at least two list-item lines (`- `, `* `, `+ `, `1. `), or a table delimiter row.
- A `file` source whose reference ends in `.md` or `.markdown` is a hint toward `markdown`, never proof on its own. Only the text decides.
- Otherwise `plain`.

Detection is deterministic and tested. A wrong guess is visible in the GUI and in run metadata, so the evaluation can separate detection errors from extraction errors.

## Blocks

A structurer turns the text into a tree of **blocks**. Each block has a local ID (`b1`, `b2`, … in source preorder), a kind, a parent, an inclusive segment range, and a byte range trimmed of surrounding whitespace. Block boundaries always fall on segment boundaries.

**Markdown**

| kind | extent |
|---|---|
| `section` | An ATX heading line through the line before the next heading of the same or higher level, or end of text. Nested headings produce nested sections. |
| `item` | One list item, including its continuation lines and nested lists. Nested items are child blocks. |
| `paragraph` | A paragraph outside a list item. |
| `code` | A fenced or indented code block. Never split; a `#` inside a fence is not a heading. |
| `quote` | A blockquote. |
| `table` | A whole table. |

Use a Markdown parser that reports source offsets (for example, `pulldown-cmark`'s offset iterator). Inspect its source and version before choosing. Map parser offsets to whole segments, and record the dependency version in `docs/development.md`. Front matter and setext headings are treated as paragraphs in this epic.

**Plain**

| kind | extent |
|---|---|
| `paragraph` | A run of non-empty lines between blank lines. |

Lines starting with `-` or `>` in plain text are not interpreted. Text needing that structure should be detected or declared as Markdown.

## Rendering for the model

Segments keep their existing numbering. Rust interleaves one unnumbered marker line before the first segment of each block, indented by nesting depth (indicative format, tuned during evaluation).

Start with inline marker lines: each block ID sits next to the text it covers, so a small model does not have to map a separate outline back to segment numbers. A separate outline (block IDs, kinds, and segment ranges listed before the segments) is the alternative to try if markers perform poorly, for example if the model copies marker text into quotes. Keep the presentation behind one rendering function so the two can be compared on the same fixtures.

```text
{b7 section 14-53}
[14] ## Improve if needed
[15]
  {b8 section 16-21}
[16] ### Retry with corrective feedback
[17]
    {b9 item 18}
[18] - **Trigger:** a high invalid-output rate, or many parses needing more than one attempt.
    {b10 item 19}
[19] - Epic 001 retries validation failures from scratch. Instead, send the specific ...
```

Marker lines are never source text. They are not numbered, cannot be quoted, and the prompt explains them.

## Extraction contract

A span is pointed at in one of two ways:

```json
{ "id": "s1", "block": "b8" }
{ "id": "s2", "start_segment": 18, "end_segment": 18, "quote": "a high invalid-output rate" }
```

- `block` and the segment range are mutually exclusive; exactly one must be present. A `quote` is allowed only with a segment range.
- A block span covers the block's trimmed byte range. Rust resolves it; the model supplies no offsets.
- Overlap rules are unchanged. A section and one of its child blocks overlap, so selecting both is invalid. The parent stays unlabelled and its children are labelled, or the reverse.
- Unknown block IDs make the output invalid, like an out-of-range segment.

## Prompt

- `prompts/scribe-v4.md`: the shared core. It keeps the v3 label semantics, anchoring and exact-copy rules, and examples, adds the block form and marker-line explanation, and adds a **granularity rule**: select the smallest block that holds the whole actionable item, including its rationale and conditions; use a segment range or quote only when the item is smaller than a block (for example, a question sentence inside a paragraph).
- `prompts/scribe-format-markdown-v1.md`: a section whose heading names an item (an option, refinement, or plan step) is usually the unit; list items are the unit in bullet lists; headings that only group other items stay unlabelled. Includes a worked example shaped like Epic 003.
- `prompts/scribe-format-plain-v1.md`: paragraphs are the usual unit for recommendations; questions and tasks inside prose may be sentences.
- Both parts are included at compile time. The recorded prompt version is `scribe-v4+markdown-v1` or `scribe-v4+plain-v1`.

## Tasks

### 1. Shared contracts

- Add `ContentType { Plain, Markdown }` (serialized `plain`, `markdown`) to `daftprompt-types`.
- Add optional `content_type` to `ParseRequest`. Unknown values are rejected by deserialization.
- Add `content_type` and `content_type_source` (`declared` or `detected`) to `ParseResult`.
- Make `ExtractedSpan` accept either `block` or `start_segment`/`end_segment`, keeping the schema clear for the model.
- Update the data model: content type is how a run interpreted a payload, recorded on the run, not on the content-addressed payload. Update the PRD's parser section.

### 2. Detection and structurers in core

- Add a `text::structure` module: detection, the plain and Markdown structurers, and the block tree with segment and byte ranges.
- Test detection on each signal and on plain prose. Test structure with CRLF, multi-byte and combining characters, nested sections and lists, fences containing `#` and list markers, tables, blockquotes, empty input, and text without a trailing newline.

### 3. Scribe v4

- Render segments with block marker lines, and validate and resolve block references alongside existing segment anchoring.
- Assemble the prompt from the core and format parts, and record the combined version on the run.
- Add deterministic stub-extractor tests: block spans, block plus quote rejected, unknown block, parent/child overlap, and mixed block and segment spans in one extraction.

### 4. API and GUI

- Pass the declared content type through `POST /parse` and return the content type used and its source. Document the fields.
- The GUI shows the content type and whether it was detected (for example, `Markdown · detected`) with the parse result. It performs no detection and offers no override of the detected type in this epic.

### 5. Evaluation

- Add `content_type` to fixtures. Tag existing fixtures and add an expected detection result.
- Add Markdown fixtures: the Epic 003 text (each `###` under *Improve if needed* is one recommendation span; each *Good to have* bullet is one), an LLM-style reply with a plan section and a question list, and a nested-list plan. Add a plain-text multi-paragraph proposal.
- Report detection accuracy and all existing metrics per content type. Record a scribe-v3 baseline and the v4 result on the same fixtures with the same model.

## Acceptance criteria

- Parsing the Epic 003 text yields one recommendation per `###` refinement, each covering heading through last bullet, and one per *Good to have* bullet. Grouping headings stay unlabelled.
- Existing plain-text and question/answer fixtures do not regress on exact or lenient span metrics.
- Detection matches the expected content type on every fixture. The GUI shows the content type and its source.
- Deterministic verification passes without LLM credentials.

## Implementation status (2026-11-04)

Tasks 1–5 are implemented and deterministic verification passes. The retained live
v4 run detects all 23 fixtures correctly and improves aggregate plain-text span
metrics over the paired v3 baseline. It extracts all expected nested-list and
Markdown reply units, but only 8 of 11 Epic 003 recommendations: three optional
bullets are omitted. Full extraction-quality acceptance therefore remains open.
Prompt experiments reached 11/11 while regressing plain-text precision, so that
variant was not retained. See [the recorded comparison and limitations](../docs/development.md#recorded-structure-aware-results-2026-11-04).

## Open questions

- Do inline marker lines work well for a small model, or does a separate outline perform better? Start with markers; switch only if fixture results and manual testing show a problem.
