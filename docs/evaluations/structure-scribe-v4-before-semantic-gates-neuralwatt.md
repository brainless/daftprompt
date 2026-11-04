# Scribe evaluation: neuralwatt / qwen3.6-35b-fast

> Development snapshot before the final semantic gates. Epic 003 reached 11/11 exact spans, but plain-text precision regressed; this prompt was not retained.

**Status:** live run completed: 23 of 23 fixture(s) parsed.

- Generated at: `2026-11-04T09:49:50Z`
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
| recommendation, exact | 29 | 30 | 29 | 96.7% | 100.0% |
| recommendation, lenient | 29 | 30 | 29 | 96.7% | 100.0% |
| task, exact | 6 | 11 | 6 | 54.5% | 100.0% |
| task, lenient | 6 | 11 | 6 | 54.5% | 100.0% |
| recommendation link, exact | 6 | 4 | 4 | 100.0% | 66.7% |
| recommendation link, lenient | 6 | 4 | 4 | 100.0% | 66.7% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 58 | 65 | 53 | 81.5% | 91.4% |
| spans, lenient | 58 | 65 | 58 | 89.2% | 100.0% |
| labels, exact | 61 | 69 | 56 | 81.2% | 91.8% |
| labels, lenient | 61 | 69 | 60 | 87.0% | 98.4% |
| answer links, exact | 7 | 8 | 5 | 62.5% | 71.4% |
| answer links, lenient | 7 | 8 | 7 | 87.5% | 100.0% |
| assignees, exact | 4 | 5 | 3 | 60.0% | 75.0% |
| assignees, lenient | 4 | 5 | 3 | 60.0% | 75.0% |

- Invalid output: 0 of 23 fixture(s) (0.0%)
- Other parse failures: 0 of 23 fixture(s)
- Attempts per parse: mean 1.09 over 23 known of 23 parse(s), max 2
- Latency (ms): mean 2750.0, median 2529.0, p95 4795.3, max 6409.4

## Per content type

### plain

Detection accuracy: 18/18 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 13 | 13 | 13 | 100.0% | 100.0% |
| recommendation, lenient | 13 | 13 | 13 | 100.0% | 100.0% |
| task, exact | 6 | 8 | 6 | 75.0% | 100.0% |
| task, lenient | 6 | 8 | 6 | 75.0% | 100.0% |
| recommendation link, exact | 6 | 4 | 4 | 100.0% | 66.7% |
| recommendation link, lenient | 6 | 4 | 4 | 100.0% | 66.7% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 38 | 42 | 35 | 83.3% | 92.1% |
| spans, lenient | 38 | 42 | 38 | 90.5% | 100.0% |
| labels, exact | 41 | 45 | 38 | 84.4% | 92.7% |
| labels, lenient | 41 | 45 | 40 | 88.9% | 97.6% |
| answer links, exact | 6 | 7 | 5 | 71.4% | 83.3% |
| answer links, lenient | 6 | 7 | 6 | 85.7% | 100.0% |
| assignees, exact | 4 | 5 | 3 | 60.0% | 75.0% |
| assignees, lenient | 4 | 5 | 3 | 60.0% | 75.0% |

- Invalid output: 0 of 18 fixture(s) (0.0%)
- Other parse failures: 0 of 18 fixture(s)
- Attempts per parse: mean 1.11 over 18 known of 18 parse(s), max 2
- Latency (ms): mean 2617.9, median 2529.0, p95 3864.2, max 4795.3

### markdown

Detection accuracy: 5/5 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 16 | 17 | 16 | 94.1% | 100.0% |
| recommendation, lenient | 16 | 17 | 16 | 94.1% | 100.0% |
| task, exact | 0 | 3 | 0 | 0.0% | n/a |
| task, lenient | 0 | 3 | 0 | 0.0% | n/a |
| recommendation link, exact | 0 | 0 | 0 | n/a | n/a |
| recommendation link, lenient | 0 | 0 | 0 | n/a | n/a |
| alternative, exact | 0 | 0 | 0 | n/a | n/a |
| alternative, lenient | 0 | 0 | 0 | n/a | n/a |
| spans, exact | 20 | 23 | 18 | 78.3% | 90.0% |
| spans, lenient | 20 | 23 | 20 | 87.0% | 100.0% |
| labels, exact | 20 | 24 | 18 | 75.0% | 90.0% |
| labels, lenient | 20 | 24 | 20 | 83.3% | 100.0% |
| answer links, exact | 1 | 1 | 0 | 0.0% | 0.0% |
| answer links, lenient | 1 | 1 | 1 | 100.0% | 100.0% |
| assignees, exact | 0 | 0 | 0 | n/a | n/a |
| assignees, lenient | 0 | 0 | 0 | n/a | n/a |

