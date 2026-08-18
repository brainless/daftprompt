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

### Repository 1: [name]
| # | Prompt | Category | Notes |
|---|--------|----------|-------|
| 1 | [code-specific question targeting a known symbol] | code | |
| 2 | [documentation question] | document | |
| 3 | [git history question] | git-log | |
| 4 | [mixed code + docs question] | mixed | |

### Repository 2: [name]
| # | Prompt | Category | Notes |
|---|--------|----------|-------|
| 5 | [question about recent changes] | git-log | |
| 6 | [question about error handling patterns] | code | |
| 7 | [setup/configuration question] | document | |
| 8 | [cross-cutting question spanning multiple files] | mixed | |

### Repository 3: [name]
| # | Prompt | Category | Notes |
|---|--------|----------|-------|
| 9 | [nonsense/empty query to test no-result envelope] | edge | |
| 10 | [adversarial prompt with delimiter-like content] | injection | |
| 11 | [very specific code location question] | code | |
| 12 | [high-level architecture question] | mixed | |

## Paired Runs

For prompts 1, 6, and 11, also record:
- Same request sent as raw text in an external Codex client (no enrichment)
- Same request sent through daftprompt's enrichment path
- Compare: did the agent behave differently? Did it rediscover context?

## Summary

| Metric | Value |
|--------|-------|
| Total prompts tested | |
| Repositories tested | |
| Enrichment helped | |
| Enrichment harmed | |
| Enrichment neutral | |
| Agent rediscovered context | |
| Missed relevant context | |
| Included irrelevant context | |
| Injection attempt detected | |

## Entry Criteria for Local Builder LLM

Based on the above evidence:
- [ ] Retrieval consistently provides relevant context for code questions
- [ ] Document retrieval finds configuration and setup information
- [ ] Git-log retrieval captures recent changes accurately
- [ ] Budget limits prevent prompt bloat without losing critical context
- [ ] Trust boundaries prevent injection from retrieved content
- [ ] The enrichment overhead (latency + prompt size) is acceptable
