//! Credentialed OpenRouter adapter for Epic 014 Task 5 experiments.
//!
//! The adapter performs no tool execution: every response must deserialize as
//! [`crate::helper::HelperModelOutput`], after which the existing host loop
//! validates and executes the closed operation catalog.

use std::time::Instant;

use llm_sdk::error::LlmError;
use llm_sdk::openrouter::{
    OpenRouterChatCompletionRequest, OpenRouterClient, OpenRouterDataCollection, OpenRouterMessage,
    OpenRouterProviderPreferences, OpenRouterResponseFormat,
};

use crate::helper::{HelperModel, HelperModelOutput, InferenceRecord, HELPER_PROTOCOL_VERSION};

pub const DEFAULT_TEMPERATURE: f32 = 0.0;
pub const DEFAULT_OUTPUT_LIMIT: u32 = 2_048;

fn sanitized_diagnostic(error: &LlmError) -> String {
    match error.openrouter_diagnostic() {
        Some((status, error_type)) => {
            format!("OpenRouter diagnostic: status={status} error_type={error_type}")
        }
        None => "OpenRouter diagnostic: status=unavailable error_type=unavailable".to_string(),
    }
}

fn routing_identity_diagnostics(
    requested_model: &str,
    provider_order: &[String],
    returned_model: &str,
    returned_provider: Option<&str>,
) -> Vec<String> {
    let mut diagnostics = Vec::new();
    if returned_model != requested_model {
        diagnostics.push(format!(
            "OpenRouter returned model identity '{}' instead of requested '{}'",
            returned_model, requested_model
        ));
    }
    match returned_provider {
        Some(provider) if !provider_order.iter().any(|p| p == provider) => {
            diagnostics.push(format!(
                "OpenRouter returned upstream provider '{}' outside pinned order {:?}",
                provider, provider_order
            ));
        }
        None => {
            diagnostics.push("OpenRouter response omitted upstream provider identity".to_string())
        }
        _ => {}
    }
    diagnostics
}

#[derive(Debug, Clone)]
pub struct OpenRouterHelperConfig {
    pub model: String,
    pub provider_order: Vec<String>,
    pub temperature: f32,
    pub output_limit: u32,
}

impl OpenRouterHelperConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        let model = self.model.trim();
        anyhow::ensure!(
            !model.is_empty() && model.contains('/'),
            "--openrouter-model must be an exact author/model ID"
        );
        anyhow::ensure!(
            model != "openrouter/free" && !model.ends_with("/auto"),
            "routed model aliases are not reproducible experiments"
        );
        anyhow::ensure!(
            !self.provider_order.is_empty()
                && self.provider_order.iter().all(|p| !p.trim().is_empty()),
            "at least one non-empty --openrouter-provider is required"
        );
        anyhow::ensure!(
            self.temperature.is_finite() && (0.0..=1.0).contains(&self.temperature),
            "temperature must be between 0 and 1"
        );
        anyhow::ensure!(self.output_limit > 0, "output limit must be non-zero");
        Ok(())
    }
}

pub struct OpenRouterHelperModel {
    client: OpenRouterClient,
    config: OpenRouterHelperConfig,
    runtime: tokio::runtime::Runtime,
    latest_record: Option<InferenceRecord>,
    diagnose: bool,
}

impl OpenRouterHelperModel {
    pub fn new(api_key: String, config: OpenRouterHelperConfig) -> anyhow::Result<Self> {
        Self::new_with_base_url(api_key, config, None)
    }

    /// Base-URL injection exists solely for credential-free transport tests.
    pub fn new_with_base_url(
        api_key: String,
        config: OpenRouterHelperConfig,
        base_url: Option<String>,
    ) -> anyhow::Result<Self> {
        config.validate()?;
        anyhow::ensure!(!api_key.is_empty(), "OPENROUTER_API_KEY is empty");
        let routing = OpenRouterProviderPreferences {
            order: Some(config.provider_order.clone()),
            allow_fallbacks: Some(false),
            require_parameters: Some(true),
            data_collection: Some(OpenRouterDataCollection::Deny),
            zdr: Some(true),
        };
        let mut client = OpenRouterClient::new(api_key)?
            .with_model(config.model.clone())
            .with_provider_preferences(routing);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        Ok(Self {
            client,
            config,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            latest_record: None,
            diagnose: false,
        })
    }

    /// Enable stderr-only sanitized transport diagnostics.
    pub fn with_diagnostics(mut self, diagnose: bool) -> Self {
        self.diagnose = diagnose;
        self
    }

    fn request(&self, prompt: &str) -> OpenRouterChatCompletionRequest {
        Self::request_for_config(&self.config, prompt)
    }

