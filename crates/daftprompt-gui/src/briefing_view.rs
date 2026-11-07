//! View model for the Project tab, the command footer and the details area.
//!
//! Pure logic: it turns [`Briefing`] slots into flat lists of [`Block`]s that
//! `briefing_ui.rs` lays out and paints, and holds the footer's overflow math
//! and the command details text. Summaries are Markdown; only a small subset
//! is rendered (see [`markdown_blocks`]). Citations reuse the Epic 004 line
//! format.

use daftprompt_types::{
    BriefingNotice, BriefingNoticeKind, BriefingResult, CitationInfo, CitationLocation,
    CommandKind, CommandOrigin, CommandsResult, ProjectCommand,
};

use crate::api::ApiError;
use crate::briefing::{Briefing, Section, Slot};
use crate::clarify::file_range_text;

/// What clicking a button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Rerun(Section),
    OpenCommand(String),
    /// The footer's "+N more": show every command as a list.
    OpenList,
    CloseDetails,
}

/// A button on a title row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleButton {
    pub label: String,
    pub action: Action,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// A section or panel title, with an optional button at its right end.
    Title(Option<TitleButton>),
    /// A Markdown heading inside a summary.
    Heading,
    /// A Markdown paragraph.
    Body,
    /// A bullet item (the bullet and its indent are part of the text).
    Bullet,
    /// A fenced code block or a command, in the monospace face.
    Code,
    /// A citation line.
    Citation,
    /// A non-fatal remark such as "Not a git repository".
    Notice,
    /// A loading or empty state.
    Muted,
    /// An error, in the error colour.
    Error,
    /// A small caption above a value in the details area.
    Field,
    /// A clickable row of the command list.
    Row(Action),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
}

fn block(kind: BlockKind, text: impl Into<String>) -> Block {
    Block {
        kind,
        text: text.into(),
    }
}

/// The section's title text.
pub fn section_title(section: Section) -> &'static str {
    match section {
        Section::History => "Recent work",
        Section::Overview => "Project overview",
        Section::Commands => "Commands",
    }
}

/// Error text for a briefing slot or the footer.
pub fn error_text(error: &ApiError) -> String {
    match error.kind.as_str() {
        "invalid_output" => format!(
            "The agent could not produce a valid briefing: {}",
            error.detail
        ),
        _ if error.headline() == "Parsing failed" => {
            format!("The briefing failed: {}", error.detail)
        }
        _ => error.to_string(),
    }
}

/// A notice as one line.
pub fn notice_text(notice: &BriefingNotice) -> String {
    let label = match notice.kind {
        BriefingNoticeKind::NotAGitRepository => "Not a git repository",
        BriefingNoticeKind::GitUnavailable => "Git is unavailable",
        BriefingNoticeKind::Truncated => "Some output was truncated",
    };
    if notice.detail.trim().is_empty() {
        label.to_owned()
    } else {
        format!("{label}: {}", notice.detail.trim())
    }
}

/// `[n] path:L1–L9`, or `[n] commit abc1234 subject`.
pub fn citation_text(citation: &CitationInfo) -> String {
    let location = match &citation.location {
        CitationLocation::File {
            path,
            start_line,
            end_line,
        } => file_range_text(path, *start_line, *end_line),
        CitationLocation::Commit { hash, subject } => {
            format!("commit {} {subject}", hash.get(..7).unwrap_or(hash))
        }
        CitationLocation::Source {
            start_byte,
            end_byte,
        } => format!("source bytes {start_byte}–{end_byte}"),
    };
    format!("[{}] {location}", citation.index)
}

