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

Start the API in another terminal. It needs provider credentials and builds the
Scribe client itself:

```sh
OPENAI_API_KEY=... cargo run -p daftprompt-api
```

Then start the app:

```sh
cargo run -p daftprompt-gui
```

Press `Ctrl+C` in the API terminal to stop it; close the window to stop the app.
The API has no authentication and binds to loopback by default. Keep it there
unless you provide your own access control.

| variable | default | meaning |
| --- | --- | --- |
| `DAFTPROMPT_API_URL` | `http://127.0.0.1:8080/parse` | Endpoint the GUI posts to. Must be the full `/parse` URL |
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
4. Scroll the right column with the wheel. A thin scrollbar appears when the
   content is taller than the viewport.

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
- content layout stacking and viewport clipping for scrolled blocks.

No test needs a model or provider credentials.

## Live run status

A live end-to-end run was **not performed** on 2026-10-31: no provider
credentials were available in the workspace environment. Consequently this
document records no observed extraction quality for the GUI. This is the same
limitation recorded by the [Scribe baseline](evaluations/scribe-baseline.md).

To perform the run later:

1. Start the API with a configured small model, for example
   `OPENAI_API_KEY=... DAFTPROMPT_API_MODEL=gpt-4o-mini cargo run -p daftprompt-api`.
2. Start the GUI and parse the sample above.
3. Record what the blocks actually show: missed or false questions, span text
   that includes or excludes too much, wrong statuses, answer links, assignees,
   invalid-output rate, attempts, and latency. A useful report should say which
   model/provider and prompt version produced the result; the main pane already
   shows the run summary from the API.
