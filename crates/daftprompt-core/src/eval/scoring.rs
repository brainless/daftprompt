//! Deterministic scoring of a prediction against a fixture.
//!
//! Matching rules from epic 001, task 3:
//!
//! - A predicted span matches an expected span exactly when the byte ranges
//!   are equal after trimming surrounding whitespace.
//! - A lenient match additionally accepts overlap divided by union of at least
//!   [`LENIENT_MIN_IOU`] (`0.5`).
//! - A label is correct when its kind matches on a matched span.
//! - An answer link is correct when both ends match an expected link.
//! - An assignee is correct when the handle is identical on a matched
//!   question.
//!
//! Every metric is reported for both exact and lenient span matching. Matches
//! are one-to-one and chosen greedily, preferring exact pairs and then higher
//! overlap, so the result does not depend on input order.

use std::collections::{HashMap, HashSet};

use daftprompt_types::AnnotationKind;

use super::fixtures::Fixture;
use super::prediction::{ByteRange, Prediction};

/// Minimum overlap/union for a lenient span match.
pub const LENIENT_MIN_IOU: f64 = 0.5;

/// How predicted spans are matched to expected spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Byte ranges are equal after trimming surrounding whitespace.
    Exact,
    /// Ranges overlap and their overlap divided by union is at least
    /// [`LENIENT_MIN_IOU`].
    Lenient,
}

/// Expected, predicted, and correctly matched counts for one metric.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchCounts {
    pub expected: usize,
    pub predicted: usize,
    pub matched: usize,
}

impl MatchCounts {
    /// Correct predictions divided by all predictions, when any were made.
    pub fn precision(&self) -> Option<f64> {
        ratio(self.matched, self.predicted)
    }

    /// Correct matches divided by all expected items.
    pub fn recall(&self) -> Option<f64> {
        ratio(self.matched, self.expected)
    }

    /// The harmonic mean of precision and recall, when both exist.
    pub fn f1(&self) -> Option<f64> {
        match (self.precision(), self.recall()) {
            (Some(precision), Some(recall)) if precision + recall > 0.0 => {
                Some(2.0 * precision * recall / (precision + recall))
            }
            (Some(_), Some(_)) => Some(0.0),
            _ => None,
        }
    }

    /// Add another metric's counts, for an aggregate over fixtures.
    pub fn add(&mut self, other: Self) {
        self.expected += other.expected;
        self.predicted += other.predicted;
        self.matched += other.matched;
    }
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

/// The category of one quality problem found while scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IssueKind {
    /// No predicted span matched the expected span, even leniently.
    MissedSpan,
    /// A predicted span matched leniently but not exactly.
    ApproximateSpan,
    /// A predicted span matched no expected span.
    UnexpectedSpan,
    /// An expected label kind is absent from the matched span.
    MissedLabel,
    /// A predicted label kind was not expected on the matched span.
    UnexpectedLabel,
    /// An expected answer link is absent.
    MissedAnswerLink,
    /// A predicted answer link was not expected.
    UnexpectedAnswerLink,
    /// Both sides assigned the question, with different handles.
    AssigneeMismatch,
    /// The expected assignment is missing.
    MissedAssignee,
    /// An assignment was predicted where none was expected.
    UnexpectedAssignee,
    /// A predicted span could not be read from the text.
    UnscorableSpan,
}

impl IssueKind {
    /// A stable, human-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissedSpan => "missed_span",
            Self::ApproximateSpan => "approximate_span",
            Self::UnexpectedSpan => "unexpected_span",
            Self::MissedLabel => "missed_label",
            Self::UnexpectedLabel => "unexpected_label",
            Self::MissedAnswerLink => "missed_answer_link",
            Self::UnexpectedAnswerLink => "unexpected_answer_link",
            Self::AssigneeMismatch => "assignee_mismatch",
            Self::MissedAssignee => "missed_assignee",
            Self::UnexpectedAssignee => "unexpected_assignee",
            Self::UnscorableSpan => "unscorable_span",
        }
    }
}

impl std::fmt::Display for IssueKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One quality problem, with enough detail to explain the score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: IssueKind,
    pub detail: String,
}

