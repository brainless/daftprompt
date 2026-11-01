//! Loading and validating the manually annotated Scribe fixtures.
//!
//! One JSON file per fixture. Byte ranges are half-open (`[start, end)`) into
//! the fixture's UTF-8 `text`, and each span repeats its text as a verbatim
//! `quote`. Validation checks the quote against the range, trims boundaries,
//! overlap, label shapes, and answer-link targets so a malformed annotation
//! fails before it can distort a score.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use daftprompt_types::{AnnotationKind, SourceMetadata};
use serde::Deserialize;
use thiserror::Error;

/// The fixture format revision understood by this loader.
pub const FIXTURE_FORMAT_VERSION: u32 = 1;

/// Where the repository's fixture set lives.
pub fn default_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/scribe")
}

/// One submitted text and its expected annotations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Format revision; currently [`FIXTURE_FORMAT_VERSION`].
    pub format_version: u32,
    /// Stable identifier, matching the file stem.
    pub id: String,
    /// The kind of text, for coverage reporting.
    pub category: FixtureCategory,
    /// Why this fixture exists and what it exercises.
    pub note: String,
    /// Optional provenance, passed through to the parse request.
    #[serde(default)]
    pub source: Option<SourceMetadata>,
    /// The complete original text.
    pub text: String,
    /// Spans that should be labelled, in source order.
    #[serde(default)]
    pub spans: Vec<ExpectedSpan>,
    /// Answer relationships between expected spans.
    #[serde(default)]
    pub answer_links: Vec<ExpectedAnswerLink>,
    #[serde(default)]
    pub recommendation_links: Vec<ExpectedAnswerLink>,
    #[serde(default)]
    pub alternative_links: Vec<ExpectedAnswerLink>,
}

/// The kind of text a fixture covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureCategory {
    /// Chat or agent replies.
    Chat,
    /// Prose, notes, or a plan.
    Prose,
    /// Pasted source code.
    Code,
    /// A pasted document, such as a design record.
    Document,
    /// An email body, including quotations.
    Email,
    /// Raw JSON text.
    Json,
    /// Text with no actionable content.
    None,
}

impl FixtureCategory {
    /// Every category, in a stable reporting order.
    pub const ALL: [Self; 7] = [
        Self::Chat,
        Self::Prose,
        Self::Code,
        Self::Document,
        Self::Email,
        Self::Json,
        Self::None,
    ];

    /// The snake-case name used in fixture files and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Prose => "prose",
            Self::Code => "code",
            Self::Document => "document",
            Self::Email => "email",
            Self::Json => "json",
            Self::None => "none",
        }
    }
}

impl fmt::Display for FixtureCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An expected span and its unary labels.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSpan {
    /// Local identifier used by answer links.
    pub id: String,
    /// Byte offset of the first byte, half-open.
    pub start: usize,
    /// Byte offset just past the last byte, half-open.
    pub end: usize,
    /// The exact text of `[start, end)`.
    pub quote: String,
    /// Unary question, recommendation, or task labels on this span; answer
    /// labels are expressed as [`ExpectedAnswerLink`]s.
    #[serde(default)]
    pub labels: Vec<ExpectedLabel>,
}

/// A unary label expected on a span.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedLabel {
    /// A unary question, recommendation, or task kind.
    pub kind: AnnotationKind,
    /// An optional explicitly assigned handle.
    #[serde(default)]
    pub assignee: Option<String>,
}

/// An expected `answer` link from one span to a question span.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedAnswerLink {
    /// The answering span.
    pub from: String,
    /// The question span being answered.
    pub to: String,
}

impl Fixture {
    /// The index of the span with `id`, if present.
    pub fn span_index(&self, id: &str) -> Option<usize> {
        self.spans.iter().position(|span| span.id == id)
    }

    /// Check every annotation against the text.
    ///
    /// Returns a human-readable reason when the fixture is malformed.
    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != FIXTURE_FORMAT_VERSION {
            return Err(format!(
                "unsupported format_version {}; expected {}",
                self.format_version, FIXTURE_FORMAT_VERSION
            ));
        }
        if self.id.trim().is_empty() {
            return Err("the fixture id is empty".to_owned());
        }
        if self.text.is_empty() {
            return Err("the fixture text is empty".to_owned());
        }