/// Render the supported Markdown subset as blocks:
///
/// - `#` to `######` headings, one heading style;
/// - paragraphs (consecutive lines joined with a space);
/// - `-`, `*`, `+` and `1.` list items, with two-space indents shown as nested
///   items and indented continuation lines joined to their item;
/// - fenced code blocks, verbatim in the monospace face;
/// - `**bold**` markers removed. Other inline markup (backticks, links,
///   emphasis, tables) is shown as written.
pub fn markdown_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut bullet: Option<String> = None;
    let mut code: Option<Vec<&str>> = None;

    fn flush(blocks: &mut Vec<Block>, paragraph: &mut String, bullet: &mut Option<String>) {
        if !paragraph.is_empty() {
            blocks.push(block(BlockKind::Body, strip_bold(paragraph)));
            paragraph.clear();
        }
        if let Some(text) = bullet.take() {
            blocks.push(block(BlockKind::Bullet, strip_bold(&text)));
        }
    }

    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if let Some(lines) = &mut code {
            if trimmed.trim_start().starts_with("```") {
                let text = lines.join("\n");
                if !text.trim().is_empty() {
                    blocks.push(block(BlockKind::Code, text));
                }
                code = None;
            } else {
                lines.push(trimmed);
            }
            continue;
        }
        let start = trimmed.trim_start();
        if start.starts_with("```") {
            flush(&mut blocks, &mut paragraph, &mut bullet);
            code = Some(Vec::new());
        } else if start.is_empty() {
            flush(&mut blocks, &mut paragraph, &mut bullet);
        } else if let Some(heading) = heading_text(start) {
            flush(&mut blocks, &mut paragraph, &mut bullet);
            blocks.push(block(BlockKind::Heading, strip_bold(heading)));
        } else if let Some(item) = list_item(start) {
            flush(&mut blocks, &mut paragraph, &mut bullet);
            let indent = (trimmed.len() - start.len()) / 2;
            let (marker, pad) = if indent == 0 {
                ("•", String::new())
            } else {
                ("–", "    ".repeat(indent.min(3)))
            };
            bullet = Some(format!("{pad}{marker} {item}"));
        } else if let Some(text) = &mut bullet {
            text.push(' ');
            text.push_str(start);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(start);
        }
    }
    if let Some(lines) = code {
        // An unterminated fence still shows what it held.
        let text = lines.join("\n");
        if !text.trim().is_empty() {
            blocks.push(block(BlockKind::Code, text));
        }
    }
    flush(&mut blocks, &mut paragraph, &mut bullet);
    blocks
}

fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        let rest = line[hashes..].strip_prefix(' ')?;
        let rest = rest.trim().trim_end_matches('#').trim_end();
        (!rest.is_empty()).then_some(rest)
    } else {
        None
    }
}

fn list_item(line: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim());
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits)
        && let Some(rest) = line[digits..].strip_prefix(". ")
    {
        return Some(rest.trim());
    }
    None
}

fn strip_bold(text: &str) -> String {
    text.replace("**", "")
}

fn summary_blocks(blocks: &mut Vec<Block>, result: &BriefingResult) {
    for notice in &result.notices {
        blocks.push(block(BlockKind::Notice, notice_text(notice)));
    }
    blocks.extend(markdown_blocks(&result.summary_payload.content));
    for citation in &result.citations {
        blocks.push(block(BlockKind::Citation, citation_text(citation)));
    }
}

fn section_blocks(
    blocks: &mut Vec<Block>,
    section: Section,
    slot: &Slot<BriefingResult>,
    loading: &str,
) {
    let busy = slot.is_loading();
    blocks.push(block(
        BlockKind::Title(Some(TitleButton {
            label: if busy { "Running…" } else { "Re-run" }.to_owned(),
            action: Action::Rerun(section),
            enabled: !busy,
        })),
        section_title(section),
    ));
    match slot {
        Slot::Idle => {}
        Slot::Loading => blocks.push(block(BlockKind::Muted, loading)),
        Slot::Failed(error) => blocks.push(block(BlockKind::Error, error_text(error))),
        Slot::Ready(result) => summary_blocks(blocks, result),
    }
}

/// The Project tab: "Recent work" then "Project overview". Empty without a
/// briefing (the caller shows the hint).
pub fn project_blocks(briefing: &Briefing) -> Vec<Block> {
    let mut blocks = Vec::new();
    section_blocks(
        &mut blocks,
        Section::History,
        &briefing.history,
        "Reading recent git history…",
    );
    section_blocks(
        &mut blocks,
        Section::Overview,
        &briefing.overview,
        "Surveying the project…",
    );
    blocks
}

/// What the footer shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterStatus<'a> {
    /// No project: nothing to show.
    Hidden,
    Loading,
    Failed(String),
    Empty,
    Commands(&'a [ProjectCommand]),
}

pub fn footer_status(briefing: Option<&Briefing>) -> FooterStatus<'_> {
    match briefing.map(|b| &b.commands) {
        None | Some(Slot::Idle) => FooterStatus::Hidden,
        Some(Slot::Loading) => FooterStatus::Loading,
        Some(Slot::Failed(error)) => {
            FooterStatus::Failed(crate::view::truncate_for_display(&error_text(error), 110))
        }
        Some(Slot::Ready(result)) if result.commands.is_empty() => FooterStatus::Empty,
        Some(Slot::Ready(result)) => FooterStatus::Commands(&result.commands),
    }
}

