//! The experimental `restate` tool strategy.
//!
//! Instead of one growing conversation, every model call is a fresh,
//! stateless request: the system prompt and one user message holding the
//! original task prompt plus an evidence section rebuilt from every tool
//! result of the attempt. The model may still call tools; each call runs
//! through the attempt's [`ProjectToolbox`] (so the ledger is filled exactly
//! as in the chat strategy), is folded into an [`Evidence`] store that merges
//! rather than appends, and the next request is built from the store. The
//! loop ends when the model replies without tool calls: that text is the
//! summary, and the briefing has no citations.

use std::collections::{BTreeMap, BTreeSet};

use daftprompt_types::BriefingExtraction;
use rig::DynModel;
use rig::completion::{CompletionRequest, CompletionResponse, PromptError, ToolDefinition};
use rig::core::ProviderError;
use rig::core::message::{AssistantContent, Message, ToolName};
use rig::operation::Completion;
use serde_json::{Value, json};

use super::model::SurveyorInput;
use super::prompt::render_restated_prompt;
use crate::project_agent::prompt_error;
use crate::project_tools::{FileRead, LIST_FILES, ProjectToolbox, READ_FILE, SEARCH};
use crate::scribe::{BoxFuture, ModelError};

/// Most bytes of rendered evidence in one request, in the spirit of the
/// preload's context limit.
pub(crate) const MAX_EVIDENCE_BYTES: usize = 96 * 1024;
/// Most bytes of the merged project tree.
const MAX_TREE_BYTES: usize = 16 * 1024;
/// Most bytes of the search hits.
const MAX_SEARCH_BYTES: usize = 16 * 1024;
/// Most bytes of the log of calls that found nothing.
const MAX_LOG_BYTES: usize = 4 * 1024;
/// Room kept for the truncation marker of the file blocks.
const MARKER_RESERVE: usize = 192;

/// What the attempt's tools have returned so far, merged for rendering.
#[derive(Debug, Default)]
pub(crate) struct Evidence {
    /// Every path any listing returned.
    listed: BTreeSet<String>,
    listing_truncated: bool,
    /// Lines `read_file` returned, per path.
    files: BTreeMap<String, FileLines>,
    /// Search hit lines per query label, in arrival order.
    searches: BTreeMap<String, SearchHits>,
    /// One line per call that errored or found nothing.
    misses: Vec<String>,
}

#[derive(Debug, Default)]
struct FileLines {
    total_lines: usize,
    lines: BTreeMap<usize, String>,
}

#[derive(Debug, Default)]
struct SearchHits {
    hits: Vec<String>,
    truncated: bool,
}

impl Evidence {
    pub(crate) fn is_empty(&self) -> bool {
        self.listed.is_empty()
            && self.files.is_empty()
            && self.searches.is_empty()
            && self.misses.is_empty()
    }

    /// Fold one tool result into the store. `read` is the ledger's record of
    /// a successful `read_file` (the lines the call returned).
    pub(crate) fn record(
        &mut self,
        tool: &str,
        args: &Value,
        text: &str,
        is_error: bool,
        read: Option<&FileRead>,
    ) {
        if is_error {
            let reason = text.lines().next().unwrap_or("error");
            self.miss(format!("{} → {reason}", describe_call(tool, args)));
            return;
        }
        match tool {
            LIST_FILES => {
                let before = self.listed.len();
                for line in text.lines() {
                    if line.is_empty() || line.starts_with('[') || line.starts_with('(') {
                        continue;
                    }
                    self.listed.insert(line.to_owned());
                }
                self.listing_truncated |= text.contains("[truncated");
                if self.listed.len() == before && !text.contains("[truncated") {
                    self.miss(format!("{} → no new files", describe_call(tool, args)));
                }
            }
            SEARCH => {
                let hits: Vec<&str> = text
                    .lines()
                    .filter(|line| {
                        !line.is_empty() && !line.starts_with('[') && !line.starts_with('(')
                    })
                    .collect();
                if hits.is_empty() {
                    self.miss(format!("{} → 0 hits", describe_call(tool, args)));
                    return;
                }
                let entry = self.searches.entry(describe_call(tool, args)).or_default();
                for hit in hits {
                    if !entry.hits.iter().any(|known| known == hit) {
                        entry.hits.push(hit.to_owned());
                    }
                }
                entry.truncated |= text.contains("[truncated");
            }
            READ_FILE => {
                let Some(read) = read else {
                    self.miss(format!("{} → no lines returned", describe_call(tool, args)));
                    return;
                };
                let file = self.files.entry(read.path.clone()).or_default();
                file.total_lines = read.total_lines;
                // The body is `N: text` lines after the header; markers and
                // headers never start with a number in the returned range.
                for line in text.lines().skip(1) {
                    if let Some((number, body)) = line.split_once(": ")
                        && let Ok(number) = number.parse::<usize>()
                        && (read.start_line..=read.end_line).contains(&number)
                    {
                        file.lines.insert(number, body.to_owned());
                    }
                }
            }
            _ => {}
        }
    }