        let mut ids = HashSet::new();
        for span in &self.spans {
            if span.id.trim().is_empty() {
                return Err("a span has an empty id".to_owned());
            }
            if !ids.insert(span.id.as_str()) {
                return Err(format!("span id {:?} is used more than once", span.id));
            }
            if span.start > span.end {
                return Err(format!(
                    "span {:?} has a reversed byte range {}..{}",
                    span.id, span.start, span.end
                ));
            }
            if span.end > self.text.len()
                || !self.text.is_char_boundary(span.start)
                || !self.text.is_char_boundary(span.end)
            {
                return Err(format!(
                    "span {:?} range {}..{} is not on char boundaries of the text",
                    span.id, span.start, span.end
                ));
            }
            let text = &self.text[span.start..span.end];
            if text != span.quote {
                return Err(format!(
                    "span {:?} quote {:?} does not match the text at {}..{} ({:?})",
                    span.id, span.quote, span.start, span.end, text
                ));
            }
            if span.quote.is_empty() {
                return Err(format!("span {:?} is empty", span.id));
            }
            if span.quote.trim() != span.quote {
                return Err(format!(
                    "span {:?} quote has surrounding whitespace; expected spans are trimmed",
                    span.id
                ));
            }

            let mut kinds = HashSet::new();
            for label in &span.labels {
                if label.kind == AnnotationKind::Answer || !kinds.insert(label.kind) {
                    return Err(format!(
                        "span {:?} has invalid or duplicate unary labels",
                        span.id
                    ));
                }
                if let Some(assignee) = &label.assignee {
                    if !matches!(label.kind, AnnotationKind::Question | AnnotationKind::Task) {
                        return Err("only questions and tasks allow assignees".into());
                    }
                    validate_handle(assignee)?;
                }
            }
        }

        let mut order: Vec<&ExpectedSpan> = self.spans.iter().collect();
        order.sort_by_key(|span| (span.start, span.end));
        for pair in order.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if right.start < left.end {
                return Err(format!(
                    "spans {:?} and {:?} overlap or nest",
                    left.id, right.id
                ));
            }
        }

        let mut links = HashSet::new();
        let mut referenced = HashSet::new();
        for link in &self.answer_links {
            let from = self
                .span_index(&link.from)
                .ok_or_else(|| format!("answer link references unknown span {:?}", link.from))?;
            let to = self
                .span_index(&link.to)
                .ok_or_else(|| format!("answer link references unknown span {:?}", link.to))?;
            if from == to {
                return Err(format!("span {:?} cannot answer itself", link.from));
            }
            if !self.spans[to]
                .labels
                .iter()
                .any(|label| label.kind == AnnotationKind::Question)
            {
                return Err(format!(
                    "answer target {:?} is not an expected question span",
                    link.to
                ));
            }
            if !links.insert((from, to)) {
                return Err(format!(
                    "duplicate answer link {:?} -> {:?}",
                    link.from, link.to
                ));
            }
            referenced.insert(from);
            referenced.insert(to);
        }

        for (name, relations, target_kind, symmetric) in [
            (
                "recommendation",
                &self.recommendation_links,
                AnnotationKind::Question,
                false,
            ),
            (
                "alternative",
                &self.alternative_links,
                AnnotationKind::Recommendation,
                true,
            ),
        ] {
            let mut seen = HashSet::new();
            for link in relations {
                let from = self
                    .span_index(&link.from)
                    .ok_or("unknown relationship source")?;
                let to = self
                    .span_index(&link.to)
                    .ok_or("unknown relationship target")?;
                let has = |index: usize, kind| {
                    self.spans[index]
                        .labels
                        .iter()
                        .any(|label| label.kind == kind)
                };
                if from == to || !has(from, AnnotationKind::Recommendation) || !has(to, target_kind)
                {
                    return Err(format!("invalid {name} relationship"));
                }
                let pair = if symmetric {
                    (from.min(to), from.max(to))
                } else {
                    (from, to)
                };
                if !seen.insert(pair) {
                    return Err(format!("duplicate {name} relationship"));
                }
            }
        }

        for (index, span) in self.spans.iter().enumerate() {
            if span.labels.is_empty() && !referenced.contains(&index) {
                return Err(format!(
                    "span {:?} is not referenced by any label or answer link",
                    span.id
                ));
            }
        }

        Ok(())
    }
}

fn validate_handle(handle: &str) -> Result<(), String> {
    let Some(name) = handle.strip_prefix('@') else {
        return Err(format!("assignee {handle:?} must start with '@'"));
    };
    if name.is_empty() {
        return Err(format!("assignee {handle:?} has no name after '@'"));
    }
    if name.chars().any(char::is_whitespace) {
        return Err(format!("assignee {handle:?} contains whitespace"));
    }
    Ok(())
}

/// Why a fixture directory could not be loaded.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// The directory itself could not be read.
    #[error("could not read the fixture directory {path}: {source}")]
    Directory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// One fixture file could not be read.
    #[error("could not read fixture {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// One fixture file was not valid JSON for the expected shape.
    #[error("could not parse fixture {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// One fixture file parsed but its annotations are inconsistent.
    #[error("invalid fixture {path}: {detail}")]
    Invalid { path: PathBuf, detail: String },
    /// The directory contains no JSON fixtures.
    #[error("no JSON fixtures found in {0}")]
    Empty(PathBuf),
}

