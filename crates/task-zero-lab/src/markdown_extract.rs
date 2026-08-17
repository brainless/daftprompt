//! Epic 014 Task 2: deterministic Markdown structure extraction.
//!
//! Pure text-in, nodes/edges-out functions. This module never touches Git or
//! the filesystem — callers ([`crate::graph_build`]) resolve a document's
//! path, revision-pinned content, and `source_version` first, then hand the
//! plain text here. Keeping this module pure makes it directly unit-testable
//! against literal strings and the real epic files without any repository
//! fixture setup (Task 2 acceptance criterion: "extracted deterministically
//! with source spans").
//!
//! Two entry points:
//! - [`extract_epic`]: headings, `### Task N: ...` nesting, `#### Acceptance
//!   Criteria` checkbox lists, the `## Dependency` section, `## Design
//!   Constraints`/`## Design Decisions` numbered entries, and (via a single
//!   whole-document reference pass) explicit backtick paths/symbols, shell
//!   commands, and cross-epic references.
//! - [`extract_project_instructions`]: a smaller heading-only pass for
//!   `AGENTS.md`/`DEVELOP.md`, tagging each top-level section with an
//!   explicit source-precedence rank (Task 2 acceptance criterion: "Project
//!   rules ... retain source precedence and provenance").

use regex::Regex;
use serde_json::json;
use std::sync::OnceLock;

use crate::graph::{
    DetectorId, Edge, EdgeProvenance, EvidenceClass, Node, NodeKind, NodeLocator, OrderedF64, RelationKind,
};

// ── Heading tree ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Heading {
    level: usize,
    text: String,
    /// 1-based line the `#` marker itself is on.
    start_line: usize,
    /// 1-based, inclusive: the last line belonging to this heading and all
    /// of its descendants.
    end_line: usize,
}

fn heading_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(#{1,6})\s+(.*?)\s*$").unwrap())
}

/// Parse `#`-style headings, skipping any line inside a fenced code block
/// (` ``` `) so a shell comment starting with `#` inside an example command
/// block is never mistaken for document structure.
fn parse_headings(text: &str) -> Vec<Heading> {
    let lines: Vec<&str> = text.lines().collect();
    let mut headings = Vec::new();
    let mut in_fence = false;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(caps) = heading_regex().captures(line) {
            let level = caps[1].len();
            let title = caps[2].to_string();
            headings.push(Heading {
                level,
                text: title,
                start_line: i + 1,
                end_line: i + 1,
            });
        }
    }
    let total_lines = lines.len().max(1);
    for idx in 0..headings.len() {
        let level = headings[idx].level;
        let mut end = total_lines;
        for other in &headings[idx + 1..] {
            if other.level <= level {
                end = other.start_line - 1;
                break;
            }
        }
        headings[idx].end_line = end;
    }
    headings
}

/// Indices of `headings[parent_idx]`'s immediate (`level + 1`) children.
fn direct_children(headings: &[Heading], parent_idx: usize) -> Vec<usize> {
    let parent = &headings[parent_idx];
    let mut out = Vec::new();
    let mut i = parent_idx + 1;
    while i < headings.len() && headings[i].start_line <= parent.end_line {
        if headings[i].level == parent.level + 1 {
            out.push(i);
        }
        i += 1;
    }
    out
}

/// Body text belonging directly to a heading: everything after its own line
/// up to (but not including) its first child heading, or its `end_line` if
/// it has no children. Returned as `(first_line, last_line, text)`.
fn heading_body<'a>(lines: &[&'a str], headings: &[Heading], idx: usize, children: &[usize]) -> (usize, usize, String) {
    let h = &headings[idx];
    let body_start = h.start_line + 1;
    let body_end = children.first().map(|&c| headings[c].start_line - 1).unwrap_or(h.end_line);
    if body_end < body_start {
        return (body_start, body_start.saturating_sub(1), String::new());
    }
    let text = lines[(body_start - 1)..body_end.min(lines.len())].join("\n");
    (body_start, body_end, text)
}

// ── Checkbox list items ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ChecklistItem {
    checked: bool,
    text: String,
    start_line: usize,
    end_line: usize,
}

