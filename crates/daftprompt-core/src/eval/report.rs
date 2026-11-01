//! Markdown evaluation reports.
//!
//! A report records either a completed live run or a baseline explaining that
//! no live run was performed. It always includes the fixture inventory, the
//! scoring rules, per-fixture results, aggregate metrics, and every failure
//! reason. Model results are never invented: the not-run form contains no
//! scores.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use super::fixtures::{Fixture, FixtureCategory};
use super::scoring::{Evaluation, IssueKind, MatchCounts};

/// The configuration and identity of one evaluation run.
#[derive(Debug, Clone)]
pub struct RunMetadata {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub prompt_version: String,
    pub max_attempts: u32,
    pub timeout: Duration,
    pub fixtures_path: PathBuf,
    /// RFC 3339 UTC, from [`now_timestamp`](super::now_timestamp).
    pub generated_at: String,
}

/// The outcome of parsing one fixture.
#[derive(Debug, Clone)]
pub enum FixtureStatus {
    /// The parser returned a validated graph, scored against the fixture.
    Parsed(Box<Evaluation>),
    /// The parser returned an explicit error; no graph was scored.
    Failed {
        kind: String,
        detail: String,
        attempts: Option<u32>,
    },
}

/// One fixture's run cost and outcome.
#[derive(Debug, Clone)]
pub struct FixtureRun {
    pub fixture_id: String,
    pub category: FixtureCategory,
    pub attempts: Option<u32>,
    pub latency_ms: f64,
    pub status: FixtureStatus,
}

/// What the fixture set covers, independent of any model run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixtureSummary {
    pub count: usize,
    pub by_category: Vec<(FixtureCategory, usize)>,
    pub expected_spans: usize,
    pub expected_questions: usize,
    pub expected_recommendations: usize,
    pub expected_tasks: usize,
    pub expected_recommendation_links: usize,
    pub expected_alternative_links: usize,
    pub expected_answer_links: usize,
    pub expected_assignees: usize,
}

impl FixtureSummary {
    /// Summarise a fixture set.
    pub fn from_fixtures(fixtures: &[Fixture]) -> Self {
        let mut by_category = Vec::new();
        for category in FixtureCategory::ALL {
            let count = fixtures
                .iter()
                .filter(|fixture| fixture.category == category)
                .count();
            if count > 0 {
                by_category.push((category, count));
            }
        }

        Self {
            count: fixtures.len(),
            by_category,
            expected_spans: fixtures.iter().map(|fixture| fixture.spans.len()).sum(),
            expected_questions: fixtures
                .iter()
                .flat_map(|fixture| &fixture.spans)
                .flat_map(|span| &span.labels)
                .filter(|label| label.kind == daftprompt_types::AnnotationKind::Question)
                .count(),
            expected_recommendations: fixtures
                .iter()
                .flat_map(|f| &f.spans)
                .flat_map(|s| &s.labels)
                .filter(|l| l.kind == daftprompt_types::AnnotationKind::Recommendation)
                .count(),
            expected_tasks: fixtures
                .iter()
                .flat_map(|f| &f.spans)
                .flat_map(|s| &s.labels)
                .filter(|l| l.kind == daftprompt_types::AnnotationKind::Task)
                .count(),
            expected_recommendation_links: fixtures
                .iter()
                .map(|f| f.recommendation_links.len())
                .sum(),
            expected_alternative_links: fixtures.iter().map(|f| f.alternative_links.len()).sum(),
            expected_answer_links: fixtures
                .iter()
                .map(|fixture| fixture.answer_links.len())
                .sum(),
            expected_assignees: fixtures
                .iter()
                .flat_map(|fixture| &fixture.spans)
                .flat_map(|span| &span.labels)
                .filter(|label| label.assignee.is_some())
                .count(),
        }
    }
}

/// An evaluation report: metadata, fixture inventory, and runs.
#[derive(Debug, Clone)]
pub struct Report {
    pub metadata: RunMetadata,
    pub summary: FixtureSummary,
    pub runs: Vec<FixtureRun>,
    /// `Some(reason)` when this report records that no live run happened.
    pub not_run: Option<String>,
}