/// The score for one fixture and one prediction.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub span_exact: MatchCounts,
    pub span_lenient: MatchCounts,
    pub label_exact: MatchCounts,
    pub label_lenient: MatchCounts,
    pub answer_exact: MatchCounts,
    pub answer_lenient: MatchCounts,
    pub assignee_exact: MatchCounts,
    pub assignee_lenient: MatchCounts,
    pub issues: Vec<Issue>,
}

/// Score `prediction` against `fixture`.
pub fn evaluate(fixture: &Fixture, prediction: &Prediction) -> Evaluation {
    let expected_ranges: Vec<Option<ByteRange>> = fixture
        .spans
        .iter()
        .map(|span| ByteRange::new(span.start, span.end).trimmed(&fixture.text))
        .collect();
    let predicted_ranges: Vec<Option<ByteRange>> = prediction
        .spans
        .iter()
        .map(|span| span.range.trimmed(&fixture.text))
        .collect();

    let exact = match_spans(&expected_ranges, &predicted_ranges, MatchMode::Exact);
    let lenient = match_spans(&expected_ranges, &predicted_ranges, MatchMode::Lenient);

    let expected_labels = expected_labels(fixture);
    let predicted_labels = predicted_labels(prediction);
    let expected_links = expected_links(fixture);
    let predicted_links = predicted_links(prediction);
    let expected_assignees = expected_assignees(fixture);
    let predicted_assignees = predicted_assignees(prediction);

    let span_exact = MatchCounts {
        expected: fixture.spans.len(),
        predicted: prediction.spans.len(),
        matched: exact.matched,
    };
    let span_lenient = MatchCounts {
        expected: fixture.spans.len(),
        predicted: prediction.spans.len(),
        matched: lenient.matched,
    };
    let label_exact = label_counts(&expected_labels, &predicted_labels, &exact);
    let label_lenient = label_counts(&expected_labels, &predicted_labels, &lenient);
    let answer_exact = link_counts(&expected_links, &predicted_links, &exact);
    let answer_lenient = link_counts(&expected_links, &predicted_links, &lenient);
    let assignee_exact = assignee_counts(&expected_assignees, &predicted_assignees, &exact);
    let assignee_lenient = assignee_counts(&expected_assignees, &predicted_assignees, &lenient);

    let mut issues = Vec::new();
    span_issues(
        fixture,
        prediction,
        &expected_ranges,
        &predicted_ranges,
        &exact,
        &lenient,
        &mut issues,
    );
    label_issues(
        fixture,
        prediction,
        &expected_labels,
        &predicted_labels,
        &lenient,
        &mut issues,
    );
    link_issues(
        fixture,
        prediction,
        &expected_links,
        &predicted_links,
        &lenient,
        &mut issues,
    );
    assignee_issues(
        fixture,
        prediction,
        &expected_assignees,
        &predicted_assignees,
        &lenient,
        &mut issues,
    );

    Evaluation {
        span_exact,
        span_lenient,
        label_exact,
        label_lenient,
        answer_exact,
        answer_lenient,
        assignee_exact,
        assignee_lenient,
        issues,
    }
}

// ---------------------------------------------------------------------------
// Span matching
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Matching {
    expected_to_predicted: Vec<Option<usize>>,
    predicted_to_expected: Vec<Option<usize>>,
    matched: usize,
}

