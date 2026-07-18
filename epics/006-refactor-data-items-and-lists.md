# Epic 006: Refactor Canvas Containers to akar Data Items and Lists

**Status:** Done (post-implementation review found unmet acceptance criteria — see notes below and "Post-Implementation Review Summary")
**Goal:** Replace sugacode's canvas-coupled git-log, search-result, and code-search card renderer with the reusable akar data-item and data-list APIs introduced by akar Epic 017, while retaining sugacode ownership of all domain data and UI policy.

**Prerequisite:** sugacode Epic 005 is complete and akar Epic 017 has shipped the required APIs.

**Pre-implementation review (2026-07-18):** Verified against the shipped akar source (`~/Projects/akar`, Epics 016/017 both `Status: Done`). `data_item`, `data_list_begin/end`, `canvas_data_item`, `canvas_portal_begin/end`, `CanvasResponse::project`/`lod_index`, and `Layout::widget_id_keyed`/`set_namespace_id` all exist and match the epic's sketch, with one signature drift: `canvas_portal_end` takes a `CanvasPortalGuard` argument, not `(core)` alone. Three findings from that review are folded into the tasks below: the fixed-height list constraint forces Task 3's decision earlier than Task 2, `CardData.id` is a positional index and not a valid ADR-016a key, and `ContainerType::DocumentGrid` is confirmed dead code (see Task 5).

---

## Problem Statement

`render_containers` currently owns responsibilities that should be separated:

- It walks `AppState::containers`, computes world-to-screen rectangles, and submits card quads directly to `core.draw_list`.
- It constructs rootless absolute Taffy nodes for every text line, then renders labels in a second pass.
- It implements hover and single-selection state inline.
- It uses `scroll_area_begin/end` and a fixed-height approximation with `list_clip` even though card heights may vary.

This works for sugacode's canvas, but it cannot be reused by other applications or readily share presentation behavior with a normal screen-space list. The `Container`, `CardData`, and `DocumentData` types are sugacode domain state and must remain so.

This refactor adopts akar Epic 017 without moving domain data, search behavior, selection policy, or canvas placement into akar.

---

## Design Decisions

### Sugacode Retains Its Data Model

`Container`, `CardData`, `DocumentData`, `CommitInfo`, `SearchResult`, and `CodeSearchResult` remain sugacode-owned. The refactor maps each visible record into an akar item presentation at render time. No commit, document, or search schema is added to akar.

### Selection Policy Remains Local

akar item responses report pointer interaction. Sugacode continues to decide whether a click means single selection, deselection, keyboard navigation, opening a detail view, or another action. The existing one-selected-card-per-container behavior is preserved initially.

### Canvas Has Summary and Portal Modes

At low detail, sugacode uses akar's canvas data-item summary helper for display-only commit/search summaries and group-level interaction. At interactive detail, a container/item uses a portal-local layout and ordinary akar data-item/list components. It must not attempt to place normal child widgets directly in world space.

### Incremental Migration Is Required

The current renderer remains usable while each data source is migrated. The refactor must not combine an API change in akar with an unverified whole-UI rewrite.

---

## Implementation Tasks

### Task 1: Upgrade and Establish an Adapter Layer

**Files:**

- `Cargo.toml`
- `src/ui/render.rs`
- `src/ui/container.rs`
- new focused adapter module if it makes the mapping clearer

**Work:**

1. Confirm sugacode builds against the shipped akar Epic 017 API. The akar crates are path dependencies already at the same version, so this is a `cargo check --workspace` verification, not a version bump — fix any call-site drift (e.g. `canvas_portal_end(core, guard)` taking a `CanvasPortalGuard`).
2. Define small sugacode-local mapping functions from commit/search/code-search data to data-item title, supporting text, metadata, and style inputs.
3. Preserve domain records and stable item keys; do not expose `CardData` to akar. `CardData.id` (a per-container loop index, not record identity) must **not** be used as the `data_item`/`data_list` key. Derive the `u64` key by hashing real record identity instead: `CommitInfo.sha` for git-log, `SearchResult.short_hash`/`identifier` for commit search, `CodeSearchResult.identifier` for code search. This is what makes ADR-016a hold — a key derived from loop position corrupts focus/selection identity across scroll and re-search exactly the way ADR-016a describes.
4. Add a local layout/portal cache keyed by stable container and item identity where portal state is needed.

