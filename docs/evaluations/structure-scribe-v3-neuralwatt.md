# Scribe evaluation: neuralwatt / qwen3.6-35b-fast

**Status:** live run completed: 22 of 23 fixture(s) parsed.

- Generated at: `2026-11-04T09:41:28Z`
- Provider / model: `neuralwatt` / `qwen3.6-35b-fast`
- Prompt version: `scribe-v3`
- Attempt budget: 3 total attempt(s), 60.0s per attempt
- Fixtures: `crates/daftprompt-core/fixtures/scribe`

## Fixture inventory

- 23 fixture(s), 58 expected span(s), 19 question label(s), 7 answer link(s), 4 explicit assignee(s)
- 29 recommendation label(s), 6 task label(s), 6 recommendation-question link(s), 3 symmetric alternative pair(s)
- Categories: chat (10), prose (5), code (1), document (2), email (2), json (1), none (2)

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
| recommendation, exact | 18 | 19 | 13 | 68.4% | 72.2% |
| recommendation, lenient | 18 | 19 | 18 | 94.7% | 100.0% |
| task, exact | 6 | 7 | 5 | 71.4% | 83.3% |
| task, lenient | 6 | 7 | 6 | 85.7% | 100.0% |
| recommendation link, exact | 6 | 4 | 4 | 100.0% | 66.7% |
| recommendation link, lenient | 6 | 4 | 4 | 100.0% | 66.7% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 47 | 49 | 36 | 73.5% | 76.6% |
| spans, lenient | 47 | 49 | 47 | 95.9% | 100.0% |
| labels, exact | 50 | 52 | 38 | 73.1% | 76.0% |
| labels, lenient | 50 | 52 | 49 | 94.2% | 98.0% |
| answer links, exact | 7 | 7 | 5 | 71.4% | 71.4% |
| answer links, lenient | 7 | 7 | 7 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 3 | 75.0% | 75.0% |
| assignees, lenient | 4 | 4 | 3 | 75.0% | 75.0% |

- Invalid output: 1 of 23 fixture(s) (4.3%)
- Other parse failures: 0 of 23 fixture(s)
- Attempts per parse: mean 1.13 over 23 known of 23 parse(s), max 3
- Latency (ms): mean 3021.2, median 2367.1, p95 3954.5, max 16898.2

## Per content type

### plain

Detection accuracy: 18/18 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 13 | 14 | 13 | 92.9% | 100.0% |
| recommendation, lenient | 13 | 14 | 13 | 92.9% | 100.0% |
| task, exact | 6 | 7 | 5 | 71.4% | 83.3% |
| task, lenient | 6 | 7 | 6 | 85.7% | 100.0% |
| recommendation link, exact | 6 | 4 | 4 | 100.0% | 66.7% |
| recommendation link, lenient | 6 | 4 | 4 | 100.0% | 66.7% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 38 | 40 | 32 | 80.0% | 84.2% |
| spans, lenient | 38 | 40 | 38 | 95.0% | 100.0% |
| labels, exact | 41 | 43 | 34 | 79.1% | 82.9% |
| labels, lenient | 41 | 43 | 40 | 93.0% | 97.6% |
| answer links, exact | 6 | 6 | 4 | 66.7% | 66.7% |
| answer links, lenient | 6 | 6 | 6 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 3 | 75.0% | 75.0% |
| assignees, lenient | 4 | 4 | 3 | 75.0% | 75.0% |

- Invalid output: 0 of 18 fixture(s) (0.0%)
- Other parse failures: 0 of 18 fixture(s)
- Attempts per parse: mean 1.06 over 18 known of 18 parse(s), max 2
- Latency (ms): mean 2448.0, median 2411.5, p95 3632.1, max 3954.5

### markdown

