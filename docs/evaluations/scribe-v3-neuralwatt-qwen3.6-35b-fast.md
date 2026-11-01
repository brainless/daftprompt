# Scribe evaluation: neuralwatt / qwen3.6-35b-fast

**Status:** live run completed: 19 of 19 fixture(s) parsed.

- Generated at: `2026-11-01T10:43:13Z`
- Provider / model: `neuralwatt` / `qwen3.6-35b-fast`
- Prompt version: `scribe-v3`
- Attempt budget: 3 total attempt(s), 60.0s per attempt
- Fixtures: `crates/daftprompt-core/fixtures/scribe`

## Fixture inventory

- 19 fixture(s), 38 expected span(s), 17 question label(s), 7 answer link(s), 4 explicit assignee(s)
- 11 recommendation label(s), 6 task label(s), 6 recommendation-question link(s), 3 symmetric alternative pair(s)
- Categories: chat (10), prose (2), code (1), document (1), email (2), json (1), none (2)

## Matching rules

- **Exact span match:** byte ranges are equal after trimming surrounding whitespace.
- **Lenient span match:** overlap divided by union is at least 0.5 (also reported in addition to exact).
- **Label:** the kind matches on a matched span.
- **Relationships:** both ends must match; alternative pairs are symmetric. Unary recommendation counts exclude relationship labels.
- **Assignee:** the handle is identical on a matched question or task.
- Matches are one-to-one and greedy, preferring exact pairs and then higher overlap.

## Aggregate

| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 11 | 12 | 11 | 91.7% | 100.0% |
| recommendation, lenient | 11 | 12 | 11 | 91.7% | 100.0% |
| task, exact | 6 | 7 | 6 | 85.7% | 100.0% |
| task, lenient | 6 | 7 | 6 | 85.7% | 100.0% |
| recommendation link, exact | 6 | 4 | 4 | 100.0% | 66.7% |
| recommendation link, lenient | 6 | 4 | 4 | 100.0% | 66.7% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 38 | 40 | 36 | 90.0% | 94.7% |
| spans, lenient | 38 | 40 | 38 | 95.0% | 100.0% |
| labels, exact | 41 | 43 | 39 | 90.7% | 95.1% |
| labels, lenient | 41 | 43 | 40 | 93.0% | 97.6% |
| answer links, exact | 7 | 7 | 6 | 85.7% | 85.7% |
| answer links, lenient | 7 | 7 | 7 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 3 | 75.0% | 75.0% |
| assignees, lenient | 4 | 4 | 3 | 75.0% | 75.0% |

- Invalid output: 0 of 19 fixture(s) (0.0%)
- Other parse failures: 0 of 19 fixture(s)
- Attempts per parse: mean 1.11 over 19 known of 19 parse(s), max 2
- Latency (ms): mean 2368.9, median 2085.2, p95 3351.8, max 5156.7

## Per fixture

| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |
| `advice-not-decision` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 3279.5 |
| `chat-local-answer` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2085.2 |
| `chat-open-questions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 3002.4 |
| `code-todo-question` | code | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2049.0 |
| `completed-and-incidental` | chat | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1212.1 |
| `direct-advice-request` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1670.9 |
| `document-adr` | document | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1703.3 |
| `email-crlf-answer` | email | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1839.6 |
| `email-quoted-answer` | email | ok | 6 | 33.3% / 33.3% | 100.0% / 100.0% | 33.3% / 33.3% | 66.7% / 66.7% | 0.0% / 0.0% / 100.0% / 100.0% | 0.0% / 0.0% / 0.0% / 0.0% | 1 | 3124.3 |
| `json-question-answer` | json | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1880.6 |
| `many-recommendation-links` | chat | ok | 2 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2319.2 |
| `none-release-notes` | none | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1192.1 |
| `none-rhetorical` | none | ok | 2 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 2 | 3351.8 |
| `prose-multiline-question` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2159.0 |
| `prose-rhetorical` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1747.3 |
| `selected-option` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1911.9 |
| `standalone-alternatives` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 2 | 5156.7 |
| `suggested-complementary-plan` | chat | ok | 2 | 66.7% / 100.0% | 66.7% / 100.0% | 66.7% / 100.0% | 66.7% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2716.2 |
| `tasks-and-exclusions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 2607.3 |

