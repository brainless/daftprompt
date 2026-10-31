# Native GUI

`daftprompt-gui` is the akar desktop app for the first parsing experiment. Two
columns: a composer on the left with a multiline textarea anchored at the bottom
and a Parse button, and a wider source view on the right. The right column shows
the original text as blocks, not a chat transcript: validated spans become cards
with label badges, statuses, assignee handles, and answer targets, and unlabelled
text between them stays visible in source order.

The app only talks to the API. It depends on `daftprompt-types`, the four akar
crates, `ureq` for HTTP, the `wgpu`/`winit`/`pollster` host integration akar does
not re-export, and `glyphon` for explicit viewport clipping in the scrollable
block view. It never links `daftprompt-core` or `daftprompt-api`, and it contains
no parsing, validation, or graph construction.

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
| `DAFTPROMPT_API_URL` | `http://127.0.0.1:8080/parse` | Endpoint the GUI posts to. Must be the full `/parse` URL |
| `DAFTPROMPT_GUI_SPAWN_API` | unset | `0` stops the GUI from spawning the API. Spawning also happens only when `DAFTPROMPT_API_URL` is unset |
| `DAFTPROMPT_API_BIN` | `daftprompt-api` beside the GUI executable | Path to the API binary to spawn |
| `DAFTPROMPT_GUI_TIMEOUT_SECS` | `300` | Client transport timeout. The API's own default attempt budget allows three 60-second model calls |

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

Sample text for the demo (the API's own sample is similar):

```text
Plan
Should we cache tokens? @infra-team
Yes, cache the tokens in Redis.

Deploy on Friday.
```

Expected result: two span cards in source order separated by the unlabelled
lines. The question card shows a `question · resolved` badge and an `@infra-team`
badge. The answer card shows an `answer` badge and an `answers → Should we cache
tokens? @infra-team` line. The plan and deployment lines stay unlabelled.

A question with no local answer shows `question · open` instead. A span with no
labels is impossible in a valid result: every span must be referenced by an
annotation. A result with no spans is valid and renders one unlabelled block.

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
exact original bytes. Answer labels show the text of their target question.
Questions also list any locally validated answer spans as `answered by:`.

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
- content layout stacking and viewport clipping for scrolled blocks.

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