**Acceptance criteria:**

- Sugacode compiles against the shipped Epic 017 API.
- The mapping layer contains presentation mapping only, not rendering math or application-owned records in akar.
- Item keys are derived from stable record content (commit SHA, search result identifier), not from `CardData.id` or list position.
- Recreated portal layouts use stable namespaces for the same logical item.

**Review finding (2026-07-18):** PARTIAL. Item keys are correctly derived from record identity (`stable_item_key_commit`/`stable_item_key_search`/`stable_item_key_code_search` hash `CommitInfo.sha` / `.identifier`; `CardData.id` is never used as a key). But the mapping-layer functions (`commit_to_item_descriptor`, `search_result_to_item_descriptor`, `code_search_result_to_item_descriptor`, `default_data_item_style`, `color_u32_to_f32` in `src/ui/adapter.rs`) are dead code — never called from `render.rs`; real rendering instead hand-parses `doc.content` strings (`"Author: "` prefix stripping), duplicating what the adapter should centralize. The "local layout/portal cache keyed by stable... identity" and "stable namespaces for the same logical item" acceptance criterion is not implemented at all — no `Layout::widget_id_keyed`/`set_namespace_id` call exists anywhere in `src/`, despite both being named as available APIs in the pre-implementation review above.

### Task 2: Resolve Variable-Height Card Policy

**Must land before Task 3.** `data_list_begin` (`crates/akar-components/src/data_list.rs`) takes one uniform `item_height` for the whole list — akar Epic 017 deliberately deferred variable-height virtualization (ADR-017). Sugacode's current `calculate_card_height`/`calculate_search_card_height` produce a variable 80–200px height per card. Task 3 cannot pick a `data_list_begin` call without this decision already made, so it is sequenced first even though it was originally numbered after the list refactor.

**Files:**

- `src/ui/container.rs`
- `src/ui/render.rs`

**Work:**

1. Audit each card type's height behavior.
2. Choose and document one initial policy compatible with akar Epic 017: normalize to a fixed item height with bounded/truncated content, or use a non-virtualized layout list for the limited variable-height case.
3. Remove the current misleading first-card-height approximation once the chosen policy is implemented.

**Acceptance criteria:**

- The renderer never incorrectly skips a visible record due to a mismatched virtualization height.
- Overflow behavior is legible and tested with long commit messages and code identifiers.

**Review finding (2026-07-18):** PASS. `CARD_HEIGHT = 120.0` (`src/ui/container.rs`); `calculate_card_height`/`calculate_search_card_height`/`card_min_height`/`card_max_height` are fully removed (zero grep hits). Overflow uses fixed-height label boxes with computed remaining height — reasonable, though truncation correctness relies on the label component's own clipping and was not independently visually verified (see Task 6 finding).

### Task 3: Refactor Normal List Rendering

**Files:**

- `src/ui/render.rs`
- `src/ui/container.rs`
- `src/state.rs`

**Work:**

1. Replace direct card quad submission and ad hoc item hover handling with akar `data_item` responses.
2. Replace direct `scroll_area_begin/end` and `list_clip` use in container rendering with `data_list_begin/end`, using the fixed `item_height` chosen in Task 2 and the stable per-record keys established in Task 1 (not `CardData.id`).
3. Build layout item subtrees only for the visible range and render title, hash, author, date, message, and code location through ordinary components.
4. Preserve the existing card styles and selected/hovered appearance through the new style configuration.

**Acceptance criteria:**