fn checkbox_line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^- \[( |x|X)\]\s+(.*)$").unwrap())
}

/// Parse a top-level checkbox list. `body_start_line` is the 1-based line
/// number of `body_text`'s first line, so returned spans stay in the
/// original document's coordinates. Indented non-blank continuation lines
/// are appended to the previous item's text; unindented content ends the
/// continuation region so trailing prose/fences cannot leak into a criterion.
fn parse_checkbox_list(body_text: &str, body_start_line: usize) -> Vec<ChecklistItem> {
    let mut items: Vec<ChecklistItem> = Vec::new();
    let mut accepting_continuation = false;
    for (offset, line) in body_text.lines().enumerate() {
        let line_no = body_start_line + offset;
        if let Some(caps) = checkbox_line_regex().captures(line) {
            let checked = matches!(&caps[1], "x" | "X");
            items.push(ChecklistItem {
                checked,
                text: caps[2].trim().to_string(),
                start_line: line_no,
                end_line: line_no,
            });
            accepting_continuation = true;
        } else if line.trim().is_empty() {
            // A blank line is harmless, but it is not part of the criterion
            // text or source span. A subsequent continuation must still be
            // indented like Markdown content belonging to the list item.
        } else if accepting_continuation && line.starts_with([' ', '\t']) {
            let Some(last) = items.last_mut() else { continue };
            let trimmed = line.trim();
            last.text.push(' ');
            last.text.push_str(trimmed);
            last.end_line = line_no;
        } else {
            // Unindented prose, a fence, or another top-level construct ends
            // the checklist item's continuation region. Do not silently turn
            // trailing section prose into acceptance-criterion or gap text.
            accepting_continuation = false;
        }
    }
    items
}

// ── Whole-document reference scan (paths, symbols, commands, epic refs) ──

