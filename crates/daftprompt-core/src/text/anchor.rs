//! Exact-once anchoring of model-proposed spans.
//!
//! A span names an inclusive segment range and optionally a verbatim quote.
//! Without a quote the span covers the first to the last non-whitespace
//! character of the range. With a quote it must match the original text of the
//! range exactly, exactly once. Matching is byte- and Unicode-exact: no
//! normalization, no case folding, and CRLF stays CRLF.

use daftprompt_types::ExtractedSpan;

use super::error::SpanError;
use super::segment::{SegmentRange, segments};

/// A validated span, expressed both as the segments the model named and as a
/// half-open byte range into the original text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchoredSpan {
    /// The inclusive segment range the model named.
    pub segments: SegmentRange,
    /// Byte offset of the first byte of the span.
    pub start_byte: usize,
    /// Byte offset just past the last byte of the span.
    pub end_byte: usize,
}

impl AnchoredSpan {
    /// The anchored text.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start_byte..self.end_byte]
    }
}

/// Anchor one model-proposed span.
pub fn anchor_span(source: &str, span: &ExtractedSpan) -> Result<AnchoredSpan, SpanError> {
    anchor_reference(
        source,
        span.start_segment,
        span.end_segment,
        span.quote.as_deref(),
    )
}

/// Anchor a span described by a segment range and an optional quote.
///
/// The quote is searched for in the original text of the segment range,
/// including the original line terminators between segments but excluding the
/// terminator after the last segment.
pub fn anchor_reference(
    source: &str,
    start_segment: u32,
    end_segment: u32,
    quote: Option<&str>,
) -> Result<AnchoredSpan, SpanError> {
    let all = segments(source);
    let segment_count = all.len();

    if start_segment == 0 || end_segment == 0 || start_segment > end_segment {
        return Err(SpanError::InvalidSegmentRange {
            start: start_segment,
            end: end_segment,
        });
    }
    if end_segment as usize > segment_count {
        return Err(SpanError::SegmentOutOfRange {
            segment: end_segment,
            segment_count,
        });
    }

    let range = SegmentRange {
        start: start_segment,
        end: end_segment,
    };

    // Indexing is safe: the range was checked against `segment_count`.
    let region_start = all[(start_segment - 1) as usize].start_byte;
    let region_end = all[(end_segment - 1) as usize].end_byte;
    let region = &source[region_start..region_end];

    let (start_byte, end_byte) = match quote {
        Some(quote) => {
            if quote.is_empty() {
                return Err(SpanError::EmptyQuote);
            }
            let occurrences = matching_offsets(region, quote);
            match occurrences.as_slice() {
                [] => {
                    return Err(SpanError::QuoteNotFound {
                        quote: quote.to_owned(),
                        segments: range,
                    });
                }
                [offset] => (region_start + offset, region_start + offset + quote.len()),
                _ => {
                    return Err(SpanError::AmbiguousQuote {
                        quote: quote.to_owned(),
                        occurrences: occurrences.len(),
                        segments: range,
                    });
                }
            }
        }
        None => {
            let trimmed = region.trim();
            if trimmed.is_empty() {
                return Err(SpanError::EmptySpan { segments: range });
            }
            let leading = region.len() - region.trim_start().len();
            let start = region_start + leading;
            (start, start + trimmed.len())
        }
    };

    Ok(AnchoredSpan {
        segments: range,
        start_byte,
        end_byte,
    })
}