fn match_spans(
    expected: &[Option<ByteRange>],
    predicted: &[Option<ByteRange>],
    mode: MatchMode,
) -> Matching {
    let mut matching = Matching {
        expected_to_predicted: vec![None; expected.len()],
        predicted_to_expected: vec![None; predicted.len()],
        matched: 0,
    };

    match mode {
        MatchMode::Exact => {
            let mut used = vec![false; expected.len()];
            for (predicted_index, predicted_range) in predicted.iter().enumerate() {
                let Some(predicted_range) = predicted_range else {
                    continue;
                };
                let found = expected.iter().enumerate().find_map(|(index, range)| {
                    (!used[index] && range.as_ref() == Some(predicted_range)).then_some(index)
                });
                if let Some(expected_index) = found {
                    used[expected_index] = true;
                    link(&mut matching, expected_index, predicted_index);
                }
            }
        }
        MatchMode::Lenient => {
            let mut candidates = Vec::new();
            for (expected_index, expected_range) in expected.iter().enumerate() {
                let Some(expected_range) = expected_range else {
                    continue;
                };
                for (predicted_index, predicted_range) in predicted.iter().enumerate() {
                    let Some(predicted_range) = predicted_range else {
                        continue;
                    };
                    let exact = expected_range == predicted_range;
                    let union = expected_range.union(*predicted_range);
                    let intersection = expected_range.intersection(*predicted_range);
                    let lenient = union > 0 && 2 * intersection >= union;
                    if exact || lenient {
                        candidates.push(Candidate {
                            expected_index,
                            predicted_index,
                            exact,
                            iou: expected_range.iou(*predicted_range),
                        });
                    }
                }
            }
            candidates.sort_by(|left, right| {
                right
                    .exact
                    .cmp(&left.exact)
                    .then_with(|| right.iou.total_cmp(&left.iou))
                    .then_with(|| left.expected_index.cmp(&right.expected_index))
                    .then_with(|| left.predicted_index.cmp(&right.predicted_index))
            });

            let mut used_expected = vec![false; expected.len()];
            let mut used_predicted = vec![false; predicted.len()];
            for candidate in candidates {
                if used_expected[candidate.expected_index]
                    || used_predicted[candidate.predicted_index]
                {
                    continue;
                }
                used_expected[candidate.expected_index] = true;
                used_predicted[candidate.predicted_index] = true;
                link(
                    &mut matching,
                    candidate.expected_index,
                    candidate.predicted_index,
                );
            }
        }
    }

    matching
}

#[derive(Debug)]
struct Candidate {
    expected_index: usize,
    predicted_index: usize,
    exact: bool,
    iou: f64,
}

fn link(matching: &mut Matching, expected_index: usize, predicted_index: usize) {
    matching.expected_to_predicted[expected_index] = Some(predicted_index);
    matching.predicted_to_expected[predicted_index] = Some(expected_index);
    matching.matched += 1;
}

// ---------------------------------------------------------------------------
// Label, link, and assignee sets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum LabelKey {
    Question,
    Answer,
}

impl LabelKey {
    fn as_str(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Answer => "answer",
        }
    }
}

fn expected_labels(fixture: &Fixture) -> HashSet<(usize, LabelKey)> {
    let mut labels = HashSet::new();
    for (index, span) in fixture.spans.iter().enumerate() {
        for label in &span.labels {
            if label.kind == AnnotationKind::Question {
                labels.insert((index, LabelKey::Question));
            }
        }
    }
    for link in &fixture.answer_links {
        if let (Some(from), Some(_)) =
            (fixture.span_index(&link.from), fixture.span_index(&link.to))
        {
            labels.insert((from, LabelKey::Answer));
        }
    }
    labels
}

fn predicted_labels(prediction: &Prediction) -> HashSet<(usize, LabelKey)> {
    let mut labels = HashSet::new();
    for label in &prediction.labels {
        if label.span >= prediction.spans.len() {
            continue;
        }
        let key = match label.kind {
            AnnotationKind::Question => LabelKey::Question,
            AnnotationKind::Answer => LabelKey::Answer,
        };
        labels.insert((label.span, key));
    }
    labels
}

fn label_counts(
    expected: &HashSet<(usize, LabelKey)>,
    predicted: &HashSet<(usize, LabelKey)>,
    matching: &Matching,
) -> MatchCounts {
    let mut matched = 0;
    for (expected_index, predicted_index) in matching.expected_to_predicted.iter().enumerate() {
        let Some(predicted_index) = predicted_index else {
            continue;
        };
        for key in [LabelKey::Question, LabelKey::Answer] {
            if expected.contains(&(expected_index, key))
                && predicted.contains(&(*predicted_index, key))
            {
                matched += 1;
            }
        }
    }
    MatchCounts {
        expected: expected.len(),
        predicted: predicted.len(),
        matched,
    }
}

fn expected_links(fixture: &Fixture) -> HashSet<(usize, usize)> {
    fixture
        .answer_links
        .iter()
        .filter_map(|link| {
            Some((
                fixture.span_index(&link.from)?,
                fixture.span_index(&link.to)?,
            ))
        })
        .collect()
}

fn predicted_links(prediction: &Prediction) -> HashSet<(usize, usize)> {
    prediction
        .answer_links
        .iter()
        .filter(|link| link.from < prediction.spans.len() && link.to < prediction.spans.len())
        .map(|link| (link.from, link.to))
        .collect()
}

