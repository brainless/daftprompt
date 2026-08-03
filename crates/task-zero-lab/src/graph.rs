//! Epic 014 Task 2: experimental evidence-graph core types.
//!
//! This module defines the minimal node/edge/evidence vocabulary Design
//! Constraint 3 asks for, reusing Epic 011/012's node-family and relation
//! naming rather than inventing new vocabulary. It is deliberately small and
//! explicitly experimental (Design Constraint 1): these types are not the
//! production `daftprompt-graph` schema, and any field promoted into Epic
//! 011 requires recorded experiment evidence first.
//!
//! Key shape decisions, each tied to one Task 2 acceptance criterion:
//!
//! - [`NodeLocator::logical_id`] is a stable, human-readable structural path
//!   (e.g. `"epic:014/task:2/criterion:3"`), never a bare line number. A line
//!   number is still recorded in [`NodeLocator::line_span`], but only as a
//!   locator/debugging aid.
//! - [`Node::source_version`] is the exact document version (a Git blob ID
//!   when the file is tracked at the resolved revision, otherwise an xxh3
//!   content hash), never the current-instant filesystem state.
//! - [`GraphExtraction`] keeps `established_edges`, `candidate_edges`,
//!   `helper_proposed_edges`, and `human_assumption_edges` in separate
//!   fields, matching Design Constraint 3 ("Helper output cannot create
//!   established relationships") and the Task 2 criterion that these four
//!   trust classes "serialize into distinct fields."
//! - [`RelationKind`], [`DetectorId`], and [`EvidenceClass`] all deserialize
//!   unrecognized string values into an `Unknown(String)` variant rather than
//!   failing, so loading a fixture written by a newer/older detector survives
//!   as a diagnostic instead of a crash (Task 2 acceptance criterion).
//! - Every [`Edge`] carries one or more [`EdgeProvenance`] records with
//!   detector identity, method, evidence locator, source version, evidence
//!   class, and a human-readable detail string, so [`Edge::explain`] can
//!   describe the edge without rerunning any detector.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The minimal node families from Epic 014 Design Constraint 3.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    RepositorySnapshot,
    ProjectInstruction,
    Epic,
    Task,
    AcceptanceCriterion,
    DesignConstraint,
    ResearchArtifact,
    UnresolvedGap,
    FileOrSection,
    CodeSymbol,
    Commit,
}

/// A stable structural locator, plus a non-authoritative line-span hint.
///
/// `logical_id` alone is the identity a caller should compare or index by.
/// Two extraction runs over the same document version must produce the same
/// `logical_id` for the same criterion/task/heading even if unrelated edits
/// elsewhere in the file shift line numbers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeLocator {
    pub logical_id: String,
    pub source_path: Option<String>,
    /// 1-based `(first_line, last_line)` at extraction time. A locator/
    /// debugging aid only — never treated as identity (Task 2 acceptance
    /// criterion: "line number alone is not treated as stable identity").
    pub line_span: Option<(usize, usize)>,
}

