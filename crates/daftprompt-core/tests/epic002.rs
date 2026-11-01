use daftprompt_core::{BoxFuture, ExtractionModel, ModelError, ModelInput, Scribe, ScribeConfig};
use daftprompt_types::{Extraction, LabelKind, LabelStatus, ParseError, ParseRequest};
use serde_json::{Value, json};

struct Stub(Extraction);
impl ExtractionModel for Stub {
    fn extract(&self, _: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        Box::pin(std::future::ready(Ok(self.0.clone())))
    }
}
const TEXT: &str = "Should we cache?\nI suggest caching.\nUse Redis instead.\n@alex, please benchmark.\nWe selected Redis.";
fn extraction(annotations: Value) -> Extraction {
    serde_json::from_value(json!({"spans": [
        {"id":"r2","start_segment":3,"end_segment":3},
        {"id":"q","start_segment":1,"end_segment":1},
        {"id":"answer","start_segment":5,"end_segment":5},
        {"id":"task","start_segment":4,"end_segment":4},
        {"id":"r1","start_segment":2,"end_segment":2}
    ], "annotations":annotations}))
    .unwrap()
}
fn annotations() -> Value {
    json!([
        {"kind":"recommendation","span":"r2","questions":["q"],"alternatives":["r1"]},
        {"kind":"task","span":"task","assignee":"@alex"},
        {"kind":"recommendation","span":"r1","questions":["q"]},
        {"kind":"answer","span":"answer","answers":["q"]},
        {"kind":"question","span":"q"}
    ])
}
async fn parse(extraction: Extraction) -> Result<daftprompt_types::ParseResult, ParseError> {
    Scribe::new(ScribeConfig::new().with_max_attempts(1), Stub(extraction))
        .parse(ParseRequest::new(TEXT))
        .await
}
#[tokio::test]
async fn source_order_numbers_and_targets_survive_shuffled_model_order() {
    let result = parse(extraction(annotations())).await.unwrap();
    assert_eq!(
        result
            .spans
            .iter()
            .map(|s| s.display_id)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    let number = |id: &str| {
        result
            .spans
            .iter()
            .find(|s| s.node_id == id)
            .unwrap()
            .display_id
    };
    for label in &result.labels {
        match label.kind {
            LabelKind::RecommendationFor => {
                assert!([2, 3].contains(&number(&label.src_node_id)));
                assert_eq!(number(label.dst_node_id.as_deref().unwrap()), 1);
            }
            LabelKind::AlternativeTo => {
                assert_eq!(number(&label.src_node_id), 2);
                assert_eq!(number(label.dst_node_id.as_deref().unwrap()), 3);
            }
            LabelKind::Question => assert_eq!(label.status, Some(LabelStatus::Resolved)),
            LabelKind::Task => {
                assert_eq!(label.status, Some(LabelStatus::Open));
                assert_eq!(label.assignee.as_deref(), Some("@alex"));
            }
            _ => {}
        }
    }
}
#[tokio::test]
async fn recommendation_only_leaves_question_open_and_multi_labels_share_number() {
    let mut value = annotations();
    value[3] = json!({"kind":"task","span":"answer"});
    value
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"recommendation","span":"task"}));
    let result = parse(extraction(value)).await.unwrap();
    assert_eq!(result.spans.len(), 5);
    assert_eq!(
        result
            .labels
            .iter()
            .find(|l| l.kind == LabelKind::Question)
            .unwrap()
            .status,
        Some(LabelStatus::Open)
    );
    let task = &result.spans[3];
    assert_eq!(
        result
            .labels
            .iter()
            .filter(|l| l.src_node_id == task.node_id)
            .count(),
        2
    );
}
#[tokio::test]
async fn rejects_invalid_fields_targets_duplicates_and_self_links() {
    let mutations = vec![
        (0, "questions", json!(["missing"])),
        (0, "questions", json!(["task"])),
        (0, "questions", json!(["q", "q"])),
        (0, "questions", json!(["r2"])),
        (0, "alternatives", json!(["q"])),
        (0, "alternatives", json!(["r2"])),
        (0, "alternatives", json!(["r1", "r1"])),
        (0, "assignee", json!("@alex")),
        (0, "answers", json!(["q"])),
        (1, "questions", json!(["q"])),
        (1, "alternatives", json!(["r1"])),
        (1, "answers", json!(["q"])),
        (1, "assignee", json!("alex")),
        (1, "assignee", json!("@")),
        (1, "assignee", json!("@bad handle")),
        (3, "answers", json!([])),
        (3, "questions", json!(["q"])),
        (3, "assignee", json!("@alex")),
        (4, "alternatives", json!(["r1"])),
    ];
    for (index, field, value) in mutations {
        let mut a = annotations();
        a[index][field] = value.clone();
        assert!(
            parse(extraction(a)).await.is_err(),
            "accepted {index} {field} {value}"
        );
    }
    let mut a = annotations();
    a[2]["alternatives"] = json!(["r2"]);
    assert!(parse(extraction(a)).await.is_err(), "reversed duplicate");
    for index in [0, 1, 2, 4] {
        let mut a = annotations();
        let duplicate = a[index].clone();
        a.as_array_mut().unwrap().push(duplicate);
        assert!(parse(extraction(a)).await.is_err(), "duplicate unary");
    }
}
#[test]
fn model_status_and_display_ids_are_rejected_on_annotations() {
    for field in ["status", "display_id", "node_id"] {
        let mut a = annotations();
        a[0][field] = json!(1);
        let value = json!({"annotations":a,"spans":[]});
        assert!(serde_json::from_value::<Extraction>(value).is_err());
    }
}
#[tokio::test]
async fn empty_output_has_no_display_identifiers() {
    let result = parse(Extraction::default()).await.unwrap();
    assert!(result.spans.is_empty());
    assert!(result.labels.is_empty());
}

#[test]
fn new_label_shapes_and_blocking_rules() {
    for (kind, binary, blocking) in [
        (LabelKind::Recommendation, false, false),
        (LabelKind::RecommendationFor, true, false),
        (LabelKind::AlternativeTo, true, false),
        (LabelKind::Task, false, true),
    ] {
        assert_eq!(kind.is_binary(), binary);
        assert_eq!(kind.is_blocking(), blocking);
    }
}

#[tokio::test]
async fn standalone_recommendations_accept_explicit_empty_lists_and_no_transitive_links() {
    let mut a = annotations();
    a[0]["questions"] = json!([]);
    a[2]["questions"] = json!([]);
    let result = parse(extraction(a)).await.unwrap();
    assert_eq!(
        result
            .labels
            .iter()
            .filter(|l| l.kind == LabelKind::Recommendation)
            .count(),
        2
    );
    assert_eq!(
        result
            .labels
            .iter()
            .filter(|l| l.kind == LabelKind::RecommendationFor)
            .count(),
        0
    );
    assert_eq!(
        result
            .labels
            .iter()
            .filter(|l| l.kind == LabelKind::AlternativeTo)
            .count(),
        1
    );
}

#[tokio::test]
async fn separate_alternative_pairs_do_not_create_a_transitive_third_pair() {
    let a = json!([
        {"kind":"recommendation","span":"q","alternatives":["r1"]},
        {"kind":"recommendation","span":"r1","alternatives":["r2"]},
        {"kind":"recommendation","span":"r2"},
        {"kind":"task","span":"task"},
        {"kind":"task","span":"answer"}
    ]);
    let result = parse(extraction(a)).await.unwrap();
    assert_eq!(
        result
            .labels
            .iter()
            .filter(|l| l.kind == LabelKind::AlternativeTo)
            .count(),
        2
    );
}