fn link_counts(
    expected: &HashSet<(usize, usize)>,
    predicted: &HashSet<(usize, usize)>,
    matching: &Matching,
) -> MatchCounts {
    let mut matched = 0;
    for (from, to) in predicted {
        let (Some(expected_from), Some(expected_to)) = (
            matching.predicted_to_expected.get(*from).copied().flatten(),
            matching.predicted_to_expected.get(*to).copied().flatten(),
        ) else {
            continue;
        };
        if expected.contains(&(expected_from, expected_to)) {
            matched += 1;
        }
    }
    MatchCounts {
        expected: expected.len(),
        predicted: predicted.len(),
        matched,
    }
}

fn expected_assignees(fixture: &Fixture) -> HashMap<usize, String> {
    let mut assignees = HashMap::new();
    for (index, span) in fixture.spans.iter().enumerate() {
        for label in &span.labels {
            if let Some(assignee) = &label.assignee {
                assignees.insert(index, assignee.clone());
            }
        }
    }
    assignees
}

fn predicted_assignees(prediction: &Prediction) -> HashMap<usize, String> {
    let mut assignees = HashMap::new();
    for label in &prediction.labels {
        if label.kind != AnnotationKind::Question || label.span >= prediction.spans.len() {
            continue;
        }
        if let Some(assignee) = &label.assignee {
            assignees
                .entry(label.span)
                .or_insert_with(|| assignee.clone());
        }
    }
    assignees
}

fn assignee_counts(
    expected: &HashMap<usize, String>,
    predicted: &HashMap<usize, String>,
    matching: &Matching,
) -> MatchCounts {
    let mut matched = 0;
    for (expected_index, predicted_index) in matching.expected_to_predicted.iter().enumerate() {
        let Some(predicted_index) = predicted_index else {
            continue;
        };
        if let (Some(expected_handle), Some(predicted_handle)) = (
            expected.get(&expected_index),
            predicted.get(predicted_index),
        ) && expected_handle == predicted_handle
        {
            matched += 1;
        }
    }
    MatchCounts {
        expected: expected.len(),
        predicted: predicted.len(),
        matched,
    }
}

// ---------------------------------------------------------------------------
// Issue details
// ---------------------------------------------------------------------------

fn span_issues(
    fixture: &Fixture,
    prediction: &Prediction,
    expected_ranges: &[Option<ByteRange>],
    predicted_ranges: &[Option<ByteRange>],
    exact: &Matching,
    lenient: &Matching,
    issues: &mut Vec<Issue>,
) {
    for (index, span) in fixture.spans.iter().enumerate() {
        if exact.expected_to_predicted[index].is_some() {
            continue;
        }
        match lenient.expected_to_predicted[index] {
            Some(predicted_index) => {
                let iou = expected_ranges[index]
                    .zip(predicted_ranges[predicted_index])
                    .map_or(0.0, |(expected, predicted)| expected.iou(predicted));
                issues.push(Issue {
                    kind: IssueKind::ApproximateSpan,
                    detail: format!(
                        "{} matched leniently to {} (IoU {iou:.2}) but not exactly",
                        describe_expected(fixture, index),
                        describe_predicted(fixture, prediction, predicted_index)
                    ),
                });
            }
            None => issues.push(Issue {
                kind: IssueKind::MissedSpan,
                detail: format!(
                    "{} was not predicted by any span",
                    describe_expected(fixture, index)
                ),
            }),
        }
        let _ = span;
    }

    for (index, span) in prediction.spans.iter().enumerate() {
        if lenient.predicted_to_expected[index].is_some() {
            continue;
        }
        let detail = match span.range.text(&fixture.text) {
            Some(_) => format!(
                "{} matched no expected span",
                describe_predicted(fixture, prediction, index)
            ),
            None => format!(
                "predicted span {}..{} is not on char boundaries of the text and cannot be scored",
                span.range.start, span.range.end
            ),
        };
        issues.push(Issue {
            kind: if span.range.text(&fixture.text).is_some() {
                IssueKind::UnexpectedSpan
            } else {
                IssueKind::UnscorableSpan
            },
            detail,
        });
    }
}