- Invalid output: 0 of 5 fixture(s) (0.0%)
- Other parse failures: 0 of 5 fixture(s)
- Attempts per parse: mean 1.00 over 5 known of 5 parse(s), max 1
- Latency (ms): mean 3225.5, median 2653.7, p95 6409.4, max 6409.4

## Per fixture

| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |
| `advice-not-decision` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 3710.6 |
| `chat-local-answer` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2547.0 |
| `chat-open-questions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 2529.0 |
| `code-todo-question` | code | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1714.7 |
| `completed-and-incidental` | chat | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1542.4 |
| `direct-advice-request` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 2 | 3864.2 |
| `document-adr` | document | ok | 5 | 0.0% / 0.0% | 66.7% / 100.0% | 0.0% / 0.0% | 50.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2245.2 |
| `email-crlf-answer` | email | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1860.3 |
| `email-quoted-answer` | email | ok | 5 | 66.7% / 66.7% | 100.0% / 100.0% | 66.7% / 66.7% | 66.7% / 66.7% | 100.0% / 100.0% / 100.0% / 100.0% | 0.0% / 0.0% / 0.0% / 0.0% | 1 | 2524.7 |
| `json-question-answer` | json | ok | 5 | 0.0% / 0.0% | 66.7% / 100.0% | 0.0% / 0.0% | 66.7% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | 0.0% / n/a / 0.0% / n/a | 1 | 2632.0 |
| `many-recommendation-links` | chat | ok | 2 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 2 | 4795.3 |
| `markdown-epic003` | document | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 6409.4 |
| `markdown-nested-list` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2752.8 |
| `markdown-reply-plan-questions` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2066.7 |
| `none-release-notes` | none | ok | 4 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2653.7 |
| `none-rhetorical` | none | ok | 5 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | 1 | 2592.1 |
| `plain-paragraph-proposal` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2206.7 |
| `prose-multiline-question` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1806.5 |
| `prose-rhetorical` | prose | ok | 2 | 50.0% / 100.0% | 50.0% / 100.0% | 50.0% / 100.0% | 50.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2190.4 |
| `selected-option` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2766.8 |
| `standalone-alternatives` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 3622.2 |
| `suggested-complementary-plan` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1704.6 |
| `tasks-and-exclusions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 2513.2 |

| fixture | recommendations exact / lenient P/R | tasks exact / lenient P/R | recommendation links exact / lenient P/R | alternatives exact / lenient P/R |
| --- | --- | --- | --- | --- |
| `advice-not-decision` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `chat-local-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `chat-open-questions` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `code-todo-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `completed-and-incidental` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `direct-advice-request` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `document-adr` | 0.0% / n/a / 0.0% / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-crlf-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-quoted-answer` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `json-question-answer` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `many-recommendation-links` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 50.0% / 100.0% / 50.0% | n/a / n/a / n/a / n/a |
| `markdown-epic003` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-nested-list` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-reply-plan-questions` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-release-notes` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `plain-paragraph-proposal` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-multiline-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `selected-option` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `standalone-alternatives` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% |
| `suggested-complementary-plan` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `tasks-and-exclusions` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |

## Failures