Detection accuracy: 5/5 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 5 | 5 | 0 | 0.0% | 0.0% |
| recommendation, lenient | 5 | 5 | 5 | 100.0% | 100.0% |
| task, exact | 0 | 0 | 0 | n/a | n/a |
| task, lenient | 0 | 0 | 0 | n/a | n/a |
| recommendation link, exact | 0 | 0 | 0 | n/a | n/a |
| recommendation link, lenient | 0 | 0 | 0 | n/a | n/a |
| alternative, exact | 0 | 0 | 0 | n/a | n/a |
| alternative, lenient | 0 | 0 | 0 | n/a | n/a |
| spans, exact | 9 | 9 | 4 | 44.4% | 44.4% |
| spans, lenient | 9 | 9 | 9 | 100.0% | 100.0% |
| labels, exact | 9 | 9 | 4 | 44.4% | 44.4% |
| labels, lenient | 9 | 9 | 9 | 100.0% | 100.0% |
| answer links, exact | 1 | 1 | 1 | 100.0% | 100.0% |
| answer links, lenient | 1 | 1 | 1 | 100.0% | 100.0% |
| assignees, exact | 0 | 0 | 0 | n/a | n/a |
| assignees, lenient | 0 | 0 | 0 | n/a | n/a |

- Invalid output: 1 of 5 fixture(s) (20.0%)
- Other parse failures: 0 of 5 fixture(s)
- Attempts per parse: mean 1.40 over 5 known of 5 parse(s), max 3
- Latency (ms): mean 5084.9, median 2367.1, p95 16898.2, max 16898.2

## Per fixture

| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |
| `advice-not-decision` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 3632.1 |
| `chat-local-answer` | chat | ok | 3 | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 3954.5 |
| `chat-open-questions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 3525.1 |
| `code-todo-question` | code | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1617.3 |
| `completed-and-incidental` | chat | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1389.4 |
| `direct-advice-request` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2421.7 |
| `document-adr` | document | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2039.5 |
| `email-crlf-answer` | email | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1721.0 |
| `email-quoted-answer` | email | ok | 5 | 66.7% / 66.7% | 100.0% / 100.0% | 66.7% / 66.7% | 66.7% / 66.7% | 100.0% / 100.0% / 100.0% / 100.0% | 0.0% / 0.0% / 0.0% / 0.0% | 1 | 2365.9 |
| `json-question-answer` | json | ok | 2 | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1743.5 |
| `many-recommendation-links` | chat | ok | 2 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2934.8 |
| `markdown-epic003` | document | `invalid_output` | — | — | — | — | — | — / — | — / — | 3 | 16898.2 |
| `markdown-nested-list` | prose | ok | 3 | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2857.9 |
| `markdown-reply-plan-questions` | prose | ok | 2 | 50.0% / 50.0% | 100.0% / 100.0% | 50.0% / 50.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2367.1 |
| `none-release-notes` | none | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1261.4 |
| `none-rhetorical` | none | ok | 2 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 2 | 3307.3 |
| `plain-paragraph-proposal` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1981.6 |
| `prose-multiline-question` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2411.5 |
| `prose-rhetorical` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1485.6 |
| `selected-option` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1761.6 |
| `standalone-alternatives` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 3091.3 |
| `suggested-complementary-plan` | chat | ok | 2 | 66.7% / 100.0% | 66.7% / 100.0% | 66.7% / 100.0% | 66.7% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2436.3 |
| `tasks-and-exclusions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 2283.0 |

| fixture | recommendations exact / lenient P/R | tasks exact / lenient P/R | recommendation links exact / lenient P/R | alternatives exact / lenient P/R |
| --- | --- | --- | --- | --- |
| `advice-not-decision` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `chat-local-answer` | n/a / n/a / n/a / n/a | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `chat-open-questions` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `code-todo-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `completed-and-incidental` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `direct-advice-request` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `document-adr` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-crlf-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-quoted-answer` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `json-question-answer` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `many-recommendation-links` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 50.0% / 100.0% / 50.0% | n/a / n/a / n/a / n/a |
| `markdown-nested-list` | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-reply-plan-questions` | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-release-notes` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `plain-paragraph-proposal` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-multiline-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `selected-option` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `standalone-alternatives` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% |
| `suggested-complementary-plan` | 66.7% / 100.0% / 66.7% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `tasks-and-exclusions` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |

## Failures

