//! Validation of a model-proposed extraction before any graph record exists.
//!
//! Everything the model can get wrong is checked here: span boundaries and
//! quoting, overlaps, references, label shapes, assignees, and answer targets.
//! A failure makes the attempt invalid; Scribe retries from scratch.

use std::collections::{HashMap, HashSet};

use daftprompt_types::{AnnotationKind, ExtractedAnnotation, Extraction, SourceRange};
use thiserror::Error;

use crate::text::{anchor_span, byte_range_to_source_range};

/// A span that anchored to a half-open byte range and graph coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedSpan {
    pub(crate) local_id: String,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) range: SourceRange,
}

/// A validated `question` annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedQuestion {
    /// Index into [`ValidatedExtraction::spans`].
    pub(crate) span: usize,
    pub(crate) assignee: Option<String>,
    /// Derived by Rust: at least one local answer targets this question.
    pub(crate) resolved: bool,
}

/// A validated `answer` link from one span to a question span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValidatedAnswer {
    /// Index of the answering span.
    pub(crate) source: usize,
    /// Index of the question span being answered.
    pub(crate) target: usize,
}

/// A whole extraction that passed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedExtraction {
    pub(crate) spans: Vec<ValidatedSpan>,
    /// Questions in annotation order.
    pub(crate) questions: Vec<ValidatedQuestion>,
    /// Answer links in annotation and target order.
    pub(crate) answers: Vec<ValidatedAnswer>,
}

/// A model proposal could not be turned into a valid extraction.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub(crate) struct InvalidExtraction(String);

fn invalid(detail: impl Into<String>) -> InvalidExtraction {
    InvalidExtraction(detail.into())
}

