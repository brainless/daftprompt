//! Validation and record construction shared by the agents that submit a
//! [`BriefingExtraction`] (Chronicler and Surveyor).
//!
//! Text is stored once in payloads; nodes carry no text. Rust owns every ID,
//! hash, timestamp, and coordinate, and no label is ever produced.

use std::collections::BTreeMap;

use daftprompt_types::{
    AgentTask, BriefingCitation, BriefingExtraction, BriefingNotice, BriefingResult, CitationInfo,
    CitationLocation, ContextTarget, Node, NodeContext, ParseError, Payload, Run, RunInput,
    RunStatus, SourceRange, ToolCall,
};

use crate::project_tools::{RecordedCall, ToolRecorder, normalize_path_text};
use crate::scribe::support::{content_hash, new_id, now};

/// Bounds on a submitted briefing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BriefingLimits {
    /// Citations accepted in one briefing.
    pub max_citations: usize,
    /// UTF-8 bytes accepted in one summary.
    pub max_summary_bytes: usize,
}

/// A citation Rust has verified and resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatedCitation {
    File {
        path: String,
        /// Inclusive, 1-based.
        start_line: u32,
        end_line: u32,
        /// SHA-256 of the file content the agent read.
        content_hash: String,
    },
    Commit {
        /// Full commit id the ledger saw.
        hash: String,
        subject: String,
    },
}

/// Check the summary, its markers, and every citation against the run's
/// ledger. The error describes the first problem; it is never fed back to the
/// model.
pub fn validate_briefing(
    extraction: &BriefingExtraction,
    recorder: &ToolRecorder,
    limits: BriefingLimits,
) -> Result<Vec<ValidatedCitation>, String> {
    if extraction.summary.trim().is_empty() {
        return Err("the summary is empty".to_owned());
    }
    if extraction.summary.len() > limits.max_summary_bytes {
        return Err(format!(
            "the summary is {} bytes, over the limit of {}",
            extraction.summary.len(),
            limits.max_summary_bytes
        ));
    }
    if extraction.citations.len() > limits.max_citations {
        return Err(format!(
            "{} citations exceed the limit of {}",
            extraction.citations.len(),
            limits.max_citations
        ));
    }
    for marker in markers(&extraction.summary) {
        if marker == 0 || marker as usize > extraction.citations.len() {
            return Err(format!(
                "the summary refers to citation [{marker}], but there are {} citation(s)",
                extraction.citations.len()
            ));
        }
    }

    let mut validated = Vec::with_capacity(extraction.citations.len());
    for (index, citation) in extraction.citations.iter().enumerate() {
        let n = index + 1;
        match citation {
            BriefingCitation::File {
                path,
                start_line,
                end_line,
            } => {
                if *start_line == 0 || end_line < start_line {
                    return Err(format!(
                        "citation [{n}] has an invalid line range {start_line}-{end_line}"
                    ));
                }
                if !recorder.covers(path, *start_line as usize, *end_line as usize) {
                    return Err(format!(
                        "citation [{n}] cites {path} lines {start_line}-{end_line}, which no \
                         read_file call returned"
                    ));
                }
                let normalized = normalize_path_text(path);
                let Some(content_hash) = recorder.file_sha256(&normalized) else {
                    return Err(format!("citation [{n}] has no recorded content for {path}"));
                };
                validated.push(ValidatedCitation::File {
                    path: normalized,
                    start_line: *start_line,
                    end_line: *end_line,
                    content_hash,
                });
            }
            BriefingCitation::Commit { hash } => {
                // A prefix must pick out exactly one commit a git tool
                // returned in this run; nothing else can be cited.
                let commit = recorder
                    .resolve_commit(hash)
                    .map_err(|error| format!("citation [{n}]: {error}"))?;
                validated.push(ValidatedCitation::Commit {
                    hash: commit.hash,
                    subject: commit.subject,
                });
            }
        }
    }
    Ok(validated)
}

/// Every `[digits]` marker in `text`, in order. Numbers too large for `u32`
/// are reported as `u32::MAX`, which is never a valid citation.
fn markers(text: &str) -> Vec<u32> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b']' {
                found.push(text[i + 1..j].parse().unwrap_or(u32::MAX));
                i = j;
            }
        }
        i += 1;
    }
    found
}

/// The user's prompt, when the request had one.
#[derive(Debug, Clone, Copy)]
pub struct PromptRecord<'a> {
    pub text: &'a str,
    pub author: &'a str,
}

/// Everything needed to build a [`BriefingResult`].
pub struct BriefingBuild<'a> {
    /// `Run.agent`, for example `chronicler`.
    pub agent: &'static str,
    pub prompt_version: &'static str,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub project_id: String,
    /// `Some` makes the task [`AgentTask::Prompt`] and creates the prompt node.
    pub prompt: Option<PromptRecord<'a>>,
    pub summary: &'a str,
    pub citations: &'a [ValidatedCitation],
    /// The accepted attempt's tool calls.
    pub calls: &'a [RecordedCall],
    /// The accepted submission, recorded as the `submit` call.
    pub submitted: &'a BriefingExtraction,
    pub notices: Vec<BriefingNotice>,
    pub run_id: String,
    pub started_at: String,
    pub attempts: u32,
}

