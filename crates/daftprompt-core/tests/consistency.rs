//! Sibling-consistency follow-up tests with scripted stub extractors.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use daftprompt_core::{BoxFuture, ExtractionModel, ModelError, ModelInput, Scribe, ScribeConfig};
use daftprompt_types::{Extraction, LabelKind, ParseRequest, ParseResult};

struct Scripted {
    script: Mutex<VecDeque<Result<Extraction, ModelError>>>,
    feedback: Mutex<Vec<Option<String>>>,
}

impl Scripted {
    fn new(script: Vec<Result<Extraction, ModelError>>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            feedback: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> usize {
        self.feedback.lock().unwrap().len()
    }
}

impl ExtractionModel for Scripted {
    fn extract(&self, input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        self.feedback.lock().unwrap().push(input.feedback);
        let next = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(ModelError::Invalid("script exhausted".into())));
        Box::pin(std::future::ready(next))
    }
}

/// Recommend each listed block, one span per block.
fn recommend(blocks: &[&str]) -> Extraction {
    let spans: Vec<_> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| serde_json::json!({"id": format!("s{i}"), "block": b}))
        .collect();
    let annotations: Vec<_> = (0..blocks.len())
        .map(|i| serde_json::json!({"kind": "recommendation", "span": format!("s{i}")}))
        .collect();
    serde_json::from_value(serde_json::json!({"spans": spans, "annotations": annotations})).unwrap()
}

async fn parse(
    text: &str,
    script: Vec<Result<Extraction, ModelError>>,
    config: ScribeConfig,
) -> (
    Result<ParseResult, daftprompt_types::ParseError>,
    Scribe<Scripted>,
) {
    let scribe = Scribe::new(config, Scripted::new(script));
    let result = scribe.parse(ParseRequest::new(text)).await;
    (result, scribe)
}

const EPIC: &str = "## Good to have\n\n- **A:** one\n- **B:** two\n- **C:** three\n- **D:** four\n";

fn recs(result: &ParseResult) -> usize {
    result
        .labels
        .iter()
        .filter(|l| l.kind == LabelKind::Recommendation)
        .count()
}

#[tokio::test]
async fn followup_completes_siblings_and_names_missing_blocks() {
    // b1 is the section; b2..b5 are the bullets.
    let script = vec![
        Ok(recommend(&["b2"])),
        Ok(recommend(&["b2", "b3", "b4", "b5"])),
    ];
    let (result, scribe) = parse(EPIC, script, ScribeConfig::default()).await;
    let result = result.unwrap();
    assert_eq!(recs(&result), 4);
    assert!(result.run.consistency_followup);
    assert!(result.run.consistency_used);
    assert_eq!(result.run.attempts, 1);
    let feedback = scribe.model().feedback.lock().unwrap().clone();
    assert_eq!(feedback.len(), 2);
    assert!(feedback[0].is_none());
    let text = feedback[1].as_deref().unwrap();
    assert!(text.contains("b2") && text.contains("b3, b4, b5"), "{text}");
    assert!(text.contains("recommendation"));
}

#[tokio::test]
async fn no_gap_makes_no_followup_call() {
    let script = vec![Ok(recommend(&["b2", "b3", "b4", "b5"]))];
    let (result, scribe) = parse(EPIC, script, ScribeConfig::default()).await;
    let result = result.unwrap();
    assert_eq!(scribe.model().calls(), 1);
    assert!(!result.run.consistency_followup && !result.run.consistency_used);
}

#[tokio::test]
async fn disabled_flag_makes_no_followup_call() {
    let script = vec![Ok(recommend(&["b2"]))];
    let config = ScribeConfig::default().with_consistency_followup(false);
    let (result, scribe) = parse(EPIC, script, config).await;
    assert_eq!(recs(&result.unwrap()), 1);
    assert_eq!(scribe.model().calls(), 1);
}

#[tokio::test]
async fn invalid_followup_keeps_the_original_result() {
    for second in [
        Ok(recommend(&["b2", "b2"])),
        Ok(recommend(&["b1", "b3"])),
        Err(ModelError::Provider("down".into())),
        Err(ModelError::Invalid("bad".into())),
    ] {
        let script = vec![Ok(recommend(&["b2"])), second];
        let (result, scribe) = parse(EPIC, script, ScribeConfig::default()).await;
        let result = result.unwrap();
        assert_eq!(recs(&result), 1);
        assert!(result.run.consistency_followup);
        assert!(!result.run.consistency_used);
        assert_eq!(scribe.model().calls(), 2);
    }
}

#[tokio::test]
async fn followup_timeout_keeps_the_original_result() {
    struct Slow(Mutex<u32>);
    impl ExtractionModel for Slow {
        fn extract(&self, _: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
            let first = {
                let mut n = self.0.lock().unwrap();
                *n += 1;
                *n == 1
            };
            if first {
                Box::pin(std::future::ready(Ok(recommend(&["b2"]))))
            } else {
                Box::pin(std::future::pending())
            }
        }
    }
    let config = ScribeConfig::default().with_timeout(Duration::from_millis(50));
    let result = Scribe::new(config, Slow(Mutex::new(0)))
        .parse(ParseRequest::new(EPIC))
        .await
        .unwrap();
    assert_eq!(recs(&result), 1);
    assert!(result.run.consistency_followup && !result.run.consistency_used);
}

#[tokio::test]
async fn unlabelled_sub_heading_sibling_is_not_a_gap() {
    // b1 section, b2 item, b3 item(?), then a sub-heading section after the list.
    let text = "## Plan\n\n- first\n- second\n\n### Details\n\nnotes here\n";
    // Label both bullets; the sub-heading section stays untouched.
    let script = vec![Ok(recommend(&["b2", "b3"]))];
    let (result, scribe) = parse(text, script, ScribeConfig::default()).await;
    result.unwrap();
    assert_eq!(scribe.model().calls(), 1);
}

#[tokio::test]
async fn parent_section_labelled_whole_is_not_a_gap() {
    let script = vec![Ok(recommend(&["b1"]))];
    let (result, scribe) = parse(EPIC, script, ScribeConfig::default()).await;
    assert_eq!(recs(&result.unwrap()), 1);
    assert_eq!(scribe.model().calls(), 1);
}
