//! Deterministic clarification-branch logic: request construction, per-span
//! branch state, and the view model the panel renders.
//!
//! Like `view`, this module has no akar or HTTP dependency. The API is
//! stateless, so the GUI keeps each branch's turns in memory for the current
//! parse result and resends them with every follow-up. A new parse discards all
//! branches. Citation locations are rendered as the API computed them; the GUI
//! never reads project files.

use std::collections::BTreeMap;

use daftprompt_types::{
    CitationInfo, CitationLocation, ClarifyRequest, ClarifyResult, ClarifySource, ClarifyTarget,
    ClarifyTurn, Grounding, ParseResult,
};

use crate::api::ApiError;
use crate::view::truncate_for_display;

const PREVIEW_CHARS: usize = 96;

/// One completed question and explanation in a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub question_node_id: String,
    pub question: String,
    pub explanation_node_id: String,
    pub explanation: String,
    pub citations: Vec<CitationInfo>,
    pub grounding: Grounding,
}

impl Turn {
    /// Validate a result into a turn. A result without usable node ids or
    /// explanation text is reported instead of silently producing a blank turn.
    pub fn from_result(question: &str, result: ClarifyResult) -> Result<Self, ApiError> {
        if result.question_node_id.trim().is_empty() || result.explanation_node_id.trim().is_empty()
        {
            return Err(ApiError::decode(
                "the clarification result has no question or explanation node id",
            ));
        }
        if result.explanation_payload.content.trim().is_empty() {
            return Err(ApiError::decode(
                "the clarification result has an empty explanation",
            ));
        }
        Ok(Self {
            question_node_id: result.question_node_id,
            question: question.to_owned(),
            explanation_node_id: result.explanation_node_id,
            explanation: result.explanation_payload.content,
            citations: result.citations,
            grounding: result.grounding,
        })
    }
}

