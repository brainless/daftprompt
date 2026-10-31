//! Construction of the in-memory graph fragment from a validated extraction.
//!
//! Text is stored once in the payload. Span nodes carry no text; their
//! contexts point at ranges of that payload. The model never supplies graph
//! ids, statuses, timestamps, or author fields.

use daftprompt_types::{
    ContextTarget, Label, LabelKind, LabelStatus, Node, NodeContext, ParseRequest, ParseResult,
    Payload, Run, RunInput, RunStatus, SpanInfo, ToolCall,
};

use super::config::ScribeConfig;
use super::prompt;
use super::support::{content_hash, new_id, now};
use super::validate::ValidatedExtraction;

/// Build the complete parse result for an accepted extraction.
pub(crate) fn build(
    request: &ParseRequest,
    extraction: &daftprompt_types::Extraction,
    validated: &ValidatedExtraction,
    config: &ScribeConfig,
    run_id: String,
    started_at: String,
    attempts: u32,
) -> Result<ParseResult, daftprompt_types::ParseError> {
    let created_at = now();
    let payload_hash = content_hash(&request.text);
    let payload = Payload {
        hash: payload_hash.clone(),
        content: request.text.clone(),
        created_at: created_at.clone(),
    };

    let root_node_id = new_id("node");
    let root = Node {
        id: root_node_id.clone(),
        parent_id: None,
        author: request.root_author().to_owned(),
        run_id: None,
        created_at: created_at.clone(),
    };
    let root_context = NodeContext {
        id: new_id("ctx"),
        node_id: root_node_id.clone(),
        seq: 0,
        target: ContextTarget::Payload {
            payload_hash: payload_hash.clone(),
        },
        range: None,
    };

    // Assign ids in validated order so labels can refer to them, then emit
    // nodes in source order for a stable, readable result.
    let span_node_ids: Vec<String> = validated.spans.iter().map(|_| new_id("node")).collect();

    let mut nodes = Vec::with_capacity(validated.spans.len() + 1);
    let mut node_contexts = Vec::with_capacity(validated.spans.len() + 1);
    let mut spans = Vec::with_capacity(validated.spans.len());
    nodes.push(root);
    node_contexts.push(root_context);

    let mut source_order: Vec<usize> = (0..validated.spans.len()).collect();
    source_order.sort_by_key(|index| {
        (
            validated.spans[*index].start_byte,
            validated.spans[*index].end_byte,
        )
    });

    for index in source_order {
        let span = &validated.spans[index];
        let node_id = span_node_ids[index].clone();
        nodes.push(Node {
            id: node_id.clone(),
            parent_id: Some(root_node_id.clone()),
            author: "agent".to_owned(),
            run_id: Some(run_id.clone()),
            created_at: created_at.clone(),
        });
        node_contexts.push(NodeContext {
            id: new_id("ctx"),
            node_id: node_id.clone(),
            seq: 0,
            target: ContextTarget::Payload {
                payload_hash: payload_hash.clone(),
            },
            range: Some(span.range),
        });
        spans.push(SpanInfo {
            node_id,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
        });
    }

    let mut labels = Vec::with_capacity(validated.questions.len() + validated.answers.len());

    for question in &validated.questions {
        labels.push(Label {
            id: new_id("label"),
            kind: LabelKind::Question,
            src_node_id: span_node_ids[question.span].clone(),
            dst_node_id: None,
            status: Some(if question.resolved {
                LabelStatus::Resolved
            } else {
                LabelStatus::Open
            }),
            assignee: question.assignee.clone(),
            note_node_id: None,
            created_by: "agent".to_owned(),
            run_id: Some(run_id.clone()),
            created_at: created_at.clone(),
        });
    }

    for answer in &validated.answers {
        labels.push(Label {
            id: new_id("label"),
            kind: LabelKind::Answer,
            src_node_id: span_node_ids[answer.source].clone(),
            dst_node_id: Some(span_node_ids[answer.target].clone()),
            status: None,
            assignee: None,
            note_node_id: None,
            created_by: "agent".to_owned(),
            run_id: Some(run_id.clone()),
            created_at: created_at.clone(),
        });
    }

    let submit_input = serde_json::to_string(extraction).map_err(|error| {
        daftprompt_types::ParseError::Internal {
            detail: format!("could not record the accepted submit arguments: {error}"),
        }
    })?;

    let run = Run {
        id: run_id.clone(),
        agent: "scribe".to_owned(),
        provider_id: config.provider_id.clone(),
        model_id: config.model_id.clone(),
        prompt_version: Some(prompt::PROMPT_VERSION.to_owned()),
        started_at,
        ended_at: Some(now()),
        status: RunStatus::Done,
        attempts,
    };

    let run_inputs = vec![RunInput {
        run_id: run_id.clone(),
        node_id: root_node_id,
    }];
    let tool_calls = vec![ToolCall {
        id: new_id("tool"),
        run_id,
        seq: 0,
        tool: "submit".to_owned(),
        input: submit_input,
        output_hash: None,
        created_at,
    }];

    Ok(ParseResult {
        payload,
        source: request.source.clone(),
        nodes,
        node_contexts,
        labels,
        spans,
        run,
        run_inputs,
        tool_calls,
    })
}
