//! Serializable contracts shared by every daftprompt crate.
//!
//! The crate is deliberately limited to `serde` and `schemars`: it must not
//! depend on HTTP, GUI, LLM, or storage crates. Everything here is plain data
//! that can cross an HTTP boundary or be handed to a Rig extraction run.
//!
//! - [`ParseRequest`] and [`SourceMetadata`]: what a client submits.
//! - [`Extraction`]: the schema-defined output the model proposes through
//!   Rig's `submit` tool. It contains source references, never offsets.
//! - [`ClarifyRequest`], [`ExplainerExtraction`], and [`ClarifyResult`]: the
//!   clarification branch's request, model-facing output, and validated result.
//! - [`AgentTaskRequest`], [`BriefingExtraction`], and [`BriefingResult`]: the
//!   project-briefing agents' request, model-facing output, and validated result.
//! - [`Project`] and its request bodies: the saved-project registry contract.
//! - [`ParseResult`] and the graph records: the validated, Rust-built result.
//! - [`ParseError`]: explicit failures; HTTP status mapping belongs to the API.
//!
//! The in-memory graph records mirror `docs/data-model.md`. Rust owns IDs,
//! hashes, timestamps, statuses, and reference resolution; the model never
//! produces these records directly.

pub mod error;
pub mod extraction;
pub mod graph;
pub mod profile;
pub mod project;
pub mod request;

pub use error::ParseError;
pub use extraction::{
    AnnotationKind, BriefingCitation, BriefingExtraction, CommandKind, CommandsExtraction,
    ExplainerCitation, ExplainerExtraction, ExtractedAnnotation, ExtractedCommand, ExtractedSpan,
    Extraction,
};
pub use graph::{
    AgentInfo, AgentTask, BriefingNotice, BriefingNoticeKind, BriefingResult, CitationInfo,
    CitationLocation, ClarifyResult, CommandOrigin, CommandsResult, ContextTarget, Grounding,
    Label, LabelKind, LabelStatus, Node, NodeContext, ParseResult, Payload, ProjectCommand, Run,
    RunInput, RunStatus, SourceRange, SourceWindow, SpanInfo, ToolCall,
};
pub use profile::{LanguageDetection, LanguageId, LanguageProject, ProjectProfile};
pub use project::{CreateProjectRequest, Project, RenameProjectRequest};
pub use request::{
    AgentTaskRequest, ClarifyRequest, ClarifySource, ClarifyTarget, ClarifyTurn, ContentType,
    ContentTypeSource, ParseRequest, SourceKind, SourceMetadata,
};