/// Build a `POST /clarify` request from a parse result.
///
/// `source.payload_hash` is the parse payload hash and the target comes from
/// the matching `spans` entry. `turns` is empty on the first turn. The question
/// is sent unchanged; only a blank question is refused. `project_id` is the
/// project selected when the request is built.
pub fn build_clarify_request(
    parse: &ParseResult,
    display_id: u32,
    turns: &[Turn],
    question: &str,
    project_id: Option<&str>,
) -> Result<ClarifyRequest, ApiError> {
    if question.trim().is_empty() {
        return Err(ApiError::local("Enter a question before asking."));
    }
    let span = parse
        .spans
        .iter()
        .find(|span| span.display_id == display_id)
        .ok_or_else(|| {
            ApiError::local(format!(
                "#{display_id} is not a span of the current result."
            ))
        })?;
    Ok(ClarifyRequest {
        source: ClarifySource {
            text: parse.payload.content.clone(),
            payload_hash: parse.payload.hash.clone(),
            metadata: parse.source.clone(),
        },
        target: ClarifyTarget {
            node_id: span.node_id.clone(),
            display_id: span.display_id,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
        },
        turns: turns
            .iter()
            .map(|turn| ClarifyTurn {
                question_node_id: turn.question_node_id.clone(),
                question: turn.question.clone(),
                explanation_node_id: turn.explanation_node_id.clone(),
                explanation: turn.explanation.clone(),
            })
            .collect(),
        question: question.to_owned(),
        author: None,
        project_id: project_id.map(str::to_owned),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    token: u64,
    question: String,
}

/// The conversation about one span.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branch {
    turns: Vec<Turn>,
    pending: Option<Pending>,
    error: Option<ApiError>,
    /// Unsent follow-up text, kept while another branch is open.
    pub draft: String,
    /// Bumped on every change the panel shows, so it rebuilds only when needed.
    revision: u64,
}

impl Branch {
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn error(&self) -> Option<&ApiError> {
        self.error.as_ref()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the source card should carry a branch marker.
    pub fn is_active(&self) -> bool {
        !self.turns.is_empty() || self.pending.is_some()
    }
}

/// All branches of the current parse result, keyed by span `#N`.
#[derive(Debug, Default)]
pub struct Branches {
    branches: BTreeMap<u32, Branch>,
    next_token: u64,
}

impl Branches {
    /// Discard every branch (a new parse). Tokens stay unique, so a reply to a
    /// request made for the old result can never land in a new branch.
    pub fn clear(&mut self) {
        self.branches.clear();
    }

    pub fn get(&self, display_id: u32) -> Option<&Branch> {
        self.branches.get(&display_id)
    }

    pub fn any_pending(&self) -> bool {
        self.branches.values().any(Branch::is_pending)
    }

    /// `(turn count, pending)` for every branch with content, for card markers.
    pub fn marks(&self) -> BTreeMap<u32, BranchMark> {
        self.branches
            .iter()
            .filter(|(_, branch)| branch.is_active())
            .map(|(id, branch)| {
                (
                    *id,
                    BranchMark {
                        turns: branch.turns.len(),
                        pending: branch.is_pending(),
                    },
                )
            })
            .collect()
    }

    pub fn set_draft(&mut self, display_id: u32, draft: String) {
        if draft.is_empty() && !self.branches.contains_key(&display_id) {
            return;
        }
        self.branches.entry(display_id).or_default().draft = draft;
    }

    pub fn draft(&self, display_id: u32) -> String {
        self.get(display_id)
            .map(|branch| branch.draft.clone())
            .unwrap_or_default()
    }

    /// Start a turn: returns the request token and the request to send, or
    /// `None` when the branch already has a request in flight or the request
    /// cannot be built (the reason is then shown in the branch).
    pub fn begin(
        &mut self,
        parse: &ParseResult,
        display_id: u32,
        question: &str,
        project_id: Option<&str>,
    ) -> Option<(u64, ClarifyRequest)> {
        let branch = self.branches.entry(display_id).or_default();
        if branch.pending.is_some() {
            return None;
        }
        match build_clarify_request(parse, display_id, &branch.turns, question, project_id) {
            Ok(request) => {
                self.next_token = self.next_token.wrapping_add(1);
                branch.pending = Some(Pending {
                    token: self.next_token,
                    question: question.to_owned(),
                });
                branch.error = None;
                branch.revision += 1;
                Some((self.next_token, request))
            }
            Err(error) => {
                branch.error = Some(error);
                branch.revision += 1;
                None
            }
        }
    }

    /// Apply a reply. Returns whether it was current; replies for a discarded
    /// branch or an older request are ignored. A failure keeps earlier turns.
    pub fn finish(
        &mut self,
        display_id: u32,
        token: u64,
        outcome: Result<ClarifyResult, ApiError>,
    ) -> bool {
        let Some(branch) = self.branches.get_mut(&display_id) else {
            return false;
        };
        let Some(pending) = branch.pending.take_if(|pending| pending.token == token) else {
            return false;
        };
        branch.revision += 1;
        match outcome.and_then(|result| Turn::from_result(&pending.question, result)) {
            Ok(turn) => {
                branch.turns.push(turn);
                branch.error = None;
                if branch.draft == pending.question {
                    branch.draft.clear();
                }
            }
            Err(error) => branch.error = Some(error),
        }
        true
    }
}

/// What a source card needs to show its branch marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchMark {
    pub turns: usize,
    pub pending: bool,
}

/// One citation as a panel line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationLine {
    /// `[n]`, matching the marker in the explanation.
    pub marker: String,
    pub text: String,
}

impl CitationLine {
    pub fn display(&self) -> String {
        format!("{} {}", self.marker, self.text)
    }
}

/// One turn, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnView {
    pub question: String,
    /// Plain text with the model's `[n]` markers untouched.
    pub explanation: String,
    pub citations: Vec<CitationLine>,
    pub grounding: String,
    /// Oddities worth showing, such as a marker with no citation.
    pub notes: Vec<String>,
}

