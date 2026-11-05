# Scribe evaluation: neuralwatt / qwen3.6-35b-fast

> Development snapshot before prompt tuning; this report does not describe the final prompt.

**Status:** live run completed: 23 of 23 fixture(s) parsed.

- Generated at: `2026-11-04T09:43:24Z`
- Provider / model: `neuralwatt` / `qwen3.6-35b-fast`
- Prompt version: `scribe-v4+plain-v1 / scribe-v4+markdown-v1`
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
| recommendation, exact | 29 | 21 | 18 | 85.7% | 62.1% |
| recommendation, lenient | 29 | 21 | 18 | 85.7% | 62.1% |
| task, exact | 6 | 8 | 5 | 62.5% | 83.3% |
| task, lenient | 6 | 8 | 5 | 62.5% | 83.3% |
| recommendation link, exact | 6 | 5 | 5 | 100.0% | 83.3% |
| recommendation link, lenient | 6 | 5 | 5 | 100.0% | 83.3% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 58 | 50 | 41 | 82.0% | 70.7% |
| spans, lenient | 58 | 50 | 45 | 90.0% | 77.6% |
| labels, exact | 61 | 56 | 44 | 78.6% | 72.1% |
| labels, lenient | 61 | 56 | 48 | 85.7% | 78.7% |
| answer links, exact | 7 | 7 | 6 | 85.7% | 85.7% |
| answer links, lenient | 7 | 7 | 7 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 2 | 50.0% | 50.0% |
| assignees, lenient | 4 | 4 | 3 | 75.0% | 75.0% |

- Invalid output: 0 of 23 fixture(s) (0.0%)
- Other parse failures: 0 of 23 fixture(s)
- Attempts per parse: mean 1.30 over 23 known of 23 parse(s), max 3
- Latency (ms): mean 2976.2, median 2082.1, p95 7003.2, max 14409.4

## Per content type

### plain

Detection accuracy: 18/18 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 13 | 12 | 10 | 83.3% | 76.9% |
| recommendation, lenient | 13 | 12 | 10 | 83.3% | 76.9% |
| task, exact | 6 | 8 | 5 | 62.5% | 83.3% |
| task, lenient | 6 | 8 | 5 | 62.5% | 83.3% |
| recommendation link, exact | 6 | 5 | 5 | 100.0% | 83.3% |
| recommendation link, lenient | 6 | 5 | 5 | 100.0% | 83.3% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 38 | 37 | 31 | 83.8% | 81.6% |
| spans, lenient | 38 | 37 | 33 | 89.2% | 86.8% |
| labels, exact | 41 | 43 | 34 | 79.1% | 82.9% |
| labels, lenient | 41 | 43 | 36 | 83.7% | 87.8% |
| answer links, exact | 6 | 6 | 5 | 83.3% | 83.3% |
| answer links, lenient | 6 | 6 | 6 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 2 | 50.0% | 50.0% |
| assignees, lenient | 4 | 4 | 3 | 75.0% | 75.0% |

- Invalid output: 0 of 18 fixture(s) (0.0%)
- Other parse failures: 0 of 18 fixture(s)
- Attempts per parse: mean 1.28 over 18 known of 18 parse(s), max 3
- Latency (ms): mean 2543.1, median 2082.1, p95 4334.7, max 7003.2

### markdown

