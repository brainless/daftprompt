//! Construction of the branch records from a validated explanation.
//!
//! Text is stored once in payloads; nodes carry no text. Rust owns every ID,
//! hash, timestamp, and coordinate. Only a `clarification` label is produced,
//! and only on the first turn.

use std::collections::BTreeMap;

use daftprompt_types::{
    CitationInfo, CitationLocation, ClarifyResult, ContextTarget, ExplainerExtraction, Grounding,
    Label, LabelKind, Node, NodeContext, ParseError, Payload, Run, RunInput, RunStatus,
    SourceRange, ToolCall,
};

use super::config::ExplainerConfig;
use super::context::Prepared;
use super::prompt::PROMPT_VERSION;
use super::validate::ValidatedCitation;
use crate::project_tools::RecordedCall;
use crate::scribe::support::{content_hash, new_id, now};

pub(crate) struct Built<'a> {
    pub(crate) prepared: &'a Prepared<'a>,
    pub(crate) extraction: &'a ExplainerExtraction,
    pub(crate) citations: &'a [ValidatedCitation],
    pub(crate) calls: &'a [RecordedCall],
    pub(crate) grounding: Grounding,
    pub(crate) config: &'a ExplainerConfig,
    pub(crate) run_id: String,
    pub(crate) started_at: String,
    pub(crate) attempts: u32,
}

pub(crate) fn build(input: Built<'_>) -> Result<ClarifyResult, ParseError> {
    let Built {
        prepared,
        extraction,
        citations,
        calls,
        grounding,
        config,
        run_id,
        started_at,
        attempts,
    } = input;
    let request = prepared.request;
    let created_at = now();
    let author = request.question_author().to_owned();

    let payload = |content: &str| Payload {
        hash: content_hash(content),
        content: content.to_owned(),
        created_at: created_at.clone(),
    };
    let question_payload = payload(&request.question);
    let explanation_payload = payload(&extraction.explanation);

    let question_node_id = new_id("node");
    let explanation_node_id = new_id("node");
    let parent_id = request.turns.last().map_or_else(
        || request.target.node_id.clone(),
        |turn| turn.explanation_node_id.clone(),
    );

    let nodes = vec![
        Node {
            id: question_node_id.clone(),
            parent_id: Some(parent_id),
            author: author.clone(),
            run_id: None,
            created_at: created_at.clone(),
        },
        Node {
            id: explanation_node_id.clone(),
            parent_id: Some(question_node_id.clone()),
            author: "agent".to_owned(),
            run_id: Some(run_id.clone()),
            created_at: created_at.clone(),
        },
    ];

    let mut node_contexts = vec![
        NodeContext {
            id: new_id("ctx"),
            node_id: question_node_id.clone(),
            seq: 0,
            target: ContextTarget::Payload {
                payload_hash: question_payload.hash.clone(),
            },
            range: None,
        },
        NodeContext {
            id: new_id("ctx"),
            node_id: explanation_node_id.clone(),
            seq: 0,
            target: ContextTarget::Payload {
                payload_hash: explanation_payload.hash.clone(),
            },
            range: None,
        },
    ];

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
                SourceRange {
                    start_line: *start_line,
                    start_col: 1,
                    end_line: end_line + 1,
                    end_col: 1,
                },
                CitationLocation::File {
                    path: path.clone(),
                    start_line: *start_line,
                    end_line: *end_line,
                },
            ),
            ValidatedCitation::Source {
                start_byte,
                end_byte,
                range,
            } => (
                ContextTarget::Payload {
                    payload_hash: prepared.payload_hash.clone(),
                },
                *range,
                CitationLocation::Source {
                    start_byte: *start_byte,
                    end_byte: *end_byte,
                },
            ),
        };
        node_contexts.push(NodeContext {
            id: context_id.clone(),
            node_id: explanation_node_id.clone(),
            seq,
            target,
            range: Some(range),
        });
        citation_infos.push(CitationInfo {
            index: seq,
            context_id,
            location,
        });
    }

    let labels = if request.turns.is_empty() {
        vec![Label {
            id: new_id("label"),
            kind: LabelKind::Clarification,
            src_node_id: question_node_id.clone(),
            dst_node_id: Some(request.target.node_id.clone()),
            status: None,
            assignee: None,
            note_node_id: None,
            created_by: author,
            run_id: None,
            created_at: created_at.clone(),
        }]
    } else {
        Vec::new()
    };

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
    let submit_input = serde_json::to_string(extraction).map_err(|error| ParseError::Internal {
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

    // What the agent was given: the target, the branch's earlier turns, and
    // the new question. The included segments are `source_window`.
    let mut run_inputs = vec![RunInput {
        run_id: run_id.clone(),
        node_id: request.target.node_id.clone(),
    }];
    for turn in &request.turns {
        for node_id in [&turn.question_node_id, &turn.explanation_node_id] {
            run_inputs.push(RunInput {
                run_id: run_id.clone(),
                node_id: node_id.clone(),
            });
        }
    }
    run_inputs.push(RunInput {
        run_id: run_id.clone(),
        node_id: question_node_id.clone(),
    });

    let run = Run {
        id: run_id,
        agent: "explainer".to_owned(),
        provider: config.provider.clone(),
        model: config.model.clone(),
        prompt_version: Some(PROMPT_VERSION.to_owned()),
        started_at,
        ended_at: Some(now()),
        status: RunStatus::Done,
        attempts,
    };

    Ok(ClarifyResult {
        question_payload,
        explanation_payload,
        tool_payloads: tool_payloads.into_values().collect(),
        question_node_id,
        explanation_node_id,
        nodes,
        node_contexts,
        labels,
        citations: citation_infos,
        grounding,
        source_window: Some(prepared.window()),
        run,
        run_inputs,
        tool_calls,
    })
}
