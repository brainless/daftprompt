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

/// A request to clarify one extracted span of an earlier parse result.
///
/// Like [`ParseRequest`], it is stateless: the client resends the source
/// text, the target, and the branch's earlier turns. All text is data, never
/// instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyRequest {
    /// The original submitted text and its identity.
    pub source: ClarifySource,
    /// The extracted span being asked about.
    pub target: ClarifyTarget,
    /// Earlier questions and explanations in this branch, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<ClarifyTurn>,
    /// The user's question, preserved unchanged.
    pub question: String,
    /// The human identity asking. Otherwise the question author is `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

impl ClarifyRequest {
    /// The question node author: the supplied author, otherwise `unknown`.
    pub fn question_author(&self) -> &str {
        self.author
            .as_deref()
            .filter(|author| !author.trim().is_empty())
            .unwrap_or("unknown")
    }
}

/// The source text a clarification refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifySource {
    /// The complete original text.
    pub text: String,
    /// The payload hash from the parse result. Rust recomputes it from `text`
    /// and rejects a mismatch.
    pub payload_hash: String,
    /// Provenance from the parse result, preserved unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SourceMetadata>,
}

/// The extracted span a clarification is about.
///
/// `node_id` is an opaque client-supplied reference: without persistence the
/// API cannot confirm it exists. `display_id` is for display only and is
/// never an identity. The byte range is half-open and must be in bounds, on
/// `char` boundaries, and non-empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyTarget {
    pub node_id: String,
    pub display_id: u32,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// One earlier question and explanation in a branch.
///
/// The node IDs are opaque client-supplied references, recorded as given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyTurn {
    pub question_node_id: String,
    pub question: String,
    pub explanation_node_id: String,
    pub explanation: String,
}
