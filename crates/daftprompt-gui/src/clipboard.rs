//! System clipboard support for the composer textarea.
//!
//! akar's textarea does not touch the clipboard itself. Its response carries a
//! `copy_text` string (the selection when the copy shortcut was pressed) and a
//! `request_paste` flag (the paste shortcut was pressed). The host performs the
//! clipboard I/O and, for paste, feeds the text back with
//! `InputState::push_paste(widget_id, text)`, which the focused textarea applies
//! on its next frame.
//!
//! This module keeps that logic free of the window and of the real clipboard:
//! [`ClipboardBackend`] abstracts the OS, [`service`] maps a request onto a
//! backend, and [`SystemClipboard`] is the lazy `arboard` implementation. akar's
//! textarea exposes no cut shortcut, so cut is not supported.

use akar_components::TextAreaResponse;

/// What the textarea asked for in one frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ClipboardRequest {
    /// Selection to place on the clipboard.
    pub copy: Option<String>,
    /// Whether the clipboard text should be pasted into the textarea.
    pub paste: bool,
}

impl ClipboardRequest {
    /// Extracts the clipboard intent from a textarea response.
    pub fn from_response(response: &TextAreaResponse) -> Self {
        Self {
            copy: response.copy_text.clone().filter(|text| !text.is_empty()),
            paste: response.request_paste,
        }
    }

    /// Whether there is anything to do.
    pub fn is_empty(&self) -> bool {
        self.copy.is_none() && !self.paste
    }
}

/// A clipboard failure, already phrased for the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardError(pub String);

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ClipboardError {}

/// Text access to a clipboard.
pub trait ClipboardBackend {
    /// Reads text. `Ok(None)` means the clipboard is empty or holds no text,
    /// which is not an error.
    fn read_text(&mut self) -> Result<Option<String>, ClipboardError>;
    /// Replaces the clipboard contents with `text`.
    fn write_text(&mut self, text: &str) -> Result<(), ClipboardError>;
}

/// The result of serving one request.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ClipboardOutcome {
    /// Text to queue as a paste for the textarea.
    pub paste: Option<String>,
    /// A failure to surface, if any. Non-text content is never an error.
    pub error: Option<String>,
}

/// Serves `request` against `backend`. Copy runs before paste, so a single
/// frame that asks for both pastes what it just copied.
pub fn service(backend: &mut dyn ClipboardBackend, request: ClipboardRequest) -> ClipboardOutcome {
    let mut outcome = ClipboardOutcome::default();
    if let Some(text) = request.copy
        && let Err(error) = backend.write_text(&text)
    {
        outcome.error = Some(format!("Copy failed: {error}"));
    }
    if request.paste {
        match backend.read_text() {
            Ok(text) => outcome.paste = text.filter(|text| !text.is_empty()),
            Err(error) => outcome.error = Some(format!("Paste failed: {error}")),
        }
    }
    outcome
}

/// Drops the characters a shortcut keypress contributed.
///
/// On macOS winit reports `text: Some("v")` for Cmd+V, and akar-winit forwards
/// it as committed text, which would insert a literal `v` next to the paste.
/// `before` is the length of `chars` before the key event was translated.
pub fn discard_shortcut_chars(
    chars: &mut Vec<char>,
    before: usize,
    modifiers: akar_core::Modifiers,
) {
    let shortcut = if cfg!(target_os = "macos") {
        modifiers.super_key
    } else {
        // Ctrl+Alt is AltGr on some layouts and does type characters.
        modifiers.control && !modifiers.alt
    };
    if shortcut && chars.len() > before {
        chars.truncate(before);
    }
}

/// The OS clipboard through `arboard`, created on first use.
///
/// Creation and every call happen on the main thread in response to a key
/// event, which is what arboard expects on macOS.
#[derive(Default)]
pub struct SystemClipboard {
    inner: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    fn open(&mut self) -> Result<&mut arboard::Clipboard, ClipboardError> {
        if self.inner.is_none() {
            let clipboard = arboard::Clipboard::new().map_err(describe)?;
            self.inner = Some(clipboard);
        }
        Ok(self.inner.as_mut().expect("clipboard was just created"))
    }
}

