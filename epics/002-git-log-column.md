# Epic 002: Git Log Reader with Column View

## Introduction

This epic adds a git log reader to the text explorer, displaying commit history as a vertically scrollable Column of Cards on the canvas. It introduces a new **Container** abstraction: Cards must be placed inside Containers, and Containers are placed on the Canvas. This is a new architectural rule going forward.

### Work Context

**Problem:** Users need to visualize git commit history in an intuitive, scrollable column layout within the existing canvas interface.

**Solution:** Build a git log reader using **gitoxide** (`gix` crate) that:
- Reads commits from a git repository (path provided via CLI argument)
- Renders commits as Cards inside a single-column Container
- Supports vertical scrolling within the Container
- Cards have variable height based on content (between min/max constraints)

**Technology Stack:**
- **gix** (from `~/Projects/gitoxide/gix`) - Git repository access via gitoxide
- Existing **wgpu** + **glyphon** + **winit** rendering stack

**Design Principles:**
- Container abstraction: Cards live in Containers, Containers live on Canvas
- Variable-height Cards with min/max constraints
- Vertical scrolling within Container bounds
- Minimal Card design: short hash, author, date, message title

---

## Architecture Changes

### Container Abstraction

Introduce a new `Container` concept that sits between Cards and the Canvas:

```
Canvas
  └── Container (positioned on canvas, has bounds)
        └── Card[] (laid out inside container)
```

**Rules:**
- Cards cannot exist directly on the Canvas — they must be inside a Container
- Containers are positioned on the Canvas like Cards were before
- Each Container manages its own internal layout and scrolling
- Container types: `DocumentGrid` (existing card layout), `GitLogColumn` (new)

### Git Log Column Layout

```
┌─────────────────────────────┐
│ ▲ scroll up                 │  ← Container header (optional)
├─────────────────────────────┤
│ ┌─────────────────────────┐ │
│ │ abc1234  Alice  Jan 15  │ │  ← Card (variable height)
│ │ Fix bug in renderer     │ │
│ └─────────────────────────┘ │
│ ┌─────────────────────────┐ │
│ │ def5678  Bob    Jan 14  │ │  ← Card
│ │ Add new feature X       │ │
│ │ with longer message     │ │
│ │ that wraps to lines     │ │
│ └─────────────────────────┘ │
│ ┌─────────────────────────┐ │
│ │ ghi9012  Carol  Jan 13  │ │  ← Card
│ │ Refactor module Y       │ │
│ └─────────────────────────┘ │
│ ...more cards...            │
├─────────────────────────────┤
│ ▼ scroll down               │
└─────────────────────────────┘
```

---

## Tasks

### Task 1: Add gix Dependency and CLI Argument

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 1 hour

**Description:** Add the `gix` crate as a local path dependency and add a CLI argument for the git repository path.

**Details:**
- Add to `Cargo.toml`:
  ```toml
  gix = { path = "../gitoxide/gix", default-features = false, features = ["basic"] }
  clap = { version = "4", features = ["derive"] }
  ```
- Add CLI argument parsing:
  ```rust
  #[derive(Parser)]
  struct Args {
      /// Path to git repository to read log from
      #[arg(short, long, default_value = ".")]
      repo: PathBuf,
  }
  ```
- Parse args in `main()` before app initialization
- Store repo path in `AppState`

**Acceptance Criteria:**
- [ ] `gix` compiles as a dependency
- [ ] CLI `--repo <path>` argument works
- [ ] Default repo path is current directory
- [ ] Repo path stored in application state

---

### Task 2: Implement Git Log Reader Module

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Create a `git_log` module that reads commits from a repository using gitoxide.

**Details:**
- New file: `src/git_log.rs`
- Core function:
  ```rust
  pub fn read_log(repo_path: &Path) -> Result<Vec<CommitInfo>, GitLogError>;

  pub struct CommitInfo {
      pub short_hash: String,
      pub author_name: String,
      pub author_email: String,
      pub time: String,           // Formatted date
      pub message_title: String,  // First line of commit message
      pub message_body: String,   // Rest of message (for expanded cards)
      pub parent_count: usize,
  }
  ```
- Implementation based on `examples/log.rs` from gitoxide:
  ```rust
  use gix::date::time::format;

  let repo = gix::discover(repo_path)?;
  let head = repo.rev_parse_single("HEAD")?.object()?.try_into_commit()?;

  let commits: Vec<CommitInfo> = repo
      .rev_walk([head.id()])
      .sorting(gix::revision::walk::Sorting::ByCommitTime(Default::default()))
      .all()?
      .map(|info| -> Result<CommitInfo, _> {
          let info = info?;
          let commit = info.object()?;
          let commit_ref = commit.decode()?;
          let author = commit_ref.author()?;
          Ok(CommitInfo {
              short_hash: commit.id().shorten_or_id().to_string(),
              author_name: author.actor().name.to_string(),
              // ... etc
          })
      })
      .collect::<Result<Vec<_>, _>>()?;
  ```
- Error handling with a custom `GitLogError` type
- Default branch: read from HEAD (follows main/master automatically)