    fn miss(&mut self, line: String) {
        if !self.misses.contains(&line) {
            self.misses.push(line);
        }
    }

    /// The evidence as text, at most `max_bytes` long. Order: merged tree,
    /// files by `(depth, path)`, search hits, then the log of empty calls.
    /// The tree, hits and log have their own caps so files cannot starve
    /// them; the file blocks fill what remains and end with a marker when
    /// cut.
    pub(crate) fn render(&self, max_bytes: usize) -> String {
        if self.is_empty() {
            return "(nothing gathered yet)\n".to_owned();
        }
        let tree = self.render_tree(MAX_TREE_BYTES.min(max_bytes));
        let search = self.render_search(MAX_SEARCH_BYTES.min(max_bytes));
        let log = self.render_log(MAX_LOG_BYTES.min(max_bytes));
        let budget = max_bytes.saturating_sub(tree.len() + search.len() + log.len());
        let files = self.render_files(budget);
        format!("{tree}{files}{search}{log}")
    }

    fn render_tree(&self, cap: usize) -> String {
        if self.listed.is_empty() {
            return String::new();
        }
        let mut paths: Vec<Vec<&str>> = self
            .listed
            .iter()
            .map(|path| path.split('/').collect())
            .collect();
        paths.sort();
        let mut text = String::from("### Project tree (merged listings; filenames only)\n");
        let mut previous: Vec<&str> = Vec::new();
        for path in &paths {
            let (name, dirs) = path.split_last().expect("a path has a component");
            let shared = dirs
                .iter()
                .zip(&previous)
                .take_while(|(a, b)| a == b)
                .count();
            for (depth, dir) in dirs.iter().enumerate().skip(shared) {
                text.push_str(&format!("{}{dir}/\n", "  ".repeat(depth)));
            }
            text.push_str(&format!("{}{name}\n", "  ".repeat(dirs.len())));
            previous = dirs.to_vec();
        }
        if self.listing_truncated {
            text.push_str("[some listings were truncated; list narrower directories for more]\n");
        }
        text.push('\n');
        cut_to_cap(text, cap, "tree")
    }

    fn render_search(&self, cap: usize) -> String {
        if self.searches.is_empty() {
            return String::new();
        }
        let mut text =
            String::from("### Search hits (single lines, not read; use read_file for context)\n");
        for (label, found) in &self.searches {
            text.push_str(&format!("{label}\n"));
            for hit in &found.hits {
                text.push_str(&format!("  {hit}\n"));
            }
            if found.truncated {
                text.push_str("  [truncated]\n");
            }
        }
        text.push('\n');
        cut_to_cap(text, cap, "search hits")
    }

    fn render_log(&self, cap: usize) -> String {
        if self.misses.is_empty() {
            return String::new();
        }
        let mut text = String::from("### Calls that found nothing or failed (do not repeat)\n");
        for miss in &self.misses {
            text.push_str(&format!("- {miss}\n"));
        }
        text.push('\n');
        cut_to_cap(text, cap, "call log")
    }

