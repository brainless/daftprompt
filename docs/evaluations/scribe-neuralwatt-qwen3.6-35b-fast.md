# Scribe evaluation: neuralwatt / qwen3.6-35b-fast

**Status:** live run completed: 10 of 11 fixture(s) parsed.

- Generated at: `2026-10-31T15:46:52Z`
- Provider / model: `neuralwatt` / `qwen3.6-35b-fast`
- Prompt version: `scribe-v1`
- Attempt budget: 3 total attempt(s), 60.0s per attempt
- Fixtures: `crates/daftprompt-core/fixtures/scribe`

## Fixture inventory

- 11 fixture(s), 17 expected span(s), 12 question label(s), 5 answer link(s), 2 explicit assignee(s)
- Categories: chat (2), prose (2), code (1), document (1), email (2), json (1), none (2)

## Matching rules

- **Exact span match:** byte ranges are equal after trimming surrounding whitespace.
- **Lenient span match:** overlap divided by union is at least 0.5 (also reported in addition to exact).
- **Label:** the kind matches on a matched span.
- **Answer link:** both ends match an expected link.
- **Assignee:** the handle is identical on a matched question.
- Matches are one-to-one and greedy, preferring exact pairs and then higher overlap.

## Aggregate

| metric | expected | predicted | matched | precision | recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| spans, exact | 16 | 17 | 11 | 64.7% | 68.8% |
| spans, lenient | 16 | 17 | 16 | 94.1% | 100.0% |
| labels, exact | 16 | 17 | 11 | 64.7% | 68.8% |
| labels, lenient | 16 | 17 | 16 | 94.1% | 100.0% |
| answer links, exact | 5 | 5 | 3 | 60.0% | 60.0% |
| answer links, lenient | 5 | 5 | 5 | 100.0% | 100.0% |
| assignees, exact | 2 | 2 | 1 | 50.0% | 50.0% |
| assignees, lenient | 2 | 2 | 2 | 100.0% | 100.0% |

- Invalid output: 1 of 11 fixture(s) (9.1%)
- Other parse failures: 0 of 11 fixture(s)
- Attempts per parse: mean 1.18 over 11 known of 11 parse(s), max 3
- Latency (ms): mean 2292.3, median 1879.2, p95 4222.1, max 4222.1

## Per fixture

| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |
| `chat-local-answer` | chat | ok | 3 | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% | 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 3693.9 |
| `chat-open-questions` | chat | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | 100.0% / 100.0% / 100.0% / 100.0% | 1 | 1879.2 |
| `code-todo-question` | code | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1966.4 |
| `document-adr` | document | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1737.4 |
| `email-crlf-answer` | email | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1788.0 |
| `email-quoted-answer` | email | ok | 2 | 33.3% / 33.3% | 100.0% / 100.0% | 33.3% / 33.3% | 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | 0.0% / 0.0% / 100.0% / 100.0% | 1 | 3684.2 |
| `json-question-answer` | json | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% / 100.0% / 100.0% | n/a / n/a / n/a / n/a | 1 | 1984.1 |
| `none-release-notes` | none | ok | 0 | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1227.7 |
| `none-rhetorical` | none | ok | 2 | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | 0.0% / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1486.1 |
| `prose-multiline-question` | prose | `invalid_output` | — | — | — | — | — | — / — | — / — | 3 | 4222.1 |
| `prose-rhetorical` | prose | ok | 0 | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | 100.0% / 100.0% | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | 1 | 1545.8 |

## Failures

- `prose-multiline-question` (prose): **invalid_output** after 3 attempt(s) in 4222.1 ms: invalid model output after 3 attempt(s): span "s1": quote "Should we build it this quarter, or wait for the rate-limit work to land first?" was not found in segments 4..=5
- `chat-local-answer` (chat): **approximate_span**: expected span "e1" [0..44] "Q: Do we need a migration for the new index?" matched leniently to predicted span [3..44] "Do we need a migration for the new index?" (IoU 0.93) but not exactly
- `chat-local-answer` (chat): **approximate_span**: expected span "e2" [45..92] "A: Yes, add one that drops the old index first." matched leniently to predicted span [48..92] "Yes, add one that drops the old index first." (IoU 0.94) but not exactly
- `chat-local-answer` (chat): **approximate_span**: expected span "e3" [93..123] "Q: Who owns the rollback plan?" matched leniently to predicted span [96..123] "Who owns the rollback plan?" (IoU 0.90) but not exactly
- `email-quoted-answer` (email): **approximate_span**: expected span "e1" [42..90] "Should we enable the cache in staging this week?" matched leniently to predicted span [40..90] "> Should we enable the cache in staging this week?" (IoU 0.96) but not exactly
- `email-quoted-answer` (email): **approximate_span**: expected span "e3" [127..184] "Who owns the rollout checklist? @ops-team please confirm." matched leniently to predicted span [125..184] "> Who owns the rollout checklist? @ops-team please confirm." (IoU 0.97) but not exactly
- `none-rhetorical` (none): **unexpected_span**: predicted span [30..49] "Was that not lucky?" matched no expected span
- `none-rhetorical` (none): **unexpected_label**: predicted question label on predicted span [30..49] "Was that not lucky?", but the span was not matched

## Reproduce

```sh
OPENAI_API_KEY=... DAFTPROMPT_EVAL_LIVE=1 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --fixtures crates/daftprompt-core/fixtures/scribe --output <same-path>
```

