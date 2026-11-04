# Scribe evaluation: neuralwatt / qwen3.6-35b-fast

> Rejected development experiment: an extra final coverage paragraph regressed extraction. Its prompt change was reverted; this report does not describe the final prompt.

**Status:** live run completed: 23 of 23 fixture(s) parsed.

- Generated at: `2026-11-04T09:55:36Z`
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
| recommendation, exact | 29 | 38 | 21 | 55.3% | 72.4% |
| recommendation, lenient | 29 | 38 | 23 | 60.5% | 79.3% |
| task, exact | 6 | 6 | 5 | 83.3% | 83.3% |
| task, lenient | 6 | 6 | 5 | 83.3% | 83.3% |
| recommendation link, exact | 6 | 5 | 5 | 100.0% | 83.3% |
| recommendation link, lenient | 6 | 5 | 5 | 100.0% | 83.3% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 58 | 67 | 42 | 62.7% | 72.4% |
| spans, lenient | 58 | 67 | 49 | 73.1% | 84.5% |
| labels, exact | 61 | 70 | 45 | 64.3% | 73.8% |
| labels, lenient | 61 | 70 | 52 | 74.3% | 85.2% |
| answer links, exact | 7 | 7 | 5 | 71.4% | 71.4% |
| answer links, lenient | 7 | 7 | 7 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 1 | 25.0% | 25.0% |
| assignees, lenient | 4 | 4 | 2 | 50.0% | 50.0% |

- Invalid output: 0 of 23 fixture(s) (0.0%)
- Other parse failures: 0 of 23 fixture(s)
- Attempts per parse: mean 1.17 over 23 known of 23 parse(s), max 2
- Latency (ms): mean 2929.9, median 2361.9, p95 5390.9, max 8100.7

## Per content type

### plain

Detection accuracy: 18/18 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 13 | 14 | 12 | 85.7% | 92.3% |
| recommendation, lenient | 13 | 14 | 12 | 85.7% | 92.3% |
| task, exact | 6 | 6 | 5 | 83.3% | 83.3% |
| task, lenient | 6 | 6 | 5 | 83.3% | 83.3% |
| recommendation link, exact | 6 | 5 | 5 | 100.0% | 83.3% |
| recommendation link, lenient | 6 | 5 | 5 | 100.0% | 83.3% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 38 | 39 | 29 | 74.4% | 76.3% |
| spans, lenient | 38 | 39 | 34 | 87.2% | 89.5% |
| labels, exact | 41 | 42 | 32 | 76.2% | 78.0% |
| labels, lenient | 41 | 42 | 37 | 88.1% | 90.2% |
| answer links, exact | 6 | 6 | 4 | 66.7% | 66.7% |
| answer links, lenient | 6 | 6 | 6 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 1 | 25.0% | 25.0% |
| assignees, lenient | 4 | 4 | 2 | 50.0% | 50.0% |

- Invalid output: 0 of 18 fixture(s) (0.0%)
- Other parse failures: 0 of 18 fixture(s)
- Attempts per parse: mean 1.17 over 18 known of 18 parse(s), max 2
- Latency (ms): mean 2693.8, median 2563.3, p95 4867.6, max 5390.9

### markdown

Detection accuracy: 5/5 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 16 | 24 | 9 | 37.5% | 56.2% |
| recommendation, lenient | 16 | 24 | 11 | 45.8% | 68.8% |
| task, exact | 0 | 0 | 0 | n/a | n/a |
| task, lenient | 0 | 0 | 0 | n/a | n/a |
| recommendation link, exact | 0 | 0 | 0 | n/a | n/a |
| recommendation link, lenient | 0 | 0 | 0 | n/a | n/a |
| alternative, exact | 0 | 0 | 0 | n/a | n/a |
| alternative, lenient | 0 | 0 | 0 | n/a | n/a |
| spans, exact | 20 | 28 | 13 | 46.4% | 65.0% |
| spans, lenient | 20 | 28 | 15 | 53.6% | 75.0% |
| labels, exact | 20 | 28 | 13 | 46.4% | 65.0% |
| labels, lenient | 20 | 28 | 15 | 53.6% | 75.0% |
| answer links, exact | 1 | 1 | 1 | 100.0% | 100.0% |
| answer links, lenient | 1 | 1 | 1 | 100.0% | 100.0% |
| assignees, exact | 0 | 0 | 0 | n/a | n/a |
| assignees, lenient | 0 | 0 | 0 | n/a | n/a |