pub(crate) fn backtick_span_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`([^`\n]+)`").unwrap())
}

pub(crate) fn path_like_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // A repo-relative-looking path: at least one `/`, only path-safe
    // characters, and a final component containing a `.` extension or a
    // trailing `/` (directory reference).
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_.\-]+(/[A-Za-z0-9_.\-]+)+/?$").unwrap())
}

pub(crate) fn symbol_like_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Rust-style qualified path, e.g. `ReviewQueue::columns` or
    // `daftprompt_indexer::db::content_hash`.
    RE.get_or_init(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)+(\(\))?$").unwrap())
}

/// A `path::symbol` composite reference, e.g.
/// `` `email_ranking/mod.rs::contains_date` `` — a file path (matching the
/// same shape as [`path_like_regex`], minus the trailing-`/` directory case)
/// followed by `::` and a bare identifier. Neither [`path_like_regex`] (no
/// `::` allowed) nor [`symbol_like_regex`] (no `/` or `.` allowed in its
/// leading segment) matches this shape on its own, but requests naming an
/// exact file and symbol together in one backtick span are common enough
/// (see Epic 014's C07 fixture) to need their own recognizer rather than
/// being silently dropped as an unrecognized span.
pub(crate) fn path_symbol_like_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([A-Za-z0-9_.\-]+(?:/[A-Za-z0-9_.\-]+)+)::([A-Za-z_][A-Za-z0-9_]*(?:\(\))?)$").unwrap())
}

const COMMAND_VERBS: &[&str] = &["cargo", "git", "rustc", "RUST_LOG"];

pub(crate) fn looks_like_command(span: &str) -> bool {
    let first_word = span.split_whitespace().next().unwrap_or("");
    COMMAND_VERBS.iter().any(|verb| first_word == *verb || first_word.starts_with(&format!("{verb}=")))
}

fn epic_prose_ref_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // "Epic 011" / "Epics 011, 012, and 013" style prose. Captures one
    // 3-digit epic number per match; callers call `find_iter` repeatedly or
    // use `epic_numbers_in_prose_match` to expand a comma/and-joined list.
    RE.get_or_init(|| Regex::new(r"\bEpics?\s+((?:\d{3}(?:\s*[,\-]\s*|\s+and\s+)?)+)").unwrap())
}

fn epic_number_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{3}").unwrap())
}

fn epic_path_ref_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"epics/(\d{3})-[A-Za-z0-9\-]+\.md").unwrap())
}

#[derive(Debug, Clone)]
enum Reference {
    Path { text: String, line: usize },
    Symbol { text: String, line: usize },
    Command { text: String, line: usize },
    EpicRef { number: u32, line: usize },
}

/// Scan every line of the document for backtick spans, fenced `bash`
/// command lines, and cross-epic references. Runs once over the whole
/// document rather than per-node so overlapping node spans (e.g. a
/// criterion nested inside a task nested inside the epic) are each only
/// scanned once, at the cost of a slightly larger single pass.
fn scan_references(text: &str) -> Vec<Reference> {
    let mut refs = Vec::new();
    let mut in_bash_fence = false;
    for (i, line) in text.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_bash_fence = !in_bash_fence && (trimmed.starts_with("```bash") || trimmed.starts_with("```text") || trimmed.starts_with("```sh"));
            // Note: this toggles on for a recognized-language fence and back
            // off on the closing ``` line of *any* fence, which is correct
            // as long as fences are not nested (Markdown does not nest
            // fences), so a closing ``` always matches the most recent open.
            if trimmed == "```" {
                in_bash_fence = false;
            }
            continue;
        }
        if in_bash_fence {
            let candidate = trimmed.trim();
            if !candidate.is_empty() && !candidate.starts_with('#') {
                refs.push(Reference::Command {
                    text: candidate.to_string(),
                    line: line_no,
                });
            }
            continue;
        }

        for caps in backtick_span_regex().captures_iter(line) {
            let span = caps[1].to_string();
            if path_like_regex().is_match(&span) {
                refs.push(Reference::Path { text: span, line: line_no });
            } else if symbol_like_regex().is_match(&span) {
                refs.push(Reference::Symbol { text: span, line: line_no });
            } else if looks_like_command(&span) {
                refs.push(Reference::Command { text: span, line: line_no });
            }
        }

        for caps in epic_path_ref_regex().captures_iter(line) {
            if let Ok(number) = caps[1].parse::<u32>() {
                refs.push(Reference::EpicRef { number, line: line_no });
            }
        }
        for caps in epic_prose_ref_regex().captures_iter(line) {
            for m in epic_number_regex().find_iter(&caps[1]) {
                if let Ok(number) = m.as_str().parse::<u32>() {
                    refs.push(Reference::EpicRef { number, line: line_no });
                }
            }
        }
    }
    refs
}

// ── Enclosing-container lookup ──────────────────────────────────────────

/// `(locator, start_line, end_line)`, used to attribute a whole-document
/// reference to its smallest enclosing structural node.
type Container = (String, usize, usize);

fn enclosing_locator(containers: &[Container], line: usize) -> Option<String> {
    containers
        .iter()
        .filter(|(_, start, end)| *start <= line && line <= *end)
        .min_by_key(|(_, start, end)| end - start)
        .map(|(locator, _, _)| locator.clone())
}

// ── Public: epic extraction ─────────────────────────────────────────────

pub struct EpicDoc<'a> {
    pub epic_number: u32,
    pub source_path: String,
    pub source_version: String,
    pub text: &'a str,
}

pub struct EpicExtraction {
    pub nodes: Vec<Node>,
    pub established_edges: Vec<Edge>,
    pub diagnostics: Vec<String>,
}

fn structural_provenance(evidence_locator: String, source_version: &str, method: &str, detail: String) -> EdgeProvenance {
    EdgeProvenance {
        detector: DetectorId::MarkdownStructure,
        method: method.to_string(),
        evidence_locator,
        source_version: source_version.to_string(),
        evidence_class: EvidenceClass::Structural,
        confidence: OrderedF64(1.0),
        detail,
    }
}

fn exact_provenance(evidence_locator: String, source_version: &str, method: &str, detail: String) -> EdgeProvenance {
    EdgeProvenance {
        detector: DetectorId::MarkdownExactReference,
        method: method.to_string(),
        evidence_locator,
        source_version: source_version.to_string(),
        evidence_class: EvidenceClass::Exact,
        confidence: OrderedF64(1.0),
        detail,
    }
}