impl NodeLocator {
    pub fn new(logical_id: impl Into<String>, source_path: Option<String>, line_span: Option<(usize, usize)>) -> Self {
        Self {
            logical_id: logical_id.into(),
            source_path,
            line_span,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub kind: NodeKind,
    pub locator: NodeLocator,
    pub label: String,
    /// Exact document version this node was extracted from: a Git blob ID
    /// when the source file is tracked at the resolved revision, otherwise
    /// an xxh3 content hash (`content_identity::inspect_file`). Never a
    /// mutable "current file" reference.
    pub source_version: String,
    /// `Some(true)`/`Some(false)` for checkbox-bearing nodes (acceptance
    /// criteria); `None` for node kinds that have no checkbox state.
    pub checked: Option<bool>,
    /// Kind-specific structured data (e.g. extracted paths/symbols/commands
    /// for a criterion, or partition counts for a repository snapshot node).
    /// Kept as a free-form JSON object instead of a wide struct so Task 2's
    /// small extractor set does not force a premature schema across all
    /// eleven node kinds (Design Constraint 1).
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// Initial relationships from Epic 014 Design Constraint 3.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RelationKind {
    Contains,
    DependsOn,
    Requires,
    Constrains,
    Mentions,
    ValidatedBy,
    BlockedBy,
    Changes,
    Defines,
    Supersedes,
    /// A relation value this crate's vocabulary does not (yet) recognize.
    /// Preserved verbatim so a fixture written against a newer/older
    /// detector set survives loading as a diagnostic rather than an error.
    Unknown(String),
}

impl RelationKind {
    pub fn as_str(&self) -> &str {
        match self {
            RelationKind::Contains => "contains",
            RelationKind::DependsOn => "depends_on",
            RelationKind::Requires => "requires",
            RelationKind::Constrains => "constrains",
            RelationKind::Mentions => "mentions",
            RelationKind::ValidatedBy => "validated_by",
            RelationKind::BlockedBy => "blocked_by",
            RelationKind::Changes => "changes",
            RelationKind::Defines => "defines",
            RelationKind::Supersedes => "supersedes",
            RelationKind::Unknown(s) => s.as_str(),
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, RelationKind::Unknown(_))
    }

    fn from_str(s: &str) -> Self {
        match s {
            "contains" => RelationKind::Contains,
            "depends_on" => RelationKind::DependsOn,
            "requires" => RelationKind::Requires,
            "constrains" => RelationKind::Constrains,
            "mentions" => RelationKind::Mentions,
            "validated_by" => RelationKind::ValidatedBy,
            "blocked_by" => RelationKind::BlockedBy,
            "changes" => RelationKind::Changes,
            "defines" => RelationKind::Defines,
            "supersedes" => RelationKind::Supersedes,
            other => RelationKind::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for RelationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for RelationKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RelationKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(RelationKind::from_str(&String::deserialize(deserializer)?))
    }
}

/// Detector identity. Like [`RelationKind`], unrecognized values survive
/// deserialization as [`DetectorId::Unknown`] rather than failing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetectorId {
    /// Deterministic Markdown structure parser (`markdown_extract.rs`):
    /// headings, task/criterion nesting, checkbox state, source spans.
    MarkdownStructure,
    /// Regex-based deterministic scanner over Markdown body text for
    /// backtick-quoted paths, Rust-style symbols, shell commands, and
    /// cross-epic references.
    MarkdownExactReference,
    /// Revision-pinned Git evidence (`git_snapshot.rs`, reused from Task 1).
    GitChangedFile,
    /// Reuse of the existing indexer's `(source_type, identifier)` identity
    /// (`index_coverage.rs`, reused from Task 1) — never the indexed text.
    IndexIdentityReuse,
    Unknown(String),
}

impl DetectorId {
    pub fn as_str(&self) -> &str {
        match self {
            DetectorId::MarkdownStructure => "markdown_structure",
            DetectorId::MarkdownExactReference => "markdown_exact_reference",
            DetectorId::GitChangedFile => "git_changed_file",
            DetectorId::IndexIdentityReuse => "index_identity_reuse",
            DetectorId::Unknown(s) => s.as_str(),
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, DetectorId::Unknown(_))
    }

    fn from_str(s: &str) -> Self {
        match s {
            "markdown_structure" => DetectorId::MarkdownStructure,
            "markdown_exact_reference" => DetectorId::MarkdownExactReference,
            "git_changed_file" => DetectorId::GitChangedFile,
            "index_identity_reuse" => DetectorId::IndexIdentityReuse,
            other => DetectorId::Unknown(other.to_string()),
        }
    }
}

impl Serialize for DetectorId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DetectorId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DetectorId::from_str(&String::deserialize(deserializer)?))
    }
}

/// Epic 011 Design Decision 4's evidence-strength classes, plus the two
/// trust classes Epic 013 adds (helper proposal, human assumption). Like
/// [`RelationKind`]/[`DetectorId`], unrecognized values survive
/// deserialization rather than failing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceClass {
    Structural,
    Exact,
    NormalizedLexicalCandidate,
    SemanticCandidate,
    HelperProposal,
    HumanAssumption,
    Unknown(String),
}

impl EvidenceClass {
    pub fn as_str(&self) -> &str {
        match self {
            EvidenceClass::Structural => "structural",
            EvidenceClass::Exact => "exact",
            EvidenceClass::NormalizedLexicalCandidate => "normalized_lexical_candidate",
            EvidenceClass::SemanticCandidate => "semantic_candidate",
            EvidenceClass::HelperProposal => "helper_proposal",
            EvidenceClass::HumanAssumption => "human_assumption",
            EvidenceClass::Unknown(s) => s.as_str(),
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, EvidenceClass::Unknown(_))
    }

    fn from_str(s: &str) -> Self {
        match s {
            "structural" => EvidenceClass::Structural,
            "exact" => EvidenceClass::Exact,
            "normalized_lexical_candidate" => EvidenceClass::NormalizedLexicalCandidate,
            "semantic_candidate" => EvidenceClass::SemanticCandidate,
            "helper_proposal" => EvidenceClass::HelperProposal,
            "human_assumption" => EvidenceClass::HumanAssumption,
            other => EvidenceClass::Unknown(other.to_string()),
        }
    }
}