impl Report {
    /// A report for `fixtures` with no runs yet.
    pub fn new(metadata: RunMetadata, fixtures: &[Fixture]) -> Self {
        Self {
            metadata,
            summary: FixtureSummary::from_fixtures(fixtures),
            runs: Vec::new(),
            not_run: None,
        }
    }

    /// Record one fixture run.
    pub fn push(&mut self, run: FixtureRun) {
        self.runs.push(run);
    }

    /// Render the report as Markdown.
    pub fn render(&self) -> String {
        let mut output = String::new();
        let model = self
            .metadata
            .model
            .as_deref()
            .unwrap_or("unspecified model");
        let provider = self
            .metadata
            .provider
            .as_deref()
            .unwrap_or("unspecified provider");

        let _ = writeln!(output, "# Scribe evaluation: {provider} / {model}");
        let _ = writeln!(output);
        match &self.not_run {
            Some(reason) => self.render_not_run(&mut output, reason),
            None => self.render_live(&mut output),
        }
        output
    }

    fn render_not_run(&self, output: &mut String, reason: &str) {
        let _ = writeln!(output, "**Status:** live evaluation not run");
        let _ = writeln!(output);
        let _ = writeln!(output, "> {reason}");
        let _ = writeln!(output);
        self.render_metadata(output);
        let _ = writeln!(output);
        self.render_inventory(output);
        let _ = writeln!(output);
        self.render_rules(output);
        let _ = writeln!(output);
        let _ = writeln!(output, "## How to run the live evaluation");
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "Set a provider credential and opt in explicitly; the binary writes the report to the path given by `--output`:"
        );
        let _ = writeln!(output);
        let _ = writeln!(output, "```sh");
        let _ = writeln!(
            output,
            "DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER={} DAFTPROMPT_EVAL_MODEL={} \\",
            self.metadata.provider.as_deref().unwrap_or("openai"),
            self.metadata.model.as_deref().unwrap_or("gpt-4o-mini")
        );
        let _ = writeln!(
            output,
            "  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \\"
        );
        let _ = writeln!(
            output,
            "  --output docs/evaluations/scribe-<provider>-<model>.md"
        );
        let _ = writeln!(output, "```");
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "No model results are recorded in this file. Scores appear only after a live run; this baseline exists so the repository has a starting artifact and a reproducible fixture set."
        );
        let _ = writeln!(output);
    }

    fn render_live(&self, output: &mut String) {
        let aggregate = self.aggregate();
        let total = self.runs.len();
        let parsed = aggregate.parsed;
        let _ = writeln!(
            output,
            "**Status:** live run completed: {parsed} of {total} fixture(s) parsed."
        );
        let _ = writeln!(output);
        self.render_metadata(output);
        let _ = writeln!(output);
        self.render_inventory(output);
        let _ = writeln!(output);
        self.render_rules(output);
        let _ = writeln!(output);

        let _ = writeln!(output, "## Aggregate");
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "| metric | expected | predicted | matched | precision | recall |"
        );
        let _ = writeln!(output, "| --- | ---: | ---: | ---: | ---: | ---: |");
        for (name, counts) in [
            ("recommendation, exact", aggregate.recommendation_exact),
            ("recommendation, lenient", aggregate.recommendation_lenient),
            ("task, exact", aggregate.task_exact),
            ("task, lenient", aggregate.task_lenient),
            (
                "recommendation link, exact",
                aggregate.recommendation_link_exact,
            ),
            (
                "recommendation link, lenient",
                aggregate.recommendation_link_lenient,
            ),
            ("alternative, exact", aggregate.alternative_exact),
            ("alternative, lenient", aggregate.alternative_lenient),
            ("spans, exact", aggregate.span_exact),
            ("spans, lenient", aggregate.span_lenient),
            ("labels, exact", aggregate.label_exact),
            ("labels, lenient", aggregate.label_lenient),
            ("answer links, exact", aggregate.answer_exact),
            ("answer links, lenient", aggregate.answer_lenient),
            ("assignees, exact", aggregate.assignee_exact),
            ("assignees, lenient", aggregate.assignee_lenient),
        ] {
            let _ = writeln!(
                output,
                "| {name} | {} | {} | {} | {} | {} |",
                counts.expected,
                counts.predicted,
                counts.matched,
                percent(counts.precision()),
                percent(counts.recall())
            );
        }
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "- Invalid output: {} of {} fixture(s) ({})",
            aggregate.invalid_output,
            total,
            percent(ratio(aggregate.invalid_output, total))
        );
        let _ = writeln!(
            output,
            "- Other parse failures: {} of {} fixture(s)",
            aggregate.failed - aggregate.invalid_output,
            total
        );
        let _ = writeln!(
            output,
            "- Attempts per parse: {}",
            attempts_summary(&aggregate.attempts, total)
        );
        let _ = writeln!(
            output,
            "- Latency (ms): mean {}, median {}, p95 {}, max {}",
            milliseconds(mean(&aggregate.latencies)),
            milliseconds(percentile(&aggregate.latencies, 50.0)),
            milliseconds(percentile(&aggregate.latencies, 95.0)),
            milliseconds(aggregate.latencies.iter().copied().reduce(f64::max))
        );
        let _ = writeln!(output);

        let _ = writeln!(output, "## Per fixture");
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "| fixture | category | parse | issues | spans exact P/R | spans lenient P/R | labels exact P/R | labels lenient P/R | links exact / lenient | assignees exact / lenient | attempts | latency ms |"
        );
        let _ = writeln!(
            output,
            "| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | ---: | ---: |"
        );
        for run in &self.runs {
            match &run.status {
                FixtureStatus::Parsed(evaluation) => {
                    let _ = writeln!(
                        output,
                        "| `{}` | {} | ok | {} | {} | {} | {} | {} | {} / {} | {} / {} | {} | {:.1} |",
                        run.fixture_id,
                        run.category,
                        evaluation.issues.len(),
                        pr(evaluation.span_exact),
                        pr(evaluation.span_lenient),
                        pr(evaluation.label_exact),
                        pr(evaluation.label_lenient),
                        pr(evaluation.answer_exact),
                        pr(evaluation.answer_lenient),
                        pr(evaluation.assignee_exact),
                        pr(evaluation.assignee_lenient),
                        attempts(run.attempts),
                        run.latency_ms
                    );
                }
                FixtureStatus::Failed { kind, .. } => {
                    let _ = writeln!(
                        output,
                        "| `{}` | {} | `{}` | — | — | — | — | — | — / — | — / — | {} | {:.1} |",
                        run.fixture_id,
                        run.category,
                        kind,
                        attempts(run.attempts),
                        run.latency_ms
                    );
                }
            }
        }
        let _ = writeln!(output);

        let _ = writeln!(
            output,
            "| fixture | recommendations exact / lenient P/R | tasks exact / lenient P/R | recommendation links exact / lenient P/R | alternatives exact / lenient P/R |"
        );
        let _ = writeln!(output, "| --- | --- | --- | --- | --- |");
        for run in &self.runs {
            if let FixtureStatus::Parsed(e) = &run.status {
                let _ = writeln!(
                    output,
                    "| `{}` | {} / {} | {} / {} | {} / {} | {} / {} |",
                    run.fixture_id,
                    pr(e.recommendation_exact),
                    pr(e.recommendation_lenient),
                    pr(e.task_exact),
                    pr(e.task_lenient),
                    pr(e.recommendation_link_exact),
                    pr(e.recommendation_link_lenient),
                    pr(e.alternative_exact),
                    pr(e.alternative_lenient)
                );
            }
        }
        let _ = writeln!(output);
        let _ = writeln!(output, "## Failures");
        let _ = writeln!(output);
        let mut wrote_failure = false;

        for run in &self.runs {
            if let FixtureStatus::Failed {
                kind,
                detail,
                attempts,
            } = &run.status
            {
                wrote_failure = true;
                let _ = writeln!(
                    output,
                    "- `{}` ({}): **{kind}** after {} attempt(s) in {:.1} ms: {detail}",
                    run.fixture_id,
                    run.category,
                    attempts.map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                    run.latency_ms
                );
            }
        }

        for run in &self.runs {
            if let FixtureStatus::Parsed(evaluation) = &run.status {
                for issue in &evaluation.issues {
                    wrote_failure = true;
                    let _ = writeln!(
                        output,
                        "- `{}` ({}): **{}**: {}",
                        run.fixture_id,
                        run.category,
                        issue_kind(issue.kind),
                        issue.detail
                    );
                }
            }
        }

        if !wrote_failure {
            let _ = writeln!(output, "No failures recorded.");
        }
        let _ = writeln!(output);

        let _ = writeln!(output, "## Reproduce");
        let _ = writeln!(output);
        let _ = writeln!(output, "```sh");
        let _ = writeln!(
            output,
            "DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER={} DAFTPROMPT_EVAL_MODEL={} \\",
            self.metadata.provider.as_deref().unwrap_or("openai"),
            self.metadata.model.as_deref().unwrap_or("gpt-4o-mini")
        );
        let _ = writeln!(
            output,
            "  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \\"
        );
        let _ = writeln!(
            output,
            "  --fixtures {} --output <same-path>",
            display_path(&self.metadata.fixtures_path)
        );
        let _ = writeln!(output, "```");
        let _ = writeln!(output);
    }

    fn render_metadata(&self, output: &mut String) {
        let _ = writeln!(output, "- Generated at: `{}`", self.metadata.generated_at);
        let _ = writeln!(
            output,
            "- Provider / model: `{}` / `{}`",
            self.metadata.provider.as_deref().unwrap_or("unspecified"),
            self.metadata.model.as_deref().unwrap_or("unspecified")
        );
        let _ = writeln!(
            output,
            "- Prompt version: `{}`",
            self.metadata.prompt_version
        );
        let _ = writeln!(
            output,
            "- Attempt budget: {} total attempt(s), {} per attempt",
            self.metadata.max_attempts.max(1),
            duration(self.metadata.timeout)
        );
        let _ = writeln!(
            output,
            "- Fixtures: `{}`",
            display_path(&self.metadata.fixtures_path)
        );
    }

    fn render_inventory(&self, output: &mut String) {
        let _ = writeln!(output, "## Fixture inventory");
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "- {} fixture(s), {} expected span(s), {} question label(s), {} answer link(s), {} explicit assignee(s)",
            self.summary.count,
            self.summary.expected_spans,
            self.summary.expected_questions,
            self.summary.expected_answer_links,
            self.summary.expected_assignees
        );
        let _ = writeln!(
            output,
            "- {} recommendation label(s), {} task label(s), {} recommendation-question link(s), {} symmetric alternative pair(s)",
            self.summary.expected_recommendations,
            self.summary.expected_tasks,
            self.summary.expected_recommendation_links,
            self.summary.expected_alternative_links
        );
        let categories = self
            .summary
            .by_category
            .iter()
            .map(|(category, count)| format!("{category} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(output, "- Categories: {categories}");
    }

    fn render_rules(&self, output: &mut String) {
        let _ = writeln!(output, "## Matching rules");
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "- **Exact span match:** byte ranges are equal after trimming surrounding whitespace."
        );
        let _ = writeln!(
            output,
            "- **Lenient span match:** overlap divided by union is at least {} (also reported in addition to exact).",
            super::scoring::LENIENT_MIN_IOU
        );
        let _ = writeln!(output, "- **Label:** the kind matches on a matched span.");
        let _ = writeln!(
            output,
            "- **Relationships:** both ends must match; alternative pairs are symmetric. Unary recommendation counts exclude relationship labels."
        );
        let _ = writeln!(
            output,
            "- **Assignee:** the handle is identical on a matched question or task."
        );
        let _ = writeln!(
            output,
            "- Matches are one-to-one and greedy, preferring exact pairs and then higher overlap."
        );
    }

    fn aggregate(&self) -> Aggregate {
        let mut aggregate = Aggregate::default();
        for run in &self.runs {
            aggregate.latencies.push(run.latency_ms);
            if let Some(attempts) = run.attempts {
                aggregate.attempts.push(attempts);
            }
            match &run.status {
                FixtureStatus::Parsed(evaluation) => {
                    aggregate.parsed += 1;
                    aggregate
                        .recommendation_exact
                        .add(evaluation.recommendation_exact);
                    aggregate
                        .recommendation_lenient
                        .add(evaluation.recommendation_lenient);
                    aggregate.task_exact.add(evaluation.task_exact);
                    aggregate.task_lenient.add(evaluation.task_lenient);
                    aggregate
                        .recommendation_link_exact
                        .add(evaluation.recommendation_link_exact);
                    aggregate
                        .recommendation_link_lenient
                        .add(evaluation.recommendation_link_lenient);
                    aggregate
                        .alternative_exact
                        .add(evaluation.alternative_exact);
                    aggregate
                        .alternative_lenient
                        .add(evaluation.alternative_lenient);

                    aggregate.span_exact.add(evaluation.span_exact);
                    aggregate.span_lenient.add(evaluation.span_lenient);
                    aggregate.label_exact.add(evaluation.label_exact);
                    aggregate.label_lenient.add(evaluation.label_lenient);
                    aggregate.answer_exact.add(evaluation.answer_exact);
                    aggregate.answer_lenient.add(evaluation.answer_lenient);
                    aggregate.assignee_exact.add(evaluation.assignee_exact);
                    aggregate.assignee_lenient.add(evaluation.assignee_lenient);
                }
                FixtureStatus::Failed { kind, .. } => {
                    aggregate.failed += 1;
                    if kind == "invalid_output" {
                        aggregate.invalid_output += 1;
                    }
                }
            }
        }
        aggregate
    }
}

