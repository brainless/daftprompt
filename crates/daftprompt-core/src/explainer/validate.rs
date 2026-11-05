//! Validation of a model-proposed explanation before any record exists.

use daftprompt_types::{ExplainerCitation, ExplainerExtraction, SourceRange};

use super::context::Prepared;
use crate::project_tools::{ToolRecorder, normalize_path_text};
use crate::text::byte_range_to_source_range;

/// A citation Rust has verified and resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValidatedCitation {
    File {
        path: String,
        /// Inclusive, 1-based.
        start_line: u32,
        end_line: u32,
        /// SHA-256 of the file content the agent read.
        content_hash: String,
    },
    Source {
        start_byte: usize,
        end_byte: usize,
        range: SourceRange,
    },
}

/// Check the explanation, its markers, and every citation. The error is a
/// description of the first problem; it is never fed back to the model.
pub(crate) fn validate(
    extraction: &ExplainerExtraction,
    prepared: &Prepared<'_>,
    recorder: Option<&ToolRecorder>,
    max_citations: usize,
) -> Result<Vec<ValidatedCitation>, String> {
    if extraction.explanation.trim().is_empty() {
        return Err("the explanation is empty".to_owned());
    }
    if extraction.citations.len() > max_citations {
        return Err(format!(
            "{} citations exceed the limit of {max_citations}",
            extraction.citations.len()
        ));
    }
    for marker in markers(&extraction.explanation) {
        if marker == 0 || marker as usize > extraction.citations.len() {
            return Err(format!(
                "the explanation refers to citation [{marker}], but there are {} citation(s)",
                extraction.citations.len()
            ));
        }
    }

    let mut validated = Vec::with_capacity(extraction.citations.len());
    for (index, citation) in extraction.citations.iter().enumerate() {
        let n = index + 1;
        match citation {
            ExplainerCitation::File {
                path,
                start_line,
                end_line,
            } => {
                let Some(recorder) = recorder else {
                    return Err(format!(
                        "citation [{n}] cites a file, but no project tools were available"
                    ));
                };
                if *start_line == 0 || end_line < start_line {
                    return Err(format!(
                        "citation [{n}] has an invalid line range {start_line}-{end_line}"
                    ));
                }
                if !recorder.covers(path, *start_line as usize, *end_line as usize) {
                    return Err(format!(
                        "citation [{n}] cites {path} lines {start_line}-{end_line}, which no \
                         read_file call returned"
                    ));
                }
                let normalized = normalize_path_text(path);
                let Some(content_hash) = recorder.file_sha256(&normalized) else {
                    return Err(format!("citation [{n}] has no recorded content for {path}"));
                };
                validated.push(ValidatedCitation::File {
                    path: normalized,
                    start_line: *start_line,
                    end_line: *end_line,
                    content_hash,
                });
            }
            ExplainerCitation::Source {
                start_segment,
                end_segment,
            } => {
                if *start_segment == 0
                    || end_segment < start_segment
                    || !prepared.segment_in_window(*start_segment)
                    || !prepared.segment_in_window(*end_segment)
                {
                    return Err(format!(
                        "citation [{n}] cites segments {start_segment}-{end_segment}, outside \
                         the provided segments {}-{}",
                        prepared.first_segment, prepared.last_segment
                    ));
                }
                let start_byte = prepared.segments[*start_segment as usize - 1].start_byte;
                let end_byte = prepared.segments[*end_segment as usize - 1].end_byte;
                let range = byte_range_to_source_range(prepared.source, start_byte, end_byte)
                    .map_err(|error| format!("citation [{n}] could not be located: {error}"))?;
                validated.push(ValidatedCitation::Source {
                    start_byte,
                    end_byte,
                    range,
                });
            }
        }
    }
    Ok(validated)
}

/// Every `[digits]` marker in `text`, in order. Numbers too large for `u32`
/// are reported as `u32::MAX`, which is never a valid citation.
fn markers(text: &str) -> Vec<u32> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b']' {
                found.push(text[i + 1..j].parse().unwrap_or(u32::MAX));
                i = j;
            }
        }
        i += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::markers;

    #[test]
    fn finds_numeric_markers_only() {
        assert_eq!(markers("a [1] b [23] [x] [] [4"), vec![1, 23]);
        assert_eq!(markers("[0][2]"), vec![0, 2]);
        assert!(markers("no markers").is_empty());
    }
}