/// Build the records of one accepted briefing.
pub fn build_briefing(input: BriefingBuild<'_>) -> Result<BriefingResult, ParseError> {
    let BriefingBuild {
        agent,
        prompt_version,
        provider,
        model,
        project_id,
        prompt,
        summary,
        citations,
        calls,
        submitted,
        notices,
        run_id,
        started_at,
        attempts,
    } = input;
    let created_at = now();
    let payload = |content: &str| Payload {
        hash: content_hash(content),
        content: content.to_owned(),
        created_at: created_at.clone(),
    };
    let summary_payload = payload(summary);
    let prompt_payload = prompt.map(|p| payload(p.text));

    let mut nodes = Vec::new();
    let mut node_contexts = Vec::new();
    let mut run_inputs = Vec::new();
    let mut prompt_node_id = None;
    if let (Some(prompt), Some(prompt_payload)) = (prompt, &prompt_payload) {
        let id = new_id("node");
        nodes.push(Node {
            id: id.clone(),
            parent_id: None,
            author: prompt.author.to_owned(),
            run_id: None,
            created_at: created_at.clone(),
        });
        node_contexts.push(NodeContext {
            id: new_id("ctx"),
            node_id: id.clone(),
            seq: 0,
            target: ContextTarget::Payload {
                payload_hash: prompt_payload.hash.clone(),
            },
            range: None,
        });
        run_inputs.push(RunInput {
            run_id: run_id.clone(),
            node_id: id.clone(),
        });
        prompt_node_id = Some(id);
    }

    let summary_node_id = new_id("node");
    nodes.push(Node {
        id: summary_node_id.clone(),
        parent_id: prompt_node_id,
        author: "agent".to_owned(),
        run_id: Some(run_id.clone()),
        created_at: created_at.clone(),
    });
    node_contexts.push(NodeContext {
        id: new_id("ctx"),
        node_id: summary_node_id.clone(),
        seq: 0,
        target: ContextTarget::Payload {
            payload_hash: summary_payload.hash.clone(),
        },
        range: None,
    });

    let mut citation_infos = Vec::with_capacity(citations.len());
    for (index, citation) in citations.iter().enumerate() {
        let seq = u32::try_from(index + 1).map_err(|_| ParseError::Internal {
            detail: "too many citations".to_owned(),
        })?;
        let context_id = new_id("ctx");
        let (target, range, location) = match citation {
            ValidatedCitation::File {
                path,
                start_line,
                end_line,
                content_hash,
            } => (
                ContextTarget::File {
                    path: path.clone(),
                    content_hash: Some(content_hash.clone()),
                },
                // Whole lines, end exclusive: column 1 of the next line.
                Some(SourceRange {
                    start_line: *start_line,
                    start_col: 1,
                    end_line: end_line + 1,
                    end_col: 1,
                }),
                CitationLocation::File {
                    path: path.clone(),
                    start_line: *start_line,
                    end_line: *end_line,
                },
            ),
            ValidatedCitation::Commit { hash, subject } => (
                ContextTarget::Commit { hash: hash.clone() },
                None,
                CitationLocation::Commit {
                    hash: hash.clone(),
                    subject: subject.clone(),
                },
            ),
        };
        node_contexts.push(NodeContext {
            id: context_id.clone(),
            node_id: summary_node_id.clone(),
            seq,
            target,
            range,
        });
        citation_infos.push(CitationInfo {
            index: seq,
            context_id,
            location,
        });
    }

    // Tool outputs are content-addressed payloads; identical outputs share one.
    let mut tool_payloads: BTreeMap<String, Payload> = BTreeMap::new();
    let mut tool_calls = Vec::with_capacity(calls.len() + 1);
    for call in calls {
        let output = payload(&call.output);
        let output_hash = output.hash.clone();
        tool_payloads.entry(output_hash.clone()).or_insert(output);
        tool_calls.push(ToolCall {
            id: new_id("tool"),
            run_id: run_id.clone(),
            seq: u32::try_from(call.index).map_err(|_| ParseError::Internal {
                detail: "too many tool calls".to_owned(),
            })?,
            tool: call.name.clone(),
            input: call.args.to_string(),
            output_hash: Some(output_hash),
            created_at: created_at.clone(),
        });
    }
    let submit_input = serde_json::to_string(submitted).map_err(|error| ParseError::Internal {
        detail: format!("could not record the accepted submit arguments: {error}"),
    })?;
    tool_calls.push(ToolCall {
        id: new_id("tool"),
        run_id: run_id.clone(),
        seq: u32::try_from(calls.len()).map_err(|_| ParseError::Internal {
            detail: "too many tool calls".to_owned(),
        })?,
        tool: "submit".to_owned(),
        input: submit_input,
        output_hash: None,
        created_at: created_at.clone(),
    });

    let run = Run {
        id: run_id,
        agent: agent.to_owned(),
        provider,
        model,
        prompt_version: Some(prompt_version.to_owned()),
        started_at,
        ended_at: Some(now()),
        status: RunStatus::Done,
        attempts,
        consistency_followup: false,
        consistency_used: false,
        project_id: Some(project_id.clone()),
    };

    Ok(BriefingResult {
        project_id,
        task: if prompt_payload.is_some() {
            AgentTask::Prompt
        } else {
            AgentTask::Default
        },
        prompt_payload,
        summary_payload,
        summary_node_id,
        nodes,
        node_contexts,
        citations: citation_infos,
        notices,
        run,
        run_inputs,
        tool_calls,
        tool_payloads: tool_payloads.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::markers;

    #[test]
    fn finds_numeric_markers_only() {
        assert_eq!(markers("a [1] b [23] [x] [] [4"), vec![1, 23]);
        assert_eq!(markers("[0][2]"), vec![0, 2]);
        assert!(markers("no markers").is_empty());
    }
}
