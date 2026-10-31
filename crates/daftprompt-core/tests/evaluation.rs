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
            answers: vec![local_ids[to].clone()],
            assignee: None,
        });
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