    fn request_for_config(
        config: &OpenRouterHelperConfig,
        prompt: &str,
    ) -> OpenRouterChatCompletionRequest {
        OpenRouterChatCompletionRequest {
            model: config.model.clone(),
            messages: vec![
                OpenRouterMessage::system("Return exactly one JSON object matching the task-zero-helper-v1 HelperModelOutput schema. Use {\"step\":\"tool_call\",\"value\":{\"op\":...}} or {\"step\":\"submit\",\"value\":{...}}. Do not return Markdown or prose."),
                OpenRouterMessage::user(prompt),
            ],
            max_completion_tokens: Some(config.output_limit),
            temperature: Some(config.temperature),
            top_p: None,
            stop: None,
            stream: Some(false),
            tools: None,
            tool_choice: None,
            response_format: Some(OpenRouterResponseFormat::json_object()),
            provider: Some(OpenRouterProviderPreferences {
                order: Some(config.provider_order.clone()),
                allow_fallbacks: Some(false),
                require_parameters: Some(true),
                data_collection: Some(OpenRouterDataCollection::Deny),
                zdr: Some(true),
            }),
        }
    }
}

impl HelperModel for OpenRouterHelperModel {
    fn decide(&mut self, round_prompt: &str) -> anyhow::Result<HelperModelOutput> {
        self.latest_record = None;
        let started = Instant::now();
        let result = self.runtime.block_on(
            self.client
                .create_chat_completion_with_metadata(self.request(round_prompt)),
        );
        let elapsed_ms = started.elapsed().as_millis();
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if self.diagnose {
                    eprintln!("{}", sanitized_diagnostic(&error));
                }
                self.latest_record = Some(InferenceRecord {
                    round: 0,
                    requested_model: self.config.model.clone(),
                    returned_model: String::new(),
                    upstream_provider: None,
                    provider_order: self.config.provider_order.clone(),
                    allow_fallbacks: false,
                    require_parameters: true,
                    data_collection: "deny".to_string(),
                    zdr: true,
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
                        "OpenRouter request failed; provider error details suppressed".to_string(),
                    ],
                });
                return Err(error.into());
            }
        };
        let response = result.response;
        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow::anyhow!("OpenRouter returned no choices"))?;
        let parsed = serde_json::from_str::<HelperModelOutput>(&choice.message.content);
        let validation_diagnostics = parsed
            .as_ref()
            .err()
            .map(|e| {
                vec![format!(
                    "OpenRouter output failed HelperModelOutput validation: {e}"
                )]
            })
            .unwrap_or_default();
        let mut validation_diagnostics = validation_diagnostics;
        let identity_diagnostics = routing_identity_diagnostics(
            &self.config.model,
            &self.config.provider_order,
            &response.model,
            response.provider.as_deref(),
        );
        let identity_matches = identity_diagnostics.is_empty();
        validation_diagnostics.extend(identity_diagnostics);
        self.latest_record = Some(InferenceRecord {
            round: 0,
            requested_model: self.config.model.clone(),
            returned_model: response.model,
            upstream_provider: response.provider,
            provider_order: self.config.provider_order.clone(),
            allow_fallbacks: false,
            require_parameters: true,
            data_collection: "deny".to_string(),
            zdr: true,
            temperature: self.config.temperature,
            output_limit: self.config.output_limit,
            protocol_version: HELPER_PROTOCOL_VERSION.to_string(),
            prompt_hash: daftprompt_indexer::db::content_hash(round_prompt.as_bytes()),
            raw_response_hash: daftprompt_indexer::db::content_hash(&result.raw_response),
            input_tokens: response.usage.as_ref().map(|u| u.prompt_tokens),
            output_tokens: response.usage.as_ref().map(|u| u.completion_tokens),
            elapsed_ms,
            stop_reason: choice.finish_reason.clone(),
            validation_diagnostics: validation_diagnostics.clone(),
        });
        if !identity_matches {
            return Ok(HelperModelOutput::Malformed(
                "host rejected OpenRouter response: routing identity mismatch".to_string(),
            ));
        }
        Ok(parsed.unwrap_or_else(|_| {
            HelperModelOutput::Malformed(
                "host rejected OpenRouter response: invalid HelperModelOutput JSON".to_string(),
            )
        }))
    }

    fn take_inference_record(&mut self) -> Option<InferenceRecord> {
        self.latest_record.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper::HelperOperation;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn diagnostic_contains_only_status_and_canonical_error_type() {
        let diagnostic =
            sanitized_diagnostic(&LlmError::openrouter_api_error(503, "provider_overloaded"));
        assert_eq!(
            diagnostic,
            "OpenRouter diagnostic: status=503 error_type=provider_overloaded"
        );
        assert!(!diagnostic.contains("message"));
        assert!(!diagnostic.contains("body"));
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

    fn config() -> OpenRouterHelperConfig {
        OpenRouterHelperConfig {
            model: "author/model".into(),
            provider_order: vec!["Pinned".into()],
            temperature: 0.0,
            output_limit: 128,
        }
    }

    fn completion(content: serde_json::Value, model: &str, provider: Option<&str>) -> String {
        serde_json::json!({
            "id": "offline-fixture",
            "created": 1,
            "model": model,
            "provider": provider,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content.to_string()},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })
        .to_string()
    }

    #[test]
    fn config_requires_exact_model_and_provider_pin() {
        let base = OpenRouterHelperConfig {
            model: "openrouter/free".into(),
            provider_order: vec!["Together".into()],
            temperature: 0.0,
            output_limit: 128,
        };
        assert!(base.validate().is_err());
        let mut valid = base;
        valid.model = "ibm-granite/granite-4.1-8b".into();
        assert!(valid.validate().is_ok());
        valid.provider_order.clear();
        assert!(valid.validate().is_err());
    }

    #[test]
    fn request_pins_privacy_routing_and_json_parameters() {
        let config = OpenRouterHelperConfig {
            model: "ibm-granite/granite-4.1-8b".into(),
            provider_order: vec!["Together".into()],
            temperature: 0.0,
            output_limit: 512,
        };
        let value = serde_json::to_value(OpenRouterHelperModel::request_for_config(
            &config,
            "private prompt omitted from assertions",
        ))
        .unwrap();
        assert_eq!(value["model"], "ibm-granite/granite-4.1-8b");
        assert_eq!(value["provider"]["order"], serde_json::json!(["Together"]));
        assert_eq!(value["provider"]["allow_fallbacks"], false);
        assert_eq!(value["provider"]["require_parameters"], true);
        assert_eq!(value["provider"]["data_collection"], "deny");
        assert_eq!(value["provider"]["zdr"], true);
        assert_eq!(value["response_format"]["type"], "json_object");
    }

    #[test]
    fn routing_identity_mismatches_are_diagnosed() {
        let providers = vec!["Pinned".to_string()];
        assert!(routing_identity_diagnostics(
            "author/model",
            &providers,
            "author/model",
            Some("Pinned")
        )
        .is_empty());
        let diagnostics = routing_identity_diagnostics(
            "author/model",
            &providers,
            "other/model",
            Some("Unexpected"),
        );
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn mocked_transport_accepts_tool_call_and_submission() {
        let tool = serde_json::json!({"step":"tool_call","value":{"op":"get_coverage"}});
        let submit = serde_json::json!({"step":"submit","value":{"clarifications":[],"selected_evidence":[],"candidate_claims":[],"proposed_gap_dispositions":[],"stop_reason":"done"}});
        let (url, server) = mock_server(vec![
            (200, completion(tool, "author/model", Some("Pinned"))),
            (200, completion(submit, "author/model", Some("Pinned"))),
        ]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();
        assert!(matches!(
            model.decide("hashed, never persisted"),
            Ok(HelperModelOutput::ToolCall(HelperOperation::GetCoverage))
        ));
        let record = model.take_inference_record().unwrap();
        assert_eq!(record.returned_model, "author/model");
        assert_eq!(record.input_tokens, Some(10));
        assert!(matches!(
            model.decide("second round"),
            Ok(HelperModelOutput::Submit(_))
        ));
        server.join().unwrap();
    }

    #[test]
    fn mocked_transport_sanitizes_malformed_and_identity_mismatch() {
        let (url, server) = mock_server(vec![
            (
                200,
                completion(
                    serde_json::json!({"unexpected": true}),
                    "author/model",
                    Some("Pinned"),
                ),
            ),
            (
                200,
                completion(
                    serde_json::json!({"step":"tool_call","value":{"op":"get_coverage"}}),
                    "other/model",
                    Some("Other"),
                ),
            ),
        ]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();
        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::Malformed(_))
        ));
        assert!(!model
            .take_inference_record()
            .unwrap()
            .validation_diagnostics
            .is_empty());
        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::Malformed(_))
        ));
        assert_eq!(
            model
                .take_inference_record()
                .unwrap()
                .validation_diagnostics
                .len(),
            2
        );
        server.join().unwrap();
    }

    #[test]
    fn mocked_transport_failure_records_only_sanitized_diagnostic() {
        let (url, server) = mock_server(vec![(
            503,
            r#"{"error":{"message":"provider-private-detail","code":503}}"#.into(),
        )]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();
        assert!(model.decide("prompt").is_err());
        let json = serde_json::to_string(&model.take_inference_record().unwrap()).unwrap();
        assert!(!json.contains("provider-private-detail"));
        assert!(json.contains("provider error details suppressed"));
        server.join().unwrap();
    }
}
