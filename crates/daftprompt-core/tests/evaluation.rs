//! Deterministic, fixture-driven tests for the evaluation path.
//!
//! A stub model returns exactly the annotated extraction for each fixture. The
//! normal Scribe pipeline validates and builds a graph; scoring the resulting
//! parse must then be perfect. This proves the fixtures and the scoring rules
//! agree without any credentials or network access.

use daftprompt_core::eval::{Prediction, default_fixture_dir, evaluate, load_fixtures};
use daftprompt_core::{
    BoxFuture, ExtractionModel, ModelError, ModelInput, Scribe, ScribeConfig, segments,
};
use daftprompt_types::{
    AnnotationKind, ExtractedAnnotation, ExtractedSpan, Extraction, ParseRequest, SourceMetadata,
};

struct StubModel {
    extraction: Extraction,
}

impl ExtractionModel for StubModel {
    fn extract(&self, _input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        Box::pin(std::future::ready(Ok(self.extraction.clone())))
    }
}

/// The extraction a perfect Scribe would submit for `fixture`.
fn expected_extraction(fixture: &daftprompt_core::eval::Fixture) -> Extraction {
    let lines = segments(&fixture.text);
    let segment_at = |byte: usize| {
        lines
            .iter()
            .find(|segment| segment.start_byte <= byte && byte <= segment.end_byte)
            .map(|segment| segment.number)
            .expect("fixture byte offset belongs to a segment")
    };

    let local_ids: Vec<String> = (1..=fixture.spans.len())
        .map(|index| format!("s{index}"))
        .collect();

    let spans: Vec<ExtractedSpan> = fixture
        .spans
        .iter()
        .zip(&local_ids)
        .map(|(span, id)| ExtractedSpan {
            id: id.clone(),
            start_segment: segment_at(span.start),
            end_segment: if span.end > span.start {
                segment_at(span.end - 1)
            } else {
                segment_at(span.start)
            },
            quote: Some(span.quote.clone()),
        })
        .collect();

    let mut annotations = Vec::new();
    for (span, id) in fixture.spans.iter().zip(&local_ids) {
        for label in &span.labels {
            annotations.push(ExtractedAnnotation {
                kind: label.kind,
                span: id.clone(),
                questions: Vec::new(),
                alternatives: Vec::new(),
                answers: Vec::new(),
                assignee: label.assignee.clone(),
            });
        }
    }
    for link in &fixture.answer_links {
        let from = fixture
            .span_index(&link.from)
            .expect("fixture loader validated the source");
        let to = fixture
            .span_index(&link.to)
            .expect("fixture loader validated the target");
        annotations.push(ExtractedAnnotation {
            kind: AnnotationKind::Answer,
            span: local_ids[from].clone(),
            questions: Vec::new(),
            alternatives: Vec::new(),
            answers: vec![local_ids[to].clone()],
            assignee: None,
        });
    }

    for (links, alternative) in [
        (&fixture.recommendation_links, false),
        (&fixture.alternative_links, true),
    ] {
        for link in links {
            let from = fixture.span_index(&link.from).unwrap();
            let to = fixture.span_index(&link.to).unwrap();
            let annotation = annotations
                .iter_mut()
                .find(|a| a.span == local_ids[from] && a.kind == AnnotationKind::Recommendation)
                .unwrap();
            if alternative {
                annotation.alternatives.push(local_ids[to].clone());
            } else {
                annotation.questions.push(local_ids[to].clone());
            }
        }
    }

    Extraction { spans, annotations }
}

fn request(fixture: &daftprompt_core::eval::Fixture) -> ParseRequest {
    match &fixture.source {
        Some(source) => ParseRequest::with_source(fixture.text.clone(), source.clone()),
        None => ParseRequest::new(fixture.text.clone()),
    }
}

