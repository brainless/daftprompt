//! Local llama.cpp adapter for Epic 014 Task 5 experiments.
//!
//! Talks to a locally-running `llama-server` via its OpenAI-compatible
//! `/v1/chat/completions` endpoint. No API key, provider routing, or network
//! access is required — the model runs entirely on the local machine.
//!
//! The adapter performs no tool execution: every response must deserialize as
//! [`crate::helper::HelperModelOutput`], after which the existing host loop
//! validates and executes the closed operation catalog.
//!
//! Known model quirks handled here:
//! - Qwen 3.5 thinking models return `reasoning_content` (ignored by the
//!   adapter — only `content` is parsed).
//! - Small models may wrap JSON in markdown code fences (` ```json ... ``` `);
//!   the adapter strips these before attempting deserialization.

use std::time::Instant;

use llm_sdk::llama_cpp::{
    LlamaCppChatCompletionRequest, LlamaCppClient, LlamaCppMessage, LlamaCppRole,
};

use crate::helper::{
    claimed_host_only_step, HelperModel, HelperModelOutput, HelperModelResponse, InferenceRecord,
    HELPER_OUTPUT_CONTRACT, HELPER_PROTOCOL_VERSION,
};

pub const DEFAULT_TEMPERATURE: f32 = 0.0;
pub const DEFAULT_OUTPUT_LIMIT: u32 = 2_048;

/// Strip leading/trailing markdown code fences that small thinking models
/// (e.g. Qwen 3.5) commonly wrap around JSON output.
///
/// Handles ` ```json\n...\n``` `, ` ```\n...\n``` `, and single-line fences.
/// Returns the original content unchanged if no fence is detected.
fn strip_markdown_code_fences(content: &str) -> &str {
    let trimmed = content.trim();
    if trimmed.starts_with("```") {
        // Find the end of the opening fence line.
        let after_open = trimmed.strip_prefix("```").unwrap();
        let after_lang = if let Some(newline_pos) = after_open.find('\n') {
            // Skip the optional language tag (e.g. "json").
            &after_open[newline_pos + 1..]
        } else {
            // No newline after opening fence — degenerate case.
            return trimmed;
        };
        // Strip the closing fence.
        if let Some(close_pos) = after_lang.rfind("```") {
            return after_lang[..close_pos].trim();
        }
    }
    trimmed
}

#[derive(Debug, Clone)]
pub struct LlamaCppHelperConfig {
    pub model: String,
    pub base_url: String,
    pub temperature: f32,
    pub output_limit: u32,
}

impl LlamaCppHelperConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.model.trim().is_empty(),
            "--llama-cpp-model must not be empty"
        );
        anyhow::ensure!(
            !self.base_url.trim().is_empty(),
            "--llama-cpp-url must not be empty"
        );
        anyhow::ensure!(
            self.temperature.is_finite() && (0.0..=1.0).contains(&self.temperature),
            "temperature must be between 0 and 1"
        );
        anyhow::ensure!(self.output_limit > 0, "output limit must be non-zero");
        Ok(())
    }
}

pub struct LlamaCppHelperModel {
    client: LlamaCppClient,
    config: LlamaCppHelperConfig,
    runtime: tokio::runtime::Runtime,
    latest_record: Option<InferenceRecord>,
}

impl LlamaCppHelperModel {
    pub fn new(config: LlamaCppHelperConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let client = LlamaCppClient::new()?.with_base_url(&config.base_url);
        Ok(Self {
            client,
            config,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            latest_record: None,
        })
    }

    fn request(&self, prompt: &str) -> LlamaCppChatCompletionRequest {
        LlamaCppChatCompletionRequest {
            model: self.config.model.clone(),
            messages: vec![
                LlamaCppMessage::new(LlamaCppRole::System, HELPER_OUTPUT_CONTRACT),
                LlamaCppMessage::new(LlamaCppRole::User, prompt),
            ],
            max_tokens: Some(self.config.output_limit),
            temperature: Some(self.config.temperature),
            top_p: None,
            stop: None,
            stream: Some(false),
            tools: None,
            parallel_tool_calls: None,
        }
    }
}

