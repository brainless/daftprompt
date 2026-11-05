//! Opt-in live check that the configured model handles multi-turn project
//! tool calls together with the output tool. Not run by default.
//!
//! ```text
//! CEREBRAS_API_KEY=... DAFTPROMPT_EXPLAINER_PROVIDER=cerebras \
//!     DAFTPROMPT_EXPLAINER_MODEL=gpt-oss-120b \
//!     cargo test -p daftprompt-core --features live-eval \
//!     --test explainer_live -- --ignored --nocapture
//! ```
//! The model comes from `DAFTPROMPT_EXPLAINER_PROVIDER`/`_MODEL` (default
//! `neuralwatt` / `glm-5.3-flash`).
#![cfg(feature = "live-eval")]

use std::sync::Arc;

use daftprompt_core::explainer::{Explainer, ExplainerConfig};
use daftprompt_core::project_tools::InMemoryProject;
use daftprompt_types::{ClarifyRequest, ClarifySource, ClarifyTarget};
use rig::DynModel;
use rig::operation::Completion;
use rig::providers::openai::{OpenAI, OpenAIConfig};
use sha2::{Digest, Sha256};

#[tokio::test]
#[ignore = "calls a live model; needs the selected provider's API key"]
async fn live_multi_turn_tool_calls_and_submit() {
    let config = ExplainerConfig::from_env();
    let model_name = config.model.clone().unwrap();
    let model: DynModel<Completion> = match config.provider.as_deref().unwrap_or("neuralwatt") {
        "openai" => OpenAI::from_env()
            .expect("set OPENAI_API_KEY")
            .completion(model_name)
            .into(),
        provider @ ("neuralwatt" | "cerebras" | "groq") => {
            let (key_name, base_name, default_base) = if provider == "groq" {
                (
                    "GROQ_API_KEY",
                    "GROQ_BASE_URL",
                    "https://api.groq.com/openai/v1",
                )
            } else if provider == "cerebras" {
                (
                    "CEREBRAS_API_KEY",
                    "CEREBRAS_BASE_URL",
                    "https://api.cerebras.ai/v1",
                )
            } else {
                (
                    "NEURALWATT_API_KEY",
                    "NEURALWATT_BASE_URL",
                    "https://api.neuralwatt.com/v1",
                )
            };
            let api_key = std::env::var(key_name).unwrap_or_else(|_| panic!("set {key_name}"));
            let base_url = std::env::var(base_name).unwrap_or_else(|_| default_base.to_owned());
            OpenAIConfig::new(api_key)
                .with_base_url(base_url)
                .client()
                .chat(model_name)
                .into()
        }
        other => panic!("unsupported provider {other:?}"),
    };

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
