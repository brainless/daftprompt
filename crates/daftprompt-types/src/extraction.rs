//! The model-facing extraction contract.
//!
//! This is the schema handed to Rig's schema-defined `submit` output tool.
//! The model proposes spans by block ID or segment reference plus an optional
//! verbatim quote, and annotations by referencing span IDs. It never produces byte
//! offsets, IDs of graph records, statuses, or timestamps.
//!
//! `schemars` derives the tool schema; `serde` deserializes the submitted
//! arguments. Both are kept in sync by deriving on the same types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One complete extraction result.
///
/// An empty annotation list is valid: unlabelled text needs no span.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Extraction {
    /// Candidate spans of the input. Every span must be referenced by at
    /// least one annotation.
    #[serde(default)]
    pub spans: Vec<ExtractedSpan>,
    /// Labels proposed for those spans.
    #[serde(default)]
    pub annotations: Vec<ExtractedAnnotation>,
}

/// A source span proposed by the model.
///
/// Select either a Rust-built block or 1-based inclusive segment numbers with
/// an optional quote. Mixed reference forms are rejected during deserialization.
/// IDs are local to this extraction; Rust maps them to generated node IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ExtractedSpan {
    /// Select one complete Rust-built block. No quote or segment range is allowed.
    Block {
        /// Extraction-local identifier referenced by annotations.
        id: String,
        /// Local block identifier, for example `b8`.
        block: String,
    },
    /// Select an inclusive segment range, optionally narrowed by a quote.
    Segments {
        /// Extraction-local identifier referenced by annotations.
        id: String,
        /// First segment, 1-based and inclusive.
        start_segment: u32,
        /// Last segment, 1-based and inclusive.
        end_segment: u32,
        /// Optional verbatim excerpt occurring exactly once in the range.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quote: Option<String>,
    },
}

impl ExtractedSpan {
    /// The extraction-local identifier shared by both reference forms.
    pub fn id(&self) -> &str {
        match self {
            Self::Block { id, .. } | Self::Segments { id, .. } => id,
        }
    }
}

/// A label attached to one span.
///
/// Recommendations may link local questions and explicitly offered alternatives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractedAnnotation {
    /// The label kind.
    pub kind: AnnotationKind,
    /// The span this label is attached to, by local span ID.
    pub span: String,
    /// For `answer`: the question span IDs this span answers. Empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<String>,
    /// Local question targets for a recommendation. Empty for other kinds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,
    /// Local recommendation alternatives. Submit each unordered pair once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<String>,
    /// For `question` or `task`: an explicitly assigned handle, `@username` or
    /// `@team-name`. A mention alone is not an assignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
}

/// Annotation kinds the first parser may propose.
///
/// `conflict`, `supersedes`, and `stale` are deliberately absent: they cannot
/// be inferred from isolated text without dependency evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    /// The span is an open question.
    Question,
    /// The span answers one or more question spans.
    Answer,
    /// A suggested approach or offered option.
    Recommendation,
    /// An outstanding requested or committed action.
    Task,
}

/// One complete Explainer result, submitted through its `submit` output tool.
///
/// The model returns an explanation and the sources it used. It never
/// produces IDs, hashes, byte offsets, labels, or statuses: Rust validates
/// every citation and builds the records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExplainerExtraction {
    /// Non-empty plain-text explanation. It may reference citations as `[1]`,
    /// `[2]`, ... numbered from 1 in `citations` order.
    pub explanation: String,
    /// Sources the explanation relied on. Empty is valid when nothing
    /// relevant was found.
    #[serde(default)]
    pub citations: Vec<ExplainerCitation>,
}

/// A source cited by an explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExplainerCitation {
    /// Lines of a project file. The range must lie within lines a `read_file`
    /// call returned in the same run.
    File {
        /// Path relative to the project root.
        path: String,
        /// First line, 1-based and inclusive.
        start_line: u32,
        /// Last line, 1-based and inclusive.
        end_line: u32,
    },
    /// Segments of the source window the agent was given.
    Source {
        /// First segment, 1-based and inclusive, as numbered in the prompt.
        start_segment: u32,
        /// Last segment, 1-based and inclusive.
        end_segment: u32,
    },
}
