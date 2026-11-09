# Native GUI

`daftprompt-gui` is the akar desktop app for the first parsing experiment. Two
columns: a composer on the left with a multiline textarea anchored at the bottom
and a Parse button, and a wider source view on the right. The right column shows
the original text as blocks, not a chat transcript: validated spans become cards
with API-assigned `#N` identifiers, label badges, statuses, assignee handles, and relationship targets, and unlabelled
text between them stays visible in source order. A span's card background, border,
and rounded corners are drawn only while the pointer is over it (hit-tested against
the scroll viewport); otherwise it reads as flowing text with its badges and
Clarify button. Blocks are spaced tightly (4 px between blocks, 6 px vertical padding).

The GUI uses its own palette, `DAFTPROMPT_THEME` in `src/theme.rs`: akar's dark theme with darker backgrounds (`base_100` `#000000` main column, `base_200` `#0a0a0b` left column, cards, and hover box, `base_300` `#26262b` borders, scrollbar thumbs, and hover/pressed fills). Text and semantic colours are akar's.

The app only talks to the API. It depends on `daftprompt-types`, the four akar
crates, `ureq` for HTTP, the `wgpu`/`winit`/`pollster` host integration akar does
not re-export, `glyphon` for explicit viewport clipping in the scrollable
block view, and `rfd` for the native folder dialog. It never links `daftprompt-core` or `daftprompt-api`, and it contains
no parsing, validation, or graph construction.

## Fonts