    fn render_files(&self, budget: usize) -> String {
        if self.files.is_empty() {
            return String::new();
        }
        let heading = "### Files read (as returned by read_file)\n";
        let mut order: Vec<(&String, &FileLines)> = self.files.iter().collect();
        order.sort_by_key(|(path, _)| (path.split('/').count(), path.as_str()));
        let mut blocks: Vec<String> = Vec::new();
        for (path, file) in order {
            let mut run: Vec<(usize, &str)> = Vec::new();
            for (&number, text) in &file.lines {
                if run.last().is_some_and(|&(last, _)| last + 1 != number) {
                    blocks.push(file_block(path, file.total_lines, &run));
                    run.clear();
                }
                run.push((number, text));
            }
            if !run.is_empty() {
                blocks.push(file_block(path, file.total_lines, &run));
            }
        }
        let room = budget.saturating_sub(heading.len() + MARKER_RESERVE);
        let mut text = String::from(heading);
        let mut used = 0;
        for (index, block) in blocks.iter().enumerate() {
            if used + block.len() <= room {
                text.push_str(block);
                used += block.len();
                continue;
            }
            // Keep as many whole lines of this block as still fit.
            let mut kept = 0;
            for line in block.split_inclusive('\n') {
                if used + kept + line.len() > room {
                    break;
                }
                text.push_str(line);
                kept += line.len();
            }
            let omitted = blocks[index..].iter().map(String::len).sum::<usize>() - kept;
            text.push_str(&format!(
                "[evidence truncated: {omitted} more bytes of file lines omitted; read_file the ranges you still need]\n"
            ));
            break;
        }
        text.push('\n');
        // The trailing blank line is the one byte-budget slack; trim if over.
        if text.len() > budget {
            text.truncate(budget);
        }
        text
    }
}

/// One merged block: a header with the covered range and numbered lines.
fn file_block(path: &str, total: usize, run: &[(usize, &str)]) -> String {
    let (first, last) = (run[0].0, run[run.len() - 1].0);
    let mut block = format!("{path} (lines {first}-{last} of {total})\n");
    for (number, text) in run {
        block.push_str(&format!("{number}: {text}\n"));
    }
    block
}

/// Cut `text` to at most `cap` bytes at a line boundary, with a marker.
fn cut_to_cap(text: String, cap: usize, what: &str) -> String {
    if text.len() <= cap {
        return text;
    }
    let marker = format!("[{what} truncated]\n\n");
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if out.len() + line.len() + marker.len() > cap {
            break;
        }
        out.push_str(line);
    }
    out.push_str(&marker);
    out.truncate(cap);
    out
}

/// A compact, human-readable description of a call, for the log and labels.
fn describe_call(tool: &str, args: &Value) -> String {
    let text = |key: &str| args.get(key).and_then(Value::as_str);
    match tool {
        SEARCH => {
            let mut label = format!("search {:?}", text("query").unwrap_or(""));
            if let Some(directory) = text("directory") {
                label.push_str(&format!(" in {directory}"));
            }
            if let Some(glob) = text("glob") {
                label.push_str(&format!(" glob {glob}"));
            }
            label
        }
        READ_FILE => {
            let mut label = format!("read_file {}", text("path").unwrap_or("?"));
            let number = |key: &str| args.get(key).and_then(Value::as_u64);
            if number("start_line").is_some() || number("end_line").is_some() {
                label.push_str(&format!(
                    " lines {}-{}",
                    number("start_line").unwrap_or(1),
                    number("end_line").map_or("end".to_owned(), |end| end.to_string())
                ));
            }
            label
        }
        LIST_FILES => {
            let mut label = String::from("list_files");
            if let Some(directory) = text("directory") {
                label.push_str(&format!(" {directory}"));
            }
            if let Some(glob) = text("glob") {
                label.push_str(&format!(" glob {glob}"));
            }
            label
        }
        other => other.to_owned(),
    }
}

/// The three file tools, as Rig tool definitions. There is no output tool:
/// a reply without tool calls is the answer.
fn tool_definitions() -> Result<Vec<ToolDefinition>, ModelError> {
    ProjectToolbox::definitions()
        .into_iter()
        .map(|(name, description, parameters)| {
            ToolName::new(name)
                .map(|name| ToolDefinition::new(name, description, parameters))
                .map_err(|error| ModelError::Internal(error.to_string()))
        })
        .collect()
}

/// One attempt on a Rig model.
pub(crate) async fn run_restate_on(
    model: &DynModel<Completion>,
    system_prompt: &str,
    input: SurveyorInput,
) -> Result<BriefingExtraction, ModelError> {
    run_restate(
        |request| Box::pin(model.call(request)),
        system_prompt,
        input,
    )
    .await
}

