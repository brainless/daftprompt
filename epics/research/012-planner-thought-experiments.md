# Epic 012 Planner Thought Experiments

This artifact records real planning cases used to revise Epic 012. It is
intentionally cumulative. Task 0 remains incomplete until the full required
corpus and acceptance criteria in the epic are satisfied.

## Experiment 1: Cross-model review of akar Epic 020

The repository facts, commits, blob IDs, evidence, and provenance gaps are
recorded in `011-provenance-thought-experiments.md`, Experiment 1.

### Intent and uncertainty

Intent: review a model-authored implementation epic before implementation and
compare findings from another model.

Uncertainty:

- the original and revised Git versions are known;
- model authorship is user-supplied but not repository-observed;
- the Codex review text was initially chat-only;
- the revised epic does not preserve rejected suggestions.

### Deterministic project facts

- akar is a Rust workspace.
- Public component capabilities span Rust and a generated C ABI.
- Repository instructions require local-source inspection and visual
  verification.
- Canonical gates include `cargo fmt --check`, `cargo test --workspace`, and
  `cargo clippy --workspace -- -D warnings`.
- The repository provides deterministic screenshot, frame-dump, component
  isolation, scripted-input, and pixel-diff tools.

### Relevant graph query

1. Resolve the original Epic 020 blob.
2. Select exact epic sections mentioning text style, card, navbar, button,
   badge, C ABI, screenshot, and isolation requirements.
3. Follow exact symbol/path links to component implementations, C wrappers,
   generated header declarations, tests, Epic 019, and debug tooling.
4. Resolve the revised blob and document diff.
5. Retrieve each saved review linked to the exact original version.

### Typed actions and dependencies

```text
Parallel evidence group
  A. read_document_section: exact original Epic 020 sections
  B. read_symbol: mentioned component and glyphon types
  C. read_symbol: affected C ABI wrappers/header declarations
  D. inspect_history: Epic 019 and Epic 020 commits
  E. inspect_configuration: canonical gates and visual tooling

Barrier
  F. summarize_evidence: evidence-linked planning findings

Optional review group
  G. read_document_section: persisted MiMo-generation metadata
  H. read_document_section: persisted Codex review

Comparison
  I. compare_reviews: agreement, conflict, unique findings, unsupported claims,
     stale-version claims, and missing provenance

Final
  J. synthesize: bounded revision recommendations
```

A-E are independently executable and read-only. F depends on A-E. G-H can run
in parallel only when their artifacts exist. I depends on F-H; a missing G or H
produces a diagnostic rather than blocking evidence review. J depends on I.

### Output contracts

Evidence action:

```text
exact source version
file/span or symbol
finding
evidence method
confidence
maximum bounded excerpt
```

Comparison action:

```text
claim identity
review author/model provenance
reviewed version
agreement/disagreement classification
independent supporting evidence
unsupported or stale status
missing provenance
```

### Commands

No mutation or build command is required to review the plan. If the user later
authorizes implementation, canonical verification commands come from
`AGENTS.md`/`DEVELOP.md`, not from language-name guessing.

### Decisions requiring reasoning

- whether a proposed API tradeoff is desirable after factual contradictions
  are established;
- whether to preserve an existing API or accept a documented pre-alpha break;
- how much C ABI scope belongs in the epic;
- whether a review finding should revise the epic or become an explicit
  deferral.

### Failure modes observed

- attributing a Git commit to Codex because the user account committed it;
- comparing a review against the current epic instead of the exact reviewed
  blob;
- treating two models agreeing as two independent evidence methods;
- reconstructing missing chat review content from the resulting document diff;
- losing rejected suggestions when only the revised epic is saved;
- returning entire implementation files instead of focused symbols/spans.

### Result

The plan is materially smaller than giving the prompt and whole repository to
one agent. Exact mentions bounded the relevant corpus, while parallel API,
C ABI, history, and verification inspection exposed concrete defects.

This experiment added `review_plan` and `compare_reviews` actions,
review-version diagnostics, persisted optional-model provenance, and
claim-versus-evidence rules to Epic 012.