impl Serialize for EvidenceClass {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EvidenceClass {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(EvidenceClass::from_str(&String::deserialize(deserializer)?))
    }
}

/// One piece of evidence contributing to an edge. Epic 011 Design Decision 3:
/// "Callers must be able to explain an edge without reconstructing the
/// detector" — every field an explanation needs is recorded here, not
/// recomputed later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeProvenance {
    pub detector: DetectorId,
    /// Short machine-readable method name, e.g. `"checkbox_state"`,
    /// `"dependency_section_reference"`, `"backtick_path_reference"`.
    pub method: String,
    /// Where the evidence was found: a locator string, e.g.
    /// `"epics/014-task-zero-prompt-lab.md:L460"` or a node's `logical_id`.
    pub evidence_locator: String,
    pub source_version: String,
    pub evidence_class: EvidenceClass,
    /// `0.0..=1.0`. Structural/exact evidence records `1.0`; candidate
    /// classes are expected to record something lower once Task 3 adds
    /// lexical/semantic detectors.
    pub confidence: OrderedF64,
    pub detail: String,
}

/// `f64` wrapper with a total order, so [`EdgeProvenance`] (and anything
/// containing it) can derive `Ord`/`Eq` for deterministic sorting. Task 2
/// only ever produces confidence values of exactly `0.0` or `1.0`
/// (structural/exact evidence), so `PartialEq`/`Ord` panicking on NaN is not
/// a practical risk here, but the wrapper documents the intent explicitly
/// rather than silently relying on IEEE 754 partial ordering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OrderedF64(pub f64);

impl Eq for OrderedF64 {}
impl PartialOrd for OrderedF64 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub relation: RelationKind,
    /// `from`/`to` are [`NodeLocator::logical_id`] values. An edge may
    /// reference a locator not present in this run's `nodes` list (e.g. a
    /// cross-epic dependency pointing at an epic outside `--epics`); that is
    /// reported honestly via [`GraphCoverage`], not silently hidden.
    pub from: String,
    pub to: String,
    pub provenance: Vec<EdgeProvenance>,
}

impl Edge {
    /// Explain this edge without rerunning any detector — every fact used
    /// here was already recorded in `provenance` at extraction time.
    pub fn explain(&self) -> String {
        let mut out = format!("{} --{}--> {}", self.from, self.relation, self.to);
        for p in &self.provenance {
            out.push_str(&format!(
                "\n  - [{}/{}] {} (evidence: {}, class: {}, confidence: {:.1}, version: {})",
                p.detector, p.method, p.detail, p.evidence_locator, p.evidence_class, p.confidence.0, p.source_version
            ));
        }
        out
    }
}

impl fmt::Display for DetectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for EvidenceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What documents this run parsed (or tried and failed to parse), so
/// [`GraphExtraction`] can report coverage rather than implying it read
/// everything relevant (Design Constraint 1, Non-goal "Claim that a bounded
/// context packet contains every relevant artifact" — the same discipline
/// applies one layer earlier, at extraction).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParsedDocument {
    pub source_path: String,
    pub source_version: String,
    /// `true` when content was read from a Git blob at the resolved
    /// revision; `false` when it fell back to current working-tree content
    /// under an explicit dirty-input policy (Design Constraint 2).
    pub read_from_pinned_revision: bool,
    pub node_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphCoverage {
    pub parsed_documents: Vec<ParsedDocument>,
    /// Epic numbers explicitly requested (`--epics`) whose file could not be
    /// found or read at the resolved revision under the active dirty-input
    /// policy.
    pub requested_epics_unavailable: Vec<u32>,
    pub git_available: bool,
    pub index_available: bool,
    /// Number of distinct `(source_type, identifier)` pairs from the
    /// existing indexer database that this run's explicit path/symbol
    /// references were checked against (see [`DetectorId::IndexIdentityReuse`]).
    /// `0` when `index_available` is `false`.
    pub index_identifiers_considered: usize,
}

/// Human-readable notice about an `Unknown` relation/detector/evidence-class
/// value encountered while loading or building a graph. Never causes a
/// crash; callers may choose to surface these to a user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    pub message: String,
}