**Acceptance Criteria:**
- [ ] Can read commits from a valid git repository
- [ ] Returns structured `CommitInfo` data
- [ ] Handles errors gracefully (missing repo, corrupt repo)
- [ ] Reads from HEAD by default (follows main/master)
- [ ] Short hash displayed (7 chars)

---

### Task 3: Introduce Container Abstraction

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Refactor existing code to introduce the Container abstraction. Cards must live inside Containers.

**Details:**
- New file: `src/ui/container.rs`
- Container types:
  ```rust
  pub enum ContainerType {
      DocumentGrid,    // Existing card layout (grid of cards)
      GitLogColumn,    // New single-column git log view
  }

  pub struct Container {
      pub id: usize,
      pub position: Vec2,           // Position on canvas
      pub size: Vec2,               // Visible size (viewport)
      pub content_height: f32,      // Total content height (for scrolling)
      pub scroll_offset: f32,       // Current scroll position
      pub container_type: ContainerType,
      pub cards: Vec<CardData>,     // Cards inside this container
  }

  impl Container {
      pub fn scroll(&mut self, delta: f32) {
          self.scroll_offset = (self.scroll_offset + delta)
              .clamp(0.0, (self.content_height - self.size.y).max(0.0));
      }

      pub fn visible_cards(&self) -> impl Iterator<Item = (&CardData, Vec2)> {
          // Returns cards that are within the visible scroll region
          // with their adjusted positions relative to container
      }
  }
  ```
- Update `AppState`:
  ```rust
  pub struct AppState {
      // ... existing fields ...
      pub containers: Vec<Container>,
      // Remove top-level cards field (cards now inside containers)
  }
  ```
- Update `UIManager` to render containers and their cards
- Migrate existing document cards into a `DocumentGrid` container

**Acceptance Criteria:**
- [ ] `Container` struct with position, size, scroll support
- [ ] Existing document cards work inside a `DocumentGrid` container
- [ ] Container renders its cards with viewport culling
- [ ] Cards cannot exist outside containers
- [ ] Canvas renders containers (not cards directly)

---

### Task 4: Implement GitLogColumn Container

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Create the `GitLogColumn` container type that lays out commit cards in a single scrollable column.

**Details:**
- Extend `Container` with git-log-specific layout logic:
  ```rust
  impl Container {
      pub fn new_git_log(commits: Vec<CommitInfo>, position: Vec2, width: f32) -> Self {
          let card_min_height = 80.0;
          let card_max_height = 200.0;
          let card_padding = 8.0;

          let cards: Vec<CardData> = commits.iter().enumerate().map(|(i, commit)| {
              let height = calculate_card_height(commit, width, card_min_height, card_max_height);
              let y_offset = cards_so_far_y;  // cumulative

              CardData {
                  id: i,
                  position: Vec2::new(0.0, y_offset),  // Relative to container
                  size: Vec2::new(width - 16.0, height),  // 8px padding each side
                  // ... commit data stored in associated document
              }
          }).collect();

          let content_height = cards.iter().map(|c| c.size.y + card_padding).sum();

          Container {
              container_type: ContainerType::GitLogColumn,
              position,
              size: Vec2::new(width, 600.0),  // Visible viewport height
              content_height,
              scroll_offset: 0.0,
              cards,
          }
      }
  }
  ```
- Card height calculation:
  ```rust
  fn calculate_card_height(commit: &CommitInfo, width: f32, min: f32, max: f32) -> f32 {
      // Base height for: hash + author + date line
      let header_height = 24.0;
      // Separator
      let separator_height = 16.0;
      // Message title (may wrap)
      let chars_per_line = (width - 32.0) / 8.0; // approx chars at 12px font
      let message_lines = (commit.message_title.len() as f32 / chars_per_line).ceil();
      let message_height = message_lines * 18.0;
      // Padding
      let padding = 20.0;

      let total = header_height + separator_height + message_height + padding;
      total.clamp(min, max)
  }
  ```

**Acceptance Criteria:**
- [ ] Commits displayed in a single vertical column
- [ ] Card heights vary based on message length
- [ ] Cards respect min (80px) and max (200px) height constraints
- [ ] Content height calculated correctly for scrolling
- [ ] Column positioned on canvas at specified position

---

### Task 5: Implement Column Scrolling

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 1.5 hours

**Description:** Add vertical scrolling support to the GitLogColumn container.

**Details:**
- Handle scroll input when mouse is over a container:
  ```rust
  // In input handling
  fn handle_scroll(&mut self, state: &mut AppState, delta: f32, mouse_pos: Vec2) {
      for container in &mut state.containers {
          if container.is_mouse_over(mouse_pos) {
              container.scroll(-delta * 40.0); // Scroll speed
              return;
          }
      }
      // If no container under mouse, pan canvas
      state.zoom_at_point(mouse_pos, delta);
  }
  ```