Two bundled SIL OFL fonts (files, versions, and checksums in
[development.md](development.md#bundled-gui-fonts); license texts in
`crates/daftprompt-gui/assets/fonts/`), declared in `src/fonts.rs`:

| role | font | used for |
| --- | --- | --- |
| UI | Inter (Regular, Medium, SemiBold) | headings (SemiBold), buttons including Clarify (Medium), badges (Medium), labels, relationship/answer-link lines, status and notice text, sidebar hint and footer, panel citations, metadata, notices, and errors |
| Text | Source Serif 4 (Regular) | composer and follow-up textareas, span quotes and unlabelled text in the main column, and the clarify panel's target, question, and explanation text |

Fonts load once at startup with `TextPipeline::load_font_bytes`; the family
handles are used as `FontFamily::Named`. akar cannot change its default
sans-serif family, so every draw names its family (`TextStyle.font_family`,
`ButtonStyle.text_style`, `akar_textarea_styled`, and glyphon `Attrs` for the
directly drawn badges and content). A font that fails to load is logged to
stderr and that role falls back to akar's generic family. Source text is drawn at
1.06x the theme base size (`TEXT_FONT_SCALE` in `render.rs`), and the height
estimates use the same size. The composer textareas use akar's fixed base size
and 1.2 line height.

## Build and run

The workspace pins the nightly toolchain akar needs (see
[development.md](development.md)). Build the app:

```sh
cargo build -p daftprompt-gui
```

The GUI starts the API for you. `cargo run -p daftprompt-gui` builds only the
GUI, not the API binary, so build it first:

```sh
cargo build -p daftprompt-api
cargo run -p daftprompt-gui
```

At startup, if `DAFTPROMPT_API_URL` is unset (the default local URL) and
`DAFTPROMPT_GUI_SPAWN_API` is not `0`, the GUI checks whether something already
listens on `127.0.0.1:8080` (a 200 ms connect probe). If nothing does, it spawns
the `daftprompt-api` binary found next to the GUI executable (cargo's target
directory), or at `DAFTPROMPT_API_BIN`. The child inherits the environment, the
working directory (so the API's own `.env` loading works), and stdout/stderr, so
its logs stay in the GUI's terminal. The GUI never waits for readiness: a parse
before the API is up shows the normal connection error. When the window closes
the child is killed and waited on. If the binary is missing or cannot start, the
idle and connection-error states say so; if the child exits early (for example
a missing API key) the GUI notices within about a second and shows that too.

To run the API yourself instead, set `DAFTPROMPT_GUI_SPAWN_API=0`, or start it
first (the GUI reuses a listener on the default address):

```sh
cargo run -p daftprompt-api
```

The API has no authentication and binds to loopback by default. Keep it there
unless you provide your own access control.

| variable | default | meaning |
| --- | --- | --- |
| `DAFTPROMPT_API_URL` | `http://127.0.0.1:8080/parse` | Endpoint the GUI posts to. Must be the full `/parse` URL; the `/clarify` URL is derived by replacing the last `/parse` segment |
| `DAFTPROMPT_GUI_SPAWN_API` | unset | `0` stops the GUI from spawning the API. Spawning also happens only when `DAFTPROMPT_API_URL` is unset |
| `DAFTPROMPT_API_BIN` | `daftprompt-api` beside the GUI executable | Path to the API binary to spawn |
| `DAFTPROMPT_GUI_TIMEOUT_SECS` | `300` | Client transport timeout for `/parse`. The API's own default attempt budget allows three 60-second model calls |
| `DAFTPROMPT_GUI_CLARIFY_TIMEOUT_SECS` | `420` | Client transport timeout for `/clarify`, which allows up to three 120-second attempts |

## Using the app

1. Paste chat, a plan, code, an email quotation, or document text into the
   textarea. The textarea is anchored at the bottom of the left column.
2. Choose **Parse text**. While one request is in flight the button reads
   **Parsing…** and further submissions are refused, so submissions cannot
   overlap.
3. The right column has four states: idle instructions, a loading message, the
   parsed blocks, or an explicit error. Each successful or failed submission
   replaces the previous result; nothing accumulates like a chat.
4. Copy and paste use the system clipboard (see below).
5. Scroll the right column with the wheel. A thin scrollbar appears when the
   content is taller than the viewport.

### Clipboard

With the textarea focused, **Cmd+V** (macOS) or **Ctrl+V** (elsewhere) pastes the
system clipboard's text at the cursor, replacing any selection, and **Cmd+C** /
**Ctrl+C** copies the selection. akar's textarea has no cut shortcut, so cut is
not supported; select and delete after copying instead. Non-text clipboard
content (an image, say) is ignored quietly; real failures such as an
unavailable clipboard appear as one line in the composer footer and clear on the
next successful clipboard action. akar's `normalize_paste` converts CRLF to LF
in the textarea, so pasted Windows line endings arrive as LF; the text is
otherwise inserted unchanged.

`clipboard.rs` holds the logic. The textarea reports `copy_text` and
`request_paste` in its response; `main.rs` records them during the frame and
serves them after the frame is presented (akar clears one-frame input in
`end_frame`). Copy writes the clipboard through `arboard`; paste reads it on the
main thread and queues `InputState::push_paste(textarea widget id, text)`, then
requests a redraw so the focused textarea applies it on the next frame. The
`arboard::Clipboard` is created on first use. The window does not enable IME,
and akar-winit ignores `Ime` events, so a paste cannot arrive twice; the host
also drops the character winit attaches to a Cmd/Ctrl shortcut key so Cmd+V
does not type a `v`.

Sample text for the demo:

```text
Should we cache tokens?
I suggest a five-minute cache.
@alex, please benchmark the cache.
```

Expected result: question **#1** remains `open`; recommendation **#2** shows
`recommendation for → #1 · Should we cache tokens?`; task **#3** shows
`task · open` and `@alex`. The question lists incoming recommendations separately
from `answered by` entries. An actual answer receives its own number and resolves
the question. Recommendations without a local question show no question target.
Explicit alternatives list their peers on both cards as `alternative to: #N · …`;
other suggestions have no implied alternative or preference.

Each span has one API number even when it has several labels. Numbers belong to
one result and restart on the next parse. A result with no spans remains valid.

## Projects

The top of the left sidebar is a project picker. It shows the current project's
name, or "No project", and every new `/parse` and `/clarify` request carries that
project's id as `project_id` and the selected path's id as `project_path_id`, so
the agents use that path's folder as their root (see
[api.md](api.md#projects-and-storage)). The GUI never sends a folder path to those
endpoints, and it keeps no project state of its own: the registry lives in the
API's SQLite database. Registering extra paths stays API-only
(`POST /projects/{id}/paths`); the GUI only chooses among registered ones.

- **Picker.** Clicking the button replaces the sidebar body with a list: "No
  project", the eight most recently opened registered projects (name, with the
  folder path shortened from the left underneath; an accent bar marks the
  current one), and "Choose folder…". Choosing a project calls
  `POST /projects/{id}/open` and makes it current when the reply arrives;
  "No project" clears the selection at once. Escape or a click outside closes the
  list (that click is not passed on to whatever lies beneath). akar paints all
  text above all ordinary quads, so a floating list would show the composer text
  through it; replacing the body avoids that.
- **Startup.** The GUI fetches `GET /projects` and preselects the first (most
  recently opened) entry, unless the user has already chosen something. That is
  how the last choice survives a restart. Because the GUI may have just started the
  API, a refused connection is retried for up to about ten seconds before an error
  is shown.
- **Choose folder…** opens a native dialog (`rfd::AsyncFileDialog::pick_folder`)
  with the window as parent. The call is made from the event loop (the main
  thread), which macOS requires: the dialog is an `NSOpenPanel` sheet serviced by
  the running `NSApplication`, so rendering continues. Only the returned future is
  moved to a worker thread, where `pollster::block_on` waits for the answer, which
  comes back through the project channel. The chosen folder goes to
  `POST /projects` (idempotent: an already registered folder returns the existing
  project) and then `POST /projects/{id}/open`; the result becomes current.
  Cancelling does nothing. Only one dialog is open at a time.
- **Drag and drop.** Dropping items anywhere on the window registers and selects
  the first directory among them, exactly like the dialog. While files hover over
  the window, an accent frame is drawn around it. Dropping only regular files
  registers nothing and shows "<name> is a file. Drop a folder to use it as the
  project." winit 0.30 reports one `DroppedFile` per path and no pointer position;
  akar-winit queues them as unpositioned window-level drops, which the render loop
  drains each frame.
- **Path selector.** When the current project has two or more registered paths,
  the meta line under the title on the Project tab becomes a row of buttons, one
  per path (folder name, `(default)` on the default path), with the current path
  highlighted. The default is preselected whenever a project becomes current.
  Clicking another button selects it and re-requests the three briefings for that
  path; a project with one path shows its folder as plain text. The API reports no
  availability for a registered folder, so a missing one appears only as that
  path's `project_unavailable` error in the briefing. If the buttons do not fit,
  the current one always stays visible and the rest are cut. The selection is not
  stored: choosing another project resets it to that project's default, and a
  path that disappears from the registry falls back to the default.
- **Requests.** The project id and path id (together, the selection stamp) are
  read when a request is built (Parse text, Ask, each briefing section) and moved
  into the worker thread. A reply is shown only if its stamp still equals the
  current selection; otherwise it is dropped: a parse goes back to idle, a clarify
  branch frees its pending question, and a briefing slot is never filled. A
  follow-up in an existing branch uses the selection current when it is asked.
- **Errors.** Project action failures (`not_found`, `forbidden`,
  `project_unavailable`, `invalid_request`, or a network error) appear in red in
  the hint's place at the top of the sidebar and clear on the next project action;
  the rest of the UI keeps working, and a failed selection keeps the previous
  project. Notices such as "dropped a file" use the warning colour. A
  `not_found` or `project_unavailable` reply to a parse or clarify is shown as
  that request's error as usual and also refreshes the project list; a project that
  has disappeared from the registry is deselected.

`projects.rs` holds the pure logic (selection, stale-reply tokens, startup
preselection, picker entries, drop classification); `main.rs` runs the HTTP calls
on worker threads exactly like `/parse` and polls a third channel; `render.rs`
paints the list and the drop frame; `api.rs` has `list_projects`,
`create_project`, `open_project` and decodes the API's `{kind, detail}` bodies
generically so project error kinds survive.

## Project briefing (epic 006)

The main column has a tab bar, **Project** and **Parse**, above its title, and a
fixed command footer below. The **Parse** tab is exactly the source view and
clarify panel described above. A project becoming current (picker, folder dialog,
drop, or the launch preselection) switches to **Project**; submitting a parse
switches to **Parse**; the user can click either tab at any time. The app opens
on Project.

When a project becomes current, or the selected path changes, the GUI sends
`POST /projects/{id}/history`, `/overview` and `/commands` concurrently (default
tasks, body `{"project_path_id": ...}` with `refresh` left false), each on its own
worker thread (`ParseClient::history`, `overview`, `commands`, 420 s timeout), and
polls a fourth channel. The GUI caches nothing itself, but these ordinary opens
let the API answer with a verified stored response instead of running the model.
"No project" clears the briefing.

`briefing.rs` is the pure state: `Briefing { project_id, path_id, generation,
history, overview, commands }` with each slot `Idle | Loading | Ready | Failed`.
Every request is stamped with the project id, the path id, the generation and a
per-section sequence number; a reply that no longer matches (another project or
path, an older generation, or a section re-run since) is dropped. A section's
**Re-run** button re-sends only that request with `"refresh": true`, which skips
reuse and runs the agent again; it is disabled (`Running…`) while it loads.
Replies are `Delivered<T>` bodies; the `cache` metadata is kept per section.
`briefing_view.rs` is the pure view model (blocks, Markdown subset, citations,
footer overflow math, command details); `briefing_ui.rs` lays out and paints it.

- **Project tab.** The title is the project name and the meta line the selected
  path's folder (or the path selector above).
  Two stacked sections follow: "Recent work" (Chronicler) and "Project overview"
  (Surveyor). Each shows a loading line, its error in the error colour, or the
  result: notices first (for example "Not a git repository"), then the summary,
  then citations as `[n] path:L3–L9` or `[n] commit abc1234 subject` (the line
  format of the clarify panel). When the API reused a stored response
  (`cache.kind` `same_path` or `other_path`), a muted line under the section title
  says `Reused from <folder> · captured 2026-11-08 10:30 UTC`; a fresh result shows
  nothing extra. The commands' reuse note is the first line of the "+N more"
  list. The stack scrolls and is clipped to its viewport
  like the source view. With no project the tab shows a hint to choose or drop a
  folder.
- **Markdown level.** A deliberately small subset: `#` headings (one heading
  style), paragraphs, `-`/`*`/`+`/`1.` list items (indented items shown as nested,
  continuation lines joined), and fenced code blocks in the monospace face.
  `**bold**` markers are removed; other inline markup (backticks, links,
  emphasis, tables) shows as written. The `[n]` markers in the text are untouched.
- **Command footer.** A fixed 44 px strip at the bottom of the **main column**
  (not the whole window: the left column's composer is anchored to the bottom and
  has its own footer text). It shows one row of buttons, one per command, labelled
  with `label`; "Finding commands…" while loading; a short error with Re-run on
  failure; "No commands found for this project." when empty; and a Re-run button at
  its right end. When the buttons do not fit, the row is cut to the buttons that
  leave room for a **+N more** button (it never wraps).
- **Details area.** Clicking a command button (or a row of the "+N more" list)
  opens a details card showing the label, the command (monospace), working
  directory ("project root" when empty), kind, origin (detected from a manifest, or
  documented in the project) and the cited evidence lines. **+N more** opens the
  same card as a list of all commands. Clicking never runs or copies anything. Escape, the
  card's **Close** button, clicking the same button again, a press outside the card
  (that click is swallowed), or switching tab or project closes it.
- **Why the card replaces the body.** akar paints all text above all ordinary
  quads, so a floating popover would show the summary or source text through its
  card (the same limitation as the project picker). The card therefore covers the
  main column body (the rectangle of the content row), where nothing else draws
  while it is open; the tab bar, title and footer lie outside it, so no other text
  can overlap it. The clarify panel is collapsed while the card is open and on the
  Project tab.

## Clarification branches (epic 004)

Every span card has a **Clarify** action. It opens a panel beside the source
view (the source column narrows; the blocks and cards are otherwise unchanged).
A card that has a branch shows an accent bar on its left edge and its button
reads `Branch · N turns` (or `Branch · asking…`); the button reopens the panel.
The panel shows:

- the target preview (`#N · <exact span text>`);
- the turns in order: `Q:` question, the explanation as plain text with its
  `[n]` markers untouched, then its citation list, and a grounding line
  (`Project-grounded` or `Text only`);
- citations as the API computed them: `[1] src/limiter.rs:L12–L30` for files
  (`path:L5` for one line), and `[2] #1 · <source text>` for a source citation
  whose byte range lies inside span `#1` (`source · <text>` when it lies in no
  span). The GUI maps source byte ranges to the submitted text only; it never
  reads a project file. A marker in the explanation with no listed citation is
  flagged, and an unusable range or line range is shown as unavailable;
- the in-flight state (`Explaining…`), or the error under the earlier turns
  (earlier turns and the typed question are kept so the question can be retried);
- a follow-up textarea and an **Ask** / **Ask follow-up** button.

Branches live only in memory, per span `#N` of the current parse result. A new
parse (or a failed one) discards all of them and closes the panel; each branch
keeps its unsent draft while another branch is open. Each branch allows one
request at a time (the button reads `Explaining…` and is refused while one is in
flight); different branches can run concurrently. Replies carry a per-request
token, so a reply for a discarded branch or an older request is ignored.

`clarify.rs` is pure logic, like `view.rs`: `build_clarify_request` builds the
`POST /clarify` body from the parse result (`source.text` and `payload_hash`
from `payload`, `source.metadata` from the parse source, the target from the
matching `spans` entry, `turns` empty on the first turn and the previous
`question_node_id` / `explanation_node_id` pairs afterwards), `Branches` holds
the state machine, and `build_branch_view` / `panel_blocks` produce the view
model. `render.rs` only lays out and paints it. The request runs on a worker
thread exactly like `/parse` (`ParseClient::clarify`, the same `ureq` client)
and reports through a second channel that the render loop polls with
`try_recv`; the loop wakes every 50 ms while any branch is pending. API errors
(`payload_mismatch`, `invalid_target`, `invalid_output`, ...) decode into the
same `ApiError` and appear in the panel. The window's minimum width is 960 px so
the panel stays readable.

## How the request stays off the render thread

`api.rs` is a small blocking `ureq` client. `main.rs` spawns one worker thread per
submission and sends `(submission id, Result<ParseResult, ApiError>)` over a
standard channel. The render loop only polls it with `try_recv`; it never waits
for HTTP. While loading, `about_to_wait` sets `ControlFlow::WaitUntil(now + 50 ms)`
and requests a redraw, so the channel is polled on a short wake deadline as
akar-winit recommends. The textarea cursor uses a separate 500 ms wake when it
has focus, and the loop otherwise sleeps with `ControlFlow::Wait`.

Results carry a submission id. `ParseView` ignores an outcome whose id is no
longer current, so a late result can never overwrite a newer one. The state
machine also clears the old result when a new submission begins.

## Rendering and source fidelity

`view.rs` is pure logic. It sorts the API's `spans` byte ranges, checks bounds
and UTF-8 `char` boundaries defensively, and walks `payload.content` from byte 0
to the end. Text outside a span becomes an unlabelled block; each span slices the
exact original bytes, including whitespace-only gaps. Answer and recommendation
links resolve node IDs to API display numbers and short source previews.
Questions list incoming `answered by:` and `recommendations:` separately;
`alternative to:` appears at both ends of each explicit alternative pair.
The client never renumbers spans: invalid zero or duplicate numbers and ambiguous
node IDs are left as unlabelled source text, as are invalid or overlapping ranges.
Unavailable relationship targets are identified as unavailable.

`render.rs` builds a separate akar layout for the source blocks only when a new
result arrives or the main column width changes. Scrolling changes that layout's
screen origin under a fixed viewport. Paragraph heights use a conservative wrap
estimate because akar measures shaped text through its own font pipeline; the
trade-off is occasional extra vertical space rather than clipped text. Block
text uses an explicit viewport clip through glyphon (the same shaping version
akar uses), so a partly scrolled block cannot paint over the title or composer.
Block backgrounds are intersected with the viewport for the same reason.

## Tests and checks

```sh
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The GUI's deterministic tests cover:

- idle/loading/success/error transitions, one submission at a time, replacement
  of the previous result, and ignoring stale outcomes;
- source-order blocks, unlabelled gaps, answer targets, `answered by`, Unicode
  boundaries, overlapping and out-of-bounds spans, and whitespace-only text;
- display excerpts and run/provenance summaries;
- API error bodies, client-side transport/decode errors, and a canned local
  HTTP exchange that decodes a `ParseResult`;
- the two-column layout at the default and minimum window sizes, including the
  textarea sitting above the Parse button;
- the spawn decision (custom URL, disabled flag, existing listener) and API
  binary path resolution, as pure functions with no subprocess;
- clipboard request mapping, paste/copy against a fake clipboard (the real one is
  never touched), quiet handling of empty or non-text content, and error
  messages instead of panics;
- content layout stacking and viewport clipping for scrolled blocks;
- projects: selection, stale and superseded replies, startup preselection of the
  most recent project, picker entries, dropped file vs directory vs several
  items, `project_id` and `project_path_id` on built parse and clarify requests,
  default and chosen path, path chips, and project error kinds decoded from
  `{kind, detail}` bodies;
- briefing: three slots loading on open, stale and superseded replies dropped
  (including a late reply for another path of the same project), `refresh` only on
  per-section re-run, path and refresh in the agent request body, `Delivered`
  decoding with and without metadata, the reuse note text, cancelled parse and
  clarify requests, clearing, tab switching on project open and parse submit,
  the Markdown subset, citation and notice text, footer overflow math, command
  details text, block layout stacking and viewport clipping, and the briefing URLs;
- clarification: request construction from a parse result (first turn, follow-up
  with earlier node ids, blank question, unknown span), one request per branch,
  stale replies, failures keeping earlier turns, malformed results (empty
  explanation, missing node ids) becoming errors, citation rendering (files,
  source ranges mapped to `#N`, invalid ranges, unlisted markers, Unicode
  boundaries), panel block order, the `/clarify` URL and a canned HTTP exchange
  with a `payload_mismatch` error, panel layout beside the source view, and
  panel clipping.

No test needs a model or provider credentials.

## Live run status

A live end-to-end run was **not performed** on 2026-10-31: no provider
credentials were available in the workspace environment. Consequently this
document records no observed extraction quality for the GUI. This is the same
limitation recorded by the [Scribe baseline](evaluations/scribe-baseline.md).

To perform the run later:

1. Put credentials in `.env` (see `.env.example`) and build the API:
   `cargo build -p daftprompt-api`.
2. Start the GUI (`cargo run -p daftprompt-gui`; it spawns the API) and parse the sample above.
3. Record what the blocks actually show: missed or false questions, span text
   that includes or excludes too much, wrong statuses, answer links, assignees,
   invalid-output rate, attempts, and latency. A useful report should say which
   model/provider and prompt version produced the result; the main pane already
   shows the run summary from the API.