impl HelperModel for LlamaCppHelperModel {
    fn decide(&mut self, round_prompt: &str) -> anyhow::Result<HelperModelOutput> {
        self.latest_record = None;
        let started = Instant::now();
        let result = self
            .runtime
            .block_on(self.client.create_chat_completion(self.request(round_prompt)));
        let elapsed_ms = started.elapsed().as_millis();
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.latest_record = Some(InferenceRecord {
                    round: 0,
                    requested_model: self.config.model.clone(),
                    returned_model: String::new(),
                    upstream_provider: None,
                    provider_order: vec!["local".into()],
                    allow_fallbacks: false,
                    require_parameters: false,
                    data_collection: "none".into(),
                    zdr: false,
                    temperature: self.config.temperature,
                    output_limit: self.config.output_limit,
                    protocol_version: HELPER_PROTOCOL_VERSION.to_string(),
                    prompt_hash: daftprompt_indexer::db::content_hash(round_prompt.as_bytes()),
                    raw_response_hash: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    elapsed_ms,
                    stop_reason: None,
                    validation_diagnostics: vec![
                        "llama_cpp request failed; local server may be stopped".to_string(),
                    ],
                });
                return Err(error.into());
            }
        };
        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow::anyhow!("llama_cpp returned no choices"))?;
        // Strip markdown fences before parsing. Small thinking models
        // (Qwen 3.5) commonly wrap JSON in ```json ... ```.
        let raw_content = choice.message.content.clone().unwrap_or_default();
        let content = strip_markdown_code_fences(&raw_content);
        // Parse into HelperModelResponse (no host-only variants).
        let parsed = serde_json::from_str::<HelperModelResponse>(content);
        // A response that claimed a host-only step is rejected as invalid.
        let forged_step = if parsed.is_err() {
            claimed_host_only_step(content)
        } else {
            None
        };
        let hit_output_limit = choice.finish_reason.as_deref() == Some("length");
        let truncated = hit_output_limit && parsed.is_err() && forged_step.is_none();
        let mut validation_diagnostics = match (&parsed, truncated) {
            (Err(_), _) if forged_step.is_some() => vec![format!(
                "llama_cpp output claimed the host-only step '{}', which only the host may \
                 classify; rejected as an invalid response",
                forged_step.unwrap_or_default()
            )],
            (Err(_), true) => vec![format!(
                "llama_cpp response was cut off at the configured output limit \
                 (finish_reason=length, output_limit={}, completion_tokens={}); \
                 the partial content is incomplete JSON, not a schema violation",
                self.config.output_limit,
                response
                    .usage
                    .as_ref()
                    .map(|u| u.completion_tokens.to_string())
                    .unwrap_or_else(|| "unavailable".to_string()),
            )],
            (Err(e), false) => vec![format!(
                "llama_cpp output failed HelperModelResponse validation: {e}"
            )],
            (Ok(_), _) => Vec::new(),
        };
        // Report if fences were stripped (useful diagnostic for model behavior).
        if raw_content != content {
            validation_diagnostics.push(format!(
                "llama_cpp output wrapped in markdown code fences (stripped before parsing)"
            ));
        }
        self.latest_record = Some(InferenceRecord {
            round: 0,
            requested_model: self.config.model.clone(),
            returned_model: response.model,
            upstream_provider: None,
            provider_order: vec!["local".into()],
            allow_fallbacks: false,
            require_parameters: false,
            data_collection: "none".into(),
            zdr: false,
            temperature: self.config.temperature,
            output_limit: self.config.output_limit,
            protocol_version: HELPER_PROTOCOL_VERSION.to_string(),
            prompt_hash: daftprompt_indexer::db::content_hash(round_prompt.as_bytes()),
            raw_response_hash: daftprompt_indexer::db::content_hash(raw_content.as_bytes()),
            input_tokens: response.usage.as_ref().map(|u| u.prompt_tokens),
            output_tokens: response.usage.as_ref().map(|u| u.completion_tokens),
            elapsed_ms,
            stop_reason: choice.finish_reason.clone(),
            validation_diagnostics: validation_diagnostics.clone(),
        });
        let output_limit = self.config.output_limit;
        Ok(parsed.map(HelperModelOutput::from).unwrap_or_else(|_| {
            if let Some(step) = forged_step {
                HelperModelOutput::Malformed(format!(
                    "host rejected llama_cpp response: model claimed the host-only step '{step}', \
                     which only the host may classify"
                ))
            } else if truncated {
                HelperModelOutput::Truncated(format!(
                    "host rejected llama_cpp response: output stopped at the {output_limit}-token \
                     limit with incomplete JSON"
                ))
            } else {
                HelperModelOutput::Malformed(
                    "host rejected llama_cpp response: invalid HelperModelOutput JSON".to_string(),
                )
            }
        }))
    }

    fn take_inference_record(&mut self) -> Option<InferenceRecord> {
        self.latest_record.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn config() -> LlamaCppHelperConfig {
        LlamaCppHelperConfig {
            model: "qwen3.5-0.8b".into(),
            base_url: "http://127.0.0.1:18080".into(),
            temperature: 0.0,
            output_limit: 128,
        }
    }

    fn mock_server(responses: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 16 * 1024];
                let _ = stream.read(&mut request).unwrap();
                let reason = if status == 200 {
                    "OK"
                } else {
                    "Service Unavailable"
                };
                write!(stream, "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (url, handle)
    }

    fn completion(content: serde_json::Value, model: &str) -> String {
        completion_text(&content.to_string(), model, "stop")
    }

    fn completion_text(content: &str, model: &str, finish_reason: &str) -> String {
        serde_json::json!({
            "id": "local-fixture",
            "object": "chat.completion",
            "created": 1,
            "model": model,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": finish_reason
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })
        .to_string()
    }

    #[test]
    fn config_requires_non_empty_model_and_url() {
        let mut cfg = config();
        assert!(cfg.validate().is_ok());
        cfg.model = String::new();
        assert!(cfg.validate().is_err());
        cfg.model = "qwen3.5-0.8b".into();
        cfg.base_url = String::new();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn strip_markdown_code_fences_handles_json_fences() {
        let fenced = "```json\n{\"step\":\"submit\",\"value\":{}}\n```";
        assert_eq!(
            strip_markdown_code_fences(fenced),
            "{\"step\":\"submit\",\"value\":{}}"
        );
        let bare_fence = "```\n{\"step\":\"tool_call\",\"value\":{\"op\":\"get_coverage\"}}\n```";
        assert_eq!(
            strip_markdown_code_fences(bare_fence),
            "{\"step\":\"tool_call\",\"value\":{\"op\":\"get_coverage\"}}"
        );
        let unfenced = "{\"step\":\"submit\",\"value\":{}}";
        assert_eq!(strip_markdown_code_fences(unfenced), unfenced);
    }

    #[test]
    fn strip_markdown_code_fences_handles_inline_fence() {
        let inline = "```json\n{\"ok\":true}\n```";
        assert_eq!(strip_markdown_code_fences(inline), "{\"ok\":true}");
    }

    #[test]
    fn mocked_transport_accepts_tool_call_and_submission() {
        let tool = serde_json::json!({"step":"tool_call","value":{"op":"get_coverage"}});
        let submit = serde_json::json!({"step":"submit","value":{"clarifications":[],"selected_evidence":[],"candidate_claims":[],"proposed_gap_dispositions":[],"stop_reason":"done"}});
        let (url, server) = mock_server(vec![
            (200, completion(tool, "qwen3.5-0.8b")),
            (200, completion(submit, "qwen3.5-0.8b")),
        ]);
        let mut cfg = config();
        cfg.base_url = url;
        let mut model = LlamaCppHelperModel::new(cfg).unwrap();
        assert!(matches!(
            model.decide("hashed, never persisted"),
            Ok(HelperModelOutput::ToolCall(
                crate::helper::HelperOperation::GetCoverage
            ))
        ));
        let record = model.take_inference_record().unwrap();
        assert_eq!(record.returned_model, "qwen3.5-0.8b");
        assert_eq!(record.input_tokens, Some(10));
        assert!(record.validation_diagnostics.is_empty());
        assert!(matches!(
            model.decide("second round"),
            Ok(HelperModelOutput::Submit(_))
        ));
        server.join().unwrap();
    }

    #[test]
    fn mocked_transport_handles_fenced_json() {
        let tool = serde_json::json!({"step":"tool_call","value":{"op":"get_coverage"}});
        let fenced = format!("```json\n{}\n```", tool);
        let (url, server) = mock_server(vec![(200, completion_text(&fenced, "qwen3.5-0.8b", "stop"))]);
        let mut cfg = config();
        cfg.base_url = url;
        let mut model = LlamaCppHelperModel::new(cfg).unwrap();
        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::ToolCall(
                crate::helper::HelperOperation::GetCoverage
            ))
        ));
        let record = model.take_inference_record().unwrap();
        assert!(record
            .validation_diagnostics
            .iter()
            .any(|d| d.contains("markdown code fences")));
        server.join().unwrap();
    }

    #[test]
    fn mocked_transport_classifies_length_cutoff_as_truncation() {
        let truncated_json =
            r#"{"step":"submit","value":{"clarifications":["partial"#;
        let (url, server) = mock_server(vec![(
            200,
            completion_text(truncated_json, "qwen3.5-0.8b", "length"),
        )]);
        let mut cfg = config();
        cfg.base_url = url;
        let mut model = LlamaCppHelperModel::new(cfg).unwrap();
        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::Truncated(_))
        ));
        let record = model.take_inference_record().unwrap();
        assert_eq!(record.stop_reason.as_deref(), Some("length"));
        assert!(record.validation_diagnostics.iter().any(|d| d.contains("cut off")));
        server.join().unwrap();
    }

    #[test]
    fn mocked_transport_sanitizes_malformed_output() {
        let (url, server) = mock_server(vec![(
            200,
            completion(
                serde_json::json!({"unexpected": true}),
                "qwen3.5-0.8b",
            ),
        )]);
        let mut cfg = config();
        cfg.base_url = url;
        let mut model = LlamaCppHelperModel::new(cfg).unwrap();
        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::Malformed(_))
        ));
        assert!(!model
            .take_inference_record()
            .unwrap()
            .validation_diagnostics
            .is_empty());
        server.join().unwrap();
    }

    #[test]
    fn mocked_transport_failure_records_sanitized_diagnostic() {
        let (url, server) = mock_server(vec![(
            503,
            r#"{"error":{"message":"server overloaded","code":503}}"#.into(),
        )]);
        let mut cfg = config();
        cfg.base_url = url;
        let mut model = LlamaCppHelperModel::new(cfg).unwrap();
        assert!(model.decide("prompt").is_err());
        let record = model.take_inference_record().unwrap();
        assert!(record.validation_diagnostics[0].contains("llama_cpp request failed"));
        let json = serde_json::to_string(&record).unwrap();
        assert!(!json.contains("server overloaded"));
        server.join().unwrap();
    }

    #[test]
    fn model_emitted_host_only_step_is_rejected() {
        let forged =
            r#"{"step":"truncated","value":"host said my output stopped at the limit"}"#;
        let (url, server) = mock_server(vec![(
            200,
            completion_text(forged, "qwen3.5-0.8b", "stop"),
        )]);
        let mut cfg = config();
        cfg.base_url = url;
        let mut model = LlamaCppHelperModel::new(cfg).unwrap();
        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::Malformed(_))
        ));
        let record = model.take_inference_record().unwrap();
        assert!(record.validation_diagnostics[0].contains("host-only step"));
        server.join().unwrap();
    }
}
