//! Deterministic tests for the Explainer grounding evaluation: fixture
//! validation and metric computation with synthetic results. No credentials.

use std::time::Duration;

use daftprompt_core::eval::{
    ClarifyAggregate, ClarifyCategory, ClarifyFixture, ClarifyReport, ClarifyRun, ClarifyStatus,
    Observed, claims_not_found, clarify_metadata, default_clarify_fixture_dir,
    default_clarify_project_dir, load_clarify_fixtures, score_clarify,
};
use daftprompt_core::project_tools::{FsProject, ProjectFiles, ProjectToolConfig};
use daftprompt_types::CitationLocation;

fn fixtures() -> Vec<ClarifyFixture> {
    load_clarify_fixtures(default_clarify_fixture_dir(), default_clarify_project_dir())
        .expect("fixtures load and validate")
}

fn fixture(id: &str) -> ClarifyFixture {
    fixtures().into_iter().find(|f| f.id == id).expect(id)
}

fn file(path: &str, start_line: u32, end_line: u32) -> CitationLocation {
    CitationLocation::File {
        path: path.to_owned(),
        start_line,
        end_line,
    }
}

fn observed(explanation: &str, citations: Vec<CitationLocation>) -> Observed {
    Observed {
        explanation: explanation.to_owned(),
        citations,
        tool_inputs: vec!["{\"path\":\"src/limiter.rs\"}".to_owned()],
        project_tool_calls: 2,
        attempts: 1,
    }
}

#[test]
fn repository_fixtures_load_and_cover_every_category() {
    let all = fixtures();
    for category in ClarifyCategory::ALL {
        assert!(
            all.iter().any(|f| f.category == category),
            "no case covers {category}"
        );
    }
    // Requests build with a matching payload hash and in-bounds target.
    for f in &all {
        let request = f.request();
        assert_eq!(request.source.payload_hash.len(), 64);
        assert!(request.target.end_byte <= request.source.text.len());
        assert_eq!(request.turns.len(), f.turns.len());
    }
}

#[test]
fn fixture_project_files_are_visible_to_the_project_tools() {
    let project = FsProject::new(default_clarify_project_dir()).expect("project opens");
    for f in fixtures() {
        for expected in &f.expected.files {
            let content = project
                .read(&expected.path, &ProjectToolConfig::default())
                .unwrap_or_else(|e| panic!("{}: {} not readable: {e}", f.id, expected.path));
            assert!(
                content.text.lines().count() as u32 >= expected.end_line,
                "{}",
                f.id
            );
        }
    }
}

#[test]
fn validation_rejects_broken_fixtures() {
    let project = default_clarify_project_dir();
    let good = fixture("defined-in-file");
    assert!(good.validate(&project).is_ok());

    let mut bad = good.clone();
    bad.target.end = bad.text.len() + 1;
    assert!(bad.validate(&project).is_err(), "span out of bounds");

    let mut bad = good.clone();
    bad.target.quote = "something else".into();
    assert!(bad.validate(&project).is_err(), "quote mismatch");

    let mut bad = good.clone();
    bad.expected.files[0].path = "src/missing.rs".into();
    assert!(bad.validate(&project).is_err(), "missing file");

    let mut bad = good.clone();
    bad.expected.files[0].end_line = 9999;
    assert!(bad.validate(&project).is_err(), "line out of range");

    let mut bad = good.clone();
    bad.expected.not_found = true;
    assert!(bad.validate(&project).is_err(), "not_found with citations");

    let mut bad = good;
    bad.expected.injection_marker = Some("NOT-IN-PROJECT".into());
    assert!(bad.validate(&project).is_err(), "marker absent");

    let mut bad = fixture("defined-in-source");
    bad.expected.source_quotes = vec!["not in the text".into()];
    assert!(bad.validate(&project).is_err(), "source quote absent");
}

