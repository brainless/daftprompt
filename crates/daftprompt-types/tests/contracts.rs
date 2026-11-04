//! Contract tests: the wire format clients and the model depend on.

use daftprompt_types::{
    AnnotationKind, CitationInfo, CitationLocation, ClarifyRequest, ClarifyResult, ContextTarget,
    ExplainerCitation, ExplainerExtraction, ExtractedSpan, Extraction, Grounding, Label, LabelKind,
    LabelStatus, Node, NodeContext, ParseError, ParseRequest, ParseResult, Payload, Run, RunInput,
    RunStatus, SourceKind, SourceMetadata, SourceRange, SpanInfo, ToolCall,
};

#[test]
fn extraction_deserializes_the_documented_shape() {
    let json = r#"{
        "spans": [
            { "id": "s1", "start_segment": 3, "end_segment": 3, "quote": "Should we cache tokens?" },
            { "id": "s2", "start_segment": 5, "end_segment": 6 }
        ],
        "annotations": [
            { "kind": "question", "span": "s1", "assignee": "@infra-team" },
            { "kind": "answer", "span": "s2", "answers": ["s1"] }
        ]
    }"#;

    let extraction: Extraction = serde_json::from_str(json).expect("extraction should parse");
    assert_eq!(extraction.spans.len(), 2);
    assert_eq!(
        extraction.spans[0],
        ExtractedSpan::Segments {
            id: "s1".to_owned(),
            start_segment: 3,
            end_segment: 3,
            quote: Some("Should we cache tokens?".to_owned()),
        }
    );
    assert!(matches!(
        extraction.spans[1],
        ExtractedSpan::Segments { quote: None, .. }
    ));
    assert_eq!(extraction.annotations[0].kind, AnnotationKind::Question);
    assert_eq!(extraction.annotations[0].answers, Vec::<String>::new());
    assert_eq!(
        extraction.annotations[1].kind,
        AnnotationKind::Answer,
        "annotation kinds are snake_case"
    );
    assert_eq!(extraction.annotations[1].assignee, None);
}

#[test]
fn empty_extraction_is_valid() {
    let extraction: Extraction = serde_json::from_str("{}").expect("empty extraction is valid");
    assert_eq!(extraction, Extraction::default());
}

#[test]
fn extraction_generates_a_tool_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(Extraction)).expect("schema");
    let properties = schema["properties"].as_object().expect("properties object");
    assert!(properties.contains_key("spans"));
    assert!(properties.contains_key("annotations"));

    let span_schema = serde_json::to_value(schemars::schema_for!(ExtractedSpan)).expect("schema");
    let variants = span_schema["anyOf"]
        .as_array()
        .expect("reference alternatives");
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0]["required"], serde_json::json!(["id", "block"]));
    assert_eq!(
        variants[1]["required"],
        serde_json::json!(["id", "start_segment", "end_segment"])
    );
    for variant in variants {
        assert_eq!(variant["additionalProperties"], false);
    }
}

#[test]
fn source_metadata_is_separate_from_text() {
    let request: ParseRequest = serde_json::from_str(
        r#"{
            "text": "the original text",
            "source": { "kind": "file", "reference": "docs/plan.md", "author": "alice" }
        }"#,
    )
    .expect("request should parse");

    assert_eq!(request.text, "the original text");
    let source = request.source.as_ref().expect("source");
    assert_eq!(source.kind, Some(SourceKind::File));
    assert_eq!(source.reference.as_deref(), Some("docs/plan.md"));
    assert_eq!(request.root_author(), "alice");
}

#[test]
fn root_author_falls_back_to_unknown() {
    assert_eq!(ParseRequest::new("text").root_author(), "unknown");
    assert_eq!(
        ParseRequest::with_source("text", SourceMetadata::default()).root_author(),
        "unknown"
    );
    assert_eq!(
        ParseRequest::with_source(
            "text",
            SourceMetadata {
                author: Some("   ".to_owned()),
                ..SourceMetadata::default()
            }
        )
        .root_author(),
        "unknown"
    );
    assert_eq!(
        ParseRequest::with_source(
            "text",
            SourceMetadata {
                kind: Some(SourceKind::Email),
                author: Some("bob".to_owned()),
                ..SourceMetadata::default()
            }
        )
        .root_author(),
        "bob"
    );
}

