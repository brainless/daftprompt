//! Offline fixtures and scoring for the opt-in Scribe evaluation.
//!
//! The fixture set is manually annotated and lives in `fixtures/scribe/` as
//! JSON: one file per text, with byte ranges and verbatim quotes for the spans
//! that should be labelled. The scoring rules turn a validated
//! [`ParseResult`](daftprompt_types::ParseResult) into comparable counts and are
//! deterministic, so they are tested without a model.
//!
//! A live run is started by the `evaluate-scribe` binary behind the
//! `live-eval` feature. It uses the same [`Scribe`](crate::scribe::Scribe)
//! parser as the future API and GUI. Ordinary builds and tests need no
//! credentials.

mod clarify;
mod fixtures;
mod prediction;
mod report;
mod scoring;

pub use clarify::{
    CLARIFY_FORMAT_VERSION, CaseScore, CaseTarget, CaseTurn, ClarifyAggregate, ClarifyCategory,
    ClarifyFixture, ClarifyFixtureError, ClarifyReport, ClarifyRun, ClarifyStatus, Coverage,
    Expected, ExpectedFile, Observed, claims_not_found, default_clarify_fixture_dir,
    default_clarify_project_dir, describe_location, load_clarify_fixtures,
    metadata as clarify_metadata, score as score_clarify,
};
pub use fixtures::{
    ExpectedAnswerLink, ExpectedLabel, ExpectedSpan, Fixture, FixtureCategory, FixtureError,
    default_fixture_dir, load_fixtures,
};
pub use prediction::{ByteRange, PredictedAnswerLink, PredictedLabel, PredictedSpan, Prediction};
pub use report::{FixtureRun, FixtureStatus, FixtureSummary, Report, RunMetadata};
pub use scoring::{
    Evaluation, Issue, IssueKind, LENIENT_MIN_IOU, MatchCounts, MatchMode, evaluate,
};

/// The current instant as an RFC 3339 UTC timestamp, for report headers.
pub fn now_timestamp() -> String {
    crate::scribe::support::now()
}
