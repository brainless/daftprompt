//! Text primitives shared by the parser, the API, and a future CLI.
//!
//! The model never deals in offsets. It names a Rust-built block or a 1-based
//! inclusive segment range and may quote a verbatim excerpt. Rust resolves a byte range
//! and then into line/character-column coordinates for the graph.

mod anchor;
mod coordinates;
mod error;
mod segment;
pub mod structure;

pub use anchor::{AnchoredSpan, anchor_reference, anchor_span, anchor_span_in_structure};
pub use coordinates::{
    LineColumn, byte_offset, byte_range_to_source_range, position_at, source_range_to_byte_range,
};
pub use error::SpanError;
pub use segment::{Segment, SegmentRange, segments};
