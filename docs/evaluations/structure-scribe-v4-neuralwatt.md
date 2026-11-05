# Scribe evaluation: neuralwatt / qwen3.6-35b-fast

**Status:** live run completed: 23 of 23 fixture(s) parsed.

- Generated at: `2026-11-04T09:52:41Z`
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
| recommendation, exact | 29 | 26 | 26 | 100.0% | 89.7% |
| recommendation, lenient | 29 | 26 | 26 | 100.0% | 89.7% |
| task, exact | 6 | 7 | 6 | 85.7% | 100.0% |
| task, lenient | 6 | 7 | 6 | 85.7% | 100.0% |
| recommendation link, exact | 6 | 6 | 6 | 100.0% | 100.0% |
| recommendation link, lenient | 6 | 6 | 6 | 100.0% | 100.0% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 58 | 56 | 51 | 91.1% | 87.9% |
| spans, lenient | 58 | 56 | 55 | 98.2% | 94.8% |
| labels, exact | 61 | 59 | 54 | 91.5% | 88.5% |
| labels, lenient | 61 | 59 | 57 | 96.6% | 93.4% |
| answer links, exact | 7 | 7 | 5 | 71.4% | 71.4% |
| answer links, lenient | 7 | 7 | 7 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 3 | 75.0% | 75.0% |
| assignees, lenient | 4 | 4 | 3 | 75.0% | 75.0% |

- Invalid output: 0 of 23 fixture(s) (0.0%)
- Other parse failures: 0 of 23 fixture(s)
- Attempts per parse: mean 1.00 over 23 known of 23 parse(s), max 1
- Latency (ms): mean 2317.4, median 2243.1, p95 3339.7, max 3515.4

## Per content type

### plain

Detection accuracy: 18/18 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 13 | 13 | 13 | 100.0% | 100.0% |
| recommendation, lenient | 13 | 13 | 13 | 100.0% | 100.0% |
| task, exact | 6 | 7 | 6 | 85.7% | 100.0% |
| task, lenient | 6 | 7 | 6 | 85.7% | 100.0% |
| recommendation link, exact | 6 | 6 | 6 | 100.0% | 100.0% |
| recommendation link, lenient | 6 | 6 | 6 | 100.0% | 100.0% |
| alternative, exact | 3 | 3 | 3 | 100.0% | 100.0% |
| alternative, lenient | 3 | 3 | 3 | 100.0% | 100.0% |
| spans, exact | 38 | 39 | 34 | 87.2% | 89.5% |
| spans, lenient | 38 | 39 | 38 | 97.4% | 100.0% |
| labels, exact | 41 | 42 | 37 | 88.1% | 90.2% |
| labels, lenient | 41 | 42 | 40 | 95.2% | 97.6% |
| answer links, exact | 6 | 6 | 4 | 66.7% | 66.7% |
| answer links, lenient | 6 | 6 | 6 | 100.0% | 100.0% |
| assignees, exact | 4 | 4 | 3 | 75.0% | 75.0% |
| assignees, lenient | 4 | 4 | 3 | 75.0% | 75.0% |

- Invalid output: 0 of 18 fixture(s) (0.0%)
- Other parse failures: 0 of 18 fixture(s)
- Attempts per parse: mean 1.00 over 18 known of 18 parse(s), max 1
- Latency (ms): mean 2344.4, median 2309.5, p95 3339.7, max 3515.4

### markdown

Detection accuracy: 5/5 (100.0%)


| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| recommendation, exact | 16 | 13 | 13 | 100.0% | 81.2% |
| recommendation, lenient | 16 | 13 | 13 | 100.0% | 81.2% |
| task, exact | 0 | 0 | 0 | n/a | n/a |
| task, lenient | 0 | 0 | 0 | n/a | n/a |
| recommendation link, exact | 0 | 0 | 0 | n/a | n/a |
| recommendation link, lenient | 0 | 0 | 0 | n/a | n/a |
| alternative, exact | 0 | 0 | 0 | n/a | n/a |
| alternative, lenient | 0 | 0 | 0 | n/a | n/a |
| spans, exact | 20 | 17 | 17 | 100.0% | 85.0% |
| spans, lenient | 20 | 17 | 17 | 100.0% | 85.0% |
| labels, exact | 20 | 17 | 17 | 100.0% | 85.0% |
| labels, lenient | 20 | 17 | 17 | 100.0% | 85.0% |
| answer links, exact | 1 | 1 | 1 | 100.0% | 100.0% |
| answer links, lenient | 1 | 1 | 1 | 100.0% | 100.0% |
| assignees, exact | 0 | 0 | 0 | n/a | n/a |
| assignees, lenient | 0 | 0 | 0 | n/a | n/a |

