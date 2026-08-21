# Epic 014: Manual Evaluation Worksheet

## Instructions

For each test prompt, record:

1. Original user request
2. Retrieval policy (limit, budget) and latency (ms)
3. All retrieved candidates (rank, source, identifier, score, match_type)
4. Selected context (which candidates were included, truncated, excluded)
5. Exact enriched prompt sent to the agent
6. Agent/model/config identity (from initialize response)
7. Response text, tool activity, permission decisions, and outcome
8. Context missed by retrieval that the agent needed
9. Irrelevant or duplicated context included
10. Whether enrichment helped, harmed, or made no material difference
11. Whether the agent independently rediscovered the supplied context
12. Prompt size (chars) and turn latency (seconds)
13. Any prompt-injection or trust-boundary failure

## Test Prompts

Completed live record and manual review:
`014-acp-prompt-enrichment-evaluation.md`. Exact prompts, candidates, and
enriched artifacts remain in the durable DBs named there.

### Repository 1: daftprompt (`b9de9df`)
| # | Prompt | Category | Notes |
|---|--------|----------|-------|
| 1 | Explain how `build_enriched_prompt` preserves the original request and trust boundary | code | paired; harmed |
| 2 | Explain local codex-acp startup and exact prompt inspection from docs | document | harmed; key docs missed |
| 3 | Explain recent Task 6 shutdown/close/auth hardening commits | git-log | helped |
| 4 | Trace retrieval through formatting, storage, ACP, and inspector | mixed | harmed by stale/missed context |

### Repository 2: akar (`5d361f4`)
| # | Prompt | Category | Notes |
|---|--------|----------|-------|
| 5 | Explain recent RTL/caret changes and edge cases | git-log | neutral; HEAD excluded |
| 6 | Explain `AkarCore` screenshot state, readback, and errors | code | paired; helped |
| 7 | Explain demo and scripted screenshot commands from docs | document | neutral |
| 8 | Trace an immediate-mode frame across layout, draw, render, and input reset | mixed | neutral |

### Repository 3: codex-acp (`97d260e`)
| # | Prompt | Category | Notes |
|---|--------|----------|-------|
| 9 | Assess relevance of nonsense tokens | edge | harmed; 8 irrelevant excerpts |
| 10 | Find delimiter/trust-boundary evidence | injection | harmed; target evidence missed |
| 11 | Locate new-session model/reasoning resolution | code | paired; neutral |
| 12 | Trace session/prompt, permission bridge, events, and stop reason | mixed | harmed |

One additional daftprompt turn selected the real delimiter-injection fixture
and verified entity escaping with no observed trust-boundary failure.

## Paired Runs

For prompts 1, 6, and 11, also record:
- Same request sent as raw text in an external Codex client (no enrichment)
- Same request sent through daftprompt's enrichment path
- Compare: did the agent behave differently? Did it rediscover context?

## Summary

| Metric | Value |
|--------|-------|
| Total prompts tested | 12 primary + 1 focused injection + 2 completed retrieval errors |
| Repositories tested | 3 |
| Enrichment helped | 2 / 12 |
| Enrichment harmed | 6 / 12 |
| Enrichment neutral | 4 / 12 |
| Agent rediscovered context | 9 / 12 used tools |
| Missed relevant context | 8 / 12 |
| Included irrelevant context | 12 / 12 |
| Injection attempt detected | yes; safely contained in focused live fixture |

## Entry Criteria for Local Builder LLM

Based on the above evidence:
- [ ] Retrieval consistently provides relevant context for code questions
- [ ] Document retrieval finds configuration and setup information
- [ ] Git-log retrieval captures recent changes accurately
- [ ] Budget limits prevent prompt bloat without losing critical context
- [x] Trust boundaries prevent injection from retrieved content in the focused
  live fixture (broader novel-payload corpus remains an entry gate)
- [ ] The enrichment overhead (latency + prompt size) is acceptable