/// Byte offsets at char boundaries where `needle` matches.
///
/// Overlapping occurrences count too: `"aa"` occurs twice in `"aaa"`, which
/// makes it ambiguous rather than anchoring at the first position.
fn matching_offsets(haystack: &str, needle: &str) -> Vec<usize> {
    haystack
        .char_indices()
        .map(|(offset, _)| offset)
        .filter(|offset| haystack[*offset..].starts_with(needle))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(
        source: &str,
        start: u32,
        end: u32,
        quote: Option<&str>,
    ) -> Result<AnchoredSpan, SpanError> {
        anchor_reference(source, start, end, quote)
    }

    fn bytes(source: &str, start: u32, end: u32, quote: Option<&str>) -> (usize, usize) {
        let anchored = anchor(source, start, end, quote).expect("span should anchor");
        (anchored.start_byte, anchored.end_byte)
    }

    #[test]
    fn quote_anchors_to_an_exact_unique_match() {
        let source = "Plan\n\nShould we cache tokens?\n";
        let anchored = anchor(source, 3, 3, Some("cache tokens")).unwrap();
        assert_eq!(anchored.text(source), "cache tokens");
        assert_eq!(anchored.segments, SegmentRange { start: 3, end: 3 });
    }

    #[test]
    fn repeated_text_is_ambiguous() {
        let source = "foo\nfoo\nbar";
        let error = anchor(source, 1, 3, Some("foo")).unwrap_err();
        assert!(matches!(
            error,
            SpanError::AmbiguousQuote { occurrences: 2, .. }
        ));
    }

    #[test]
    fn overlapping_occurrences_are_ambiguous() {
        let source = "aaa";
        let error = anchor(source, 1, 1, Some("aa")).unwrap_err();
        assert!(matches!(
            error,
            SpanError::AmbiguousQuote { occurrences: 2, .. }
        ));
    }

    #[test]
    fn longer_quote_disambiguates_repeated_text() {
        let source = "foo foo";
        assert_eq!(bytes(source, 1, 1, Some(" foo")), (3, 7));
    }

    #[test]
    fn missing_quote_is_invalid() {
        let source = "hello";
        let error = anchor(source, 1, 1, Some("goodbye")).unwrap_err();
        assert!(matches!(error, SpanError::QuoteNotFound { .. }));
    }

    #[test]
    fn empty_quote_is_invalid() {
        let error = anchor("hello", 1, 1, Some("")).unwrap_err();
        assert_eq!(error, SpanError::EmptyQuote);
    }

    #[test]
    fn quote_is_exact_and_not_normalized() {
        let source = "Naïve café";
        assert!(anchor(source, 1, 1, Some("Naive cafe")).is_err());
        assert_eq!(bytes(source, 1, 1, Some("café")), (7, 12));
    }

    #[test]
    fn quote_may_span_segments_including_terminators() {
        let source = "first\nsecond\nthird";
        assert_eq!(bytes(source, 1, 2, Some("first\nsecond")), (0, 12));
        // The terminator after the last segment is outside the span.
        assert!(anchor(source, 1, 2, Some("second\n")).is_err());
        // A quote may start or end mid-segment.
        assert_eq!(bytes(source, 1, 2, Some("st\nsec")), (3, 9));
    }

    #[test]
    fn crlf_is_preserved_and_never_matched_as_lf() {
        let source = "first\r\nsecond\r\n";
        assert_eq!(bytes(source, 1, 2, Some("first\r\nsecond")), (0, 13));
        // Exact matching: LF-only quotations do not anchor in CRLF text.
        assert!(anchor(source, 1, 2, Some("first\nsecond")).is_err());
    }

    #[test]
    fn quote_is_absolute_even_for_later_segments() {
        let source = "a\nb\nc";
        assert_eq!(bytes(source, 3, 3, Some("c")), (4, 5));
    }

    #[test]
    fn without_a_quote_the_span_trims_surrounding_whitespace() {
        let source = "  a\n  b  \n";
        let anchored = anchor(source, 1, 2, None).unwrap();
        assert_eq!(anchored.text(source), "a\n  b");
    }

    #[test]
    fn without_a_quote_internal_whitespace_is_kept() {
        let source = "one   two\n   three";
        assert_eq!(bytes(source, 1, 2, None), (0, 18));
    }

    #[test]
    fn whitespace_only_ranges_are_invalid() {
        let error = anchor("a\n   \n\t\nb", 2, 3, None).unwrap_err();
        assert!(matches!(error, SpanError::EmptySpan { .. }));
    }

    #[test]
    fn unicode_quote_anchors_on_char_boundaries() {
        let source = "héllo 😀\ncombiné e\u{301}";
        let anchored = anchor(source, 1, 1, Some("😀")).unwrap();
        assert_eq!(&source[anchored.start_byte..anchored.end_byte], "😀");
        assert_eq!(
            bytes(source, 2, 2, Some("e\u{301}")),
            ("héllo 😀\n".len() + "combiné ".len(), source.len())
        );
    }

    #[test]
    fn spans_outside_the_input_are_rejected() {
        let source = "a\nb";
        assert_eq!(
            anchor(source, 3, 3, None).unwrap_err(),
            SpanError::SegmentOutOfRange {
                segment: 3,
                segment_count: 2
            }
        );
        assert!(matches!(
            anchor(source, 0, 1, None).unwrap_err(),
            SpanError::InvalidSegmentRange { .. }
        ));
        assert!(matches!(
            anchor(source, 2, 1, None).unwrap_err(),
            SpanError::InvalidSegmentRange { .. }
        ));
    }

    #[test]
    fn span_can_cover_the_whole_input() {
        let source = "a\nb\n";
        let anchored = anchor(source, 1, 3, None).unwrap();
        assert_eq!(anchored.text(source), "a\nb");
    }
}
