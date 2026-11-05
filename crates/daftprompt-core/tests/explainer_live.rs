//! Opt-in live check that the configured model handles multi-turn project
//! tool calls together with the output tool. Not run by default.
//!
//! ```text
//! NEURALWATT_API_KEY=... cargo test -p daftprompt-core --features live-eval \
//!     --test explainer_live -- --ignored --nocapture
//! ```
//! The model comes from `DAFTPROMPT_EXPLAINER_PROVIDER`/`_MODEL` (default
//! `neuralwatt` / `glm-5.3-flash`); only `neuralwatt` is wired here.
#![cfg(feature = "live-eval")]

use std::sync::Arc;

use daftprompt_core::explainer::{Explainer, ExplainerConfig};
use daftprompt_core::project_tools::InMemoryProject;
use daftprompt_types::{ClarifyRequest, ClarifySource, ClarifyTarget};
use rig::providers::openai::OpenAIConfig;
use sha2::{Digest, Sha256};

#[tokio::test]
#[ignore = "calls a live model; needs NEURALWATT_API_KEY"]
async fn live_multi_turn_tool_calls_and_submit() {
    let config = ExplainerConfig::from_env();
    assert_eq!(
        config.provider.as_deref().unwrap_or("neuralwatt"),
        "neuralwatt"
    );
    let api_key = std::env::var("NEURALWATT_API_KEY").expect("set NEURALWATT_API_KEY");
    let base_url = std::env::var("NEURALWATT_BASE_URL")
        .unwrap_or_else(|_| "https://api.neuralwatt.com/v1".to_owned());
    let model = OpenAIConfig::new(api_key)
        .with_base_url(base_url)
        .client()
        .chat(config.model.clone().unwrap());

    let project = InMemoryProject::new().with_file(
        "src/limit.rs",
        "// A token bucket refills at a fixed rate and allows bursts up to its capacity.\n\
         pub struct TokenBucket { capacity: u32, refill_per_sec: u32 }\n",
    );
    let text = "We rate limit with a token bucket.\nIt is per user.";
    let start = text.find("token bucket").unwrap();
    let request = ClarifyRequest {
        source: ClarifySource {
            text: text.to_owned(),
            payload_hash: format!("{:x}", Sha256::digest(text.as_bytes())),
            metadata: None,
        },
        target: ClarifyTarget {
            node_id: "node-target".to_owned(),
            display_id: 1,
            start_byte: start,
            end_byte: start + "token bucket".len(),
        },
        turns: Vec::new(),
        question: "What does token bucket mean here? Check the project code.".to_owned(),
        author: None,
    };
    let explainer = Explainer::from_rig_model(config, model).with_project(Arc::new(project));
    let result = explainer.clarify(request).await.expect("clarify");
    println!("{:#?}", result.explanation_payload.content);
    println!(
        "tool calls: {:?}",
        result
            .tool_calls
            .iter()
            .map(|c| &c.tool)
            .collect::<Vec<_>>()
    );
    println!("citations: {:?}", result.citations);
    assert!(
        result.tool_calls.len() >= 2,
        "expected at least one tool call and submit"
    );
}
