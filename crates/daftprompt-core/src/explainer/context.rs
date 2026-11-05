//! Request validation and context assembly.
//!
//! Rust, not the model, decides what the agent sees: the subject span marked
//! as the subject, a window of numbered neighbouring segments that never
//! extends past the payload, earlier turns oldest first within limits, the
//! question, and the grounding mode.

use daftprompt_types::{ClarifyRequest, Grounding, ParseError, SourceWindow};

use super::config::ExplainerConfig;
use crate::scribe::support::content_hash;
use crate::text::{Segment, segments};

/// A validated request with everything derived from the source text.
pub(crate) struct Prepared<'a> {
    pub(crate) request: &'a ClarifyRequest,
    pub(crate) source: &'a str,
    pub(crate) segments: Vec<Segment>,
    pub(crate) payload_hash: String,
    /// First and last segment numbers (1-based, inclusive) of the window.
    pub(crate) first_segment: u32,
    pub(crate) last_segment: u32,
    /// First and last segment numbers the subject touches.
    pub(crate) subject_first: u32,
    pub(crate) subject_last: u32,
}

impl Prepared<'_> {
    /// The recorded window: segment numbers and the bytes they cover.
    pub(crate) fn window(&self) -> SourceWindow {
        SourceWindow {
            start_segment: self.first_segment,
            end_segment: self.last_segment,
            start_byte: self.segments[self.first_segment as usize - 1].start_byte,
            end_byte: self.segments[self.last_segment as usize - 1].end_byte,
        }
    }

    pub(crate) fn segment_in_window(&self, number: u32) -> bool {
        (self.first_segment..=self.last_segment).contains(&number)
    }
}

fn oversized(detail: String) -> ParseError {
    ParseError::OversizedInput { detail }
}

/// Validate the request and derive the segment window.
pub(crate) fn prepare<'a>(
    request: &'a ClarifyRequest,
    config: &ExplainerConfig,
) -> Result<Prepared<'a>, ParseError> {
    let source = request.source.text.as_str();

    if request.question.trim().is_empty() {
        return Err(ParseError::InvalidRequest {
            detail: "the question is empty".to_owned(),
        });
    }
    if source.is_empty() {
        return Err(ParseError::InvalidRequest {
            detail: "the source text is empty".to_owned(),
        });
    }
    if request.question.len() > config.max_question_bytes {
        return Err(oversized(format!(
            "the question is {} byte(s); the limit is {} byte(s)",
            request.question.len(),
            config.max_question_bytes
        )));
    }
    if request.turns.len() > config.max_earlier_turns {
        return Err(oversized(format!(
            "the request has {} earlier turn(s); the limit is {}",
            request.turns.len(),
            config.max_earlier_turns
        )));
    }
    let history_bytes: usize = request
        .turns
        .iter()
        .map(|turn| turn.question.len() + turn.explanation.len())
        .sum();
    if history_bytes > config.max_history_bytes {
        return Err(oversized(format!(
            "the earlier turns are {history_bytes} byte(s); the limit is {} byte(s)",
            config.max_history_bytes
        )));
    }
    if source.len() > config.max_input_bytes {
        return Err(oversized(format!(
            "the source text is {} byte(s); the limit is {} byte(s)",
            source.len(),
            config.max_input_bytes
        )));
    }
    let all = segments(source);
    if all.len() > config.max_segments {
        return Err(oversized(format!(
            "the source text has {} segment(s); the limit is {}",
            all.len(),
            config.max_segments
        )));
    }

    let payload_hash = content_hash(source);
    if payload_hash != request.source.payload_hash {
        return Err(ParseError::PayloadMismatch {
            detail: format!(
                "the source text hashes to {payload_hash}, not the supplied {}",
                request.source.payload_hash
            ),
        });
    }

    let (start, end) = (request.target.start_byte, request.target.end_byte);
    if start >= end {
        return Err(ParseError::InvalidTarget {
            detail: format!("the target range {start}..{end} is empty or reversed"),
        });
    }
    if end > source.len() {
        return Err(ParseError::InvalidTarget {
            detail: format!(
                "the target range {start}..{end} is past the end of the {}-byte source",
                source.len()
            ),
        });
    }
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return Err(ParseError::InvalidTarget {
            detail: format!("the target range {start}..{end} is not on character boundaries"),
        });
    }

    let containing = |byte: usize| {
        all.partition_point(|segment| segment.start_byte <= byte)
            .saturating_sub(1)
    };
    let subject_first = containing(start);
    let subject_last = containing(end - 1);
    let first = subject_first.saturating_sub(config.window_segments);
    let last = (subject_last + config.window_segments).min(all.len() - 1);

    Ok(Prepared {
        request,
        source,
        payload_hash,
        first_segment: all[first].number,
        last_segment: all[last].number,
        subject_first: all[subject_first].number,
        subject_last: all[subject_last].number,
        segments: all,
    })
}

/// Render the user message for the model.
pub(crate) fn render_prompt(prepared: &Prepared<'_>, grounding: Grounding) -> String {
    let request = prepared.request;
    let mut out = String::new();

    out.push_str("## Subject span\n");
    out.push_str(&format!(
        "The user is asking about this exact text (source segment(s) {}):\n<subject>\n{}\n</subject>\n\n",
        range_label(prepared.subject_first, prepared.subject_last),
        &prepared.source[request.target.start_byte..request.target.end_byte]
    ));

    out.push_str(&format!(
        "## Neighbouring source text\nSegments {} of {}. The subject lies in segment(s) {}. \
         Cite these with `source` citations by segment number.\n<source>\n",
        range_label(prepared.first_segment, prepared.last_segment),
        prepared.segments.len(),
        range_label(prepared.subject_first, prepared.subject_last),
    ));
    for number in prepared.first_segment..=prepared.last_segment {
        let segment = &prepared.segments[number as usize - 1];
        out.push_str(&format!("[{number}] {}\n", segment.text(prepared.source)));
    }
    out.push_str("</source>\n\n");

    if !request.turns.is_empty() {
        out.push_str("## Earlier turns in this conversation (oldest first)\n");
        for (index, turn) in request.turns.iter().enumerate() {
            let n = index + 1;
            out.push_str(&format!(
                "<question n=\"{n}\">\n{}\n</question>\n<explanation n=\"{n}\">\n{}\n</explanation>\n",
                turn.question, turn.explanation
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "## Question\n<question>\n{}\n</question>\n\n",
        request.question
    ));

    out.push_str("## Grounding\n");
    match grounding {
        Grounding::Project => out.push_str(
            "project: the tools list_files, search, and read_file are available for the project \
             files.\n",
        ),
        Grounding::TextOnly => out.push_str(
            "text_only: no tools are available. Use only the text above; use no file citations.\n",
        ),
    }
    out
}

fn range_label(first: u32, last: u32) -> String {
    if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    }
}