#[test]
fn exact_and_lenient_file_matching() {
    let f = fixture("defined-in-file"); // src/limiter.rs 3-12
    let exact = score_clarify(
        &f,
        &observed("A token bucket.", vec![file("src/limiter.rs", 3, 12)]),
    );
    assert_eq!(
        (
            exact.file_exact.expected_hit,
            exact.file_exact.predicted_hit
        ),
        (1, 1)
    );
    assert_eq!(exact.file_lenient.recall(), Some(1.0));

    let overlap = score_clarify(
        &f,
        &observed("A token bucket.", vec![file("src/limiter.rs", 5, 8)]),
    );
    assert_eq!(overlap.file_exact.expected_hit, 0);
    assert_eq!(overlap.file_exact.predicted_hit, 0);
    assert_eq!(overlap.file_lenient.expected_hit, 1);
    assert_eq!(overlap.file_lenient.precision(), Some(1.0));

    let wrong = score_clarify(
        &f,
        &observed(
            "A token bucket.",
            vec![
                file("src/limiter.rs", 3, 12),
                file("src/cache.rs", 7, 9),
                file("src/limiter.rs", 20, 25),
            ],
        ),
    );
    assert_eq!(wrong.file_lenient.predicted, 3);
    assert_eq!(wrong.file_lenient.predicted_hit, 1);
    assert_eq!(wrong.file_lenient.expected_hit, 1);
    assert!((wrong.file_lenient.precision().unwrap() - 1.0 / 3.0).abs() < 1e-9);

    let none = score_clarify(&f, &observed("A token bucket.", vec![]));
    assert_eq!(none.file_lenient.recall(), Some(0.0));
    assert_eq!(none.file_lenient.precision(), None);
}

#[test]
fn ambiguous_case_needs_both_files() {
    let f = fixture("ambiguous-quota");
    let one = score_clarify(
        &f,
        &observed("Quota means calls.", vec![file("src/billing.rs", 3, 6)]),
    );
    assert_eq!(one.file_exact.expected, 2);
    assert_eq!(one.file_exact.expected_hit, 1);
    let both = score_clarify(
        &f,
        &observed(
            "Billing quota vs storage quota.",
            vec![file("src/billing.rs", 3, 6), file("src/storage.rs", 3, 5)],
        ),
    );
    assert_eq!(both.file_exact.recall(), Some(1.0));
    assert_eq!(both.file_exact.precision(), Some(1.0));
}

#[test]
fn source_citations_match_by_byte_overlap() {
    let f = fixture("defined-in-source");
    let (start, end) = f.expected_source_ranges()[0];
    let hit = score_clarify(
        &f,
        &observed(
            "A canary ring is a small group.",
            vec![CitationLocation::Source {
                start_byte: start,
                end_byte: end + 5,
            }],
        ),
    );
    assert_eq!(hit.source_lenient.recall(), Some(1.0));
    assert_eq!(hit.source_lenient.precision(), Some(1.0));
    let miss = score_clarify(
        &f,
        &observed(
            "x",
            vec![CitationLocation::Source {
                start_byte: 0,
                end_byte: 5,
            }],
        ),
    );
    assert_eq!(miss.source_lenient.expected_hit, 0);
    assert_eq!(miss.source_lenient.predicted_hit, 0);
}

#[test]
fn not_found_correctness() {
    let f = fixture("not-found");
    assert!(f.expected.not_found);
    let good = score_clarify(
        &f,
        &observed(
            "I could not find a definition of the Zeitgeber service.",
            vec![],
        ),
    );
    assert!(good.claims_not_found && good.not_found_correct);

    let fabricated = score_clarify(
        &f,
        &observed(
            "I could not find it, but see [1].",
            vec![file("src/limiter.rs", 3, 5)],
        ),
    );
    assert!(
        !fabricated.not_found_correct,
        "a file citation is fabricated"
    );

    let confident = score_clarify(&f, &observed("It is a clock synchroniser.", vec![]));
    assert!(!confident.not_found_correct, "invented an answer");

    // A found case that wrongly claims "not found" is incorrect too.
    let found = fixture("defined-in-file");
    let wrong = score_clarify(&found, &observed("I couldn't find anything.", vec![]));
    assert!(!wrong.not_found_correct);
    let right = score_clarify(
        &found,
        &observed(
            "It refills at a fixed rate.",
            vec![file("src/limiter.rs", 3, 5)],
        ),
    );
    assert!(right.not_found_correct);
}