#[test]
fn graph_records_round_trip_as_json() {
    let result = ParseResult {
        content_type: daftprompt_types::ContentType::Plain,
        content_type_source: daftprompt_types::ContentTypeSource::Detected,
        payload: Payload {
            hash: "abc".to_owned(),
            content: "hello".to_owned(),
            created_at: "2026-10-31T00:00:00Z".to_owned(),
        },
        source: Some(SourceMetadata {
            kind: Some(SourceKind::Clipboard),
            reference: None,
            author: Some("alice".to_owned()),
        }),
        nodes: vec![Node {
            id: "n1".to_owned(),
            parent_id: None,
            author: "unknown".to_owned(),
            run_id: None,
            created_at: "2026-10-31T00:00:00Z".to_owned(),
        }],
        node_contexts: vec![NodeContext {
            id: "c1".to_owned(),
            node_id: "n1".to_owned(),
            seq: 0,
            target: ContextTarget::Payload {
                payload_hash: "abc".to_owned(),
            },
            range: Some(SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 6,
            }),
        }],
        labels: vec![Label {
            id: "l1".to_owned(),
            kind: LabelKind::Question,
            src_node_id: "n1".to_owned(),
            dst_node_id: None,
            status: Some(LabelStatus::Open),
            assignee: Some("@infra-team".to_owned()),
            note_node_id: None,
            created_by: "agent".to_owned(),
            run_id: None,
            created_at: "2026-10-31T00:00:00Z".to_owned(),
        }],
        spans: vec![SpanInfo {
            display_id: 1,
            node_id: "n1".to_owned(),
            start_byte: 0,
            end_byte: 5,
        }],
        run: Run {
            id: "r1".to_owned(),
            agent: "scribe".to_owned(),
            provider: Some("openai".to_owned()),
            model: Some("gpt-4o-mini".to_owned()),
            prompt_version: Some("scribe-v1".to_owned()),
            started_at: "2026-10-31T00:00:00Z".to_owned(),
            ended_at: Some("2026-10-31T00:00:01Z".to_owned()),
            status: RunStatus::Done,
            attempts: 1,
            consistency_followup: false,
            consistency_used: false,
        },
        run_inputs: vec![RunInput {
            run_id: "r1".to_owned(),
            node_id: "n1".to_owned(),
        }],
        tool_calls: vec![ToolCall {
            id: "t1".to_owned(),
            run_id: "r1".to_owned(),
            seq: 0,
            tool: "submit".to_owned(),
            input: "{}".to_owned(),
            output_hash: None,
            created_at: "2026-10-31T00:00:01Z".to_owned(),
        }],
    };

    let json = serde_json::to_string(&result).expect("serialize");
    let decoded: ParseResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, result);

    let value: serde_json::Value = serde_json::from_str(&json).expect("value");
    assert_eq!(value["node_contexts"][0]["target"]["kind"], "payload");
    assert_eq!(value["labels"][0]["kind"], "question");
    assert_eq!(value["run"]["status"], "done");
}

#[test]
fn errors_serialize_with_a_kind_discriminant() {
    let error = ParseError::InvalidOutput {
        detail: "quote matched twice".to_owned(),
        attempts: 3,
    };
    let value = serde_json::to_value(&error).expect("serialize");
    assert_eq!(value["kind"], "invalid_output");
    assert_eq!(value["attempts"], 3);
    assert_eq!(error.kind(), "invalid_output");
    assert_eq!(
        error.to_string(),
        "invalid model output after 3 attempt(s): quote matched twice"
    );
}

#[test]
fn every_error_kind_serializes_like_the_kind_method() {
    let cases = [
        ParseError::InvalidRequest {
            detail: String::new(),
        },
        ParseError::OversizedInput {
            detail: String::new(),
        },
        ParseError::InvalidOutput {
            detail: String::new(),
            attempts: 1,
        },
        ParseError::PayloadMismatch {
            detail: String::new(),
        },
        ParseError::InvalidTarget {
            detail: String::new(),
        },
        ParseError::Provider {
            detail: String::new(),
        },
        ParseError::Timeout {
            detail: String::new(),
        },
        ParseError::Internal {
            detail: String::new(),
        },
    ];

    for error in cases {
        let value = serde_json::to_value(&error).expect("serialize");
        assert_eq!(
            value["kind"],
            error.kind(),
            "wire kind must match ParseError::kind()"
        );
        let decoded: ParseError = serde_json::from_value(value).expect("deserialize");
        assert_eq!(decoded, error);
    }
}

