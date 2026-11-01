//! The model-facing extraction contract.
//!
//! This is the schema handed to Rig's schema-defined `submit` output tool.
//! The model proposes spans by segment reference plus an optional verbatim
//! quote, and annotations by referencing span IDs. It never produces byte
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
/// `start_segment` and `end_segment` are 1-based inclusive segment numbers as
/// presented in the prompt. IDs are local to this extraction; Rust maps them
/// to generated node IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtractedSpan {
    /// Local identifier referenced by annotations, for example `s1`.
    pub id: String,
    /// First segment of the span, 1-based and inclusive.
    pub start_segment: u32,
    /// Last segment of the span, 1-based and inclusive.
    pub end_segment: u32,
    /// Optional verbatim excerpt from the segment range. When present it must
    /// match the original text exactly and occur exactly once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
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