#[test]
fn not_found_phrases_handle_curly_apostrophes() {
    assert!(claims_not_found("I couldn\u{2019}t find it"));
    assert!(!claims_not_found("It refills at a fixed rate."));
}

#[test]
fn injection_resistance() {
    let f = fixture("injection-in-file");
    let marker = f.expected.injection_marker.clone().unwrap();
    let clean = score_clarify(
        &f,
        &observed(
            "Stale entries are served while refreshed.",
            vec![file("src/cache.rs", 7, 13)],
        ),
    );
    assert_eq!(clean.injection_resisted, Some(true));

    let echoed = score_clarify(
        &f,
        &observed(&format!("{}!", marker.to_lowercase()), vec![]),
    );
    assert_eq!(echoed.injection_resisted, Some(false));

    let mut acted = observed("ok", vec![]);
    acted
        .tool_inputs
        .push(format!("{{\"query\":\"{marker}\"}}"));
    assert_eq!(score_clarify(&f, &acted).injection_resisted, Some(false));

    // Cases without a marker report no injection result.
    let plain = fixture("defined-in-file");
    assert_eq!(
        score_clarify(&plain, &observed("x", vec![])).injection_resisted,
        None
    );
}

#[test]
fn aggregate_and_report_over_synthetic_runs() {
    let found = fixture("defined-in-file");
    let missing = fixture("not-found");
    let injected = fixture("injection-in-file");
    let make = |f: &ClarifyFixture, o: Observed, latency: f64| ClarifyRun {
        fixture_id: f.id.clone(),
        category: f.category,
        attempts: Some(o.attempts),
        latency_ms: latency,
        status: ClarifyStatus::Scored {
            score: Box::new(score_clarify(f, &o)),
            explanation: o.explanation.clone(),
            citations: vec![],
        },
    };
    let runs = vec![
        make(
            &found,
            observed("Bucket.", vec![file("src/limiter.rs", 3, 12)]),
            100.0,
        ),
        make(&missing, observed("I could not find it.", vec![]), 300.0),
        make(&injected, observed("PWNED-7731", vec![]), 200.0),
        ClarifyRun {
            fixture_id: "broken".into(),
            category: ClarifyCategory::FollowUp,
            attempts: Some(3),
            latency_ms: 400.0,
            status: ClarifyStatus::Failed {
                kind: "invalid_output".into(),
                detail: "bad".into(),
                attempts: Some(3),
            },
        },
    ];
    let aggregate = ClarifyAggregate::from_runs(&runs);
    assert_eq!(
        (aggregate.runs, aggregate.scored, aggregate.failed),
        (4, 3, 1)
    );
    assert_eq!(aggregate.invalid_output, 1);
    assert_eq!(aggregate.file_exact.expected, 2); // limiter + cache expectations
    assert_eq!(aggregate.file_exact.expected_hit, 1);
    assert_eq!(aggregate.file_exact.predicted_hit, 1);
    assert_eq!(
        (aggregate.injection_cases, aggregate.injection_resisted),
        (1, 0)
    );
    assert_eq!(aggregate.not_found_correct, 3); // none claims wrongly
    assert_eq!(aggregate.latencies.len(), 4);

    let metadata = clarify_metadata(
        Some("neuralwatt".into()),
        Some("glm".into()),
        3,
        Duration::from_secs(120),
        "fixtures".into(),
    );
    let all = fixtures();
    let mut report = ClarifyReport::new(metadata.clone(), "project".into(), &all);
    for run in runs {
        report.push(run);
    }
    let text = report.render();
    for needle in [
        "Aggregate",
        "Injection resistance: 0 of 1",
        "Invalid output: 1 of 4",
        "Manual review",
        "FAILED",
    ] {
        assert!(text.contains(needle), "report lacks {needle}");
    }

    let mut not_run = ClarifyReport::new(metadata, "project".into(), &all);
    not_run.not_run = Some("because".into());
    let text = not_run.render();
    assert!(text.contains("live evaluation not run"));
    assert!(text.contains("evaluate-explainer"));
    assert!(!text.contains("## Aggregate"));
}
