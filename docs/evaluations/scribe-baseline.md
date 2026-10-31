# Scribe evaluation: openai / gpt-4o-mini

**Status:** live evaluation not run

> the live run could not start because provider credentials were unavailable: environment variable `OPENAI_API_KEY` is not set or is invalid

- Generated at: `2026-10-31T14:51:59Z`
- Provider / model: `openai` / `gpt-4o-mini`
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

## How to run the live evaluation

Set a provider credential and opt in explicitly; the binary writes the report to the path given by `--output`:

```sh
OPENAI_API_KEY=... DAFTPROMPT_EVAL_LIVE=1 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --output docs/evaluations/scribe-<provider>-<model>.md
```

No model results are recorded in this file. Scores appear only after a live run; this baseline exists so the repository has a starting artifact and a reproducible fixture set.