- Git log, commit search, and code search render through the reusable item/list path.
- Only visible fixed-height list items are constructed and rendered.
- Existing single-select behavior remains correct.
- No card background is pushed directly from sugacode to `core.draw_list`.

**Review finding (2026-07-18):** PASS. All three container types render through `data_list_begin/end` + `akar_data_item`. The one remaining `core.draw_list.push_quad` call in `render.rs` is a 1px separator line inside a card, not a card background — the background quad itself is pushed internally by akar's own `data_item()` implementation. Literal acceptance criterion holds.

### Task 4: Add Canvas LOD Presentation

**Files:**

- `src/ui/render.rs`
- `src/ui/container.rs`
- `src/state.rs` if explicit LOD state is required

**Work:**

1. Define visible LOD thresholds for sugacode containers and items.
2. Render low-detail containers/items with the akar canvas summary helper, using display-only title/metadata and group hover/click.
3. At interactive detail, create or restore a portal-local layout, render the normal item/list subtree, and keep it clipped through `canvas_portal_begin/end`.
4. Preserve pan and zoom behavior and ensure portal content remains within both the item and canvas clips.

**Acceptance criteria:**

- Overview mode is readable and has group-level interaction only.
- Interactive mode supports normal data-item/list behavior through a portal.
- No normal component is used directly as a transformed world-space child.

**Review finding (2026-07-18):** PASS. `canvas_data_item` is used at LOD 0, `canvas_portal_begin/end` wraps the high-detail path, threshold is `CONTAINER_LOD_THRESHOLDS = &[200.0]` via `canvas_resp.lod_index`. `canvas_portal_end(core, portal_guard)` correctly passes the `CanvasPortalGuard`, matching the signature drift flagged in the pre-implementation review. No normal component is placed as a directly-transformed world child outside the portal.

### Task 5: Remove Superseded Rendering Paths

**Files:**

- `src/ui/render.rs`
- `src/ui/container.rs`
- `src/state.rs`

**Work:**

1. Remove rootless absolute text overlays used only by the old card renderer.
2. Remove redundant per-card hover state if it is fully derivable from the current item response; retain selected state only where it represents application policy.
3. Simplify container fields and helpers that existed solely for manual screen-coordinate card rendering.
4. Remove `ContainerType::DocumentGrid` and `Container::new_document_grid` (`src/ui/container.rs`) along with their two match arms in `src/ui/render.rs`. Verified dead code: `new_document_grid` has no call sites anywhere in the codebase, so it is out of this epic's scope by construction, not something to migrate.

**Acceptance criteria:**

- The legacy manual card path is gone.
- Domain and selection state remain clear and minimal.
- No unrelated drawer, search, indexer, or canvas behavior regresses.
- `ContainerType::DocumentGrid` and `new_document_grid` no longer exist.

**Review finding (2026-07-18):** FAIL. `ContainerType::DocumentGrid`, `Container::new_document_grid`, and their match arms in `src/ui/render.rs` are still present (`grep -rn "DocumentGrid" src/` returns 5 hits: `container.rs:9,27,45`, `render.rs:259,613`). `cargo check --workspace` still emits "variant `DocumentGrid` is never constructed" / "associated item `new_document_grid`... never used" warnings. This is a literal, explicitly-named acceptance criterion of this task that was not met — the commit claims removal but the code was never deleted. Also unaddressed: Task 5's own goal to "simplify container fields... that existed solely for manual screen-coordinate card rendering" — `CardData.id`, `.position`, `.size`, and `Container.content_height` are dead per `cargo check` warnings, leftover from the old manual-render path.

### Task 6: Verification and Documentation

**Files:**

- relevant tests and scripted capture assets
- `README.md` or development documentation if present

**Work:**

1. Add tests for mapping, selection policy, fixed-height/overflow behavior, and portal key stability where practical without a live GPU.
2. Capture and inspect a low-detail canvas overview and an interactive portal list state.
3. Use frame inspection to confirm the expected canvas, portal, and list scissors.
4. Run formatting, clippy, and the relevant test suite.