fn label_issues(
    fixture: &Fixture,
    prediction: &Prediction,
    expected: &HashSet<(usize, LabelKey)>,
    predicted: &HashSet<(usize, LabelKey)>,
    matching: &Matching,
    issues: &mut Vec<Issue>,
) {
    let mut expected_items: Vec<_> = expected.iter().copied().collect();
    expected_items.sort();
    for (expected_index, key) in expected_items {
        match matching.expected_to_predicted[expected_index] {
            Some(predicted_index) if predicted.contains(&(predicted_index, key)) => {}
            Some(_) => issues.push(Issue {
                kind: IssueKind::MissedLabel,
                detail: format!(
                    "expected {} label on {} but it was not predicted",
                    key.as_str(),
                    describe_expected(fixture, expected_index)
                ),
            }),
            None => issues.push(Issue {
                kind: IssueKind::MissedLabel,
                detail: format!(
                    "expected {} label on {}, but the span was not matched",
                    key.as_str(),
                    describe_expected(fixture, expected_index)
                ),
            }),
        }
    }

    let mut predicted_items: Vec<_> = predicted.iter().copied().collect();
    predicted_items.sort();
    for (predicted_index, key) in predicted_items {
        match matching.predicted_to_expected[predicted_index] {
            Some(expected_index) if expected.contains(&(expected_index, key)) => {}
            Some(_) => issues.push(Issue {
                kind: IssueKind::UnexpectedLabel,
                detail: format!(
                    "predicted {} label on {} but none was expected",
                    key.as_str(),
                    describe_predicted(fixture, prediction, predicted_index)
                ),
            }),
            None => issues.push(Issue {
                kind: IssueKind::UnexpectedLabel,
                detail: format!(
                    "predicted {} label on {}, but the span was not matched",
                    key.as_str(),
                    describe_predicted(fixture, prediction, predicted_index)
                ),
            }),
        }
    }
}

fn link_issues(
    fixture: &Fixture,
    prediction: &Prediction,
    expected: &HashSet<(usize, usize)>,
    predicted: &HashSet<(usize, usize)>,
    matching: &Matching,
    issues: &mut Vec<Issue>,
) {
    let mut expected_items: Vec<_> = expected.iter().copied().collect();
    expected_items.sort();
    for (from, to) in expected_items {
        let mapped = (
            matching.expected_to_predicted[from],
            matching.expected_to_predicted[to],
        );
        match mapped {
            (Some(predicted_from), Some(predicted_to))
                if predicted.contains(&(predicted_from, predicted_to)) => {}
            (Some(_), Some(_)) => issues.push(Issue {
                kind: IssueKind::MissedAnswerLink,
                detail: format!(
                    "expected answer link {} -> {} but it was not predicted",
                    describe_expected(fixture, from),
                    describe_expected(fixture, to)
                ),
            }),
            _ => issues.push(Issue {
                kind: IssueKind::MissedAnswerLink,
                detail: format!(
                    "expected answer link {} -> {}, but a span end was not matched",
                    describe_expected(fixture, from),
                    describe_expected(fixture, to)
                ),
            }),
        }
    }

    let mut predicted_items: Vec<_> = predicted.iter().copied().collect();
    predicted_items.sort();
    for (from, to) in predicted_items {
        let mapped = (
            matching.predicted_to_expected[from],
            matching.predicted_to_expected[to],
        );
        match mapped {
            (Some(expected_from), Some(expected_to))
                if expected.contains(&(expected_from, expected_to)) => {}
            (Some(_), Some(_)) => issues.push(Issue {
                kind: IssueKind::UnexpectedAnswerLink,
                detail: format!(
                    "predicted answer link {} -> {} but none was expected",
                    describe_predicted(fixture, prediction, from),
                    describe_predicted(fixture, prediction, to)
                ),
            }),
            _ => issues.push(Issue {
                kind: IssueKind::UnexpectedAnswerLink,
                detail: format!(
                    "predicted answer link {} -> {}, but a span was not matched",
                    describe_predicted(fixture, prediction, from),
                    describe_predicted(fixture, prediction, to)
                ),
            }),
        }
    }
}

