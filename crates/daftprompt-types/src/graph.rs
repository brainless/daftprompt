//! In-memory graph records, aligned with `docs/data-model.md`.
//!
//! These mirror the future SQLite tables, with two differences that are
//! recorded in the data model:
//!
//! - [`NodeContext::target`] is a tagged payload/file target rather than two
//!   nullable foreign-key columns; exactly one target is always set.
//! - [`Run`] carries the provider, model, prompt version, and attempt count
//!   the parsing epic requires, which the run table stores as columns.
//!
//! IDs, hashes, and timestamps are strings so the records stay plain data.
//! Rust (not the model) generates them.

use serde::{Deserialize, Serialize};

use crate::request::SourceMetadata;

/// Immutable text.
///
/// `hash` is the lowercase hex SHA-256 digest of `content`; it is the
/// payload's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    pub hash: String,
    pub content: String,
    pub created_at: String,
}

/// A relationship in the graph. A node never carries text: its content is
/// defined by its [`NodeContext`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    /// `None` only for a tree root. A standalone import may be a root even
    /// when it was not a user prompt.
    pub parent_id: Option<String>,
    /// A human identity, or `agent`. The root uses the source author or
    /// `unknown`; span nodes use `agent`.
    pub author: String,
    /// Set when an agent produced this node.
    pub run_id: Option<String>,
    pub created_at: String,
}

/// The content a node points at: a payload or a file, optionally narrowed to
/// a range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextTarget {
    /// A range of an immutable payload. `None` range means the whole payload.
    Payload { payload_hash: String },
    /// A file relative to the project root. `content_hash` is the file hash at
    /// capture time; range drift is detected by comparing hashes later.
    File {
        path: String,
        content_hash: Option<String>,
    },
}

/// A range in a payload or file.
///
/// Lines are 1-based and counted by `\n`; columns are 1-based and counted in
/// Unicode scalar values (`char`s). The end position is exclusive: it is the
/// position just after the last character, so a span ending at the end of a
/// five-character line has `end_col = 6`. CRLF text is preserved unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// One piece of content referenced by a node, ordered by `seq`.
///
/// A sub-part of a text is a node whose context points at a range of an
/// existing payload. Text is never copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeContext {
    pub id: String,
    pub node_id: String,
    pub seq: u32,
    pub target: ContextTarget,
    /// `None` means the whole payload or file.
    pub range: Option<SourceRange>,
}

/// A fixed-vocabulary annotation on the graph.
///
/// Unary kinds describe one node (`dst_node_id` is `None`); binary kinds
/// relate two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelKind {
    /// Unary: the source node is an open question.
    Question,
    /// Binary: the source answers the destination question.
    Answer,
    /// Unary: a suggested approach or offered option.
    Recommendation,
    /// Binary: a recommendation addresses a local question.
    RecommendationFor,
    /// Symmetric binary: explicitly offered alternative recommendations.
    AlternativeTo,
    /// Unary: an outstanding action, initially open.
    Task,
    /// Binary: both nodes cannot hold at the same time.
    Conflict,
    /// Binary: the source replaces the destination.
    Supersedes,
    /// Unary: the source was built on superseded or changed content.
    Stale,
}

impl LabelKind {
    /// Whether the kind is a relationship between two nodes.
    pub fn is_binary(self) -> bool {
        matches!(
            self,
            Self::Answer
                | Self::RecommendationFor
                | Self::AlternativeTo
                | Self::Conflict
                | Self::Supersedes
        )
    }

    /// Whether the kind blocks completion while `open` or `deferred`.
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            Self::Question | Self::Task | Self::Conflict | Self::Stale
        )
    }
}

/// The workflow state of a blocking label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelStatus {
    Open,
    Deferred,
    Resolved,
    Dismissed,
}

/// An annotation record.
///
/// Statuses are derived by Rust, never proposed by the model: a question with
/// at least one validated local answer is `resolved`, otherwise `open`;
/// non-blocking kinds have `None` status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub id: String,
    pub kind: LabelKind,
    pub src_node_id: String,
    /// `None` for unary kinds.
    pub dst_node_id: Option<String>,
    /// `None` for non-blocking kinds.
    pub status: Option<LabelStatus>,
    /// An opaque `@username` or `@team-name` handle. At most one per label.
    pub assignee: Option<String>,
    /// Optional node explaining the label.
    pub note_node_id: Option<String>,
    pub created_by: String,
    pub run_id: Option<String>,
    pub created_at: String,
}

/// Lifecycle status of one agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
}

/// Run log entry. It is stored alongside the graph but is not part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    /// Which agent or mode, for example `scribe`.
    pub agent: String,
    /// Provider identifier, for example `openai`.
    pub provider: Option<String>,
    /// Model identifier, for example `gpt-4o-mini`.
    pub model: Option<String>,
    /// Identifier of the prompt revision used, for example `scribe-v1`.
    pub prompt_version: Option<String>,
    pub started_at: String,
    /// `None` while the run is in progress.
    pub ended_at: Option<String>,
    pub status: RunStatus,
    /// Total attempts used, including failed deserialization and validation
    /// attempts. Always at least 1 for a started run.
    pub attempts: u32,
}

/// One node an agent run took as input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInput {
    pub run_id: String,
    pub node_id: String,
}

/// One tool call made during a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub run_id: String,
    pub seq: u32,
    /// Tool name, for example `submit`.
    pub tool: String,
    /// JSON arguments, kept as text.
    pub input: String,
    /// Payload holding the tool output, when the tool produced one.
    pub output_hash: Option<String>,
    pub created_at: String,
}

/// A span node's byte range in the root payload.
///
/// Byte ranges are half-open (`[start, end)`) and always fall on `char`
/// boundaries of the payload's UTF-8 text. Node contexts carry the same span
/// as line/column coordinates; this list lets clients render source blocks
/// without recomputing coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanInfo {
    /// Result-local consecutive source-order number, beginning at 1.
    pub display_id: u32,
    pub node_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// The validated graph fragment produced by one parse.
///
/// Each submission replaces the previous in-memory result. There is no
/// persistence and no conversation history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseResult {
    /// The immutable payload holding the complete original text.
    pub payload: Payload,
    /// Provenance supplied with the request, preserved unchanged. `None` when
    /// the request had none.
    pub source: Option<SourceMetadata>,
    pub nodes: Vec<Node>,
    pub node_contexts: Vec<NodeContext>,
    pub labels: Vec<Label>,
    /// Byte ranges and display IDs of span nodes, in source order.
    pub spans: Vec<SpanInfo>,
    /// Run metadata for the Scribe run that produced this fragment.
    pub run: Run,
    pub run_inputs: Vec<RunInput>,
    pub tool_calls: Vec<ToolCall>,
}
