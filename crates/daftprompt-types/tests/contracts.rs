//! Contract tests: the wire format clients and the model depend on.

use daftprompt_types::{
    AnnotationKind, ContextTarget, ExtractedSpan, Extraction, Label, LabelKind, LabelStatus, Node,
    NodeContext, ParseError, ParseRequest, ParseResult, Payload, Run, RunInput, RunStatus,
    SourceKind, SourceMetadata, SourceRange, SpanInfo, ToolCall,
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
        ExtractedSpan {
            id: "s1".to_owned(),
            start_segment: 3,
            end_segment: 3,
            quote: Some("Should we cache tokens?".to_owned()),
        }
    );
    assert_eq!(extraction.spans[1].quote, None);
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
    let required = span_schema["required"].as_array().expect("required list");
    for field in ["id", "start_segment", "end_segment"] {
        assert!(
            required.iter().any(|item| item == field),
            "{field} must be required"
        );
    }
    // The quote is optional, so the model can omit it.
    assert!(!required.iter().any(|item| item == "quote"));
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