fn assignee_issues(
    fixture: &Fixture,
    prediction: &Prediction,
    expected: &HashMap<usize, String>,
    predicted: &HashMap<usize, String>,
    matching: &Matching,
    issues: &mut Vec<Issue>,
) {
    for (expected_index, expected_handle) in expected {
        match matching.expected_to_predicted[*expected_index] {
            Some(predicted_index) => match predicted.get(&predicted_index) {
                Some(predicted_handle) if predicted_handle == expected_handle => {}
                Some(predicted_handle) => issues.push(Issue {
                    kind: IssueKind::AssigneeMismatch,
                    detail: format!(
                        "expected {expected_handle} on {} but predicted {predicted_handle}",
                        describe_expected(fixture, *expected_index)
                    ),
                }),
                None => issues.push(Issue {
                    kind: IssueKind::MissedAssignee,
                    detail: format!(
                        "expected {expected_handle} on {} but none was predicted",
                        describe_expected(fixture, *expected_index)
                    ),
                }),
            },
            None => issues.push(Issue {
                kind: IssueKind::MissedAssignee,
                detail: format!(
                    "expected {expected_handle} on {}, but the span was not matched",
                    describe_expected(fixture, *expected_index)
                ),
            }),
        }
    }

    for (predicted_index, predicted_handle) in predicted {
        match matching.predicted_to_expected[*predicted_index] {
            Some(expected_index) if expected.contains_key(&expected_index) => {}
            Some(_) => issues.push(Issue {
                kind: IssueKind::UnexpectedAssignee,
                detail: format!(
                    "predicted {predicted_handle} on {} but no assignment was expected",
                    describe_predicted(fixture, prediction, *predicted_index)
                ),
            }),
            None => issues.push(Issue {
                kind: IssueKind::UnexpectedAssignee,
                detail: format!(
                    "predicted {predicted_handle} on {}, but the span was not matched",
                    describe_predicted(fixture, prediction, *predicted_index)
                ),
            }),
        }
    }
}

fn describe_expected(fixture: &Fixture, index: usize) -> String {
    let span = &fixture.spans[index];
    format!(
        "expected span {:?} [{}..{}] {:?}",
        span.id,
        span.start,
        span.end,
        truncate(&span.quote, 60)
    )
}