**Acceptance criteria:**

- Representative git-log and search-result screenshots are verified at overview and interactive detail.
- Scroll, hover, click, selection, pan, and zoom work in the migrated views.
- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, and `cargo test --workspace` pass.

**Review finding (2026-07-18):** PARTIAL/FAIL. `cargo test --workspace` genuinely passes (35/35: 18 sugacode-indexer + 17 text-explorer), and tests meaningfully cover key determinism/collision, descriptor field mapping, and style theming — though the selection-policy tests exercise a hand-rolled `apply_selection` reimplementation, not the actual click-handling code path in `render.rs`. However `cargo fmt --check` **fails** (diffs in `crates/sugacode-indexer/src/code.rs` and `db.rs`) and `cargo clippy --workspace -- -D warnings` **fails with 2 errors** (`unnecessary_map_or` in `code.rs:464`, `missing_transmute_annotations` in `db.rs:10`). Both issues predate this epic (trace to Epic 004) and are outside the epic's touched files, but the acceptance criterion is unconditional ("pass") and was not actually run before marking the epic Done. Additionally, no screenshot or frame-inspection artifacts exist anywhere in the 7 commits — the criteria "Representative git-log and search-result screenshots are verified" and "Scroll, hover, click, selection, pan, and zoom work in the migrated views" have no supporting evidence and were likely never executed.

---

## Post-Implementation Review Summary (2026-07-18)

Tasks 2, 3, and 4 are correctly implemented and their acceptance criteria hold up against the live code. Task 1 is half-delivered: stable keys are right, but the adapter/mapping functions are dead code and the portal-namespace-caching requirement was never implemented. **Task 5 is not done** — `ContainerType::DocumentGrid` and `Container::new_document_grid`, explicitly named for removal, are still in the codebase. Task 6's stated gate is falsely satisfied: tests pass, but `cargo fmt --check` and `cargo clippy -- -D warnings` both fail, and there's no evidence the required visual/screenshot verification was ever performed. The epic's own top-level "## Acceptance Criteria" checklist below was never checked off despite the "Mark as Done" commit — a signal the closure was not actually verified against this file's own gates before being marked complete.

**Recommended before this epic is genuinely closed:**
1. Delete `ContainerType::DocumentGrid`, `Container::new_document_grid`, and their two match arms in `render.rs` (Task 5).
2. Either wire `src/ui/adapter.rs`'s descriptor functions into real rendering, or delete them if the inline mapping in `render.rs` is intentionally being kept instead (Task 1).
3. Fix the 2 clippy errors and run `cargo fmt` (Task 6) — small, pre-existing, unrelated to this epic's scope but blocking the stated gate.
4. Do a manual pass exercising scroll/hover/click/selection/pan/zoom in the migrated views, and capture at least one overview + one portal-detail screenshot per the Task 6 acceptance criteria.

---

## Scope

### Included

- Migrating git-log, commit-search, and code-search card presentation to akar data items/lists.
- Preserving sugacode domain models and selection policy.
- Low-detail canvas summaries and interactive portal reuse.
- Removing the manual card drawing and absolute text-overlay path after migration.

### Deferred

- Changes to indexing, database schema, search ranking, or CLI behavior.
- New application data types or moving data models into akar.
- Generic variable-height virtualization beyond the policy selected in Task 3.
- Canvas-native child-widget interaction outside portals.

---

## Acceptance Criteria

- [ ] Sugacode uses akar's reusable data-item/list APIs for git-log and search records.
- [ ] Sugacode remains the owner of all commit, document, and search data.
- [ ] Selection policy remains in sugacode.
- [ ] Low-detail canvas cards are display-only/group-interactive; interactive cards use portals.
- [ ] The old direct card quad and absolute-label rendering path is removed.
- [ ] Overview and interactive portal visual states are verified.
- [ ] Formatting, clippy, and tests pass.
