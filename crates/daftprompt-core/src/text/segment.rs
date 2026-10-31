//! Segmentation of submitted text.
//!
//! Lines are split on `\n` and numbered from 1. Empty lines are segments.
//! When a line ends in CRLF, the `\r` belongs to the terminator, not to the
//! segment text; the original text itself is never modified.

/// One line of the submitted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// 1-based segment number.
    pub number: u32,
    /// Byte offset of the first character of the segment.
    pub start_byte: usize,
    /// Byte offset just past the last character of the segment. A trailing
    /// `\r` of a CRLF line terminator is excluded.
    pub end_byte: usize,
}

impl Segment {
    /// The segment text, excluding its line terminator.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start_byte..self.end_byte]
    }
}

/// An inclusive, 1-based range of segments, as proposed by the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentRange {
    pub start: u32,
    pub end: u32,
}

/// Split `source` into 1-based line segments.
///
/// The input is split on `\n` only. A final line terminator therefore produces
/// a trailing empty segment, matching "lines split on `\n`, including empty
/// lines". A lone trailing `\r` at end of input is not a line terminator and
/// stays part of the final segment.
pub fn segments(source: &str) -> Vec<Segment> {
    let mut result = Vec::new();
    let mut start_byte = 0usize;
    let mut number = 1u32;

    while let Some(relative) = source[start_byte..].find('\n') {
        let newline = start_byte + relative;
        let end_byte = if newline > start_byte && source.as_bytes()[newline - 1] == b'\r' {
            newline - 1
        } else {
            newline
        };
        result.push(Segment {
            number,
            start_byte,
            end_byte,
        });
        start_byte = newline + 1;
        number += 1;
    }

    result.push(Segment {
        number,
        start_byte,
        end_byte: source.len(),
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &str) -> Vec<&str> {
        segments(source)
            .into_iter()
            .map(|segment| segment.text(source))
            .collect()
    }

    #[test]
    fn splits_lines_and_numbers_them_from_one() {
        let source = "first\nsecond\nthird";
        let segments = segments(source);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].number, 1);
        assert_eq!(segments[2].number, 3);
        assert_eq!(segments[0].text(source), "first");
        assert_eq!(segments[1].text(source), "second");
        assert_eq!(segments[2].text(source), "third");
    }

    #[test]
    fn keeps_empty_lines() {
        assert_eq!(texts("a\n\nb"), vec!["a", "", "b"]);
        assert_eq!(texts(""), vec![""]);
        assert_eq!(texts("\n"), vec!["", ""]);
    }

    #[test]
    fn a_final_newline_starts_a_trailing_empty_segment() {
        let source = "a\n";
        let segments = segments(source);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].start_byte, 2);
        assert_eq!(segments[1].end_byte, 2);
        assert_eq!(segments[1].text(source), "");
    }

    #[test]
    fn crlf_carriage_return_belongs_to_the_terminator() {
        let source = "a\r\nb\r\n";
        let segments = segments(source);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text(source), "a");
        assert_eq!(segments[1].text(source), "b");
        assert_eq!(segments[2].text(source), "");

        // Byte offsets still address the original text, terminators included.
        assert_eq!((segments[0].start_byte, segments[0].end_byte), (0, 1));
        assert_eq!((segments[1].start_byte, segments[1].end_byte), (3, 4));
        assert_eq!((segments[2].start_byte, segments[2].end_byte), (6, 6));
    }

    #[test]
    fn empty_crlf_lines_are_empty_segments() {
        assert_eq!(texts("a\r\n\r\nb"), vec!["a", "", "b"]);
    }

    #[test]
    fn a_lone_trailing_carriage_return_is_text() {
        let source = "a\r";
        let segments = segments(source);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].end_byte, 2);
        assert_eq!(segments[0].text(source), "a\r");
    }

    #[test]
    fn handles_unicode_without_splitting_characters() {
        let source = "héllo\r\n😀 done\r\ncombining e\u{301}";
        assert_eq!(
            texts(source),
            vec!["héllo", "😀 done", "combining e\u{301}"]
        );
        let segments = segments(source);
        // "héllo" is 6 bytes; CRLF adds 2; the emoji is 4 bytes.
        assert_eq!(segments[1].start_byte, 8);
        assert_eq!(segments[1].end_byte, 8 + "😀 done".len());
    }
}