/// The whole panel content for one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchView {
    pub display_id: u32,
    pub target_preview: String,
    pub turns: Vec<TurnView>,
}

/// Build the branch view model against the parse result it belongs to.
pub fn build_branch_view(display_id: u32, turns: &[Turn], parse: &ParseResult) -> BranchView {
    let content = &parse.payload.content;
    let target_preview = parse
        .spans
        .iter()
        .find(|span| span.display_id == display_id)
        .and_then(|span| slice(content, span.start_byte, span.end_byte))
        .map(|text| {
            format!(
                "#{display_id} · {}",
                truncate_for_display(text, PREVIEW_CHARS)
            )
        })
        .unwrap_or_else(|| format!("#{display_id} · source text unavailable"));
    BranchView {
        display_id,
        target_preview,
        turns: turns.iter().map(|turn| turn_view(turn, parse)).collect(),
    }
}

fn turn_view(turn: &Turn, parse: &ParseResult) -> TurnView {
    let mut notes = Vec::new();
    let citations: Vec<CitationLine> = turn
        .citations
        .iter()
        .map(|citation| citation_line(citation, parse))
        .collect();
    for marker in markers_in(&turn.explanation) {
        if !turn
            .citations
            .iter()
            .any(|citation| citation.index == marker)
        {
            notes.push(format!("[{marker}] is cited in the text but not listed."));
        }
    }
    let grounding = match turn.grounding {
        Grounding::Project => "Project-grounded: file citations come from the API's project tools",
        Grounding::TextOnly => "Text only: no project files were available",
    };
    TurnView {
        question: turn.question.clone(),
        explanation: turn.explanation.clone(),
        citations,
        grounding: grounding.to_owned(),
        notes,
    }
}

fn citation_line(citation: &CitationInfo, parse: &ParseResult) -> CitationLine {
    let marker = format!("[{}]", citation.index);
    let text =
        match &citation.location {
            CitationLocation::File {
                path,
                start_line,
                end_line,
            } => {
                if *start_line == 0 || end_line < start_line {
                    format!("{path} (line range unavailable)")
                } else if start_line == end_line {
                    format!("{path}:L{start_line}")
                } else {
                    format!("{path}:L{start_line}–L{end_line}")
                }
            }
            CitationLocation::Source {
                start_byte,
                end_byte,
            } => {
                match slice(&parse.payload.content, *start_byte, *end_byte) {
                    Some(text) => {
                        let preview = truncate_for_display(text, PREVIEW_CHARS);
                        // Only a range wholly inside a span is attributed to it.
                        match parse.spans.iter().find(|span| {
                            span.start_byte <= *start_byte && *end_byte <= span.end_byte
                        }) {
                            Some(span) => format!("#{} · {preview}", span.display_id),
                            None => format!("source · {preview}"),
                        }
                    }
                    None => format!("source bytes {start_byte}–{end_byte} (unavailable)"),
                }
            }
            CitationLocation::Commit { hash, subject } => {
                format!("commit {} {subject}", hash.get(..7).unwrap_or(hash))
            }
        };
    CitationLine { marker, text }
}

/// The text at `[start, end)`, or `None` when the range is empty, out of
/// bounds, or off a `char` boundary.
fn slice(content: &str, start: usize, end: usize) -> Option<&str> {
    (start < end && end <= content.len()).then_some(())?;
    content.get(start..end)
}

/// Citation numbers written as `[n]` in `text`, in order, without repeats.
fn markers_in(text: &str) -> Vec<u32> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find(']') else { break };
        let inner = &rest[..close];
        if !inner.is_empty()
            && inner.len() <= 6
            && inner.bytes().all(|byte| byte.is_ascii_digit())
            && let Ok(number) = inner.parse::<u32>()
            && !found.contains(&number)
        {
            found.push(number);
        }
    }
    found
}

/// How a panel block is styled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Target,
    Question,
    Explanation,
    Citation,
    Meta,
    Notice,
    Error,
}

