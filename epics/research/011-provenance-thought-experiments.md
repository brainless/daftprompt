# Epic 011 Provenance Thought Experiments

This artifact records real repository/prompt cases used to revise Epic 011.
It is intentionally cumulative. Task 0 remains incomplete until the full
required corpus and acceptance criteria in the epic are satisfied.

## Experiment 1: Cross-model review of an implementation epic

### Repository and real workflow

Repository: `/Users/brainless/Projects/akar`

Workflow:

1. MiMo authored `epics/020-component-webpage-sample.md`.
2. The original epic was committed.
3. Codex was asked to analyze the epic and suggest planning changes.
4. The review initially existed only in chat.
5. Codex revised the epic and the revision was committed.

This is a recurring planning workflow: use different models to cross-check a
plan before implementation so omissions and incorrect assumptions surface
early.

### Exact observed versions

Logical document:

```text
epics/020-component-webpage-sample.md
```

Original version:

```text
commit: febfa42e747ee6f5b64f7c2f0549f9b82d1babaa
blob:   git-sha1:cff16996884d251b8d4ddc3bbae54b1cc75d506e
time:   2026-07-29T14:37:03+05:30
subject: epic(020): component-based webpage sample — rebuild using akar components
```

Revised version:

```text
commit: 9334431bbb37dd0db20f4b80c660387c62cf34bc
blob:   git-sha1:49878280500dfdbb61a707ae4879723369c13848
time:   2026-07-29T15:39:10+05:30
subject: docs: revise component webpage epic
```

Git proves that these blobs are two immutable versions of one path-level
logical document. It records Sumit Datta as Git author/committer. It does not
prove that MiMo generated the first version or Codex generated the review and
revision; that attribution came from the user conversation and was not stored
in the repository.

### Prompt intent

```text
Analyze Epic 020 and suggest changes before implementation.
```

Intent: read-only planning review followed, in the actual workflow, by an
explicit request to revise and commit the epic.

### Required nodes

- repository `akar`;
- Epic 020 logical document;
- original and revised Epic 020 versions;
- individual Epic 020 sections/tasks;
- Epic 019 prerequisite and its implementation commits;
- mentioned Rust component symbols and files;
- affected C ABI functions and generated header declarations;
- component isolation and screenshot tooling;
- saved Codex review, if exported;
- review request/prompt, if exported.

### Required edges and evidence

```text
repository --contains(structural)--> Epic 020
Epic 020 graph_node_versions includes original blob (git tree)
Epic 020 graph_node_versions includes revised blob (git tree)
revised version predecessor_version_id --> original version
review --reviews(exact input metadata)--> Epic 020
review graph_artifact_inputs.input_version_id --> original version
review --derived_from(explicit host metadata)--> review prompt
review finding --mentions(exact span/symbol)--> component or C ABI symbol
commit 9334431 --changes(git diff)--> Epic 020
```

The initial Epic 011 relation catalog lacked `reviews`, `derived_from`,
`revises`, and `supersedes`. This experiment added them and added a normalized
artifact-to-exact-input-version boundary.

### Deterministic evidence review

The original epic mentioned exact symbols and paths sufficient to select a
small review corpus without repository-wide discovery:

- `glyphon::FamilyOwned` does not implement `Copy`, contradicting the proposed
  `TextStyle: Copy`.
- Appending `Option` arguments changes Rust function signatures and therefore
  breaks existing call sites.
- Existing `akar_button`, `akar_badge`, and `akar_navbar` C wrappers lacked the
  proposed styling inputs.
- Existing navbar combined slot construction, root-style mutation, and paint,
  exposing a construct/compute/paint lifecycle problem for the proposed card.
- Paragraph wrapping required intrinsic text measurement so wrapped height
  could participate in Taffy layout.
- New components required explicit demo isolation registry work.
- Pixel-identical MiMo verification required a fixed viewport and saved
  baseline, not a qualitative screenshot assertion.

The committed revision incorporated these findings and expanded them into
component lifecycle, text measurement, akar-owned typography types, additive
styled APIs, C ABI coverage, deterministic capture, isolation, and explicit
deferrals.

### What the graph could and could not establish

Could establish:

- exact original/revised file content;
- commit times and Git authors;
- document diff and section changes;
- exact symbol/path references;
- repository evidence supporting many review findings.

Could not establish:

- MiMo or Codex authorship;
- the exact Codex review text;
- the review prompt and conversation identifier;
- rejected suggestions that did not enter the revised epic;
- whether matching conclusions from two models were independent.

The correct degraded result is: committed revision lineage is known; review
authorship and unseen claim provenance are missing.

### Design conclusions

1. Git blob IDs are exact version identity, not logical document identity.
2. A review must point to the exact version it examined.
3. Saved reviews are attributed artifacts and their findings are claims.
4. Cross-model agreement is not independent repository evidence.
5. Git author metadata must not be repurposed as model attribution.
6. Chat-only reviews require explicit export or host ingestion.
7. A separate review artifact preserves reasoning, rejected suggestions, and
   disagreements that disappear from a revised document.
8. Useful optional commit trailers include:

```text
Generated-With: Codex
Reviewed-Version: git-sha1:cff16996884d251b8d4ddc3bbae54b1cc75d506e
Review-Conversation: <opaque ID>
```

### Material usefulness

This case meets the Epic 011 usefulness threshold. Exact/structural retrieval
identified multiple high-value planning defects and their source evidence
without broad repository exploration.

It also produced concrete schema, relation, failure-mode, task, and test changes
in Epic 011. It should be reused by Epic 012 as a cross-review planning fixture.