- `document-adr` (document): **approximate_span**: expected span "e1" [95..137] "Should the cache be shared across workers?" matched leniently to predicted span [77..137] "## Open question\n\nShould the cache be shared across workers?" (IoU 0.70) but not exactly
- `document-adr` (document): **approximate_span**: expected span "e2" [150..214] "No. Each worker keeps its own cache; sharing adds a network …" matched leniently to predicted span [139..214] "## Answer\n\nNo. Each worker keeps its own cache; sharing adds…" (IoU 0.85) but not exactly
- `document-adr` (document): **unexpected_span**: predicted span [24..75] "## Decision\n\nWe will cache parsed tokens in memory." matched no expected span
- `document-adr` (document): **unexpected_label**: predicted recommendation label on predicted span [24..75] "## Decision\n\nWe will cache parsed tokens in memory.", but the span was not matched
- `document-adr` (document): **unexpected_label**: predicted task label on predicted span [139..214] "## Answer\n\nNo. Each worker keeps its own cache; sharing adds…" but none was expected
- `email-quoted-answer` (email): **approximate_span**: expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." matched leniently to predicted span [127..158] "Who owns the rollout checklist?" (IoU 0.54) but not exactly
- `email-quoted-answer` (email): **missed_label**: expected question label on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but it was not predicted
- `email-quoted-answer` (email): **unexpected_label**: predicted task label on predicted span [127..158] "Who owns the rollout checklist?" but none was expected
- `email-quoted-answer` (email): **missed_assignee**: expected @ops-team on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but none was predicted
- `email-quoted-answer` (email): **unexpected_assignee**: predicted @ops-team on predicted span [127..158] "Who owns the rollout checklist?" but no assignment was expected
- `json-question-answer` (json): **approximate_span**: expected span "e1" [44..86] "Should the parser retry on invalid output?" matched leniently to predicted span [31..88] "\"question\": \"Should the parser retry on invalid output?\"," (IoU 0.74) but not exactly
- `json-question-answer` (json): **approximate_span**: expected span "e2" [102..128] "Yes, up to three attempts." matched leniently to predicted span [91..130] "\"answer\": \"Yes, up to three attempts.\"," (IoU 0.67) but not exactly
- `json-question-answer` (json): **unexpected_span**: predicted span [133..183] "\"note\": \"@alex raised the retry budget last week.\"" matched no expected span
- `json-question-answer` (json): **unexpected_label**: predicted task label on predicted span [133..183] "\"note\": \"@alex raised the retry budget last week.\"", but the span was not matched
- `json-question-answer` (json): **unexpected_assignee**: predicted @alex on predicted span [133..183] "\"note\": \"@alex raised the retry budget last week.\"", but the span was not matched
- `many-recommendation-links` (chat): **missed_recommendation_link**: expected recommendation link expected span "e3" [49..69] "I recommend caching." -> expected span "e2" [26..48] "Should we reduce load?" but it was not predicted
- `many-recommendation-links` (chat): **missed_recommendation_link**: expected recommendation link expected span "e4" [70..89] "I suggest batching." -> expected span "e1" [0..25] "Should we reduce latency?" but it was not predicted
- `none-release-notes` (none): **unexpected_span**: predicted span [14..52] "- Fix a crash when the cache is empty." matched no expected span
- `none-release-notes` (none): **unexpected_span**: predicted span [53..76] "- Improve log messages." matched no expected span
- `none-release-notes` (none): **unexpected_label**: predicted task label on predicted span [14..52] "- Fix a crash when the cache is empty.", but the span was not matched
- `none-release-notes` (none): **unexpected_label**: predicted task label on predicted span [53..76] "- Improve log messages.", but the span was not matched
- `none-rhetorical` (none): **unexpected_span**: predicted span [30..49] "Was that not lucky?" matched no expected span
- `none-rhetorical` (none): **unexpected_span**: predicted span [50..82] "Thanks to @sam for staying late." matched no expected span
- `none-rhetorical` (none): **unexpected_label**: predicted question label on predicted span [30..49] "Was that not lucky?", but the span was not matched
- `none-rhetorical` (none): **unexpected_label**: predicted answer label on predicted span [50..82] "Thanks to @sam for staying late.", but the span was not matched
- `none-rhetorical` (none): **unexpected_answer_link**: predicted answer link predicted span [50..82] "Thanks to @sam for staying late." -> predicted span [30..49] "Was that not lucky?", but a span was not matched
- `prose-rhetorical` (prose): **unexpected_span**: predicted span [63..97] "Who cares about the old dashboard?" matched no expected span
- `prose-rhetorical` (prose): **unexpected_label**: predicted question label on predicted span [63..97] "Who cares about the old dashboard?", but the span was not matched

## Reproduce

```sh
DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --fixtures crates/daftprompt-core/fixtures/scribe --output <same-path>
```

