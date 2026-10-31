//! Conversion between byte ranges and line/character coordinates.
//!
//! Graph contexts store 1-based lines and 1-based columns counted in Unicode
//! scalar values (`char`s); API results and internal spans use half-open byte
//! ranges into the payload's UTF-8 text. Lines are counted by `\n`; CRLF is
//! preserved, so the `\r` of a CRLF terminator is a line character.

use daftprompt_types::SourceRange;

use super::error::SpanError;

/// A 1-based line and 1-based character column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineColumn {
    pub line: u32,
    pub column: u32,
}

/// Convert a byte offset to a 1-based line/character-column position.
///
/// The offset must be on a `char` boundary and at most `source.len()`.
pub fn position_at(source: &str, byte: usize) -> Result<LineColumn, SpanError> {
    if byte > source.len() {
        return Err(SpanError::ByteOffsetOutOfRange {
            offset: byte,
            len: source.len(),
        });
    }
    if !source.is_char_boundary(byte) {
        return Err(SpanError::NotCharBoundary { offset: byte });
    }

    let mut line = 1u32;
    let mut column = 1u32;
    for (offset, character) in source.char_indices() {
        if offset >= byte {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    Ok(LineColumn { line, column })
}

/// Convert a half-open byte range to graph coordinates.
///
/// Both ends must fall on `char` boundaries. The end position is exclusive.
pub fn byte_range_to_source_range(
    source: &str,
    start_byte: usize,
    end_byte: usize,
) -> Result<SourceRange, SpanError> {
    if start_byte > end_byte {
        return Err(SpanError::ReversedByteRange {
            start: start_byte,
            end: end_byte,
        });
    }
    let start = position_at(source, start_byte)?;
    let end = position_at(source, end_byte)?;
    Ok(SourceRange {
        start_line: start.line,
        start_col: start.column,
        end_line: end.line,
        end_col: end.column,
    })
}

/// Convert a 1-based line/character-column position to a byte offset.
///
/// The position must exist: `column` may be one past the last character of a
/// line (the position before its terminator), but not further.
pub fn byte_offset(source: &str, position: LineColumn) -> Result<usize, SpanError> {
    if position.line == 0 || position.column == 0 {
        return Err(SpanError::PositionOutOfRange {
            line: position.line,
            column: position.column,
        });
    }

    let mut line = 1u32;
    let mut column = 1u32;
    for (offset, character) in source.char_indices() {
        if line == position.line && column == position.column {
            return Ok(offset);
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    if line == position.line && column == position.column {
        return Ok(source.len());
    }
    Err(SpanError::PositionOutOfRange {
        line: position.line,
        column: position.column,
    })
}

/// Convert graph coordinates back to a half-open byte range.
pub fn source_range_to_byte_range(
    source: &str,
    range: &SourceRange,
) -> Result<(usize, usize), SpanError> {
    let start = byte_offset(
        source,
        LineColumn {
            line: range.start_line,
            column: range.start_col,
        },
    )?;
    let end = byte_offset(
        source,
        LineColumn {
            line: range.end_line,
            column: range.end_col,
        },
    )?;
    if start > end {
        return Err(SpanError::ReversedByteRange { start, end });
    }
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(source: &str, start: usize, end: usize) -> SourceRange {
        byte_range_to_source_range(source, start, end).expect("range should convert")
    }

    #[test]
    fn ascii_positions_are_one_based() {
        let source = "ab\ncd";
        assert_eq!(
            position_at(source, 0).unwrap(),
            LineColumn { line: 1, column: 1 }
        );
        assert_eq!(
            position_at(source, 2).unwrap(),
            LineColumn { line: 1, column: 3 }
        );
        assert_eq!(
            position_at(source, 3).unwrap(),
            LineColumn { line: 2, column: 1 }
        );
        assert_eq!(
            position_at(source, source.len()).unwrap(),
            LineColumn { line: 2, column: 3 }
        );
    }

    #[test]
    fn end_position_is_exclusive() {
        // "hello" is five characters; a span over the whole line ends at col 6.
        assert_eq!(
            range("hello", 0, 5),
            SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 6,
            }
        );
    }

    #[test]
    fn multiline_range_reports_both_positions() {
        let source = "  a\n  b  \n";
        assert_eq!(
            range(source, 2, 7),
            SourceRange {
                start_line: 1,
                start_col: 3,
                end_line: 2,
                end_col: 4,
            }
        );
    }

    #[test]
    fn multibyte_characters_count_as_one_column() {
        let source = "héllo 😀 x";
        // Byte 7 is after "héllo " (h + é(2) + llo + space = 7 bytes, 6 chars).
        assert_eq!(
            position_at(source, 7).unwrap(),
            LineColumn { line: 1, column: 7 }
        );
        // The emoji is four bytes, one column.
        assert_eq!(
            position_at(source, 11).unwrap(),
            LineColumn { line: 1, column: 8 }
        );
        assert_eq!(range(source, 7, 11).start_col, 7);
        assert_eq!(range(source, 7, 11).end_col, 8);
    }

    #[test]
    fn combining_characters_count_as_separate_scalar_values() {
        // "e" + combining acute is two chars.
        let source = "e\u{301}x";
        assert_eq!(source.len(), 4);
        assert_eq!(
            position_at(source, 1).unwrap(),
            LineColumn { line: 1, column: 2 }
        );
        assert_eq!(
            position_at(source, 3).unwrap(),
            LineColumn { line: 1, column: 3 }
        );
        assert_eq!(range(source, 0, 4).end_col, 4);
    }

    #[test]
    fn crlf_carriage_return_is_a_line_character() {
        let source = "a\r\nb";
        assert_eq!(
            position_at(source, 2).unwrap(),
            LineColumn { line: 1, column: 3 }
        );
        assert_eq!(
            position_at(source, 3).unwrap(),
            LineColumn { line: 2, column: 1 }
        );
        assert_eq!(
            range(source, 0, 3),
            SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 2,
                end_col: 1,
            }
        );
    }

    #[test]
    fn empty_source_has_one_position() {
        assert_eq!(
            position_at("", 0).unwrap(),
            LineColumn { line: 1, column: 1 }
        );
        assert_eq!(range("", 0, 0).end_col, 1);
    }

    #[test]
    fn trailing_newline_has_a_next_line_position() {
        let source = "a\n";
        assert_eq!(
            position_at(source, 2).unwrap(),
            LineColumn { line: 2, column: 1 }
        );
    }

    #[test]
    fn rejects_invalid_byte_offsets() {
        let source = "aé";
        assert_eq!(
            position_at(source, 4).unwrap_err(),
            SpanError::ByteOffsetOutOfRange { offset: 4, len: 3 }
        );
        assert_eq!(
            position_at(source, 2).unwrap_err(),
            SpanError::NotCharBoundary { offset: 2 }
        );
        assert_eq!(
            byte_range_to_source_range(source, 2, 1).unwrap_err(),
            SpanError::ReversedByteRange { start: 2, end: 1 }
        );
    }

    #[test]
    fn converts_coordinates_back_to_bytes() {
        let source = "  a\n  b  \n";
        let start = LineColumn { line: 1, column: 3 };
        let end = LineColumn { line: 2, column: 4 };
        assert_eq!(byte_offset(source, start).unwrap(), 2);
        assert_eq!(byte_offset(source, end).unwrap(), 7);
        assert_eq!(
            source_range_to_byte_range(
                source,
                &SourceRange {
                    start_line: 1,
                    start_col: 3,
                    end_line: 2,
                    end_col: 4,
                }
            )
            .unwrap(),
            (2, 7)
        );
    }

    #[test]
    fn accepts_one_past_the_end_of_a_line() {
        let source = "ab\ncd";
        let end_of_first = LineColumn { line: 1, column: 3 };
        assert_eq!(byte_offset(source, end_of_first).unwrap(), 2);
        let past_end = LineColumn { line: 1, column: 4 };
        assert!(matches!(
            byte_offset(source, past_end).unwrap_err(),
            SpanError::PositionOutOfRange { .. }
        ));
    }

    #[test]
    fn byte_and_coordinate_conversions_round_trip() {
        let source = "héllo\r\n😀 done\r\n\r\ncombining e\u{301}\n";
        for (start, end) in [
            (0usize, 0usize),
            (0, 6),
            (8, 17),
            (19, 21),
            (21, 34),
            (35, 35),
        ] {
            let range = byte_range_to_source_range(source, start, end).unwrap();
            assert_eq!(
                source_range_to_byte_range(source, &range).unwrap(),
                (start, end),
                "round trip failed for {start}..{end} ({range:?})"
            );
        }
    }

    #[test]
    fn rejects_impossible_positions() {
        assert!(matches!(
            byte_offset("a", LineColumn { line: 0, column: 1 }).unwrap_err(),
            SpanError::PositionOutOfRange { .. }
        ));
        assert!(matches!(
            byte_offset("a", LineColumn { line: 2, column: 1 }).unwrap_err(),
            SpanError::PositionOutOfRange { .. }
        ));
        assert!(matches!(
            byte_offset("a\nb", LineColumn { line: 1, column: 4 }).unwrap_err(),
            SpanError::PositionOutOfRange { .. }
        ));
    }
}