/// The full Task 2 output: nodes, the four trust-distinct edge classes,
/// coverage, and gaps.
///
/// Deliberately excludes any run-timing field (e.g. "generated at") so two
/// extractions over byte-identical inputs produce byte-identical
/// `serde_json::to_string` output (Task 2 acceptance criterion: "Repeated
/// extraction from identical inputs produces byte-stable normalized JSON
/// apart from explicitly excluded run timing fields"). A caller that wants a
/// timestamp should record it alongside this value, not inside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphExtraction {
    pub lab_version: String,
    pub repo_path: String,
    pub resolved_commit: String,
    pub nodes: Vec<Node>,
    /// Established edges: created only from structural/exact evidence
    /// (Markdown structure, checkboxes, explicit paths/symbols, Git facts).
    pub established_edges: Vec<Edge>,
    /// Lexical/semantic candidate edges. Always empty in Task 2 — no
    /// lexical/semantic detector exists yet (Epic 014 Task 3) — but the
    /// field is present now so the trust-class boundary is a schema fact
    /// from the start, not something bolted on later.
    pub candidate_edges: Vec<Edge>,
    /// Helper-proposed edges. Always empty until Epic 014 Task 5 exists;
    /// present now for the same reason as `candidate_edges`. Design
    /// Constraint 3: "Helper output cannot create established
    /// relationships" — helper output must only ever land here.
    pub helper_proposed_edges: Vec<Edge>,
    /// Edges asserted by a human rather than detected. Empty unless a caller
    /// explicitly supplies one (no such input exists yet in Task 2's CLI).
    pub human_assumption_edges: Vec<Edge>,
    /// Locator IDs of nodes with `kind == UnresolvedGap`, for quick access
    /// without duplicating their content; the full node (with its verbatim
    /// unchecked-criterion label) lives in `nodes`.
    pub gaps: Vec<String>,
    pub coverage: GraphCoverage,
    pub diagnostics: Vec<Diagnostic>,
}

impl GraphExtraction {
    /// Scan for `Unknown(..)` relation/detector/evidence-class values across
    /// every edge class and append one diagnostic per occurrence. Task 2
    /// acceptance criterion: "Unknown relation/detector values survive
    /// fixture loading as diagnostics, not crashes" — this is the "survive
    /// as diagnostics" half; the "not crash" half is already guaranteed by
    /// the custom `Deserialize` impls in this module.
    pub fn collect_unknown_value_diagnostics(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for edges in [
            &self.established_edges,
            &self.candidate_edges,
            &self.helper_proposed_edges,
            &self.human_assumption_edges,
        ] {
            for edge in edges {
                if edge.relation.is_unknown() {
                    out.push(Diagnostic {
                        message: format!(
                            "edge {} -> {}: unrecognized relation kind '{}'",
                            edge.from,
                            edge.to,
                            edge.relation.as_str()
                        ),
                    });
                }
                for p in &edge.provenance {
                    if p.detector.is_unknown() {
                        out.push(Diagnostic {
                            message: format!(
                                "edge {} -> {}: unrecognized detector '{}'",
                                edge.from,
                                edge.to,
                                p.detector.as_str()
                            ),
                        });
                    }
                    if p.evidence_class.is_unknown() {
                        out.push(Diagnostic {
                            message: format!(
                                "edge {} -> {}: unrecognized evidence class '{}'",
                                edge.from,
                                edge.to,
                                p.evidence_class.as_str()
                            ),
                        });
                    }
                }
            }
        }
        out
    }

    /// Deterministically sort every collection so repeated extraction from
    /// identical inputs produces byte-identical `serde_json` output
    /// regardless of intermediate `HashMap`/filesystem-listing iteration
    /// order upstream.
    pub fn normalize(&mut self) {
        self.nodes.sort_by(|a, b| a.locator.logical_id.cmp(&b.locator.logical_id));
        self.nodes.dedup_by(|a, b| a.locator.logical_id == b.locator.logical_id);
        for edges in [
            &mut self.established_edges,
            &mut self.candidate_edges,
            &mut self.helper_proposed_edges,
            &mut self.human_assumption_edges,
        ] {
            for edge in edges.iter_mut() {
                edge.provenance.sort();
            }
            edges.sort_by(|a, b| (&a.from, &a.to, a.relation.as_str()).cmp(&(&b.from, &b.to, b.relation.as_str())));
        }
        self.gaps.sort();
        self.gaps.dedup();
        self.coverage.parsed_documents.sort();
        self.coverage.requested_epics_unavailable.sort();
        self.coverage.requested_epics_unavailable.dedup();
        self.diagnostics.sort();
        self.diagnostics.dedup();
    }