/// One text block of the panel, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelBlock {
    pub kind: BlockKind,
    pub text: String,
}

/// Flatten a branch into display blocks: target, turns in order, then the
/// in-flight or error state.
pub fn panel_blocks(view: &BranchView, branch: Option<&Branch>) -> Vec<PanelBlock> {
    let block = |kind, text: &str| PanelBlock {
        kind,
        text: text.to_owned(),
    };
    let mut blocks = vec![block(BlockKind::Target, &view.target_preview)];
    if view.turns.is_empty() && !branch.is_some_and(Branch::is_pending) {
        blocks.push(block(
            BlockKind::Notice,
            "Ask a question about this span. The explanation cites project files \
             when the API has a project root, and the source text otherwise.",
        ));
    }
    for turn in &view.turns {
        blocks.push(block(BlockKind::Question, &format!("Q: {}", turn.question)));
        blocks.push(block(BlockKind::Explanation, &turn.explanation));
        for line in &turn.citations {
            blocks.push(block(BlockKind::Citation, &line.display()));
        }
        for note in &turn.notes {
            blocks.push(block(BlockKind::Meta, note));
        }
        blocks.push(block(BlockKind::Meta, &turn.grounding));
    }
    if let Some(branch) = branch {
        if let Some(pending) = &branch.pending {
            blocks.push(block(
                BlockKind::Question,
                &format!("Q: {}", pending.question),
            ));
            blocks.push(block(
                BlockKind::Notice,
                "Explaining… this can take a while. The window stays responsive.",
            ));
        }
        if let Some(error) = &branch.error {
            blocks.push(block(BlockKind::Error, &clarify_error_text(error)));
        }
    }
    blocks
}