/// Estimated width of a footer button for `label`.
pub fn button_width(label: &str) -> f32 {
    (label.chars().count() as f32 * 7.4 + 28.0).clamp(64.0, 220.0)
}

/// The "+N more" button's text.
pub fn more_label(hidden: usize) -> String {
    format!("+{hidden} more")
}

/// How many buttons fit in one row of `available` width, leaving room for a
/// "+N more" button when some do not. Never wraps.
pub fn visible_buttons(widths: &[f32], available: f32, gap: f32) -> usize {
    let count = widths.len();
    if count == 0 {
        return 0;
    }
    let all: f32 = widths.iter().sum::<f32>() + gap * (count - 1) as f32;
    if all <= available {
        return count;
    }
    for keep in (0..count).rev() {
        let used: f32 = widths[..keep].iter().map(|w| w + gap).sum();
        if used + button_width(&more_label(count - keep)) <= available {
            return keep;
        }
    }
    0
}

pub fn kind_name(kind: CommandKind) -> &'static str {
    match kind {
        CommandKind::Setup => "setup",
        CommandKind::Build => "build",
        CommandKind::Run => "run",
        CommandKind::Dev => "dev",
        CommandKind::Test => "test",
        CommandKind::Lint => "lint",
        CommandKind::Format => "format",
        CommandKind::Other => "other",
    }
}

pub fn origin_name(origin: CommandOrigin) -> &'static str {
    match origin {
        CommandOrigin::Detected => "detected from a manifest",
        CommandOrigin::Documented => "documented in the project",
    }
}

/// The text of one command's details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDetails {
    pub label: String,
    pub command: String,
    pub working_dir: String,
    pub kind: &'static str,
    pub origin: &'static str,
    pub evidence: Vec<String>,
}

pub fn command_details(command: &ProjectCommand, citations: &[CitationInfo]) -> CommandDetails {
    let evidence = command
        .citation_indexes
        .iter()
        .map(|index| {
            citations
                .iter()
                .find(|citation| citation.index == *index)
                .map_or_else(
                    || format!("[{index}] (citation unavailable)"),
                    citation_text,
                )
        })
        .collect();
    CommandDetails {
        label: command.label.clone(),
        command: command.command.clone(),
        working_dir: if command.working_dir.trim().is_empty() {
            "project root".to_owned()
        } else {
            command.working_dir.clone()
        },
        kind: kind_name(command.kind),
        origin: origin_name(command.origin),
        evidence,
    }
}

fn close_title(text: impl Into<String>) -> Block {
    block(
        BlockKind::Title(Some(TitleButton {
            label: "Close".to_owned(),
            action: Action::CloseDetails,
            enabled: true,
        })),
        text,
    )
}

/// The details area for one command.
pub fn details_blocks(details: &CommandDetails) -> Vec<Block> {
    let mut blocks = vec![
        close_title(details.label.clone()),
        block(BlockKind::Field, "Command"),
        block(BlockKind::Code, details.command.clone()),
        block(BlockKind::Field, "Working directory"),
        block(BlockKind::Body, details.working_dir.clone()),
        block(BlockKind::Field, "Kind"),
        block(BlockKind::Body, details.kind),
        block(BlockKind::Field, "Origin"),
        block(BlockKind::Body, details.origin),
        block(BlockKind::Field, "Evidence"),
    ];
    if details.evidence.is_empty() {
        blocks.push(block(BlockKind::Muted, "No citations."));
    }
    blocks.extend(
        details
            .evidence
            .iter()
            .map(|line| block(BlockKind::Citation, line.clone())),
    );
    blocks.push(block(
        BlockKind::Muted,
        "daftprompt never runs or copies commands.",
    ));
    blocks
}

/// The details area as a list of every command (the "+N more" button).
pub fn list_blocks(result: &CommandsResult) -> Vec<Block> {
    let mut blocks = vec![close_title(format!(
        "All commands ({})",
        result.commands.len()
    ))];
    blocks.extend(result.commands.iter().map(|command| {
        block(
            BlockKind::Row(Action::OpenCommand(command.id.clone())),
            format!("{}\n{}", command.label, command.command),
        )
    }));
    blocks
}

