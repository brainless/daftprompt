//! Validation failures in text anchoring and coordinate conversion.
//!
//! These are internal to the library; the public parse error type
//! (`daftprompt_types::ParseError`) wraps them when a whole parse fails.

use super::segment::SegmentRange;

/// A text span could not be turned into a byte range or coordinates.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpanError {
    #[error("segment {segment} does not exist; the input has {segment_count} segment(s)")]
    SegmentOutOfRange { segment: u32, segment_count: usize },

    #[error(
        "segment range {start}..={end} is invalid; both ends are 1-based and start must not exceed end"
    )]
    InvalidSegmentRange { start: u32, end: u32 },

    #[error("segments {}..={} contain no non-whitespace text", .segments.start, .segments.end)]
    EmptySpan { segments: SegmentRange },

    #[error("quote must not be empty")]
    EmptyQuote,

    #[error(
        "quote {quote:?} was not found in segments {}..={}",
        .segments.start,
        .segments.end
    )]
    QuoteNotFound {
        quote: String,
        segments: SegmentRange,
    },

    #[error(
        "quote {quote:?} occurs {occurrences} times in segments {}..={}; it must occur exactly once",
        .segments.start,
        .segments.end
    )]
    AmbiguousQuote {
        quote: String,
        occurrences: usize,
        segments: SegmentRange,
    },

    #[error("byte offset {offset} is out of range for text of {len} byte(s)")]
    ByteOffsetOutOfRange { offset: usize, len: usize },

    #[error("byte offset {offset} is not on a UTF-8 character boundary")]
    NotCharBoundary { offset: usize },

    #[error("byte range {start}..{end} is reversed")]
    ReversedByteRange { start: usize, end: usize },

    #[error("line/column position {line}:{column} is out of range")]
    PositionOutOfRange { line: u32, column: u32 },
}
