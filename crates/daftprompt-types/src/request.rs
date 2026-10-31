//! Requests accepted from any client (GUI, CLI, future connectors).

use serde::{Deserialize, Serialize};

/// A request to parse one piece of text.
///
/// The text is treated as data to classify, never as instructions to execute.
/// Each submission is independent: there is no conversation history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseRequest {
    /// The complete original text. It is preserved unchanged in the result
    /// payload.
    pub text: String,
    /// Optional provenance. Absent when the caller does not know or does not
    /// supply it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceMetadata>,
}

impl ParseRequest {
    /// A request without source metadata.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            source: None,
        }
    }

    /// A request with provenance.
    pub fn with_source(text: impl Into<String>, source: SourceMetadata) -> Self {
        Self {
            text: text.into(),
            source: Some(source),
        }
    }

    /// The root node author: the supplied source author, otherwise `unknown`.
    ///
    /// The author is never derived from the text itself.
    pub fn root_author(&self) -> &str {
        self.source
            .as_ref()
            .and_then(|source| source.author.as_deref())
            .filter(|author| !author.trim().is_empty())
            .unwrap_or("unknown")
    }
}

/// Where submitted text came from.
///
/// This is provenance for an already-captured payload. Pasted file contents
/// stay a captured payload; the metadata is not a claim that a verified file
/// exists on disk. Fetching files, clipboards, and email is later work.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMetadata {
    /// The kind of source, when the caller knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SourceKind>,
    /// Opaque reference for the source, such as a file path, message ID, or
    /// URL. Never used to read the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// The human identity that wrote the text, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// The kind of place the submitted text was copied from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Chat messages, including agent replies.
    Chat,
    /// Contents of a file, as captured at submission time.
    File,
    /// Clipboard contents.
    Clipboard,
    /// Email bodies and quotations.
    Email,
    /// Anything else.
    Other,
}