    /// Byte-stable normalized JSON (Task 2 acceptance criterion). Callers
    /// wanting run timing should record it outside this string, e.g.
    /// alongside it in a wrapper object.
    pub fn to_normalized_json(&self) -> anyhow::Result<String> {
        let mut copy = self.clone();
        copy.normalize();
        Ok(serde_json::to_string_pretty(&copy)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_relation_kind_deserializes_without_error() {
        let json = r#""some_future_relation""#;
        let relation: RelationKind = serde_json::from_str(json).unwrap();
        assert!(relation.is_unknown());
        assert_eq!(relation.as_str(), "some_future_relation");
    }

    #[test]
    fn unknown_detector_and_evidence_class_deserialize_without_error() {
        let detector: DetectorId = serde_json::from_str(r#""future_detector_v2""#).unwrap();
        assert!(detector.is_unknown());
        let class: EvidenceClass = serde_json::from_str(r#""future_evidence_class""#).unwrap();
        assert!(class.is_unknown());
    }

    #[test]
    fn known_relation_round_trips_through_serde() {
        let relation = RelationKind::DependsOn;
        let json = serde_json::to_string(&relation).unwrap();
        assert_eq!(json, "\"depends_on\"");
        let back: RelationKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, relation);
    }

    #[test]
    fn edge_explain_needs_no_detector_rerun() {
        let edge = Edge {
            relation: RelationKind::BlockedBy,
            from: "epic:014/task:2/criterion:1".to_string(),
            to: "epic:014/task:2/criterion:1#gap".to_string(),
            provenance: vec![EdgeProvenance {
                detector: DetectorId::MarkdownStructure,
                method: "checkbox_state".to_string(),
                evidence_locator: "epics/014-task-zero-prompt-lab.md:L460".to_string(),
                source_version: "abc123".to_string(),
                evidence_class: EvidenceClass::Structural,
                confidence: OrderedF64(1.0),
                detail: "criterion checkbox is unchecked ('- [ ]')".to_string(),
            }],
        };
        let explanation = edge.explain();
        assert!(explanation.contains("checkbox_state"));
        assert!(explanation.contains("epics/014-task-zero-prompt-lab.md:L460"));
        assert!(explanation.contains("structural"));
    }

    #[test]
    fn collect_unknown_value_diagnostics_reports_each_unknown_field() {
        let mut extraction = empty_extraction();
        extraction.established_edges.push(Edge {
            relation: RelationKind::Unknown("weird_relation".to_string()),
            from: "a".to_string(),
            to: "b".to_string(),
            provenance: vec![EdgeProvenance {
                detector: DetectorId::Unknown("weird_detector".to_string()),
                method: "m".to_string(),
                evidence_locator: "loc".to_string(),
                source_version: "v".to_string(),
                evidence_class: EvidenceClass::Unknown("weird_class".to_string()),
                confidence: OrderedF64(1.0),
                detail: "d".to_string(),
            }],
        });
        let diags = extraction.collect_unknown_value_diagnostics();
        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn normalize_is_idempotent_and_sorts_collections() {
        let mut extraction = empty_extraction();
        extraction.nodes.push(node("b", NodeKind::Task));
        extraction.nodes.push(node("a", NodeKind::Task));
        extraction.normalize();
        assert_eq!(extraction.nodes[0].locator.logical_id, "a");
        assert_eq!(extraction.nodes[1].locator.logical_id, "b");

        let json1 = extraction.to_normalized_json().unwrap();
        let json2 = extraction.to_normalized_json().unwrap();
        assert_eq!(json1, json2);
    }

    fn node(id: &str, kind: NodeKind) -> Node {
        Node {
            kind,
            locator: NodeLocator::new(id, None, None),
            label: id.to_string(),
            source_version: "v".to_string(),
            checked: None,
            extra: serde_json::Value::Null,
        }
    }

    fn empty_extraction() -> GraphExtraction {
        GraphExtraction {
            lab_version: "0.1.0".to_string(),
            repo_path: "/tmp/repo".to_string(),
            resolved_commit: "deadbeef".to_string(),
            nodes: Vec::new(),
            established_edges: Vec::new(),
            candidate_edges: Vec::new(),
            helper_proposed_edges: Vec::new(),
            human_assumption_edges: Vec::new(),
            gaps: Vec::new(),
            coverage: GraphCoverage {
                parsed_documents: Vec::new(),
                requested_epics_unavailable: Vec::new(),
                git_available: true,
                index_available: false,
                index_identifiers_considered: 0,
            },
            diagnostics: Vec::new(),
        }
    }
}
