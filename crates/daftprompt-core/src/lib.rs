//! The shared daftprompt library.
//!
//! Everything that is not HTTP or GUI lives here: text handling, validation,
//! graph construction, agents, and prompts. The public API stays independent
//! of Axum, akar, and any other transport so a future CLI can reuse it.
//!
//! - [`text`]: segmentation, exact-once quote anchoring, and byte/line-column
//!   coordinate conversion.
//! - [`scribe`]: the first parsing agent. Scribe turns submitted text into a
//!   validated in-memory graph through a typed Rig extraction.
//! - [`eval`]: the manually annotated fixture set and the scoring rules used by
//!   the opt-in live Scribe evaluation.

pub mod eval;
pub mod scribe;
pub mod text;

pub use scribe::{
    BoxFuture, DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_SEGMENTS,
    DEFAULT_TIMEOUT, ExtractionModel, ModelError, ModelInput, PROMPT_VERSION, Parser, RigExtractor,
    SYSTEM_PROMPT, Scribe, ScribeConfig, render_segments,
};
pub use text::{
    AnchoredSpan, LineColumn, Segment, SegmentRange, SpanError, anchor_reference, anchor_span,
    byte_offset, byte_range_to_source_range, position_at, segments, source_range_to_byte_range,
};