#[test]
fn clarification_label_is_binary_and_non_blocking() {
    assert!(LabelKind::Clarification.is_binary());
    assert!(!LabelKind::Clarification.is_blocking());
    assert_eq!(
        serde_json::to_value(LabelKind::Clarification).expect("serialize"),
        "clarification"
    );
}

#[test]
fn clarify_request_deserializes_the_documented_shape() {
    let json = r#"{
        "source": {
            "text": "the original text",
            "payload_hash": "abc",
            "metadata": { "kind": "chat", "author": "ada" }
        },
        "target": { "node_id": "node-1", "display_id": 3, "start_byte": 4, "end_byte": 12 },
        "turns": [
            { "question_node_id": "q0", "question": "What?", "explanation_node_id": "e0", "explanation": "That." }
        ],
        "question": "Why not a fixed window?",
        "author": "ada"
    }"#;
    let request: ClarifyRequest = serde_json::from_str(json).expect("request should parse");
    assert_eq!(request.target.display_id, 3);
    assert_eq!(request.turns.len(), 1);
    assert_eq!(request.question_author(), "ada");
    assert_eq!(
        request.source.metadata.as_ref().and_then(|m| m.kind),
        Some(SourceKind::Chat)
    );

    let minimal: ClarifyRequest = serde_json::from_str(
        r#"{
            "source": { "text": "t", "payload_hash": "h" },
            "target": { "node_id": "n", "display_id": 1, "start_byte": 0, "end_byte": 1 },
            "question": "q"
        }"#,
    )
    .expect("turns, metadata and author are optional");
    assert!(minimal.turns.is_empty());
    assert_eq!(minimal.question_author(), "unknown");

    let round_trip: ClarifyRequest =
        serde_json::from_str(&serde_json::to_string(&request).expect("serialize"))
            .expect("deserialize");
    assert_eq!(round_trip, request);
}

#[test]
fn explainer_extraction_deserializes_both_citation_kinds() {
    let json = r#"{
        "explanation": "A token bucket allows bursts [1] as the text says [2].",
        "citations": [
            { "kind": "file", "path": "src/api/limit.rs", "start_line": 12, "end_line": 30 },
            { "kind": "source", "start_segment": 4, "end_segment": 5 }
        ]
    }"#;
    let extraction: ExplainerExtraction = serde_json::from_str(json).expect("extraction");
    assert_eq!(
        extraction.citations,
        vec![
            ExplainerCitation::File {
                path: "src/api/limit.rs".to_owned(),
                start_line: 12,
                end_line: 30,
            },
            ExplainerCitation::Source {
                start_segment: 4,
                end_segment: 5,
            },
        ]
    );

    let empty: ExplainerExtraction =
        serde_json::from_str(r#"{ "explanation": "Not found." }"#).expect("citations optional");
    assert!(empty.citations.is_empty());
    assert!(serde_json::from_str::<ExplainerExtraction>("{}").is_err());
    assert!(serde_json::from_str::<ExplainerCitation>(r#"{ "kind": "web", "url": "x" }"#).is_err());
}

#[test]
fn explainer_extraction_generates_a_tool_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(ExplainerExtraction)).expect("schema");
    let required = schema["required"].as_array().expect("required list");
    assert!(required.iter().any(|item| item == "explanation"));
    assert!(!required.iter().any(|item| item == "citations"));
    assert!(
        schema["properties"]
            .as_object()
            .expect("properties")
            .contains_key("citations")
    );
}