#[tokio::test]
async fn every_fixture_scores_perfectly_when_the_model_returns_the_annotations() {
    let fixtures = load_fixtures(default_fixture_dir()).expect("fixtures load");
    assert!(!fixtures.is_empty());

    for fixture in &fixtures {
        let scribe = Scribe::new(
            ScribeConfig::new().with_max_attempts(1),
            StubModel {
                extraction: expected_extraction(fixture),
            },
        );
        let result = scribe
            .parse(request(fixture))
            .await
            .unwrap_or_else(|error| panic!("fixture {:?} failed to parse: {error}", fixture.id));

        let prediction = Prediction::from_parse_result(&result);
        let evaluation = evaluate(fixture, &prediction);

        assert_eq!(
            evaluation.span_exact.matched,
            fixture.spans.len(),
            "fixture {:?}: exact span matches",
            fixture.id
        );
        assert_eq!(
            evaluation.label_exact.matched, evaluation.label_exact.expected,
            "fixture {:?}: exact label matches",
            fixture.id
        );
        assert_eq!(
            evaluation.answer_exact.matched, evaluation.answer_exact.expected,
            "fixture {:?}: exact answer links",
            fixture.id
        );
        assert_eq!(
            evaluation.assignee_exact.matched, evaluation.assignee_exact.expected,
            "fixture {:?}: exact assignees",
            fixture.id
        );
        for counts in [
            evaluation.recommendation_exact,
            evaluation.task_exact,
            evaluation.recommendation_link_exact,
            evaluation.alternative_exact,
        ] {
            assert_eq!(counts.expected, counts.matched, "{}", fixture.id);
            assert_eq!(counts.predicted, counts.matched, "{}", fixture.id);
        }
        assert!(
            evaluation.issues.is_empty(),
            "fixture {:?} produced quality issues: {:?}",
            fixture.id,
            evaluation.issues
        );
    }
}

#[tokio::test]
async fn provenance_from_a_fixture_reaches_the_parse_request() {
    let fixtures = load_fixtures(default_fixture_dir()).expect("fixtures load");
    let fixture = fixtures
        .iter()
        .find(|fixture| fixture.source.is_some())
        .expect("at least one fixture has provenance");

    let scribe = Scribe::new(
        ScribeConfig::new().with_max_attempts(1),
        StubModel {
            extraction: Extraction::default(),
        },
    );
    let expected_source: SourceMetadata = fixture.source.clone().expect("source exists");
    let result = scribe
        .parse(request(fixture))
        .await
        .expect("parse succeeds");

    assert_eq!(result.source, Some(expected_source));
}

#[test]
fn recommendation_links_alternatives_and_task_assignments_score_separately() {
    use daftprompt_core::eval::{
        ByteRange, IssueKind, PredictedAnswerLink, PredictedLabel, PredictedSpan,
    };
    let fixtures = load_fixtures(default_fixture_dir()).unwrap();
    let fixture = fixtures
        .iter()
        .find(|f| f.id == "standalone-alternatives")
        .unwrap();
    let mut prediction = Prediction {
        spans: fixture
            .spans
            .iter()
            .map(|s| PredictedSpan {
                range: ByteRange::new(s.start, s.end),
            })
            .collect(),
        labels: fixture
            .spans
            .iter()
            .enumerate()
            .map(|(span, _)| PredictedLabel {
                kind: AnnotationKind::Recommendation,
                span,
                assignee: None,
            })
            .collect(),
        answer_links: vec![],
        recommendation_links: vec![],
        alternative_links: vec![
            PredictedAnswerLink { from: 0, to: 1 },
            PredictedAnswerLink { from: 2, to: 0 },
            PredictedAnswerLink { from: 1, to: 2 },
        ],
    };
    let score = evaluate(fixture, &prediction);
    assert_eq!(score.alternative_exact.matched, 3);
    assert_eq!(score.recommendation_exact.predicted, 4);
    assert!(score.issues.is_empty());
    prediction.alternative_links.pop();
    let score = evaluate(fixture, &prediction);
    assert_eq!(score.alternative_exact.matched, 2);
    assert!(
        score
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::MissedAlternativeLink)
    );
    let fixture = fixtures
        .iter()
        .find(|f| f.id == "advice-not-decision")
        .unwrap();
    prediction = Prediction {
        spans: fixture
            .spans
            .iter()
            .map(|s| PredictedSpan {
                range: ByteRange::new(s.start, s.end),
            })
            .collect(),
        labels: fixture
            .spans
            .iter()
            .enumerate()
            .flat_map(|(span, s)| {
                s.labels.iter().map(move |l| PredictedLabel {
                    kind: l.kind,
                    span,
                    assignee: l.assignee.clone(),
                })
            })
            .collect(),
        answer_links: vec![],
        recommendation_links: vec![PredictedAnswerLink { from: 1, to: 0 }],
        alternative_links: vec![],
    };
    let score = evaluate(fixture, &prediction);
    assert_eq!(score.task_exact.matched, 1);
    assert_eq!(score.assignee_exact.matched, 1);
    assert_eq!(score.recommendation_link_exact.matched, 1);
    assert_eq!(score.answer_exact.predicted, 0);
    prediction.labels[2].kind = AnnotationKind::Question;
    let score = evaluate(fixture, &prediction);
    assert_eq!(score.assignee_exact.matched, 0);
    assert_eq!(score.task_exact.matched, 0);
    prediction.recommendation_links.clear();
    assert!(
        evaluate(fixture, &prediction)
            .issues
            .iter()
            .any(|i| i.kind == IssueKind::MissedRecommendationLink)
    );
}