fn describe_predicted(fixture: &Fixture, prediction: &Prediction, index: usize) -> String {
    let range = prediction.spans[index].range;
    let text = range
        .text(&fixture.text)
        .map_or("<invalid range>".to_owned(), |text| {
            format!("{:?}", truncate(text, 60))
        });
    format!("predicted span [{}..{}] {text}", range.start, range.end)
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut result: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::fixtures::{
        ExpectedAnswerLink, ExpectedLabel, ExpectedSpan, Fixture, FixtureCategory,
    };
    use crate::eval::prediction::{PredictedAnswerLink, PredictedLabel, PredictedSpan};

    const FIXTURE_FORMAT_VERSION: u32 = 1;

    fn fixture(text: &str, spans: Vec<ExpectedSpan>, links: Vec<ExpectedAnswerLink>) -> Fixture {
        Fixture {
            format_version: FIXTURE_FORMAT_VERSION,
            id: "test".to_owned(),
            category: FixtureCategory::Chat,
            note: String::new(),
            source: None,
            text: text.to_owned(),
            spans,
            answer_links: links,
        }
    }

    fn expected(
        id: &str,
        start: usize,
        end: usize,
        quote: &str,
        question: bool,
        assignee: Option<&str>,
    ) -> ExpectedSpan {
        ExpectedSpan {
            id: id.to_owned(),
            start,
            end,
            quote: quote.to_owned(),
            labels: if question {
                vec![ExpectedLabel {
                    kind: AnnotationKind::Question,
                    assignee: assignee.map(str::to_owned),
                }]
            } else {
                Vec::new()
            },
        }
    }

    fn answer_link(from: &str, to: &str) -> ExpectedAnswerLink {
        ExpectedAnswerLink {
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    fn predicted_span(start: usize, end: usize) -> PredictedSpan {
        PredictedSpan {
            range: ByteRange::new(start, end),
        }
    }

    fn predicted_question(span: usize, assignee: Option<&str>) -> PredictedLabel {
        PredictedLabel {
            kind: AnnotationKind::Question,
            span,
            assignee: assignee.map(str::to_owned),
        }
    }

    fn predicted_answer(span: usize) -> PredictedLabel {
        PredictedLabel {
            kind: AnnotationKind::Answer,
            span,
            assignee: None,
        }
    }

    fn question_answer_fixture() -> Fixture {
        let text = "Should we cache tokens?\nYes, cache them.\n";
        assert_eq!(&text[0..23], "Should we cache tokens?");
        assert_eq!(&text[24..40], "Yes, cache them.");
        fixture(
            text,
            vec![
                expected("e1", 0, 23, "Should we cache tokens?", true, None),
                expected("e2", 24, 40, "Yes, cache them.", false, None),
            ],
            vec![answer_link("e2", "e1")],
        )
    }

    #[test]
    fn a_perfect_prediction_scores_one_hundred_percent() {
        let fixture = question_answer_fixture();
        let prediction = Prediction {
            spans: vec![predicted_span(0, 23), predicted_span(24, 40)],
            labels: vec![predicted_question(0, None), predicted_answer(1)],
            answer_links: vec![PredictedAnswerLink { from: 1, to: 0 }],
        };

        let evaluation = evaluate(&fixture, &prediction);
        for counts in [
            evaluation.span_exact,
            evaluation.span_lenient,
            evaluation.label_exact,
            evaluation.label_lenient,
            evaluation.answer_exact,
            evaluation.answer_lenient,
        ] {
            assert_eq!(counts.matched, counts.expected);
            assert_eq!(counts.matched, counts.predicted);
        }
        assert_eq!(evaluation.span_exact.expected, 2);
        assert_eq!(evaluation.label_exact.expected, 2);
        assert_eq!(evaluation.answer_exact.expected, 1);
        assert_eq!(evaluation.assignee_exact.precision(), None);
        assert!(evaluation.issues.is_empty(), "{:?}", evaluation.issues);
    }

    #[test]
    fn whitespace_is_trimmed_before_comparing_ranges() {
        let fixture = fixture(
            "  Question?  ",
            vec![expected("e1", 2, 11, "Question?", true, None)],
            Vec::new(),
        );
        let prediction = Prediction {
            spans: vec![predicted_span(0, 13)],
            labels: vec![predicted_question(0, None)],
            answer_links: Vec::new(),
        };

        let evaluation = evaluate(&fixture, &prediction);
        assert_eq!(evaluation.span_exact.matched, 1);
        assert_eq!(evaluation.label_exact.matched, 1);
        assert!(evaluation.issues.is_empty(), "{:?}", evaluation.issues);
    }

    #[test]
    fn a_partial_overlap_counts_only_leniently() {
        let fixture = question_answer_fixture();
        let prediction = Prediction {
            spans: vec![predicted_span(0, 20), predicted_span(24, 40)],
            labels: vec![predicted_question(0, None), predicted_answer(1)],
            answer_links: vec![PredictedAnswerLink { from: 1, to: 0 }],
        };

        let evaluation = evaluate(&fixture, &prediction);
        assert_eq!(evaluation.span_exact.matched, 1);
        assert_eq!(evaluation.span_lenient.matched, 2);
        assert_eq!(evaluation.label_exact.matched, 1);
        assert_eq!(evaluation.label_lenient.matched, 2);
        assert_eq!(evaluation.answer_exact.matched, 0);
        assert_eq!(evaluation.answer_lenient.matched, 1);
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::ApproximateSpan)
        );
    }

    #[test]
    fn a_span_below_the_overlap_threshold_is_missed() {
        let fixture = question_answer_fixture();
        let prediction = Prediction {
            spans: vec![predicted_span(0, 10)],
            labels: vec![predicted_question(0, None)],
            answer_links: Vec::new(),
        };

        let evaluation = evaluate(&fixture, &prediction);
        assert_eq!(evaluation.span_exact.matched, 0);
        assert_eq!(evaluation.span_lenient.matched, 0);
        let kinds: Vec<_> = evaluation.issues.iter().map(|issue| issue.kind).collect();
        assert!(kinds.contains(&IssueKind::MissedSpan));
        assert!(kinds.contains(&IssueKind::UnexpectedSpan));
        assert!(kinds.contains(&IssueKind::MissedLabel));
        assert!(kinds.contains(&IssueKind::UnexpectedLabel));
    }

    #[test]
    fn labels_must_match_kind_on_the_matched_span() {
        let fixture = question_answer_fixture();
        let prediction = Prediction {
            spans: vec![predicted_span(0, 23)],
            labels: vec![predicted_answer(0)],
            answer_links: Vec::new(),
        };

        let evaluation = evaluate(&fixture, &prediction);
        assert_eq!(evaluation.label_exact.expected, 2);
        assert_eq!(evaluation.label_exact.predicted, 1);
        assert_eq!(evaluation.label_exact.matched, 0);
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::MissedLabel)
        );
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::UnexpectedLabel)
        );
    }

    #[test]
    fn answer_links_need_both_ends_to_match() {
        let fixture = question_answer_fixture();
        let prediction = Prediction {
            spans: vec![predicted_span(0, 23), predicted_span(24, 40)],
            labels: vec![predicted_question(0, None), predicted_answer(1)],
            answer_links: vec![PredictedAnswerLink { from: 0, to: 1 }],
        };

        let evaluation = evaluate(&fixture, &prediction);
        assert_eq!(evaluation.answer_exact.expected, 1);
        assert_eq!(evaluation.answer_exact.predicted, 1);
        assert_eq!(evaluation.answer_exact.matched, 0);
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::MissedAnswerLink)
        );
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::UnexpectedAnswerLink)
        );
    }

    fn assigned_fixture(assignee: Option<&str>) -> Fixture {
        fixture(
            "Who owns it?",
            vec![expected("e1", 0, 12, "Who owns it?", true, assignee)],
            Vec::new(),
        )
    }

    #[test]
    fn assignees_are_compared_only_on_matched_questions() {
        let assignment = assigned_fixture(Some("@infra-team"));
        let mismatched = Prediction {
            spans: vec![predicted_span(0, 12)],
            labels: vec![predicted_question(0, Some("@alex"))],
            answer_links: Vec::new(),
        };
        let evaluation = evaluate(&assignment, &mismatched);
        assert_eq!(evaluation.assignee_exact.expected, 1);
        assert_eq!(evaluation.assignee_exact.predicted, 1);
        assert_eq!(evaluation.assignee_exact.matched, 0);
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::AssigneeMismatch)
        );

        let missing = Prediction {
            spans: vec![predicted_span(0, 12)],
            labels: vec![predicted_question(0, None)],
            answer_links: Vec::new(),
        };
        let evaluation = evaluate(&assignment, &missing);
        assert_eq!(evaluation.assignee_exact.matched, 0);
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::MissedAssignee)
        );

        let unchanged = assigned_fixture(None);
        let unexpected = Prediction {
            spans: vec![predicted_span(0, 12)],
            labels: vec![predicted_question(0, Some("@alex"))],
            answer_links: Vec::new(),
        };
        let evaluation = evaluate(&unchanged, &unexpected);
        assert_eq!(evaluation.assignee_exact.expected, 0);
        assert_eq!(evaluation.assignee_exact.predicted, 1);
        assert_eq!(evaluation.assignee_exact.matched, 0);
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::UnexpectedAssignee)
        );
    }

    #[test]
    fn an_assignee_on_an_unmatched_span_is_counted_as_missed() {
        let assignment = assigned_fixture(Some("@infra-team"));
        let prediction = Prediction {
            spans: vec![predicted_span(50, 62)],
            labels: vec![predicted_question(0, None)],
            answer_links: Vec::new(),
        };
        let evaluation = evaluate(&assignment, &prediction);
        assert_eq!(evaluation.assignee_exact.expected, 1);
        assert_eq!(evaluation.assignee_exact.matched, 0);
        assert!(
            evaluation
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::MissedAssignee)
        );
    }

    #[test]
    fn counts_aggregate_with_add() {
        let mut total = MatchCounts::default();
        total.add(MatchCounts {
            expected: 2,
            predicted: 3,
            matched: 1,
        });
        total.add(MatchCounts {
            expected: 1,
            predicted: 1,
            matched: 1,
        });
        assert_eq!(
            total,
            MatchCounts {
                expected: 3,
                predicted: 4,
                matched: 2
            }
        );
        assert_eq!(total.precision(), Some(0.5));
        assert_eq!(total.recall(), Some(2.0 / 3.0));
    }
}