fn contains_edge(from: &str, to: &str, provenance: EdgeProvenance) -> Edge {
    Edge {
        relation: RelationKind::Contains,
        from: from.to_string(),
        to: to.to_string(),
        provenance: vec![provenance],
    }
}

pub fn extract_epic(doc: &EpicDoc<'_>) -> EpicExtraction {
    let lines: Vec<&str> = doc.text.lines().collect();
    let headings = parse_headings(doc.text);
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut diagnostics = Vec::new();
    let mut containers: Vec<Container> = Vec::new();

    let epic_locator = format!("epic:{:03}", doc.epic_number);
    let span_loc = |line: usize| -> String { format!("{}:L{}", doc.source_path, line) };

    let Some(h1_idx) = headings.iter().position(|h| h.level == 1) else {
        diagnostics.push(format!("{}: no top-level '# Epic N: ...' heading found", doc.source_path));
        return EpicExtraction {
            nodes,
            established_edges: edges,
            diagnostics,
        };
    };
    let h1 = &headings[h1_idx];
    containers.push((epic_locator.clone(), 1, lines.len().max(1)));
    nodes.push(Node {
        kind: NodeKind::Epic,
        locator: NodeLocator::new(epic_locator.clone(), Some(doc.source_path.clone()), Some((h1.start_line, h1.end_line))),
        label: h1.text.clone(),
        source_version: doc.source_version.clone(),
        checked: None,
        extra: json!({ "epic_number": doc.epic_number }),
    });

    for &h2_idx in &direct_children(&headings, h1_idx) {
        let h2 = &headings[h2_idx];
        let section_title = h2.text.trim();

        if section_title.eq_ignore_ascii_case("Dependency") {
            let (body_start, body_end, body) = heading_body(&lines, &headings, h2_idx, &direct_children(&headings, h2_idx));
            for reference in scan_references(&body) {
                if let Reference::EpicRef { number, line } = reference {
                    if number == doc.epic_number {
                        continue;
                    }
                    let actual_line = body_start + line - 1;
                    if actual_line > body_end {
                        continue;
                    }
                    edges.push(Edge {
                        relation: RelationKind::DependsOn,
                        from: epic_locator.clone(),
                        to: format!("epic:{:03}", number),
                        provenance: vec![exact_provenance(
                            span_loc(actual_line),
                            &doc.source_version,
                            "dependency_section_reference",
                            format!("'## Dependency' section names Epic {:03}", number),
                        )],
                    });
                }
            }
            continue;
        }

        if section_title.eq_ignore_ascii_case("Design Constraints") || section_title.eq_ignore_ascii_case("Design Decisions") {
            for &h3_idx in &direct_children(&headings, h2_idx) {
                let h3 = &headings[h3_idx];
                let constraint_number = h3
                    .text
                    .split_once('.')
                    .and_then(|(n, _)| n.trim().parse::<u32>().ok());
                let Some(number) = constraint_number else {
                    diagnostics.push(format!(
                        "{}: design constraint/decision heading '{}' has no leading 'N. ' number",
                        doc.source_path, h3.text
                    ));
                    continue;
                };
                let locator = format!("{}/constraint:{}", epic_locator, number);
                containers.push((locator.clone(), h3.start_line, h3.end_line));
                nodes.push(Node {
                    kind: NodeKind::DesignConstraint,
                    locator: NodeLocator::new(locator.clone(), Some(doc.source_path.clone()), Some((h3.start_line, h3.end_line))),
                    label: h3.text.clone(),
                    source_version: doc.source_version.clone(),
                    checked: None,
                    extra: json!({}),
                });
                edges.push(contains_edge(
                    &epic_locator,
                    &locator,
                    structural_provenance(
                        span_loc(h3.start_line),
                        &doc.source_version,
                        "heading_containment",
                        format!("'{}' is a direct subsection of '{}'", h3.text, section_title),
                    ),
                ));
            }
            continue;
        }

        if section_title.eq_ignore_ascii_case("Tasks") {
            static TASK_HEADING_RE_CELL: OnceLock<Regex> = OnceLock::new();
            let task_heading_re = TASK_HEADING_RE_CELL.get_or_init(|| Regex::new(r"^Task (\d+):\s*(.*)$").unwrap());

            for &h3_idx in &direct_children(&headings, h2_idx) {
                let h3 = &headings[h3_idx];
                let Some(caps) = task_heading_re.captures(h3.text.trim()) else {
                    diagnostics.push(format!("{}: heading '{}' under 'Tasks' does not match 'Task N: ...'", doc.source_path, h3.text));
                    continue;
                };
                let task_number: u32 = caps[1].parse().unwrap_or(0);
                let task_locator = format!("{}/task:{}", epic_locator, task_number);
                containers.push((task_locator.clone(), h3.start_line, h3.end_line));
                nodes.push(Node {
                    kind: NodeKind::Task,
                    locator: NodeLocator::new(task_locator.clone(), Some(doc.source_path.clone()), Some((h3.start_line, h3.end_line))),
                    label: h3.text.clone(),
                    source_version: doc.source_version.clone(),
                    checked: None,
                    extra: json!({ "task_number": task_number }),
                });
                edges.push(contains_edge(
                    &epic_locator,
                    &task_locator,
                    structural_provenance(
                        span_loc(h3.start_line),
                        &doc.source_version,
                        "heading_containment",
                        format!("'{}' is a direct subsection of 'Tasks'", h3.text),
                    ),
                ));

                let task_children = direct_children(&headings, h3_idx);
                let Some(&h4_idx) = task_children
                    .iter()
                    .find(|&&idx| headings[idx].text.trim().eq_ignore_ascii_case("Acceptance Criteria"))
                else {
                    diagnostics.push(format!("{}: Task {} has no 'Acceptance Criteria' subsection", doc.source_path, task_number));
                    continue;
                };
                let h4 = &headings[h4_idx];
                let (body_start, _body_end, body) = heading_body(&lines, &headings, h4_idx, &direct_children(&headings, h4_idx));
                let items = parse_checkbox_list(&body, body_start);
                if items.is_empty() {
                    diagnostics.push(format!(
                        "{}: Task {} 'Acceptance Criteria' section (L{}-L{}) contains no '- [ ]'/'- [x]' items",
                        doc.source_path, task_number, h4.start_line, h4.end_line
                    ));
                }
                for (ordinal, item) in items.iter().enumerate() {
                    let ordinal = ordinal + 1;
                    let criterion_locator = format!("{}/criterion:{}", task_locator, ordinal);
                    containers.push((criterion_locator.clone(), item.start_line, item.end_line));
                    nodes.push(Node {
                        kind: NodeKind::AcceptanceCriterion,
                        locator: NodeLocator::new(
                            criterion_locator.clone(),
                            Some(doc.source_path.clone()),
                            Some((item.start_line, item.end_line)),
                        ),
                        label: item.text.clone(),
                        source_version: doc.source_version.clone(),
                        checked: Some(item.checked),
                        extra: json!({}),
                    });
                    edges.push(contains_edge(
                        &task_locator,
                        &criterion_locator,
                        structural_provenance(
                            span_loc(item.start_line),
                            &doc.source_version,
                            "checkbox_list_containment",
                            format!("criterion is item {} of Task {}'s 'Acceptance Criteria' list", ordinal, task_number),
                        ),
                    ));

                    if !item.checked {
                        let gap_locator = format!("{}#gap", criterion_locator);
                        nodes.push(Node {
                            kind: NodeKind::UnresolvedGap,
                            locator: NodeLocator::new(
                                gap_locator.clone(),
                                Some(doc.source_path.clone()),
                                Some((item.start_line, item.end_line)),
                            ),
                            // Verbatim copy of the criterion text: this node
                            // must not imply why the criterion is unchecked
                            // (Task 2 acceptance criterion), so it carries no
                            // interpretation beyond the source text itself.
                            label: item.text.clone(),
                            source_version: doc.source_version.clone(),
                            checked: None,
                            extra: json!({ "criterion": criterion_locator }),
                        });
                        edges.push(Edge {
                            relation: RelationKind::BlockedBy,
                            from: criterion_locator.clone(),
                            to: gap_locator,
                            provenance: vec![structural_provenance(
                                span_loc(item.start_line),
                                &doc.source_version,
                                "unchecked_checkbox",
                                "criterion checkbox is unchecked ('- [ ]'); this records that the criterion is unresolved, not why".to_string(),
                            )],
                        });
                    }
                }
            }
            continue;
        }
        // Other H2 sections (Introduction, Goals, Non-goals, ...) are not
        // extracted into their own node kind in Task 2 — none of the
        // eleven minimal node families fits arbitrary prose sections, and
        // inventing one would be premature production architecture (Design
        // Constraint 1). Their text is still covered by the whole-document
        // reference scan below via the epic-level container.
    }

    // Whole-document reference pass, attributed to the smallest enclosing
    // container recorded above.
    containers.sort_by_key(|(_, start, end)| end.saturating_sub(*start));
    let mut seen_reference_edges = std::collections::HashSet::new();
    for reference in scan_references(doc.text) {
        let (line, kind_key, target_locator, target_node) = match &reference {
            Reference::Path { text, line } => {
                let locator = format!("file:{}", text);
                (
                    *line,
                    "path",
                    locator.clone(),
                    Some(Node {
                        kind: NodeKind::FileOrSection,
                        locator: NodeLocator::new(locator, None, None),
                        label: text.clone(),
                        source_version: "unresolved-reference".to_string(),
                        checked: None,
                        extra: json!({ "referenced_path": text }),
                    }),
                )
            }
            Reference::Symbol { text, line } => {
                let locator = format!("symbol:{}", text);
                (
                    *line,
                    "symbol",
                    locator.clone(),
                    Some(Node {
                        kind: NodeKind::CodeSymbol,
                        locator: NodeLocator::new(locator, None, None),
                        label: text.clone(),
                        source_version: "unresolved-reference".to_string(),
                        checked: None,
                        extra: json!({ "referenced_symbol": text }),
                    }),
                )
            }
            Reference::Command { .. } => continue, // attached to owning node's `extra` below, not a standalone node
            Reference::EpicRef { number, line } => (*line, "epic_ref", format!("epic:{:03}", number), None),
        };
        if let Some(node) = target_node {
            if !nodes.iter().any(|n| n.locator.logical_id == node.locator.logical_id) {
                nodes.push(node);
            }
        }
        let Some(container) = enclosing_locator(&containers, line) else {
            continue;
        };
        if container == target_locator {
            continue; // an epic mentioning itself is not a useful edge
        }
        let dedup_key = (container.clone(), target_locator.clone(), kind_key, line);
        if !seen_reference_edges.insert(dedup_key) {
            continue;
        }
        edges.push(Edge {
            relation: RelationKind::Mentions,
            from: container.clone(),
            to: target_locator,
            provenance: vec![exact_provenance(
                span_loc(line),
                &doc.source_version,
                match kind_key {
                    "path" => "backtick_path_reference",
                    "symbol" => "backtick_symbol_reference",
                    _ => "cross_epic_reference",
                },
                format!("explicit `{}`-style reference in {}", kind_key, doc.source_path),
            )],
        });
    }

    // Commands: attach to the smallest enclosing container's `extra.commands`
    // list rather than creating a node kind Design Constraint 3 does not
    // define.
    let mut commands_by_container: std::collections::BTreeMap<String, Vec<serde_json::Value>> = std::collections::BTreeMap::new();
    for reference in scan_references(doc.text) {
        if let Reference::Command { text, line } = reference {
            if let Some(container) = enclosing_locator(&containers, line) {
                commands_by_container.entry(container).or_default().push(json!({ "text": text, "line": line }));
            }
        }
    }
    for node in nodes.iter_mut() {
        if let Some(commands) = commands_by_container.get(&node.locator.logical_id) {
            if let Some(obj) = node.extra.as_object_mut() {
                obj.insert("commands".to_string(), json!(commands));
            }
        }
    }

    EpicExtraction {
        nodes,
        established_edges: edges,
        diagnostics,
    }
}