/// The blocks for the open details area, or `None` when its command is gone
/// (the caller then closes it).
pub fn details_view(
    briefing: Option<&Briefing>,
    details: &crate::briefing::Details,
) -> Option<Vec<Block>> {
    let Slot::Ready(result) = &briefing?.commands else {
        return None;
    };
    match details {
        crate::briefing::Details::List => Some(list_blocks(result)),
        crate::briefing::Details::Command(id) => {
            let command = result.commands.iter().find(|c| &c.id == id)?;
            Some(details_blocks(&command_details(
                command,
                &result.briefing.citations,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::briefing::tests::briefing_result;
    use crate::briefing::{BriefingMessage, BriefingState, Details, Reply};

    fn kinds(blocks: &[Block]) -> Vec<&BlockKind> {
        blocks.iter().map(|b| &b.kind).collect()
    }

    fn file_citation(index: u32, path: &str, start: u32, end: u32) -> CitationInfo {
        CitationInfo {
            index,
            context_id: format!("c{index}"),
            location: CitationLocation::File {
                path: path.into(),
                start_line: start,
                end_line: end,
            },
        }
    }

    fn command(id: &str, label: &str, dir: &str) -> ProjectCommand {
        ProjectCommand {
            id: id.into(),
            label: label.into(),
            command: "cargo test --workspace".into(),
            working_dir: dir.into(),
            kind: CommandKind::Test,
            language: None,
            origin: CommandOrigin::Detected,
            citation_indexes: vec![1, 3],
        }
    }

    #[test]
    fn markdown_headings_paragraphs_and_lists() {
        let md = "# Title\nFirst line\nsecond **line**.\n\n- one\n  more of one\n- two\n  - nested\n1. numbered\n\n## Next\nEnd";
        let blocks = markdown_blocks(md);
        let texts: Vec<_> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Title",
                "First line second line.",
                "• one more of one",
                "• two",
                "    – nested",
                "• numbered",
                "Next",
                "End"
            ]
        );
        assert_eq!(
            kinds(&blocks),
            [
                &BlockKind::Heading,
                &BlockKind::Body,
                &BlockKind::Bullet,
                &BlockKind::Bullet,
                &BlockKind::Bullet,
                &BlockKind::Bullet,
                &BlockKind::Heading,
                &BlockKind::Body
            ]
        );
    }

    #[test]
    fn markdown_code_fences_keep_their_lines() {
        let blocks = markdown_blocks("Run:\n```sh\ncargo test\ncargo fmt\n```\nDone");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].kind, BlockKind::Code);
        assert_eq!(blocks[1].text, "cargo test\ncargo fmt");
        let open = markdown_blocks("```\nunterminated");
        assert_eq!(open[0].text, "unterminated");
        assert!(markdown_blocks("").is_empty());
        // Not a heading without a space; not a list marker mid-word.
        assert_eq!(markdown_blocks("#hash")[0].kind, BlockKind::Body);
    }

    #[test]
    fn citations_use_the_file_and_commit_formats() {
        assert_eq!(
            citation_text(&file_citation(1, "src/a.rs", 3, 9)),
            "[1] src/a.rs:L3–L9"
        );
        assert_eq!(
            citation_text(&file_citation(2, "src/a.rs", 4, 4)),
            "[2] src/a.rs:L4"
        );
        let commit = CitationInfo {
            index: 3,
            context_id: "c".into(),
            location: CitationLocation::Commit {
                hash: "abcdef0123456789".into(),
                subject: "Fix the cache".into(),
            },
        };
        assert_eq!(citation_text(&commit), "[3] commit abcdef0 Fix the cache");
    }

    #[test]
    fn notices_read_as_one_line() {
        let notice = BriefingNotice {
            kind: BriefingNoticeKind::NotAGitRepository,
            detail: String::new(),
        };
        assert_eq!(notice_text(&notice), "Not a git repository");
        let notice = BriefingNotice {
            kind: BriefingNoticeKind::Truncated,
            detail: "log cut at 200".into(),
        };
        assert_eq!(
            notice_text(&notice),
            "Some output was truncated: log cut at 200"
        );
    }

    #[test]
    fn project_blocks_show_loading_errors_and_results() {
        let mut state = BriefingState::default();
        let requests = state.open("p1");
        let blocks = project_blocks(state.briefing().unwrap());
        assert_eq!(blocks[0].text, "Recent work");
        assert_eq!(blocks[1].kind, BlockKind::Muted);
        assert_eq!(blocks[2].text, "Project overview");
        let BlockKind::Title(Some(button)) = &blocks[0].kind else {
            panic!("a title with a button");
        };
        assert!(!button.enabled, "re-run is off while loading");

        let mut result = briefing_result("p1", "# Themes\n- cache work");
        result.notices.push(BriefingNotice {
            kind: BriefingNoticeKind::NotAGitRepository,
            detail: String::new(),
        });
        result.citations.push(file_citation(1, "a.rs", 1, 2));
        state.apply(BriefingMessage {
            request: requests[0].clone(),
            outcome: Ok(Reply::Summary(result)),
        });
        state.apply(BriefingMessage {
            request: requests[1].clone(),
            outcome: Err(ApiError::new("timeout", "took too long")),
        });
        let blocks = project_blocks(state.briefing().unwrap());
        let texts: Vec<_> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts[1], "Not a git repository");
        assert_eq!(texts[2], "Themes");
        assert_eq!(texts[3], "• cache work");
        assert_eq!(texts[4], "[1] a.rs:L1–L2");
        assert_eq!(texts[5], "Project overview");
        assert_eq!(blocks[6].kind, BlockKind::Error);
        let BlockKind::Title(Some(button)) = &blocks[0].kind else {
            panic!()
        };
        assert!(button.enabled);
        assert_eq!(button.action, Action::Rerun(Section::History));
    }

    #[test]
    fn overflow_math_truncates_to_a_more_button() {
        let widths = [100.0, 100.0, 100.0, 100.0];
        // Everything fits: 400 + 3 gaps.
        assert_eq!(visible_buttons(&widths, 424.0, 8.0), 4);
        // Not everything fits: keep the ones that leave room for "+N more".
        let keep = visible_buttons(&widths, 300.0, 8.0);
        assert_eq!(keep, 2);
        let used = 2.0 * 108.0 + button_width(&more_label(2));
        assert!(used <= 300.0);
        // Too narrow even for one button plus "more".
        assert_eq!(visible_buttons(&widths, 120.0, 8.0), 0);
        assert_eq!(visible_buttons(&[], 300.0, 8.0), 0);
        assert_eq!(more_label(3), "+3 more");
    }

    #[test]
    fn command_details_cover_every_field() {
        let citations = vec![
            file_citation(1, "Cargo.toml", 1, 5),
            file_citation(2, "README.md", 1, 2),
        ];
        let details = command_details(&command("c1", "Test", ""), &citations);
        assert_eq!(details.working_dir, "project root");
        assert_eq!(details.kind, "test");
        assert_eq!(details.origin, "detected from a manifest");
        assert_eq!(
            details.evidence,
            ["[1] Cargo.toml:L1–L5", "[3] (citation unavailable)"]
        );
        let nested = command_details(&command("c2", "Test", "web"), &citations);
        assert_eq!(nested.working_dir, "web");
        let blocks = details_blocks(&details);
        assert_eq!(blocks[0].text, "Test");
        assert!(blocks.iter().any(|b| b.kind == BlockKind::Code));
        assert!(blocks.iter().any(|b| b.text == "[1] Cargo.toml:L1–L5"));
    }

    #[test]
    fn footer_status_follows_the_commands_slot() {
        assert_eq!(footer_status(None), FooterStatus::Hidden);
        let mut state = BriefingState::default();
        let requests = state.open("p1");
        assert_eq!(footer_status(state.briefing()), FooterStatus::Loading);
        state.apply(BriefingMessage {
            request: requests[2].clone(),
            outcome: Ok(Reply::Commands(CommandsResult {
                briefing: briefing_result("p1", "x"),
                commands: Vec::new(),
            })),
        });
        assert_eq!(footer_status(state.briefing()), FooterStatus::Empty);
        state.rerun(Section::Commands);
        assert_eq!(footer_status(state.briefing()), FooterStatus::Loading);
    }

    #[test]
    fn details_view_lists_or_finds_commands() {
        let mut state = BriefingState::default();
        let requests = state.open("p1");
        state.apply(BriefingMessage {
            request: requests[2].clone(),
            outcome: Ok(Reply::Commands(CommandsResult {
                briefing: briefing_result("p1", "x"),
                commands: vec![command("c1", "Test", ""), command("c2", "Build", "web")],
            })),
        });
        let list = details_view(state.briefing(), &Details::List).unwrap();
        assert_eq!(list[0].text, "All commands (2)");
        assert_eq!(
            list[1].kind,
            BlockKind::Row(Action::OpenCommand("c1".into()))
        );
        let one = details_view(state.briefing(), &Details::Command("c2".into())).unwrap();
        assert_eq!(one[0].text, "Build");
        assert!(details_view(state.briefing(), &Details::Command("zz".into())).is_none());
        assert!(details_view(None, &Details::List).is_none());
    }
}