- Invalid output: 0 of 5 fixture(s) (0.0%)
- Other parse failures: 0 of 5 fixture(s)
- Attempts per parse: mean 1.00 over 5 known of 5 parse(s), max 1
- Latency (ms): mean 2220.5, median 2243.1, p95 2919.9, max 2919.9

## Per fixture

| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |
| `advice-not-decision` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 3305.0 |
| `chat-local-answer` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2956.6 |
| `chat-open-questions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 1735.9 |
| `code-todo-question` | code | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1394.8 |
| `completed-and-incidental` | chat | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1230.7 |
| `direct-advice-request` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2150.8 |
| `document-adr` | document | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2243.1 |
| `email-crlf-answer` | email | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2127.2 |
| `email-quoted-answer` | email | ok | 6 | 33.3% / 33.3% | 100.0% / 100.0% | 33.3% / 33.3% | 66.7% / 66.7% | 0.0% / 0.0% / 100.0% / 100.0% | 0.0% / 0.0% / 0.0% / 0.0% | 1 | 2405.1 |
| `json-question-answer` | json | ok | 2 | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 3515.4 |
| `many-recommendation-links` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 3119.1 |
| `markdown-epic003` | document | ok | 6 | 100.0% / 72.7% | 100.0% / 72.7% | 100.0% / 72.7% | 100.0% / 72.7% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2919.9 |
| `markdown-nested-list` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2257.4 |
| `markdown-reply-plan-questions` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2160.6 |
| `none-release-notes` | none | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1521.6 |
| `none-rhetorical` | none | ok | 2 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1612.1 |
| `plain-paragraph-proposal` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2309.5 |
| `prose-multiline-question` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1944.0 |
| `prose-rhetorical` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1765.8 |
| `selected-option` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 2540.9 |
| `standalone-alternatives` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 3339.7 |
| `suggested-complementary-plan` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 2182.9 |
| `tasks-and-exclusions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 2563.2 |

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
| `many-recommendation-links` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a |
| `markdown-epic003` | 100.0% / 72.7% / 100.0% / 72.7% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-nested-list` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `markdown-reply-plan-questions` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-release-notes` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `none-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `plain-paragraph-proposal` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-multiline-question` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `prose-rhetorical` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `selected-option` | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `standalone-alternatives` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% |
| `suggested-complementary-plan` | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| `tasks-and-exclusions` | 100.0% / 100.0% / 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |

## Failures

- `email-quoted-answer` (email): **approximate_span**: expected span "e1" [42..90] "Should we enable the cache in staging this week?" matched leniently to predicted span [40..90] "> Should we enable the cache in staging this week?" (IoU 0.96) but not exactly
- `email-quoted-answer` (email): **approximate_span**: expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." matched leniently to predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." (IoU 0.97) but not exactly
- `email-quoted-answer` (email): **missed_label**: expected question label on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but it was not predicted
- `email-quoted-answer` (email): **unexpected_label**: predicted task label on predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." but none was expected
- `email-quoted-answer` (email): **missed_assignee**: expected @ops-team on expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." but none was predicted
- `email-quoted-answer` (email): **unexpected_assignee**: predicted @ops-team on predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." but no assignment was expected
- `json-question-answer` (json): **approximate_span**: expected span "e1" [44..86] "Should the parser retry on invalid output?" matched leniently to predicted span [43..87] "\"Should the parser retry on invalid output?\"" (IoU 0.95) but not exactly
- `json-question-answer` (json): **approximate_span**: expected span "e2" [102..128] "Yes, up to three attempts." matched leniently to predicted span [101..129] "\"Yes, up to three attempts.\"" (IoU 0.93) but not exactly
- `markdown-epic003` (document): **missed_span**: expected span "e9" [3241..3431] "- **Stable toolchain for non-GUI crates:** keep `daftprompt-…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e10" [3432..3569] "- **Evaluation history:** write evaluation results to a repo…" was not predicted by any span
- `markdown-epic003` (document): **missed_span**: expected span "e11" [3570..3688] "- **GUI interaction:** click an answer to jump to the questi…" was not predicted by any span
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e9" [3241..3431] "- **Stable toolchain for non-GUI crates:** keep `daftprompt-…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e10" [3432..3569] "- **Evaluation history:** write evaluation results to a repo…", but the span was not matched
- `markdown-epic003` (document): **missed_label**: expected recommendation label on expected span "e11" [3570..3688] "- **GUI interaction:** click an answer to jump to the questi…", but the span was not matched
- `none-rhetorical` (none): **unexpected_span**: predicted span [30..49] "Was that not lucky?" matched no expected span
- `none-rhetorical` (none): **unexpected_label**: predicted question label on predicted span [30..49] "Was that not lucky?", but the span was not matched

## Reproduce

```sh
DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast DAFTPROMPT_EVAL_PROMPT_VERSION=scribe-v4 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --fixtures crates/daftprompt-core/fixtures/scribe --output <same-path>
```