Detection accuracy: 5/5 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 16 | 9 | 8 | 88.9% | 50.0% |
| recommendation, lenient | 16 | 9 | 8 | 88.9% | 50.0% |
| task, exact | 0 | 0 | 0 | n/a | n/a |
| task, lenient | 0 | 0 | 0 | n/a | n/a |
| recommendation link, exact | 0 | 0 | 0 | n/a | n/a |
| recommendation link, lenient | 0 | 0 | 0 | n/a | n/a |
| alternative, exact | 0 | 0 | 0 | n/a | n/a |
| alternative, lenient | 0 | 0 | 0 | n/a | n/a |
| spans, exact | 20 | 13 | 10 | 76.9% | 50.0% |
| spans, lenient | 20 | 13 | 12 | 92.3% | 60.0% |
| labels, exact | 20 | 13 | 10 | 76.9% | 50.0% |
| labels, lenient | 20 | 13 | 12 | 92.3% | 60.0% |
| answer links, exact | 1 | 1 | 1 | 100.0% | 100.0% |
| answer links, lenient | 1 | 1 | 1 | 100.0% | 100.0% |
| assignees, exact | 0 | 0 | 0 | n/a | n/a |
| assignees, lenient | 0 | 0 | 0 | n/a | n/a |

- Invalid output: 0 of 5 fixture(s) (0.0%)
- Other parse failures: 0 of 5 fixture(s)
- Attempts per parse: mean 1.40 over 5 known of 5 parse(s), max 3
- Latency (ms): mean 4535.3, median 2165.5, p95 14409.4, max 14409.4

## Per fixture

| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |
| `advice-not-decision` | chat | ok | 13 | 0.0% / 0.0% | 0.0% / 0.0% | 0.0% / 0.0% | 0.0% / 0.0% | n/a / n/a / n/a / n/a | 0.0% / 0.0% / 0.0% / 0.0% | 1 | 2470.6 |
| `chat-local-answer` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2817.6 |
| `chat-open-questions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 1615.3 |
| `code-todo-question` | code | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1934.4 |
| `completed-and-incidental` | chat | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1313.9 |
| `direct-advice-request` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2082.1 |
| `document-adr` | document | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2831.3 |
| `email-crlf-answer` | email | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2266.0 |
| `email-quoted-answer` | email | ok | 3 | 33.3% / 33.3% | 100.0% / 100.0% | 25.0% / 33.3% | 75.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | 2 | 4334.7 |
| `json-question-answer` | json | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1786.4 |
| `many-recommendation-links` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 3 | 7003.2 |
| `markdown-epic003` | document | ok | 12 | 100.0% / 45.5% | 100.0% / 45.5% | 100.0% / 45.5% | 100.0% / 45.5% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 3 | 14409.4 |
| `markdown-nested-list` | prose | ok | 6 | 50.0% / 33.3% | 50.0% / 33.3% | 50.0% / 33.3% | 50.0% / 33.3% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1932.5 |
| `markdown-reply-plan-questions` | prose | ok | 2 | 50.0% / 50.0% | 100.0% / 100.0% | 50.0% / 50.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2165.5 |
| `none-release-notes` | none | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1337.8 |
| `none-rhetorical` | none | ok | 2 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 3 | 4253.5 |
| `plain-paragraph-proposal` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1878.4 |
| `prose-multiline-question` | prose | ok | 2 | 50.0% / 100.0% | 50.0% / 100.0% | 50.0% / 100.0% | 50.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1810.4 |
| `prose-rhetorical` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1773.4 |
| `selected-option` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1694.3 |
| `standalone-alternatives` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2489.6 |
| `suggested-complementary-plan` | chat | ok | 6 | 0.0% / 0.0% | 0.0% / 0.0% | 0.0% / 0.0% | 0.0% / 0.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1752.9 |
| `tasks-and-exclusions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 2499.3 |

