//! Serializable contracts shared by every daftprompt crate.
//!
//! The crate is deliberately limited to `serde` and `schemars`: it must not
//! depend on HTTP, GUI, LLM, or storage crates. Everything here is plain data
//! that can cross an HTTP boundary or be handed to a Rig extraction run.
//!
//! - [`ParseRequest`] and [`SourceMetadata`]: what a client submits.
//! - [`Extraction`]: the schema-defined output the model proposes through
//!   Rig's `submit` tool. It contains source references, never offsets.
//! - [`ParseResult`] and the graph records: the validated, Rust-built result.
//! - [`ParseError`]: explicit failures; HTTP status mapping belongs to the API.
//!
//! The in-memory graph records mirror `docs/data-model.md`. Rust owns IDs,
//! hashes, timestamps, statuses, and reference resolution; the model never
//! produces these records directly.

pub mod error;
pub mod extraction;
pub mod graph;
pub mod request;

pub use error::ParseError;
pub use extraction::{AnnotationKind, ExtractedAnnotation, ExtractedSpan, Extraction};
pub use graph::{
    ContextTarget, Label, LabelKind, LabelStatus, Node, NodeContext, ParseResult, Payload, Run,
    RunInput, RunStatus, SourceRange, SpanInfo, ToolCall,
};
pub use request::{ParseRequest, SourceKind, SourceMetadata};