- `markdown-epic003` (document): **invalid_output** after 3 attempt(s) in 16898.2 ms: invalid model output after 3 attempt(s): span "s1": quote "Trigger: a high invalid-output rate, or many parses needing more than one attempt." was not found in segments 19..=19
- `chat-local-answer` (chat): **approximate_span**: expected span "e1" [0..44] "Q: Do we need a migration for the new index?" matched leniently to predicted span [3..44] "Do we need a migration for the new index?" (IoU 0.93) but not exactly
- `chat-local-answer` (chat): **approximate_span**: expected span "e2" [45..92] "A: Yes, add one that drops the old index first." matched leniently to predicted span [48..92] "Yes, add one that drops the old index first." (IoU 0.94) but not exactly
- `chat-local-answer` (chat): **approximate_span**: expected span "e3" [93..123] "Q: Who owns the rollback plan?" matched leniently to predicted span [96..123] "Who owns the rollback plan?" (IoU 0.90) but not exactly
- `email-quoted-answer` (email): **approximate_span**: expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." matched leniently to predicted span [127..158] "Who owns the rollout checklist?" (IoU 0.54) but not exactly
- `email-quoted-answer` (email): **missed_label**: expected question label on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but it was not predicted
- `email-quoted-answer` (email): **unexpected_label**: predicted task label on predicted span [127..158] "Who owns the rollout checklist?" but none was expected
- `email-quoted-answer` (email): **missed_assignee**: expected @ops-team on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but none was predicted
- `email-quoted-answer` (email): **unexpected_assignee**: predicted @ops-team on predicted span [127..158] "Who owns the rollout checklist?" but no assignment was expected
- `json-question-answer` (json): **approximate_span**: expected span "e1" [44..86] "Should the parser retry on invalid output?" matched leniently to predicted span [43..87] "\"Should the parser retry on invalid output?\"" (IoU 0.95) but not exactly
- `json-question-answer` (json): **approximate_span**: expected span "e2" [102..128] "Yes, up to three attempts." matched leniently to predicted span [91..129] "\"answer\": \"Yes, up to three attempts.\"" (IoU 0.68) but not exactly
- `many-recommendation-links` (chat): **missed_recommendation_link**: expected recommendation link expected span "e3" [49..69] "I recommend caching." -> expected span "e2" [26..48] "Should we reduce load?" but it was not predicted
- `many-recommendation-links` (chat): **missed_recommendation_link**: expected recommendation link expected span "e4" [70..89] "I suggest batching." -> expected span "e1" [0..25] "Should we reduce latency?" but it was not predicted
- `markdown-nested-list` (prose): **approximate_span**: expected span "e1" [42..95] "- Measure current latency. Record p95 before changes." matched leniently to predicted span [44..95] "Measure current latency. Record p95 before changes." (IoU 0.96) but not exactly
- `markdown-nested-list` (prose): **approximate_span**: expected span "e2" [98..146] "- Add a bounded cache. Limit it to five minutes." matched leniently to predicted span [100..146] "Add a bounded cache. Limit it to five minutes." (IoU 0.96) but not exactly
- `markdown-nested-list` (prose): **approximate_span**: expected span "e3" [147..181] "- Monitor hit rates after rollout." matched leniently to predicted span [149..181] "Monitor hit rates after rollout." (IoU 0.94) but not exactly
- `markdown-reply-plan-questions` (prose): **approximate_span**: expected span "e1" [19..108] "### Measure latency\n\n- Record p95 before changes.\n- This giv…" matched leniently to predicted span [40..108] "- Record p95 before changes.\n- This gives a baseline for com…" (IoU 0.76) but not exactly
- `markdown-reply-plan-questions` (prose): **approximate_span**: expected span "e2" [110..195] "### Cache responses\n\n- Add a five-minute cache.\n- Keep memor…" matched leniently to predicted span [131..195] "- Add a five-minute cache.\n- Keep memory bounded during roll…" (IoU 0.75) but not exactly
- `none-rhetorical` (none): **unexpected_span**: predicted span [30..49] "Was that not lucky?" matched no expected span
- `none-rhetorical` (none): **unexpected_label**: predicted question label on predicted span [30..49] "Was that not lucky?", but the span was not matched
- `suggested-complementary-plan` (chat): **unexpected_span**: predicted span [48..92] "Do not use a global lock: it blocks workers." matched no expected span
- `suggested-complementary-plan` (chat): **unexpected_label**: predicted recommendation label on predicted span [48..92] "Do not use a global lock: it blocks workers.", but the span was not matched

## Reproduce

```sh
DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast DAFTPROMPT_EVAL_PROMPT_VERSION=scribe-v3 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --fixtures crates/daftprompt-core/fixtures/scribe --output <same-path>
```