#[test]
fn clarify_result_round_trips_as_json() {
    let payload = |hash: &str, content: &str| Payload {
        hash: hash.to_owned(),
        content: content.to_owned(),
        created_at: "2026-10-31T00:00:00Z".to_owned(),
    };
    let result = ClarifyResult {
        question_payload: payload("hq", "What?"),
        explanation_payload: payload("he", "That [1]."),
        tool_payloads: vec![payload("ht", "1: fn limit()")],
        question_node_id: "q1".to_owned(),
        explanation_node_id: "e1".to_owned(),
        nodes: vec![Node {
            id: "q1".to_owned(),
            parent_id: Some("span-3".to_owned()),
            author: "ada".to_owned(),
            run_id: None,
            created_at: "2026-10-31T00:00:00Z".to_owned(),
        }],
        node_contexts: vec![NodeContext {
            id: "c1".to_owned(),
            node_id: "e1".to_owned(),
            seq: 1,
            target: ContextTarget::File {
                path: "src/api/limit.rs".to_owned(),
                content_hash: Some("filehash".to_owned()),
            },
            range: Some(SourceRange {
                start_line: 12,
                start_col: 1,
                end_line: 31,
                end_col: 1,
            }),
        }],
        labels: vec![Label {
            id: "l1".to_owned(),
            kind: LabelKind::Clarification,
            src_node_id: "q1".to_owned(),
            dst_node_id: Some("span-3".to_owned()),
            status: None,
            assignee: None,
            note_node_id: None,
            created_by: "ada".to_owned(),
            run_id: None,
            created_at: "2026-10-31T00:00:00Z".to_owned(),
        }],
        citations: vec![
            CitationInfo {
                index: 1,
                context_id: "c1".to_owned(),
                location: CitationLocation::File {
                    path: "src/api/limit.rs".to_owned(),
                    start_line: 12,
                    end_line: 30,
                },
            },
            CitationInfo {
                index: 2,
                context_id: "c2".to_owned(),
                location: CitationLocation::Source {
                    start_byte: 0,
                    end_byte: 9,
                },
            },
        ],
        grounding: Grounding::Project,
        source_window: None,
        run: Run {
            id: "r1".to_owned(),
            agent: "explainer".to_owned(),
            provider: Some("neuralwatt".to_owned()),
            model: Some("glm-5.3-flash".to_owned()),
            prompt_version: Some("explainer-v1".to_owned()),
            started_at: "2026-10-31T00:00:00Z".to_owned(),
            ended_at: Some("2026-10-31T00:00:01Z".to_owned()),
            status: RunStatus::Done,
            attempts: 1,
            consistency_followup: false,
            consistency_used: false,
        },
        run_inputs: vec![RunInput {
            run_id: "r1".to_owned(),
            node_id: "span-3".to_owned(),
        }],
        tool_calls: vec![ToolCall {
            id: "t1".to_owned(),
            run_id: "r1".to_owned(),
            seq: 0,
            tool: "read_file".to_owned(),
            input: "{}".to_owned(),
            output_hash: Some("ht".to_owned()),
            created_at: "2026-10-31T00:00:01Z".to_owned(),
        }],
    };

    let json = serde_json::to_string(&result).expect("serialize");
    let decoded: ClarifyResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, result);

    let value: serde_json::Value = serde_json::from_str(&json).expect("value");
    assert_eq!(value["grounding"], "project");
    assert_eq!(value["labels"][0]["kind"], "clarification");
    assert_eq!(value["labels"][0]["status"], serde_json::Value::Null);
    assert_eq!(value["citations"][0]["location"]["kind"], "file");
    assert_eq!(value["citations"][1]["location"]["kind"], "source");
    assert_eq!(
        serde_json::to_value(Grounding::TextOnly).expect("serialize"),
        "text_only"
    );
}

#[test]
fn content_type_contracts() {
    use daftprompt_types::{ContentType, ContentTypeSource};
    let request: ParseRequest =
        serde_json::from_value(serde_json::json!({"text":"# Plan", "content_type":"markdown"}))
            .unwrap();
    assert_eq!(request.content_type, Some(ContentType::Markdown));
    assert_eq!(ParseRequest::new("text").content_type, None);
    assert!(
        serde_json::from_value::<ParseRequest>(
            serde_json::json!({"text":"text", "content_type":"html"})
        )
        .is_err()
    );
    assert_eq!(serde_json::to_value(ContentType::Plain).unwrap(), "plain");
    assert_eq!(
        serde_json::to_value(ContentTypeSource::Declared).unwrap(),
        "declared"
    );
    assert_eq!(
        serde_json::to_value(ContentTypeSource::Detected).unwrap(),
        "detected"
    );
}

#[test]
fn block_span_contract_rejects_mixed_or_incomplete_references() {
    let block = serde_json::json!({"id":"s1", "block":"b8"});
    let span: ExtractedSpan = serde_json::from_value(block.clone()).unwrap();
    assert_eq!(span.id(), "s1");
    assert_eq!(serde_json::to_value(span).unwrap(), block);
    for invalid in [
        serde_json::json!({"id":"s1"}),
        serde_json::json!({"id":"s1", "start_segment":1}),
        serde_json::json!({"id":"s1", "end_segment":1}),
        serde_json::json!({"id":"s1", "block":"b1", "quote":"text"}),
        serde_json::json!({"id":"s1", "block":"b1", "quote":null}),
        serde_json::json!({"id":"s1", "block":"b1", "start_segment":1, "end_segment":1}),
    ] {
        assert!(
            serde_json::from_value::<ExtractedSpan>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}