- Update rendering to account for scroll offset:
  ```rust
  impl Container {
      pub fn render_card_at(&self, card: &CardData) -> Option<(Vec2, Vec2)> {
          let card_abs_pos = self.position + card.position - Vec2::new(0.0, self.scroll_offset);

          // Cull if above or below visible area
          if card_abs_pos.y + card.size.y < self.position.y
              || card_abs_pos.y > self.position.y + self.size.y
          {
              return None; // Not visible
          }

          Some((card_abs_pos, card.size))
      }
  }
  ```
- Clip rendering to container bounds (don't draw cards outside container viewport)
- Scroll bar indicator (optional, can be a thin bar on the right side)

**Acceptance Criteria:**
- [ ] Mouse wheel scrolls the column when cursor is over it
- [ ] Scrolling is clamped (can't scroll past top or bottom)
- [ ] Cards outside visible area are culled
- [ ] Canvas panning still works when not over a container
- [ ] Smooth scrolling feel

---

### Task 6: Render Git Commit Cards

**Priority:** Medium
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Style the commit cards within the GitLogColumn with the minimal design.

**Details:**
- Card layout for each commit:
  ```
  ┌──────────────────────────────────────┐
  │ abc1234   Alice   Jan 15, 2024 14:30 │  ← Header line (hash, author, date)
  │ ──────────────────────────────────── │  ← Separator
  │ Fix bug in rendering pipeline        │  ← Message title (may wrap)
  └──────────────────────────────────────┘
  ```
- Colors (dark theme):
  - Background: `rgba(35, 35, 40, 230)` (slightly different from doc cards)
  - Hash: `rgb(100, 200, 255)` (cyan-ish)
  - Author: `rgb(180, 180, 180)` (gray)
  - Date: `rgb(140, 140, 140)` (dimmer gray)
  - Separator: `rgba(80, 80, 80, 150)`
  - Message: `rgb(230, 230, 230)` (near white)
  - Hover: lighten background slightly
- Font sizes:
  - Header: 12px
  - Message: 13px
- Differentiate git log cards from document cards visually (different background tint, border style)

**Acceptance Criteria:**
- [ ] Cards show short hash, author, date, message title
- [ ] Hash rendered in distinct color (cyan)
- [ ] Proper text truncation for long messages
- [ ] Hover state works
- [ ] Visually distinct from document cards

---

### Task 7: Wire Everything Together

**Priority:** Medium
**Status:** ⬜ Not Started
**Estimated Time:** 1.5 hours

**Description:** Connect the git log reader, container, and rendering into the main application flow.

**Details:**
- In `main.rs`, after parsing CLI args:
  ```rust
  let args = Args::parse();

  // Read git log
  let commits = git_log::read_log(&args.repo)?;

  // Create git log container
  let git_container = Container::new_git_log(
      commits,
      Vec2::new(100.0, 100.0),  // Position on canvas
      500.0,                     // Width
  );

  // Add to app state
  state.containers.push(git_container);
  ```
- Ensure existing document grid container also works
- Test with the sugacode repo itself: `cargo run -- --repo .`

**Acceptance Criteria:**
- [ ] `cargo run -- --repo .` shows git log of sugacode project
- [ ] `cargo run -- --repo ~/Projects/gitoxide` shows gitoxide log
- [ ] Default (no args) reads from current directory
- [ ] Existing document grid still works alongside git log
- [ ] No regressions in zoom/pan/drawer/search

---

## Implementation Order

1. **Task 1:** Add gix dependency + CLI arg (1 hour)
2. **Task 2:** Git log reader module (2 hours)
3. **Task 3:** Container abstraction refactor (2 hours)
4. **Task 4:** GitLogColumn layout (2 hours)
5. **Task 5:** Column scrolling (1.5 hours)
6. **Task 6:** Commit card rendering (2 hours)
7. **Task 7:** Wire together and test (1.5 hours)

**Total Estimated Time:** ~12 hours

---

## File Changes Summary

| File | Action | Description |
|------|--------|-------------|
| `Cargo.toml` | Modify | Add `gix`, `clap` dependencies |
| `src/main.rs` | Modify | Add CLI arg parsing, init git log |
| `src/git_log.rs` | **New** | Git log reader using gitoxide |
| `src/ui/container.rs` | **New** | Container abstraction |
| `src/ui/mod.rs` | Modify | Add container module, update rendering |
| `src/state.rs` | Modify | Add containers to AppState, add CommitInfo |
| `src/input.rs` | Modify | Route scroll to containers |
| `src/ui/card.rs` | Modify | Support rendering inside containers |

---

## Success Criteria

The feature is complete when:
1. `cargo run -- --repo <path>` displays git log in a scrollable column
2. Each commit is a Card with: short hash, author, date, message
3. Cards have variable height (80-200px) based on content
4. Column is vertically scrollable with mouse wheel
5. Scrolling is smooth and clamped to content bounds
6. Existing document grid container still works
7. Code compiles without errors

---

## Future Enhancements (Post-Epic)

- Commit detail view (click to expand full message + diff stats)
- Branch visualization (color-coded branches)
- Search/filter commits
- Click commit to highlight related files
- Multiple containers on canvas (git log + file tree side by side)
- Lazy loading for very large repos