| fixture | recommendations exact / lenient P/R | tasks exact / lenient P/R | recommendation links exact / lenient P/R | alternatives exact / lenient P/R |
| --- | --- | --- | --- | --- |
| `advice-not-decision` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `chat-local-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `chat-open-questions` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `code-todo-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `completed-and-incidental` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `direct-advice-request` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `document-adr` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-crlf-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-quoted-answer` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `json-question-answer` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `many-recommendation-links` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 50.0% / 100.0% / 50.0% | n/a / n/a / n/a / n/a |
| `none-release-notes` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-multiline-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `selected-option` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `standalone-alternatives` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% |
| `suggested-complementary-plan` | 66.7% / 100.0% / 66.7% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `tasks-and-exclusions` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |

## Failures

- `email-quoted-answer` (email): **approximate_span**: expected span "e1" [42..90] "Should we enable the cache in staging this week?" matched leniently to predicted span [40..90] "> Should we enable the cache in staging this week?" (IoU 0.96) but not exactly
- `email-quoted-answer` (email): **approximate_span**: expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." matched leniently to predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." (IoU 0.97) but not exactly
- `email-quoted-answer` (email): **missed_label**: expected question label on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but it was not predicted
- `email-quoted-answer` (email): **unexpected_label**: predicted task label on predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." but none was expected
- `email-quoted-answer` (email): **missed_assignee**: expected @ops-team on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but none was predicted
- `email-quoted-answer` (email): **unexpected_assignee**: predicted @ops-team on predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." but no assignment was expected
- `many-recommendation-links` (chat): **missed_recommendation_link**: expected recommendation link expected span "e3" [49..69] "I recommend caching." -> expected span "e2" [26..48] "Should we reduce load?" but it was not predicted
- `many-recommendation-links` (chat): **missed_recommendation_link**: expected recommendation link expected span "e4" [70..89] "I suggest batching." -> expected span "e1" [0..25] "Should we reduce latency?" but it was not predicted
- `none-rhetorical` (none): **unexpected_span**: predicted span [30..49] "Was that not lucky?" matched no expected span
- `none-rhetorical` (none): **unexpected_label**: predicted question label on predicted span [30..49] "Was that not lucky?", but the span was not matched
- `suggested-complementary-plan` (chat): **unexpected_span**: predicted span [48..92] "Do not use a global lock: it blocks workers." matched no expected span
- `suggested-complementary-plan` (chat): **unexpected_label**: predicted recommendation label on predicted span [48..92] "Do not use a global lock: it blocks workers.", but the span was not matched

## Reproduce

```sh
DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --fixtures crates/daftprompt-core/fixtures/scribe --output <same-path>
```

## Interpretation and comparison limits

This is one stochastic run of Neuralwatt `qwen3.6-35b-fast` with `scribe-v3` on
2026-11-01, not a quality guarantee. All 19 submissions returned valid results;
two needed a second attempt. Lenient source recall was 100%, while exact source
recall was 94.7%. The question/advice/task worked example and standalone three-way
alternative set matched completely. Complementary steps acquired no alternative links.
The model missed two recommendation-question links, treated a rhetorical question as
open, extracted a rejected approach as a recommendation, and classified a quoted
question plus confirmation request as only a task. Email quote markers also caused
exact-boundary mismatches. These semantic and anchoring limitations remain visible
in the failure list rather than being hidden by valid schemas or lenient scores.

The 11 Epic 001 fixtures retain their original question spans, answer links, and
assignee expectations. The explicit action-request answer spans in `chat-local-answer`
and `email-crlf-answer` now additionally expect unary task labels under Epic 002;
this is a documented change to the gold annotations, not a removal of failures.
Eight additional fixtures cover the expanded semantics. Original answer links all
matched leniently; `email-quoted-answer` lost a question label to a task interpretation,
and `none-rhetorical` produced a false question. No live v2 scores exist in the
historical baseline, so a quantitative before/after model comparison is unavailable.
Deterministic tests exercise all old and new fixtures without a provider.