/// Error text for the panel. Earlier turns stay visible above it.
pub fn clarify_error_text(error: &ApiError) -> String {
    let headline = match error.kind.as_str() {
        "invalid_request" => "Nothing was sent",
        "payload_mismatch" => "The source text no longer matches the parse result",
        "invalid_target" => "The span cannot be explained",
        "invalid_output" => "The Explainer could not produce a valid explanation",
        "provider_error" => "The model provider failed",
        "timeout" => "The model call timed out",
        "oversized_input" => "The text is too large",
        "internal_error" => "The API had an internal error",
        "connection" => "Could not reach the API",
        "decode" => "The API response could not be read",
        _ => "Clarification failed",
    };
    let mut text = format!("{headline}: {}", error.detail);
    if let Some(attempts) = error.attempts {
        text.push_str(&format!(" ({attempts} attempt(s))"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use daftprompt_types::{Payload, Run, RunStatus, SourceKind, SourceMetadata, SpanInfo};

    const TEXT: &str = "We use a token bucket.\nIt refills slowly.\nThe limit is per user.";

    fn payload(content: &str) -> Payload {
        Payload {
            hash: "h".to_owned(),
            content: content.to_owned(),
            created_at: "now".to_owned(),
        }
    }

    fn run(agent: &str) -> Run {
        Run {
            id: "run".to_owned(),
            agent: agent.to_owned(),
            provider: None,
            model: None,
            prompt_version: None,
            started_at: "now".to_owned(),
            ended_at: None,
            status: RunStatus::Done,
            attempts: 1,
            consistency_followup: false,
            consistency_used: false,
            project_id: None,
        }
    }

    fn parse_result() -> ParseResult {
        ParseResult {
            content_type: daftprompt_types::ContentType::Plain,
            content_type_source: daftprompt_types::ContentTypeSource::Detected,
            payload: Payload {
                hash: "parse-hash".to_owned(),
                ..payload(TEXT)
            },
            source: Some(SourceMetadata {
                kind: Some(SourceKind::Chat),
                reference: Some("thread-1".to_owned()),
                author: Some("ada".to_owned()),
            }),
            nodes: Vec::new(),
            node_contexts: Vec::new(),
            labels: Vec::new(),
            spans: vec![
                SpanInfo {
                    display_id: 1,
                    node_id: "node-s1".to_owned(),
                    start_byte: 7,
                    end_byte: 22,
                },
                SpanInfo {
                    display_id: 2,
                    node_id: "node-s2".to_owned(),
                    start_byte: 23,
                    end_byte: 41,
                },
            ],
            run: run("scribe"),
            run_inputs: Vec::new(),
            tool_calls: Vec::new(),
        }
    }

    fn clarify_result(explanation: &str, citations: Vec<CitationInfo>) -> ClarifyResult {
        ClarifyResult {
            question_payload: payload("Why?"),
            explanation_payload: payload(explanation),
            tool_payloads: Vec::new(),
            question_node_id: "node-q1".to_owned(),
            explanation_node_id: "node-e1".to_owned(),
            nodes: Vec::new(),
            node_contexts: Vec::new(),
            labels: Vec::new(),
            citations,
            grounding: Grounding::TextOnly,
            source_window: None,
            run: run("explainer"),
            run_inputs: Vec::new(),
            tool_calls: Vec::new(),
        }
    }

    fn source_citation(index: u32, start: usize, end: usize) -> CitationInfo {
        CitationInfo {
            index,
            context_id: format!("ctx-{index}"),
            location: CitationLocation::Source {
                start_byte: start,
                end_byte: end,
            },
        }
    }

    #[test]
    fn first_turn_request_uses_the_parse_payload_and_span() {
        let parse = parse_result();
        let request = build_clarify_request(&parse, 1, &[], "Why a bucket?", None).unwrap();
        assert_eq!(request.source.text, TEXT);
        assert_eq!(request.source.payload_hash, "parse-hash");
        assert_eq!(request.source.metadata, parse.source);
        assert_eq!(request.target.node_id, "node-s1");
        assert_eq!(request.target.display_id, 1);
        assert_eq!(
            (request.target.start_byte, request.target.end_byte),
            (7, 22)
        );
        assert!(request.turns.is_empty());
        assert_eq!(request.question, "Why a bucket?");
        let json = serde_json::to_value(&request).unwrap();
        assert!(json.get("turns").is_none(), "turns are omitted on turn one");
    }

    #[test]
    fn follow_up_request_carries_previous_node_ids_in_order() {
        let parse = parse_result();
        let first = Turn::from_result("Why?", clarify_result("Because [1].", Vec::new())).unwrap();
        let mut second = first.clone();
        second.question_node_id = "node-q2".to_owned();
        second.explanation_node_id = "node-e2".to_owned();
        second.question = "And?".to_owned();
        let request =
            build_clarify_request(&parse, 2, &[first, second], "Anything else?", None).unwrap();
        assert_eq!(request.turns.len(), 2);
        assert_eq!(request.turns[0].question_node_id, "node-q1");
        assert_eq!(request.turns[0].explanation_node_id, "node-e1");
        assert_eq!(request.turns[0].question, "Why?");
        assert_eq!(request.turns[0].explanation, "Because [1].");
        assert_eq!(request.turns[1].explanation_node_id, "node-e2");
        assert_eq!(request.target.node_id, "node-s2");
    }

    #[test]
    fn blank_questions_and_unknown_spans_are_refused_locally() {
        let parse = parse_result();
        let blank = build_clarify_request(&parse, 1, &[], "  \n", None).unwrap_err();
        assert_eq!(blank.kind, "invalid_request");
        let unknown = build_clarify_request(&parse, 9, &[], "Why?", None).unwrap_err();
        assert!(unknown.detail.contains("#9"));
    }

    #[test]
    fn requests_carry_the_project_selected_when_they_are_built() {
        let parse = parse_result();
        let none = build_clarify_request(&parse, 1, &[], "Why?", None).unwrap();
        assert_eq!(none.project_id, None);
        let mut branches = Branches::default();
        let (_, first) = branches.begin(&parse, 1, "Why?", Some("p1")).unwrap();
        let (_, second) = branches.begin(&parse, 2, "Why?", Some("p2")).unwrap();
        assert_eq!(first.project_id.as_deref(), Some("p1"));
        assert_eq!(second.project_id.as_deref(), Some("p2"));
    }

    #[test]
    fn a_branch_allows_one_request_at_a_time() {
        let parse = parse_result();
        let mut branches = Branches::default();
        let (token, _) = branches
            .begin(&parse, 1, "Why?", None)
            .expect("first request");
        assert!(branches.begin(&parse, 1, "Again?", None).is_none());
        assert!(branches.get(1).unwrap().error().is_none());
        // Another span's branch is independent.
        assert!(branches.begin(&parse, 2, "Why?", None).is_some());
        assert!(branches.finish(1, token, Ok(clarify_result("Because.", Vec::new()))));
        assert!(branches.begin(&parse, 1, "Again?", None).is_some());
    }

    #[test]
    fn replies_for_stale_tokens_or_discarded_branches_are_ignored() {
        let parse = parse_result();
        let mut branches = Branches::default();
        let (token, _) = branches.begin(&parse, 1, "Why?", None).unwrap();
        assert!(!branches.finish(1, token + 100, Ok(clarify_result("x", Vec::new()))));
        branches.clear();
        assert!(!branches.finish(1, token, Ok(clarify_result("x", Vec::new()))));
        assert!(branches.get(1).is_none());
        // A new branch with the same `#N` never accepts the old token.
        let (new_token, _) = branches.begin(&parse, 1, "Why?", None).unwrap();
        assert_ne!(token, new_token);
        assert!(!branches.finish(1, token, Ok(clarify_result("x", Vec::new()))));
        assert!(branches.get(1).unwrap().is_pending());
    }

    #[test]
    fn failures_keep_earlier_turns_and_the_draft() {
        let parse = parse_result();
        let mut branches = Branches::default();
        let (token, _) = branches.begin(&parse, 1, "Why?", None).unwrap();
        branches.finish(1, token, Ok(clarify_result("Because.", Vec::new())));
        branches.set_draft(1, "next question".to_owned());
        let (token, request) = branches.begin(&parse, 1, "next question", None).unwrap();
        assert_eq!(request.turns.len(), 1);
        branches.finish(1, token, Err(ApiError::new("timeout", "too slow")));
        let branch = branches.get(1).unwrap();
        assert_eq!(branch.turns().len(), 1);
        assert_eq!(branch.error().unwrap().kind, "timeout");
        assert_eq!(branch.draft, "next question");
        assert!(!branch.is_pending());
        let marks = branches.marks();
        assert_eq!(marks[&1].turns, 1);
    }

    #[test]
    fn malformed_results_become_errors_not_turns() {
        let parse = parse_result();
        let mut branches = Branches::default();
        let (token, _) = branches.begin(&parse, 1, "Why?", None).unwrap();
        let blank = clarify_result("  ", Vec::new());
        branches.finish(1, token, Ok(blank));
        let branch = branches.get(1).unwrap();
        assert!(branch.turns().is_empty());
        assert_eq!(branch.error().unwrap().kind, "decode");

        let mut no_ids = clarify_result("Fine.", Vec::new());
        no_ids.explanation_node_id.clear();
        assert!(Turn::from_result("Why?", no_ids).is_err());
    }

    #[test]
    fn citations_render_files_and_source_ranges() {
        let parse = parse_result();
        let citations = vec![
            CitationInfo {
                index: 1,
                context_id: "c1".to_owned(),
                location: CitationLocation::File {
                    path: "src/limiter.rs".to_owned(),
                    start_line: 12,
                    end_line: 30,
                },
            },
            source_citation(2, 9, 21),
            source_citation(3, 0, 22),
            CitationInfo {
                index: 4,
                context_id: "c4".to_owned(),
                location: CitationLocation::File {
                    path: "a.rs".to_owned(),
                    start_line: 5,
                    end_line: 5,
                },
            },
        ];
        let turn = Turn::from_result(
            "Why?",
            clarify_result("It limits [1] and refills [2], per [3].", citations),
        )
        .unwrap();
        let view = build_branch_view(1, &[turn], &parse);
        assert_eq!(view.target_preview, "#1 · a token bucket.");
        let lines: Vec<String> = view.turns[0]
            .citations
            .iter()
            .map(|l| l.display())
            .collect();
        assert_eq!(lines[0], "[1] src/limiter.rs:L12–L30");
        assert_eq!(lines[1], "[2] #1 · token bucket");
        assert!(lines[2].starts_with("[3] source · We use a token bucket."));
        assert_eq!(lines[3], "[4] a.rs:L5");
        assert_eq!(
            view.turns[0].grounding,
            "Text only: no project files were available"
        );
        assert!(view.turns[0].notes.is_empty());
    }

    #[test]
    fn bad_citations_and_missing_markers_degrade_gracefully() {
        let parse = parse_result();
        let citations = vec![
            source_citation(1, 5, 5),
            source_citation(2, 0, 9999),
            CitationInfo {
                index: 3,
                context_id: "c".to_owned(),
                location: CitationLocation::File {
                    path: "x.rs".to_owned(),
                    start_line: 0,
                    end_line: 0,
                },
            },
        ];
        let turn =
            Turn::from_result("Why?", clarify_result("See [7] and [1].", citations)).unwrap();
        let view = build_branch_view(99, &[turn], &parse);
        assert_eq!(view.target_preview, "#99 · source text unavailable");
        let t = &view.turns[0];
        assert!(t.citations[0].text.contains("unavailable"));
        assert!(t.citations[1].text.contains("unavailable"));
        assert!(t.citations[2].text.contains("line range unavailable"));
        assert_eq!(t.notes, ["[7] is cited in the text but not listed."]);
    }

    #[test]
    fn unicode_boundaries_do_not_panic() {
        let mut parse = parse_result();
        parse.payload.content = "é is odd".to_owned();
        parse.spans = vec![SpanInfo {
            display_id: 1,
            node_id: "n".to_owned(),
            start_byte: 1,
            end_byte: 4,
        }];
        let view = build_branch_view(1, &[], &parse);
        assert_eq!(view.target_preview, "#1 · source text unavailable");
    }

    #[test]
    fn panel_blocks_follow_turn_order_then_state() {
        let parse = parse_result();
        let mut branches = Branches::default();
        let (token, _) = branches.begin(&parse, 1, "Why?", None).unwrap();
        let blocks = {
            let branch = branches.get(1);
            let view = build_branch_view(1, &[], &parse);
            panel_blocks(&view, branch)
        };
        assert_eq!(blocks[0].kind, BlockKind::Target);
        assert_eq!(blocks[1].text, "Q: Why?");
        assert_eq!(blocks[2].kind, BlockKind::Notice);

        branches.finish(
            1,
            token,
            Ok(clarify_result(
                "Because [1].",
                vec![source_citation(1, 7, 22)],
            )),
        );
        let (token, _) = branches.begin(&parse, 1, "And then?", None).unwrap();
        branches.finish(
            1,
            token,
            Err(ApiError::new("payload_mismatch", "hash differs")),
        );
        let branch = branches.get(1).unwrap();
        let view = build_branch_view(1, branch.turns(), &parse);
        let kinds: Vec<BlockKind> = panel_blocks(&view, Some(branch))
            .iter()
            .map(|b| b.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                BlockKind::Target,
                BlockKind::Question,
                BlockKind::Explanation,
                BlockKind::Citation,
                BlockKind::Meta,
                BlockKind::Error
            ]
        );
        let text = clarify_error_text(branch.error().unwrap());
        assert!(text.starts_with("The source text no longer matches"));
    }

    #[test]
    fn markers_are_found_once_in_order() {
        assert_eq!(markers_in("a [2] b [1] c [2] [x] [] [ 3]"), [2, 1]);
        assert!(markers_in("no markers [").is_empty());
    }
}