/// Validate every model-proposed span and annotation against the source text.
pub(crate) fn validate(
    source: &str,
    extraction: &Extraction,
) -> Result<ValidatedExtraction, InvalidExtraction> {
    let mut span_index = HashMap::new();
    let mut spans = Vec::with_capacity(extraction.spans.len());

    for span in &extraction.spans {
        if span.id.trim().is_empty() {
            return Err(invalid("every span needs a non-empty id"));
        }
        if span_index.contains_key(&span.id) {
            return Err(invalid(format!(
                "span id {:?} is used more than once",
                span.id
            )));
        }

        let anchored = anchor_span(source, span)
            .map_err(|error| invalid(format!("span {:?}: {error}", span.id)))?;
        let range = byte_range_to_source_range(source, anchored.start_byte, anchored.end_byte)
            .map_err(|error| invalid(format!("span {:?}: {error}", span.id)))?;

        span_index.insert(span.id.clone(), spans.len());
        spans.push(ValidatedSpan {
            local_id: span.id.clone(),
            start_byte: anchored.start_byte,
            end_byte: anchored.end_byte,
            range,
        });
    }

    ensure_no_overlaps(&spans)?;

    // Questions come first so answer targets can be checked against them.
    let mut question_spans = HashSet::new();
    let mut questions = Vec::new();
    for (index, annotation) in extraction.annotations.iter().enumerate() {
        if annotation.kind != AnnotationKind::Question {
            continue;
        }

        let span = resolve_span(&span_index, &annotation.span, index)?;
        if !annotation.answers.is_empty() {
            return Err(invalid(format!(
                "question annotation {index} must not list answer targets"
            )));
        }
        if let Some(assignee) = &annotation.assignee {
            validate_assignee(assignee, index)?;
        }
        if !question_spans.insert(span) {
            return Err(invalid(format!(
                "span {:?} has more than one question annotation",
                annotation.span
            )));
        }

        questions.push(ValidatedQuestion {
            span,
            assignee: annotation.assignee.clone(),
            resolved: false,
        });
    }

    let mut referenced = HashSet::new();
    let mut answers = Vec::new();
    let mut seen_links = HashSet::new();

    for (index, annotation) in extraction.annotations.iter().enumerate() {
        let source_span = resolve_span(&span_index, &annotation.span, index)?;
        referenced.insert(source_span);

        match annotation.kind {
            AnnotationKind::Question => {}
            AnnotationKind::Answer => {
                validate_answer(
                    annotation,
                    index,
                    source_span,
                    &span_index,
                    &question_spans,
                    &mut referenced,
                    &mut answers,
                    &mut seen_links,
                )?;
            }
        }
    }

    for (index, span) in spans.iter().enumerate() {
        if !referenced.contains(&index) {
            return Err(invalid(format!(
                "span {:?} is not referenced by any annotation",
                span.local_id
            )));
        }
    }

    for question in &mut questions {
        question.resolved = answers.iter().any(|answer| answer.target == question.span);
    }

    Ok(ValidatedExtraction {
        spans,
        questions,
        answers,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_answer(
    annotation: &ExtractedAnnotation,
    index: usize,
    source_span: usize,
    span_index: &HashMap<String, usize>,
    question_spans: &HashSet<usize>,
    referenced: &mut HashSet<usize>,
    answers: &mut Vec<ValidatedAnswer>,
    seen_links: &mut HashSet<(usize, usize)>,
) -> Result<(), InvalidExtraction> {
    if annotation.assignee.is_some() {
        return Err(invalid(format!(
            "annotation {index}: only question labels may carry an assignee"
        )));
    }
    if annotation.answers.is_empty() {
        return Err(invalid(format!(
            "answer annotation {index} must name at least one question span"
        )));
    }

    for target_id in &annotation.answers {
        let target = resolve_span(span_index, target_id, index)?;
        referenced.insert(target);

        if target == source_span {
            return Err(invalid(format!(
                "annotation {index}: a span cannot answer itself"
            )));
        }
        if !question_spans.contains(&target) {
            return Err(invalid(format!(
                "annotation {index}: answer target {target_id:?} is not a question span"
            )));
        }
        if !seen_links.insert((source_span, target)) {
            return Err(invalid(format!(
                "annotation {index}: duplicate answer link to {target_id:?}"
            )));
        }

        answers.push(ValidatedAnswer {
            source: source_span,
            target,
        });
    }

    Ok(())
}

fn resolve_span(
    span_index: &HashMap<String, usize>,
    id: &str,
    annotation: usize,
) -> Result<usize, InvalidExtraction> {
    span_index.get(id).copied().ok_or_else(|| {
        invalid(format!(
            "annotation {annotation} references unknown span {id:?}"
        ))
    })
}

fn validate_assignee(assignee: &str, annotation: usize) -> Result<(), InvalidExtraction> {
    let Some(name) = assignee.strip_prefix('@') else {
        return Err(invalid(format!(
            "annotation {annotation}: assignee {assignee:?} must start with '@'"
        )));
    };
    if name.is_empty() {
        return Err(invalid(format!(
            "annotation {annotation}: assignee {assignee:?} has no name after '@'"
        )));
    }
    if name.chars().any(char::is_whitespace) {
        return Err(invalid(format!(
            "annotation {annotation}: assignee {assignee:?} contains whitespace"
        )));
    }
    Ok(())
}

fn ensure_no_overlaps(spans: &[ValidatedSpan]) -> Result<(), InvalidExtraction> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|index| (spans[*index].start_byte, spans[*index].end_byte));

    for pair in order.windows(2) {
        let left = &spans[pair[0]];
        let right = &spans[pair[1]];
        if right.start_byte < left.end_byte {
            return Err(invalid(format!(
                "spans {:?} and {:?} overlap or nest; spans must be disjoint",
                left.local_id, right.local_id
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use daftprompt_types::{ExtractedSpan, Extraction};

    fn span(id: &str, start: u32, end: u32) -> ExtractedSpan {
        ExtractedSpan {
            id: id.to_owned(),
            start_segment: start,
            end_segment: end,
            quote: None,
        }
    }

    fn question(span: &str) -> ExtractedAnnotation {
        ExtractedAnnotation {
            kind: AnnotationKind::Question,
            span: span.to_owned(),
            answers: Vec::new(),
            assignee: None,
        }
    }

    fn valid(
        source: &str,
        extraction: &Extraction,
    ) -> Result<ValidatedExtraction, InvalidExtraction> {
        validate(source, extraction)
    }

    #[test]
    fn rejects_invalid_assignees() {
        let text = "Should we?";
        for handle in ["infra-team", "@", "@infra team", "@ infra", "@infra\tteam"] {
            let extraction = Extraction {
                spans: vec![span("s1", 1, 1)],
                annotations: vec![ExtractedAnnotation {
                    assignee: Some(handle.to_owned()),
                    ..question("s1")
                }],
            };
            assert!(
                valid(text, &extraction).is_err(),
                "handle {handle:?} should be invalid"
            );
        }
    }

    #[test]
    fn accepts_a_valid_assignee() {
        let extraction = Extraction {
            spans: vec![span("s1", 1, 1)],
            annotations: vec![ExtractedAnnotation {
                assignee: Some("@infra-team".to_owned()),
                ..question("s1")
            }],
        };
        let validated = valid("Should we?", &extraction).unwrap();
        assert_eq!(
            validated.questions[0].assignee.as_deref(),
            Some("@infra-team")
        );
    }

    #[test]
    fn overlapping_spans_are_rejected() {
        let extraction = Extraction {
            spans: vec![span("s1", 1, 2), span("s2", 2, 3)],
            annotations: vec![question("s1"), question("s2")],
        };
        let error = valid("a\nb\nc", &extraction).unwrap_err();
        assert!(error.to_string().contains("overlap"));
    }
}