| fixture | recommendations exact / lenient P/R | tasks exact / lenient P/R | recommendation links exact / lenient P/R | alternatives exact / lenient P/R |
| --- | --- | --- | --- | --- |
| `advice-not-decision` | 0.0% / 0.0% / 0.0% / 0.0% | 0.0% / 0.0% / 0.0% / 0.0% | n/a / 0.0% / n/a / 0.0% | n/a / n/a / n/a / n/a |
| `chat-local-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `chat-open-questions` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `code-todo-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `completed-and-incidental` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `direct-advice-request` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `document-adr` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-crlf-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-quoted-answer` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `json-question-answer` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `many-recommendation-links` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `markdown-epic003` | 100.0% / 45.5% / 100.0% / 45.5% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-nested-list` | 50.0% / 33.3% / 50.0% / 33.3% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-reply-plan-questions` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-release-notes` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `plain-paragraph-proposal` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-multiline-question` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `selected-option` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `standalone-alternatives` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% |
| `suggested-complementary-plan` | 0.0% / 0.0% / 0.0% / 0.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `tasks-and-exclusions` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |

## Failures

- `advice-not-decision` (chat): **missed_span**: expected span "e1" [0..23] "Should we cache tokens?" was not predicted by any span
- `advice-not-decision` (chat): **missed_span**: expected span "e2" [24..54] "I suggest a five-minute cache." was not predicted by any span
- `advice-not-decision` (chat): **missed_span**: expected span "e3" [55..89] "@alex, please benchmark the cache." was not predicted by any span
- `advice-not-decision` (chat): **unexpected_span**: predicted span [0..89] "Should we cache tokens?\nI suggest a five-minute cache.\n@alex…" matched no expected span
- `advice-not-decision` (chat): **missed_label**: expected question label on expected span "e1" [0..23] "Should we cache tokens?", but the span was not matched
- `advice-not-decision` (chat): **missed_label**: expected recommendation label on expected span "e2" [24..54] "I suggest a five-minute cache.", but the span was not matched
- `advice-not-decision` (chat): **missed_label**: expected task label on expected span "e3" [55..89] "@alex, please benchmark the cache.", but the span was not matched
- `advice-not-decision` (chat): **unexpected_label**: predicted question label on predicted span [0..89] "Should we cache tokens?\nI suggest a five-minute cache.\n@alex…", but the span was not matched
- `advice-not-decision` (chat): **unexpected_label**: predicted recommendation label on predicted span [0..89] "Should we cache tokens?\nI suggest a five-minute cache.\n@alex…", but the span was not matched
- `advice-not-decision` (chat): **unexpected_label**: predicted task label on predicted span [0..89] "Should we cache tokens?\nI suggest a five-minute cache.\n@alex…", but the span was not matched
- `advice-not-decision` (chat): **missed_assignee**: expected @alex on expected span "e3" [55..89] "@alex, please benchmark the cache.", but the span was not matched
- `advice-not-decision` (chat): **unexpected_assignee**: predicted @alex on predicted span [0..89] "Should we cache tokens?\nI suggest a five-minute cache.\n@alex…", but the span was not matched
- `advice-not-decision` (chat): **missed_recommendation_link**: expected recommendation link expected span "e2" [24..54] "I suggest a five-minute cache." -> expected span "e1" [0..23] "Should we cache tokens?", but a span end was not matched
- `email-quoted-answer` (email): **approximate_span**: expected span "e1" [42..90] "Should we enable the cache in staging this week?" matched leniently to predicted span [40..90] "> Should we enable the cache in staging this week?" (IoU 0.96) but not exactly
- `email-quoted-answer` (email): **approximate_span**: expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." matched leniently to predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." (IoU 0.97) but not exactly
- `email-quoted-answer` (email): **unexpected_label**: predicted task label on predicted span [92..123] "Yes, enable it behind the flag." but none was expected
- `markdown-epic003` (document): **missed_span**: expected span "e1" [864..1316] "### Retry with corrective feedback\n\n- **Trigger:** a high in…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e2" [1318..1812] "### Sub-line segmentation\n\n- **Trigger:** poor span accuracy…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e3" [1814..2094] "### Overlapping and nested spans\n\n- **Trigger:** fixtures or…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e4" [2096..2341] "### Unreferenced spans\n\n- **Trigger:** a meaningful share of…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e5" [2343..2554] "### Native JSON-schema output\n\n- **Trigger:** the chosen pro…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e6" [2556..2858] "### Input limits and chunking\n\n- **Trigger:** the default 32…" was not predicted by any span
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e1" [864..1316] "### Retry with corrective feedback\n\n- **Trigger:** a high in…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e2" [1318..1812] "### Sub-line segmentation\n\n- **Trigger:** poor span accuracy…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e3" [1814..2094] "### Overlapping and nested spans\n\n- **Trigger:** fixtures or…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e4" [2096..2341] "### Unreferenced spans\n\n- **Trigger:** a meaningful share of…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e5" [2343..2554] "### Native JSON-schema output\n\n- **Trigger:** the chosen pro…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e6" [2556..2858] "### Input limits and chunking\n\n- **Trigger:** the default 32…", but the span was not matched
- `markdown-nested-list` (prose): **missed_span**: expected span "e1" [42..95] "- Measure current latency. Record p95 before changes." was not predicted by any span
- `markdown-nested-list` (prose): **missed_span**: expected span "e2" [98..146] "- Add a bounded cache. Limit it to five minutes." was not predicted by any span
- `markdown-nested-list` (prose): **unexpected_span**: predicted span [17..146] "- Prepare the rollout:\n  - Measure current latency. Record p…" matched no expected span
- `markdown-nested-list` (prose): **missed_label**: expected recommendation label on expected span "e1" [42..95] "- Measure current latency. Record p95 before changes.", but the span was not matched
- `markdown-nested-list` (prose): **missed_label**: expected recommendation label on expected span "e2" [98..146] "- Add a bounded cache. Limit it to five minutes.", but the span was not matched
- `markdown-nested-list` (prose): **unexpected_label**: predicted recommendation label on predicted span [17..146] "- Prepare the rollout:\n  - Measure current latency. Record p…", but the span was not matched
- `markdown-reply-plan-questions` (prose): **approximate_span**: expected span "e3" [211..239] "- What is the memory budget?" matched leniently to predicted span [213..239] "What is the memory budget?" (IoU 0.93) but not exactly
- `markdown-reply-plan-questions` (prose): **approximate_span**: expected span "e4" [240..263] "- Who owns the rollout?" matched leniently to predicted span [242..263] "Who owns the rollout?" (IoU 0.91) but not exactly
- `none-rhetorical` (none): **unexpected_span**: predicted span [30..49] "Was that not lucky?" matched no expected span
- `none-rhetorical` (none): **unexpected_label**: predicted question label on predicted span [30..49] "Was that not lucky?", but the span was not matched
- `prose-multiline-question` (prose): **unexpected_span**: predicted span [64..119] "It would need an owner before we can schedule the work." matched no expected span
- `prose-multiline-question` (prose): **unexpected_label**: predicted task label on predicted span [64..119] "It would need an owner before we can schedule the work.", but the span was not matched
- `suggested-complementary-plan` (chat): **missed_span**: expected span "e1" [16..28] "Add metrics." was not predicted by any span
- `suggested-complementary-plan` (chat): **missed_span**: expected span "e2" [29..47] "Benchmark latency." was not predicted by any span
- `suggested-complementary-plan` (chat): **unexpected_span**: predicted span [0..92] "Suggested plan:\nAdd metrics.\nBenchmark latency.\nDo not use a…" matched no expected span
- `suggested-complementary-plan` (chat): **missed_label**: expected recommendation label on expected span "e1" [16..28] "Add metrics.", but the span was not matched
- `suggested-complementary-plan` (chat): **missed_label**: expected recommendation label on expected span "e2" [29..47] "Benchmark latency.", but the span was not matched
- `suggested-complementary-plan` (chat): **unexpected_label**: predicted recommendation label on predicted span [0..92] "Suggested plan:\nAdd metrics.\nBenchmark latency.\nDo not use a…", but the span was not matched

## Reproduce

```sh
DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast DAFTPROMPT_EVAL_PROMPT_VERSION=scribe-v4 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --fixtures crates/daftprompt-core/fixtures/scribe --output <same-path>
```