- Invalid output: 0 of 5 fixture(s) (0.0%)
- Other parse failures: 0 of 5 fixture(s)
- Attempts per parse: mean 1.20 over 5 known of 5 parse(s), max 2
- Latency (ms): mean 3779.7, median 2361.9, p95 8100.7, max 8100.7

## Per fixture

| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |
| `advice-not-decision` | chat | ok | 11 | 0.0% / 0.0% | 0.0% / 0.0% | 0.0% / 0.0% | 0.0% / 0.0% | n/a / n/a / n/a / n/a | 0.0% / 0.0% / 0.0% / 0.0% | 1 | 2563.3 |
| `chat-local-answer` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 4010.8 |
| `chat-open-questions` | chat | ok | 7 | 0.0% / 0.0% | 50.0% / 50.0% | 0.0% / 0.0% | 50.0% / 50.0% | n/a / n/a / n/a / n/a | 0.0% / 0.0% / 0.0% / 0.0% | 1 | 1609.5 |
| `code-todo-question` | code | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1611.6 |
| `completed-and-incidental` | chat | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1324.8 |
| `direct-advice-request` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 2 | 4217.0 |
| `document-adr` | document | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2077.3 |
| `email-crlf-answer` | email | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2256.5 |
| `email-quoted-answer` | email | ok | 2 | 33.3% / 33.3% | 100.0% / 100.0% | 33.3% / 33.3% | 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | 1 | 2661.1 |
| `json-question-answer` | json | ok | 2 | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 2 | 4867.6 |
| `many-recommendation-links` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2588.6 |
| `markdown-epic003` | document | ok | 38 | 21.1% / 36.4% | 31.6% / 54.5% | 21.1% / 36.4% | 31.6% / 54.5% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 8100.7 |
| `markdown-nested-list` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2126.5 |
| `markdown-reply-plan-questions` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2361.9 |
| `none-release-notes` | none | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 2 | 4232.2 |
| `none-rhetorical` | none | ok | 2 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1705.0 |
| `plain-paragraph-proposal` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2716.1 |
| `prose-multiline-question` | prose | ok | 2 | 50.0% / 100.0% | 50.0% / 100.0% | 50.0% / 100.0% | 50.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2040.2 |
| `prose-rhetorical` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2016.9 |
| `selected-option` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2037.3 |
| `standalone-alternatives` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 2 | 5390.9 |
| `suggested-complementary-plan` | chat | ok | 2 | 66.7% / 100.0% | 66.7% / 100.0% | 66.7% / 100.0% | 66.7% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2258.7 |
| `tasks-and-exclusions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 2612.7 |

| fixture | recommendations exact / lenient P/R | tasks exact / lenient P/R | recommendation links exact / lenient P/R | alternatives exact / lenient P/R |
| --- | --- | --- | --- | --- |
| `advice-not-decision` | n/a / 0.0% / n/a / 0.0% | n/a / 0.0% / n/a / 0.0% | n/a / 0.0% / n/a / 0.0% | n/a / n/a / n/a / n/a |
| `chat-local-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `chat-open-questions` | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `code-todo-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `completed-and-incidental` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `direct-advice-request` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `document-adr` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-crlf-answer` | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `email-quoted-answer` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `json-question-answer` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `many-recommendation-links` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `markdown-epic003` | 21.1% / 36.4% / 31.6% / 54.5% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-nested-list` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-reply-plan-questions` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-release-notes` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `plain-paragraph-proposal` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-multiline-question` | n/a / n/a / n/a / n/a | 0.0% / n/a / 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `selected-option` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `standalone-alternatives` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% |
| `suggested-complementary-plan` | 66.7% / 100.0% / 66.7% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
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
- `advice-not-decision` (chat): **missed_assignee**: expected @alex on expected span "e3" [55..89] "@alex, please benchmark the cache.", but the span was not matched
- `advice-not-decision` (chat): **unexpected_assignee**: predicted @alex on predicted span [0..89] "Should we cache tokens?\nI suggest a five-minute cache.\n@alex…", but the span was not matched
- `advice-not-decision` (chat): **missed_recommendation_link**: expected recommendation link expected span "e2" [24..54] "I suggest a five-minute cache." -> expected span "e1" [0..23] "Should we cache tokens?", but a span end was not matched
- `chat-open-questions` (chat): **missed_span**: expected span "e1" [31..69] "@mira should we cache tokens in Redis?" was not predicted by any span
- `chat-open-questions` (chat): **approximate_span**: expected span "e2" [70..115] "What is the current rate limit for /v1/parse?" matched leniently to predicted span [31..115] "@mira should we cache tokens in Redis?\nWhat is the current r…" (IoU 0.54) but not exactly
- `chat-open-questions` (chat): **unexpected_span**: predicted span [0..29] "Next steps for token caching." matched no expected span
- `chat-open-questions` (chat): **missed_label**: expected question label on expected span "e1" [31..69] "@mira should we cache tokens in Redis?", but the span was not matched
- `chat-open-questions` (chat): **unexpected_label**: predicted recommendation label on predicted span [0..29] "Next steps for token caching.", but the span was not matched
- `chat-open-questions` (chat): **missed_assignee**: expected @mira on expected span "e1" [31..69] "@mira should we cache tokens in Redis?", but the span was not matched
- `chat-open-questions` (chat): **unexpected_assignee**: predicted @mira on predicted span [31..115] "@mira should we cache tokens in Redis?\nWhat is the current r…" but no assignment was expected
- `email-quoted-answer` (email): **approximate_span**: expected span "e1" [42..90] "Should we enable the cache in staging this week?" matched leniently to predicted span [40..90] "> Should we enable the cache in staging this week?" (IoU 0.96) but not exactly
- `email-quoted-answer` (email): **approximate_span**: expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." matched leniently to predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." (IoU 0.97) but not exactly
- `json-question-answer` (json): **approximate_span**: expected span "e1" [44..86] "Should the parser retry on invalid output?" matched leniently to predicted span [31..87] "\"question\": \"Should the parser retry on invalid output?\"" (IoU 0.75) but not exactly
- `json-question-answer` (json): **approximate_span**: expected span "e2" [102..128] "Yes, up to three attempts." matched leniently to predicted span [91..129] "\"answer\": \"Yes, up to three attempts.\"" (IoU 0.68) but not exactly
- `markdown-epic003` (document): **approximate_span**: expected span "e1" [864..1316] "### Retry with corrective feedback\n\n- **Trigger:** a high in…" matched leniently to predicted span [989..1219] "- Epic 001 retries validation failures from scratch. Instead…" (IoU 0.51) but not exactly
- `markdown-epic003` (document): **missed_span**: expected span "e2" [1318..1812] "### Sub-line segmentation\n\n- **Trigger:** poor span accuracy…" was not predicted by any span
- `markdown-epic003` (document): **approximate_span**: expected span "e3" [1814..2094] "### Overlapping and nested spans\n\n- **Trigger:** fixtures or…" matched leniently to predicted span [1848..1988] "- **Trigger:** fixtures or real use show annotations that ne…" (IoU 0.50) but not exactly
- `markdown-epic003` (document): **missed_span**: expected span "e4" [2096..2341] "### Unreferenced spans\n\n- **Trigger:** a meaningful share of…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e5" [2343..2554] "### Native JSON-schema output\n\n- **Trigger:** the chosen pro…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e6" [2556..2858] "### Input limits and chunking\n\n- **Trigger:** the default 32…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e7" [2860..3060] "### Second provider or model\n\n- **Trigger:** the first model…" was not predicted by any span
- `markdown-epic003` (document): **unexpected_span**: predicted span [900..988] "- **Trigger:** a high invalid-output rate, or many parses ne…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [1220..1316] "- Compare invalid-output rate, attempts per parse, and laten…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [1345..1511] "- **Trigger:** poor span accuracy on long lines, such as unw…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [1512..1654] "- Split long lines further (for example, by sentence) and pr…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [1989..2094] "- Relax the no-overlap rule, and define how the GUI renders …" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2120..2222] "- **Trigger:** a meaningful share of invalid outputs is caus…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2223..2341] "- Drop unreferenced spans instead of rejecting the whole out…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2374..2456] "- **Trigger:** the chosen provider or model handles the `sub…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2457..2554] "- Evaluate Rig's native structured-output mode against the o…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2587..2723] "- **Trigger:** the default 32 KiB / 1,000-segment limit reje…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2724..2858] "- Tune the default per model. If inputs routinely exceed the…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2890..2970] "- **Trigger:** the first model's baseline quality or latency…" matched no expected span
- `markdown-epic003` (document): **unexpected_span**: predicted span [2971..3060] "- Run the same fixture evaluation with another provider or m…" matched no expected span
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e2" [1318..1812] "### Sub-line segmentation\n\n- **Trigger:** poor span accuracy…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e4" [2096..2341] "### Unreferenced spans\n\n- **Trigger:** a meaningful share of…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e5" [2343..2554] "### Native JSON-schema output\n\n- **Trigger:** the chosen pro…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e6" [2556..2858] "### Input limits and chunking\n\n- **Trigger:** the default 32…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e7" [2860..3060] "### Second provider or model\n\n- **Trigger:** the first model…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [900..988] "- **Trigger:** a high invalid-output rate, or many parses ne…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [1220..1316] "- Compare invalid-output rate, attempts per parse, and laten…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [1345..1511] "- **Trigger:** poor span accuracy on long lines, such as unw…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [1512..1654] "- Split long lines further (for example, by sentence) and pr…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [1989..2094] "- Relax the no-overlap rule, and define how the GUI renders …", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2120..2222] "- **Trigger:** a meaningful share of invalid outputs is caus…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2223..2341] "- Drop unreferenced spans instead of rejecting the whole out…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2374..2456] "- **Trigger:** the chosen provider or model handles the `sub…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2457..2554] "- Evaluate Rig's native structured-output mode against the o…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2587..2723] "- **Trigger:** the default 32 KiB / 1,000-segment limit reje…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2724..2858] "- Tune the default per model. If inputs routinely exceed the…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2890..2970] "- **Trigger:** the first model's baseline quality or latency…", but the span was not matched
- `markdown-epic003` (document): **unexpected_label**: predicted recommendation label on predicted span [2971..3060] "- Run the same fixture evaluation with another provider or m…", but the span was not matched
- `none-rhetorical` (none): **unexpected_span**: predicted span [30..49] "Was that not lucky?" matched no expected span
- `none-rhetorical` (none): **unexpected_label**: predicted question label on predicted span [30..49] "Was that not lucky?", but the span was not matched
- `prose-multiline-question` (prose): **unexpected_span**: predicted span [64..119] "It would need an owner before we can schedule the work." matched no expected span
- `prose-multiline-question` (prose): **unexpected_label**: predicted task label on predicted span [64..119] "It would need an owner before we can schedule the work.", but the span was not matched
- `suggested-complementary-plan` (chat): **unexpected_span**: predicted span [48..92] "Do not use a global lock: it blocks workers." matched no expected span
- `suggested-complementary-plan` (chat): **unexpected_label**: predicted recommendation label on predicted span [48..92] "Do not use a global lock: it blocks workers.", but the span was not matched

## Reproduce

```sh
DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast DAFTPROMPT_EVAL_PROMPT_VERSION=scribe-v4 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --fixtures crates/daftprompt-core/fixtures/scribe --output <same-path>
```