// ── Public: project instructions (AGENTS.md / DEVELOP.md) ──────────────

/// One top-level (`##`) section of a project-instruction document becomes
/// one [`NodeKind::ProjectInstruction`] node. `precedence` is a caller-
/// supplied rank (lower = higher precedence) recorded in `extra.precedence`
/// so downstream consumers can resolve conflicting rules deterministically
/// (Task 2 acceptance criterion: "Project rules ... retain source
/// precedence and provenance").
pub fn extract_project_instructions(source_path: &str, source_version: &str, text: &str, precedence: u32) -> Vec<Node> {
    let lines: Vec<&str> = text.lines().collect();
    let headings = parse_headings(text);
    let mut nodes = Vec::new();
    for (idx, h) in headings.iter().enumerate() {
        if h.level != 2 {
            continue;
        }
        let children = direct_children(&headings, idx);
        let (_start, _end, body) = heading_body(&lines, &headings, idx, &children);
        let locator = format!(
            "instruction:{}#{}",
            source_path,
            h.text.trim().to_lowercase().replace(' ', "-")
        );
        nodes.push(Node {
            kind: NodeKind::ProjectInstruction,
            locator: NodeLocator::new(locator, Some(source_path.to_string()), Some((h.start_line, h.end_line))),
            label: h.text.clone(),
            source_version: source_version.to_string(),
            checked: None,
            extra: json!({ "precedence": precedence, "body_excerpt": body.chars().take(400).collect::<String>() }),
        });
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_EPIC: &str = r#"# Epic 999: Sample Epic

## Dependency

Requires Epic 011 and Epics 012, 013.

## Design Constraints

### 1. First constraint

Some prose mentioning `crates/task-zero-lab/src/graph.rs` and
`daftprompt_indexer::db::content_hash`.

## Tasks

### Task 1: Do the thing

#### Acceptance Criteria

- [x] First criterion is done, see `epics/011-provenance-graph.md`.
- [ ] Second criterion is not done yet.
  Continuation line for the second criterion.

```bash
cargo test --workspace
```

### Task 2: Do another thing

#### Acceptance Criteria

- [ ] Only criterion here.
"#;

    fn doc(text: &str) -> EpicDoc<'_> {
        EpicDoc {
            epic_number: 999,
            source_path: "epics/999-sample.md".to_string(),
            source_version: "v1".to_string(),
            text,
        }
    }

    #[test]
    fn extracts_epic_task_and_criterion_nodes_with_stable_locators() {
        let extraction = extract_epic(&doc(SAMPLE_EPIC));
        let locators: Vec<&str> = extraction.nodes.iter().map(|n| n.locator.logical_id.as_str()).collect();
        assert!(locators.contains(&"epic:999"));
        assert!(locators.contains(&"epic:999/task:1"));
        assert!(locators.contains(&"epic:999/task:1/criterion:1"));
        assert!(locators.contains(&"epic:999/task:1/criterion:2"));
        assert!(locators.contains(&"epic:999/task:2/criterion:1"));
        assert!(locators.contains(&"epic:999/constraint:1"));
    }

    #[test]
    fn checkbox_state_is_captured_and_second_criterion_includes_continuation_line() {
        let extraction = extract_epic(&doc(SAMPLE_EPIC));
        let c1 = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "epic:999/task:1/criterion:1")
            .unwrap();
        assert_eq!(c1.checked, Some(true));

        let c2 = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "epic:999/task:1/criterion:2")
            .unwrap();
        assert_eq!(c2.checked, Some(false));
        assert!(c2.label.contains("Continuation line"));
    }

    #[test]
    fn unindented_content_after_checklist_is_not_absorbed_into_last_criterion() {
        let text = "# Epic 999: Sample\n\n## Tasks\n\n### Task 1: Do it\n\n#### Acceptance Criteria\n\n- [ ] Actual criterion.\n  Indented detail belongs to it.\n\nTrailing prose does not belong to the checklist.\n\n```bash\ncargo test --workspace\n```\n";
        let extraction = extract_epic(&doc(text));
        let criterion = extraction
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::AcceptanceCriterion)
            .unwrap();
        assert!(criterion.label.contains("Indented detail"));
        assert!(!criterion.label.contains("Trailing prose"));
        assert!(!criterion.label.contains("cargo test"));
        assert_eq!(criterion.locator.line_span, Some((9, 10)));
    }

    #[test]
    fn unchecked_criterion_produces_unresolved_gap_without_implying_cause() {
        let extraction = extract_epic(&doc(SAMPLE_EPIC));
        let gap = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "epic:999/task:1/criterion:2#gap")
            .expect("expected an UnresolvedGap node for the unchecked second criterion");
        assert_eq!(gap.kind, NodeKind::UnresolvedGap);
        // Verbatim copy of the criterion text, no added interpretation.
        assert!(gap.label.starts_with("Second criterion is not done yet."));

        let checked_criterion_gap = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "epic:999/task:1/criterion:1#gap");
        assert!(checked_criterion_gap.is_none(), "a checked criterion must not produce a gap node");

        let edge = extraction
            .established_edges
            .iter()
            .find(|e| e.from == "epic:999/task:1/criterion:2" && e.relation == RelationKind::BlockedBy)
            .expect("expected criterion --blocked_by--> gap edge");
        assert_eq!(edge.to, "epic:999/task:1/criterion:2#gap");
    }

    #[test]
    fn dependency_section_produces_depends_on_edges_to_referenced_epics() {
        let extraction = extract_epic(&doc(SAMPLE_EPIC));
        let deps: Vec<&str> = extraction
            .established_edges
            .iter()
            .filter(|e| e.relation == RelationKind::DependsOn && e.from == "epic:999")
            .map(|e| e.to.as_str())
            .collect();
        assert!(deps.contains(&"epic:011"));
        assert!(deps.contains(&"epic:012"));
        assert!(deps.contains(&"epic:013"));
    }

    #[test]
    fn explicit_path_and_symbol_references_produce_mentions_edges_with_source_spans() {
        let extraction = extract_epic(&doc(SAMPLE_EPIC));
        let path_edge = extraction
            .established_edges
            .iter()
            .find(|e| e.relation == RelationKind::Mentions && e.to == "file:crates/task-zero-lab/src/graph.rs")
            .expect("expected a mentions edge to the referenced path");
        assert_eq!(path_edge.from, "epic:999/constraint:1");
        assert_eq!(path_edge.provenance[0].evidence_locator, "epics/999-sample.md:L11");

        let symbol_edge = extraction
            .established_edges
            .iter()
            .find(|e| e.relation == RelationKind::Mentions && e.to == "symbol:daftprompt_indexer::db::content_hash")
            .expect("expected a mentions edge to the referenced symbol");
        assert_eq!(symbol_edge.from, "epic:999/constraint:1");
    }

    #[test]
    fn fenced_bash_commands_after_checklist_attach_to_enclosing_task() {
        let extraction = extract_epic(&doc(SAMPLE_EPIC));
        let task = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "epic:999/task:1")
            .unwrap();
        let commands = task.extra.get("commands").expect("expected commands attached to the enclosing node");
        assert_eq!(commands[0]["text"], "cargo test --workspace");
    }

    #[test]
    fn repeated_extraction_is_byte_stable() {
        let e1 = extract_epic(&doc(SAMPLE_EPIC));
        let e2 = extract_epic(&doc(SAMPLE_EPIC));
        let j1 = serde_json::to_string(&e1.nodes).unwrap();
        let j2 = serde_json::to_string(&e2.nodes).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn missing_epic_heading_reports_diagnostic_without_panicking() {
        let extraction = extract_epic(&doc("no heading here\njust text\n"));
        assert!(extraction.nodes.is_empty());
        assert!(!extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_project_instructions_tags_source_precedence() {
        let text = "# AGENTS.md\n\n## Build, Run, Test\n\nSome instructions.\n\n## Conventions\n\nMore rules.\n";
        let nodes = extract_project_instructions("AGENTS.md", "v1", text, 0);
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|n| n.extra["precedence"] == 0));
        assert!(nodes.iter().any(|n| n.label == "Build, Run, Test"));
    }
}