#[derive(Debug, Default)]
struct Aggregate {
    span_exact: MatchCounts,
    span_lenient: MatchCounts,
    label_exact: MatchCounts,
    label_lenient: MatchCounts,
    answer_exact: MatchCounts,
    answer_lenient: MatchCounts,
    assignee_exact: MatchCounts,
    assignee_lenient: MatchCounts,
    recommendation_exact: MatchCounts,
    recommendation_lenient: MatchCounts,
    task_exact: MatchCounts,
    task_lenient: MatchCounts,
    recommendation_link_exact: MatchCounts,
    recommendation_link_lenient: MatchCounts,
    alternative_exact: MatchCounts,
    alternative_lenient: MatchCounts,
    parsed: usize,
    failed: usize,
    invalid_output: usize,
    attempts: Vec<u32>,
    latencies: Vec<f64>,
}

fn issue_kind(kind: IssueKind) -> &'static str {
    kind.as_str()
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(
        || "n/a".to_owned(),
        |value| format!("{:.1}%", value * 100.0),
    )
}

fn pr(counts: MatchCounts) -> String {
    format!(
        "{} / {}",
        percent(counts.precision()),
        percent(counts.recall())
    )
}

fn attempts(value: Option<u32>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| value.to_string())
}

fn attempts_summary(values: &[u32], total: usize) -> String {
    if values.is_empty() {
        return "n/a (no attempts recorded)".to_owned();
    }
    let sum: u32 = values.iter().sum();
    let mean = f64::from(sum) / values.len() as f64;
    let max = values.iter().copied().max().unwrap_or_default();
    format!(
        "mean {mean:.2} over {} known of {total} parse(s), max {max}",
        values.len()
    )
}