/// Load every `*.json` fixture in `dir`, sorted by file name.
///
/// The order is stable so reports and fixture-driven tests are reproducible.
pub fn load_fixtures(dir: impl AsRef<Path>) -> Result<Vec<Fixture>, FixtureError> {
    let dir = dir.as_ref();
    let entries = fs::read_dir(dir).map_err(|source| FixtureError::Directory {
        path: dir.to_path_buf(),
        source,
    })?;

    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| FixtureError::Directory {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(FixtureError::Empty(dir.to_path_buf()));
    }

    let mut fixtures = Vec::with_capacity(paths.len());
    let mut seen = HashSet::new();
    for path in paths {
        let content = fs::read_to_string(&path).map_err(|source| FixtureError::Read {
            path: path.clone(),
            source,
        })?;
        let fixture: Fixture =
            serde_json::from_str(&content).map_err(|source| FixtureError::Parse {
                path: path.clone(),
                source,
            })?;

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if file_name != format!("{}.json", fixture.id) {
            return Err(FixtureError::Invalid {
                path,
                detail: format!(
                    "file name {file_name:?} does not match the fixture id {:?}",
                    fixture.id
                ),
            });
        }
        if !seen.insert(fixture.id.clone()) {
            return Err(FixtureError::Invalid {
                path,
                detail: format!("duplicate fixture id {:?}", fixture.id),
            });
        }
        fixture.validate().map_err(|detail| FixtureError::Invalid {
            path: path.clone(),
            detail,
        })?;
        fixtures.push(fixture);
    }

    Ok(fixtures)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(id: &str, range: std::ops::Range<usize>, quote: &str) -> ExpectedSpan {
        ExpectedSpan {
            id: id.to_owned(),
            start: range.start,
            end: range.end,
            quote: quote.to_owned(),
            labels: vec![ExpectedLabel {
                kind: AnnotationKind::Question,
                assignee: None,
            }],
        }
    }

    fn fixture(text: &str, spans: Vec<ExpectedSpan>) -> Fixture {
        Fixture {
            format_version: FIXTURE_FORMAT_VERSION,
            id: "test".to_owned(),
            category: FixtureCategory::Chat,
            note: String::new(),
            source: None,
            text: text.to_owned(),
            spans,
            answer_links: Vec::new(),
            recommendation_links: Vec::new(),
            alternative_links: Vec::new(),
        }
    }

    #[test]
    fn the_repository_fixtures_load_and_validate() {
        let fixtures = load_fixtures(default_fixture_dir()).expect("fixtures load");
        assert!(
            fixtures.len() >= 10,
            "expected a small but broad fixture set"
        );

        for category in FixtureCategory::ALL {
            let count = fixtures
                .iter()
                .filter(|fixture| fixture.category == category)
                .count();
            assert!(count > 0, "no fixture covers category {category}");
        }

        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.category == FixtureCategory::None)
        );
        let answered = fixtures
            .iter()
            .filter(|fixture| !fixture.answer_links.is_empty())
            .count();
        let assigned = fixtures
            .iter()
            .flat_map(|fixture| &fixture.spans)
            .flat_map(|span| &span.labels)
            .filter(|label| label.assignee.is_some())
            .count();
        assert!(
            answered >= 3,
            "locally answered questions are under-covered"
        );
        assert!(assigned >= 2, "explicit assignments are under-covered");
    }

    #[test]
    fn untrimmed_quotes_are_rejected() {
        let mut fixture = fixture("  Question?  ", vec![span("e1", 0..13, "  Question?  ")]);
        assert!(fixture.validate().unwrap_err().contains("whitespace"));

        fixture.spans = vec![span("e1", 2..11, "Question?")];
        fixture.validate().expect("trimmed span is valid");
    }

    #[test]
    fn quotes_must_match_their_byte_range() {
        let error = fixture("What now?", vec![span("e1", 0..4, "When")])
            .validate()
            .unwrap_err();
        assert!(error.contains("does not match"));
    }

    #[test]
    fn overlapping_spans_are_rejected() {
        let fixture = fixture(
            "one two",
            vec![span("e1", 0..3, "one"), span("e2", 2..7, "e two")],
        );
        assert!(fixture.validate().unwrap_err().contains("overlap"));
    }

    #[test]
    fn dangling_answer_links_are_rejected() {
        let mut fixture = fixture("Should we?\nYes.", vec![span("e1", 0..10, "Should we?")]);
        fixture.spans.push(ExpectedSpan {
            id: "e2".to_owned(),
            start: 11,
            end: 14,
            quote: "Yes".to_owned(),
            labels: Vec::new(),
        });
        fixture.answer_links = vec![ExpectedAnswerLink {
            from: "e1".to_owned(),
            to: "e2".to_owned(),
        }];
        assert!(
            fixture
                .validate()
                .unwrap_err()
                .contains("not an expected question")
        );
    }

    #[test]
    fn malformed_handles_are_rejected() {
        let mut fixture = fixture("Question?", vec![span("e1", 0..9, "Question?")]);
        fixture.spans[0].labels[0].assignee = Some("@infra team".to_owned());
        assert!(fixture.validate().unwrap_err().contains("whitespace"));
    }
}