/// The restate loop over any completion function (a stub in tests).
///
/// A reply with tool calls continues the run; a reply without any is the
/// answer, and its text becomes the summary of a briefing with no citations.
/// The last allowed call offers no tools, so the model must answer.
pub(crate) async fn run_restate<'a>(
    complete: impl Fn(CompletionRequest) -> BoxFuture<'a, Result<CompletionResponse, ProviderError>>,
    system_prompt: &str,
    input: SurveyorInput,
) -> Result<BriefingExtraction, ModelError> {
    let toolbox = input.toolset.files();
    let recorder = toolbox.recorder();
    let tools = tool_definitions()?;
    let mut evidence = Evidence::default();

    for turn in 1..=input.max_turns {
        let last = turn == input.max_turns;
        let rendered = evidence.render(MAX_EVIDENCE_BYTES);
        let prompt = render_restated_prompt(&input.prompt, &rendered, turn, input.max_turns);
        recorder.log_event(
            "restate_turn",
            json!({
                "turn": turn,
                "evidence_bytes": rendered.len(),
                "prompt_bytes": prompt.len(),
                "tools_offered": !last,
            }),
        );
        let mut request = CompletionRequest::new(Message::user(prompt)).preamble(system_prompt);
        if !last {
            request = request.tools(tools.clone());
        }
        let response = complete(request)
            .await
            .map_err(|error| prompt_error(PromptError::from(error)))?;
        if let Some(detail) = &response.error {
            return Err(ModelError::Provider(detail.clone()));
        }

        let calls: Vec<_> = response
            .choice
            .iter()
            .filter_map(|content| match content {
                AssistantContent::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect();
        if calls.is_empty() {
            let summary = response
                .choice
                .iter()
                .filter_map(|content| match content {
                    AssistantContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
                .trim()
                .to_owned();
            recorder.log_event("final_text", json!({"output": summary}));
            if summary.is_empty() {
                return Err(ModelError::Invalid(
                    "the model replied without tool calls or summary text".to_owned(),
                ));
            }
            return Ok(BriefingExtraction {
                summary,
                citations: Vec::new(),
            });
        }
        if last {
            return Err(ModelError::Invalid(
                "the model called tools on its last model call instead of answering".to_owned(),
            ));
        }
        if let Some(call) = calls
            .iter()
            .find(|call| !is_known_tool(call.function.name.as_ref()))
        {
            return Err(ModelError::Invalid(format!(
                "the model called an unavailable tool `{}`",
                call.function.name
            )));
        }
        for call in calls {
            let args = match &call.function.invalid_arguments {
                Some(raw) => Value::String(raw.clone()),
                None => Value::Object(call.function.arguments.clone()),
            };
            let name = call.function.name.as_ref();
            let result = toolbox.call(name, args.clone());
            let read = recorder.calls().last().and_then(|call| call.read.clone());
            evidence.record(name, &args, &result.text, result.is_error, read.as_ref());
        }
    }
    Err(ModelError::Invalid(format!(
        "the model did not answer within {} model call(s)",
        input.max_turns
    )))
}

fn is_known_tool(name: &str) -> bool {
    matches!(name, LIST_FILES | SEARCH | READ_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_agent::AttemptInput;
    use crate::project_tools::{InMemoryProject, ProjectFiles, ProjectToolConfig};
    use rig::core::message::Origin;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::super::model::SurveyorTools;

    fn read(path: &str, start: usize, end: usize, total: usize) -> (FileRead, String, Value) {
        let mut text = format!("{path} (lines {start}-{end} of {total})\n");
        for n in start..=end {
            text.push_str(&format!("{n}: line {n}\n"));
        }
        let file_read = FileRead {
            path: path.to_owned(),
            start_line: start,
            end_line: end,
            total_lines: total,
            file_sha256: String::new(),
        };
        (
            file_read,
            text,
            json!({"path": path, "start_line": start, "end_line": end}),
        )
    }

    fn add_read(evidence: &mut Evidence, path: &str, start: usize, end: usize, total: usize) {
        let (file_read, text, args) = read(path, start, end, total);
        evidence.record(READ_FILE, &args, &text, false, Some(&file_read));
    }

    #[test]
    fn overlapping_and_adjacent_ranges_merge_into_one_block() {
        let mut evidence = Evidence::default();
        add_read(&mut evidence, "a.rs", 1, 3, 20);
        add_read(&mut evidence, "a.rs", 3, 5, 20);
        add_read(&mut evidence, "a.rs", 6, 6, 20);
        add_read(&mut evidence, "a.rs", 10, 11, 20);
        let text = evidence.render(MAX_EVIDENCE_BYTES);
        assert!(
            text.contains("a.rs (lines 1-6 of 20)\n1: line 1\n"),
            "{text}"
        );
        assert!(text.contains("a.rs (lines 10-11 of 20)\n"), "{text}");
        assert_eq!(text.matches("3: line 3").count(), 1);
        assert_eq!(text.matches("(lines ").count(), 2);
    }

    #[test]
    fn listings_merge_into_one_tree() {
        let mut evidence = Evidence::default();
        evidence.record(
            LIST_FILES,
            &json!({}),
            "src/lib.rs\nsrc/a/b.rs\nREADME.md\n",
            false,
            None,
        );
        evidence.record(
            LIST_FILES,
            &json!({"directory": "src"}),
            "src/lib.rs\nsrc/a/c.rs\n[truncated: showing 2 files; narrow]\n",
            false,
            None,
        );
        let text = evidence.render(MAX_EVIDENCE_BYTES);
        assert!(
            text.contains("README.md\nsrc/\n  a/\n    b.rs\n    c.rs\n  lib.rs\n"),
            "{text}"
        );
        assert_eq!(text.matches("lib.rs").count(), 1);
        assert!(text.contains("listings were truncated"));
    }

    #[test]
    fn files_are_ordered_by_depth_then_path() {
        let mut evidence = Evidence::default();
        add_read(&mut evidence, "z/deep/x.rs", 1, 1, 1);
        add_read(&mut evidence, "b.rs", 1, 1, 1);
        add_read(&mut evidence, "a/m.rs", 1, 1, 1);
        add_read(&mut evidence, "a.rs", 1, 1, 1);
        let text = evidence.render(MAX_EVIDENCE_BYTES);
        let position = |needle: &str| text.find(needle).unwrap();
        assert!(position("a.rs (") < position("b.rs ("));
        assert!(position("b.rs (") < position("a/m.rs ("));
        assert!(position("a/m.rs (") < position("z/deep/x.rs ("));
    }

    #[test]
    fn search_hits_are_separate_and_misses_are_logged() {
        let mut evidence = Evidence::default();
        evidence.record(
            SEARCH,
            &json!({"query": "foo"}),
            "a.rs:3: foo()\na.rs:3: foo()\n",
            false,
            None,
        );
        evidence.record(
            SEARCH,
            &json!({"query": "bar"}),
            "(no matches)\n",
            false,
            None,
        );
        evidence.record(
            READ_FILE,
            &json!({"path": "x"}),
            "error: file not found: x",
            true,
            None,
        );
        add_read(&mut evidence, "a.rs", 1, 2, 2);
        let text = evidence.render(MAX_EVIDENCE_BYTES);
        assert!(text.contains("Search hits (single lines, not read"));
        assert_eq!(text.matches("a.rs:3: foo()").count(), 1);
        assert!(text.contains("- search \"bar\" → 0 hits"));
        assert!(text.contains("- read_file x → error: file not found: x"));
        assert!(text.find("Files read").unwrap() < text.find("Search hits").unwrap());
        assert!(text.find("Search hits").unwrap() < text.find("found nothing").unwrap());
    }

    #[test]
    fn rendering_is_capped_with_a_marker() {
        let mut evidence = Evidence::default();
        for file in 0..20 {
            add_read(&mut evidence, &format!("f{file:02}.txt"), 1, 100, 100);
        }
        evidence.record(SEARCH, &json!({"query": "q"}), "a.rs:1: x\n", false, None);
        let cap = 8 * 1024;
        let text = evidence.render(cap);
        assert!(text.len() <= cap, "{} > {cap}", text.len());
        assert!(text.contains("[evidence truncated:"));
        assert!(text.contains("Search hits"), "small sections survive");
        assert!(
            text.contains("f00.txt (lines 1-100 of 100)") || text.contains("f00.txt (lines 1-")
        );
        assert_eq!(evidence.render(cap), text, "rendering is deterministic");
    }

    fn input(max_turns: usize) -> SurveyorInput {
        let files: Arc<dyn ProjectFiles> =
            Arc::new(InMemoryProject::new().with_file("a.txt", "hello\nworld\n"));
        AttemptInput {
            prompt: "## Task\n\nSummarize.\n".to_owned(),
            toolset: Arc::new(SurveyorTools::new(files, ProjectToolConfig::default())),
            max_turns,
        }
    }

    fn response(choice: Vec<AssistantContent>) -> CompletionResponse {
        CompletionResponse::new(
            choice,
            Default::default(),
            Origin::new("test", "test", "test"),
            Value::Null,
        )
    }

    fn call(name: &str, args: Value) -> AssistantContent {
        AssistantContent::tool_call("id", ToolName::new(name).unwrap(), args)
    }

    /// Replies from a script; every request is kept for inspection.
    type Requests = Arc<Mutex<Vec<CompletionRequest>>>;
    fn stub(
        script: Vec<Vec<AssistantContent>>,
    ) -> (
        Requests,
        impl Fn(CompletionRequest) -> BoxFuture<'static, Result<CompletionResponse, ProviderError>>,
    ) {
        let requests: Requests = Arc::default();
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let seen = requests.clone();
        let complete = move |request: CompletionRequest| {
            seen.lock().unwrap().push(request);
            let next = script.lock().unwrap().pop_front().expect("scripted reply");
            Box::pin(async move { Ok(response(next)) }) as BoxFuture<'static, _>
        };
        (requests, complete)
    }

    fn user_text(request: &CompletionRequest) -> String {
        format!("{:?}", request.chat_history.last().unwrap())
    }

    #[tokio::test]
    async fn loop_restates_evidence_in_fresh_requests_and_returns_the_text() {
        let input = input(4);
        let toolset = input.toolset.clone();
        let (requests, complete) = stub(vec![
            vec![
                call(
                    "read_file",
                    json!({"path": "a.txt", "start_line": 1, "end_line": 1}),
                ),
                call("search", json!({"query": "nothing-here"})),
            ],
            vec![
                AssistantContent::text("Reading more."),
                call(
                    "read_file",
                    json!({"path": "a.txt", "start_line": 2, "end_line": 2}),
                ),
            ],
            vec![AssistantContent::text("  It greets the `world`.\n")],
        ]);
        let extraction = run_restate(complete, "SYSTEM", input).await.unwrap();
        assert_eq!(extraction.summary, "It greets the `world`.");
        assert!(extraction.citations.is_empty());

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for request in requests.iter() {
            assert_eq!(request.chat_history.len(), 2, "system + one user message");
            let names: Vec<&str> = request.tools.iter().map(|t| t.name.as_ref()).collect();
            assert_eq!(names, ["list_files", "search", "read_file"]);
            assert!(request.tool_choice.is_none());
        }
        assert!(!user_text(&requests[0]).contains("1: hello"));
        assert!(user_text(&requests[1]).contains("1: hello"));
        let third = user_text(&requests[2]);
        assert!(third.contains("(lines 1-2 of 2)"), "{third}");
        assert!(
            third.contains("search \\\"nothing-here\\\" → 0 hits"),
            "{third}"
        );
        assert!(third.contains("Model call 3 of 4"));

        // Tool calls still reach the ledger.
        let calls = toolset.files().recorder().calls();
        assert_eq!(calls.iter().filter(|c| c.read.is_some()).count(), 2);
        assert_eq!(calls.len(), 3);
    }

    #[tokio::test]
    async fn last_turn_offers_no_tools() {
        let (requests, complete) = stub(vec![
            vec![call("list_files", json!({}))],
            vec![AssistantContent::text("ok")],
        ]);
        let extraction = run_restate(complete, "SYSTEM", input(2)).await.unwrap();
        assert_eq!(extraction.summary, "ok");
        let requests = requests.lock().unwrap();
        assert_eq!(requests[0].tools.len(), 3);
        assert!(requests[1].tools.is_empty());
        assert!(user_text(&requests[1]).contains("last model call"));
    }

    #[tokio::test]
    async fn bad_outputs_are_invalid_not_provider_errors() {
        let cases: Vec<(Vec<Vec<AssistantContent>>, &str)> = vec![
            (
                vec![vec![AssistantContent::text("   ")]],
                "without tool calls or summary text",
            ),
            (
                vec![vec![call("final_result", json!({"summary": "x"}))]],
                "unavailable tool",
            ),
            (vec![vec![call("rm_rf", json!({}))]], "unavailable tool"),
            (
                vec![
                    vec![call("list_files", json!({}))],
                    vec![call("list_files", json!({}))],
                ],
                "on its last model call",
            ),
        ];
        for (script, expected) in cases {
            let (_, complete) = stub(script);
            match run_restate(complete, "SYSTEM", input(2)).await {
                Err(ModelError::Invalid(detail)) => assert!(detail.contains(expected), "{detail}"),
                other => panic!("expected invalid ({expected}), got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn provider_failures_stay_provider_errors() {
        let complete = |_request: CompletionRequest| {
            Box::pin(async {
                Err(ProviderError::from_http_response(
                    503u16.try_into().unwrap(),
                    "down",
                ))
            }) as BoxFuture<'static, _>
        };
        assert!(matches!(
            run_restate(complete, "SYSTEM", input(2)).await,
            Err(ModelError::Provider(_))
        ));
    }
}