fn milliseconds(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| format!("{value:.1}"))
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn percentile(values: &[f64], percent: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((percent / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted.get(rank).copied()
}

fn duration(value: Duration) -> String {
    format!("{:.1}s", value.as_secs_f64())
}

/// A repository-relative path when the path is inside this checkout.
fn display_path(path: &std::path::Path) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent);
    match root.and_then(|root| path.strip_prefix(root).ok()) {
        Some(relative) => relative.display().to_string(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::fixtures::{ExpectedAnswerLink, ExpectedLabel, ExpectedSpan, FixtureCategory};
    use crate::eval::prediction::{ByteRange, PredictedLabel, PredictedSpan, Prediction};
    use crate::eval::scoring::evaluate;
    use daftprompt_types::AnnotationKind;

    fn fixture() -> Fixture {
        Fixture {
            format_version: 1,
            id: "one".to_owned(),
            category: FixtureCategory::Chat,
            note: String::new(),
            source: None,
            text: "Should we?".to_owned(),
            spans: vec![ExpectedSpan {
                id: "e1".to_owned(),
                start: 0,
                end: 10,
                quote: "Should we?".to_owned(),
                labels: vec![ExpectedLabel {
                    kind: AnnotationKind::Question,
                    assignee: Some("@team".to_owned()),
                }],
            }],
            answer_links: Vec::new(),
            recommendation_links: Vec::new(),
            alternative_links: Vec::new(),
        }
    }

    fn metadata() -> RunMetadata {
        RunMetadata {
            provider: Some("openai".to_owned()),
            model: Some("small-model".to_owned()),
            prompt_version: "scribe-v1".to_owned(),
            max_attempts: 3,
            timeout: Duration::from_secs(60),
            fixtures_path: PathBuf::from("fixtures/scribe"),
            generated_at: "2026-10-31T12:00:00Z".to_owned(),
        }
    }

    #[test]
    fn not_run_reports_contain_no_scores() {
        let fixtures = vec![fixture()];
        let mut report = Report::new(metadata(), &fixtures);
        report.not_run = Some("DAFTPROMPT_EVAL_LIVE was not set.".to_owned());
        let rendered = report.render();

        assert!(rendered.contains("live evaluation not run"));
        assert!(rendered.contains("No model results are recorded"));
        assert!(rendered.contains("1 fixture(s)"));
        assert!(rendered.contains("DAFTPROMPT_EVAL_LIVE=1"));
        assert!(!rendered.contains("100.0%"));
    }

    #[test]
    fn live_reports_include_successes_and_failure_reasons() {
        let fixtures = vec![fixture()];
        let mut report = Report::new(metadata(), &fixtures);

        let prediction = Prediction {
            spans: vec![PredictedSpan {
                range: ByteRange::new(0, 10),
            }],
            labels: vec![PredictedLabel {
                kind: AnnotationKind::Question,
                span: 0,
                assignee: Some("@team".to_owned()),
            }],
            answer_links: Vec::new(),
            recommendation_links: Vec::new(),
            alternative_links: Vec::new(),
        };
        report.push(FixtureRun {
            fixture_id: "one".to_owned(),
            category: FixtureCategory::Chat,
            attempts: Some(1),
            latency_ms: 12.5,
            status: FixtureStatus::Parsed(Box::new(evaluate(&fixtures[0], &prediction))),
        });
        report.push(FixtureRun {
            fixture_id: "two".to_owned(),
            category: FixtureCategory::Email,
            attempts: Some(3),
            latency_ms: 40.0,
            status: FixtureStatus::Failed {
                kind: "invalid_output".to_owned(),
                detail: "spans overlap".to_owned(),
                attempts: Some(3),
            },
        });

        let rendered = report.render();
        assert!(rendered.contains("1 of 2 fixture(s) parsed"));
        assert!(rendered.contains("100.0%"));
        assert!(rendered.contains("invalid_output"));
        assert!(rendered.contains("spans overlap"));
        assert!(rendered.contains("| `one` | chat | ok |"));
        assert!(rendered.contains("Invalid output: 1 of 2 fixture(s) (50.0%)"));
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&[10.0, 20.0, 30.0, 40.0], 50.0), Some(30.0));
        assert_eq!(percentile(&[10.0, 20.0, 30.0, 40.0], 95.0), Some(40.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn summary_counts_expected_annotations() {
        let summary = FixtureSummary::from_fixtures(&[fixture()]);
        assert_eq!(summary.count, 1);
        assert_eq!(summary.expected_spans, 1);
        assert_eq!(summary.expected_questions, 1);
        assert_eq!(summary.expected_assignees, 1);
        assert_eq!(summary.expected_answer_links, 0);
        assert_eq!(summary.by_category, vec![(FixtureCategory::Chat, 1)]);
    }

    #[test]
    fn answer_links_count_as_expected_questions_are_separate() {
        let mut fixture = fixture();
        fixture.text = "Should we?\nYes.".to_owned();
        fixture.spans.push(ExpectedSpan {
            id: "e2".to_owned(),
            start: 11,
            end: 15,
            quote: "Yes.".to_owned(),
            labels: Vec::new(),
        });
        fixture.answer_links = vec![ExpectedAnswerLink {
            from: "e2".to_owned(),
            to: "e1".to_owned(),
        }];
        let summary = FixtureSummary::from_fixtures(&[fixture]);
        assert_eq!(summary.expected_questions, 1);
        assert_eq!(summary.expected_answer_links, 1);
        assert_eq!(summary.expected_spans, 2);
    }
}