impl ClipboardBackend for SystemClipboard {
    fn read_text(&mut self) -> Result<Option<String>, ClipboardError> {
        match self.open()?.get_text() {
            Ok(text) => Ok(Some(text)),
            // Empty, or an image/file list: nothing to paste, nothing to report.
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(describe(error)),
        }
    }

    fn write_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.open()?.set_text(text.to_owned()).map_err(describe)
    }
}

fn describe(error: arboard::Error) -> ClipboardError {
    ClipboardError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        text: Option<String>,
        fail: bool,
    }

    impl ClipboardBackend for Fake {
        fn read_text(&mut self) -> Result<Option<String>, ClipboardError> {
            if self.fail {
                return Err(ClipboardError("unavailable".into()));
            }
            Ok(self.text.clone())
        }

        fn write_text(&mut self, text: &str) -> Result<(), ClipboardError> {
            if self.fail {
                return Err(ClipboardError("unavailable".into()));
            }
            self.text = Some(text.to_owned());
            Ok(())
        }
    }

    #[test]
    fn a_response_maps_to_copy_and_paste_intent() {
        let response = TextAreaResponse {
            changed: false,
            copy_text: Some("sel".into()),
            request_paste: true,
        };
        let request = ClipboardRequest::from_response(&response);
        assert_eq!(request.copy.as_deref(), Some("sel"));
        assert!(request.paste);

        let idle = ClipboardRequest::from_response(&TextAreaResponse::default());
        assert!(idle.is_empty());

        let empty_copy = TextAreaResponse {
            copy_text: Some(String::new()),
            ..TextAreaResponse::default()
        };
        assert!(ClipboardRequest::from_response(&empty_copy).is_empty());
    }

    #[test]
    fn copy_writes_and_paste_reads_the_clipboard() {
        let mut fake = Fake::default();
        let outcome = service(
            &mut fake,
            ClipboardRequest {
                copy: Some("héllo\nworld".into()),
                paste: false,
            },
        );
        assert_eq!(outcome, ClipboardOutcome::default());
        assert_eq!(fake.text.as_deref(), Some("héllo\nworld"));

        let outcome = service(
            &mut fake,
            ClipboardRequest {
                copy: None,
                paste: true,
            },
        );
        assert_eq!(outcome.paste.as_deref(), Some("héllo\nworld"));
        assert_eq!(outcome.error, None);
    }

    #[test]
    fn an_empty_or_non_text_clipboard_pastes_nothing_quietly() {
        let paste = ClipboardRequest {
            copy: None,
            paste: true,
        };
        let mut none = Fake::default();
        assert_eq!(service(&mut none, paste), ClipboardOutcome::default());

        let mut blank = Fake {
            text: Some(String::new()),
            fail: false,
        };
        let paste = ClipboardRequest {
            copy: None,
            paste: true,
        };
        assert_eq!(service(&mut blank, paste), ClipboardOutcome::default());
    }

    #[test]
    fn backend_failures_become_status_messages_not_panics() {
        let mut fake = Fake {
            text: None,
            fail: true,
        };
        let outcome = service(
            &mut fake,
            ClipboardRequest {
                copy: Some("x".into()),
                paste: true,
            },
        );
        assert_eq!(outcome.paste, None);
        assert_eq!(outcome.error.as_deref(), Some("Paste failed: unavailable"));

        let outcome = service(
            &mut fake,
            ClipboardRequest {
                copy: Some("x".into()),
                paste: false,
            },
        );
        assert_eq!(outcome.error.as_deref(), Some("Copy failed: unavailable"));
    }

    #[test]
    fn shortcut_keypresses_do_not_type_their_letter() {
        let shortcut = if cfg!(target_os = "macos") {
            akar_core::Modifiers {
                super_key: true,
                ..Default::default()
            }
        } else {
            akar_core::Modifiers {
                control: true,
                ..Default::default()
            }
        };
        let mut chars = vec!['a', 'v'];
        discard_shortcut_chars(&mut chars, 1, shortcut);
        assert_eq!(chars, ['a']);

        let mut chars = vec!['a', 'v'];
        discard_shortcut_chars(&mut chars, 1, akar_core::Modifiers::default());
        assert_eq!(chars, ['a', 'v']);
    }
}
