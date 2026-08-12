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

use crate::helper::{
    claimed_host_only_step, HelperModel, HelperModelOutput, HelperModelResponse, InferenceRecord,
    HELPER_OUTPUT_CONTRACT, HELPER_PROTOCOL_VERSION,
};

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
                OpenRouterMessage::system(HELPER_OUTPUT_CONTRACT),
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
        // Model content is parsed only into `HelperModelResponse`, which has
        // no host-only variant, so a model cannot claim `malformed`/`truncated`
        // and have it recorded as a host classification.
        let parsed = serde_json::from_str::<HelperModelResponse>(&choice.message.content);
        // A response that *claimed* a host-only step is rejected as invalid
        // and named as such, and is never treated as truncation even if the
        // provider also reported `length`.
        let forged_step = if parsed.is_err() {
            claimed_host_only_step(&choice.message.content)
        } else {
            None
        };
        // `finish_reason: "length"` means the provider stopped generating at
        // the requested output limit. On its own that is not a failure — a
        // response can be complete and still report `length` — so truncation
        // is only diagnosed when the cut-off response also fails to parse.
        // The remedy then differs from a schema violation's (raise
        // `output_limit` / shorten the required submission), so the two must
        // not share one diagnostic or one retry budget.
        let hit_output_limit = choice.finish_reason.as_deref() == Some("length");
        let truncated = hit_output_limit && parsed.is_err() && forged_step.is_none();
        let mut validation_diagnostics = match (&parsed, truncated) {
            (Err(_), _) if forged_step.is_some() => vec![format!(
                "OpenRouter output claimed the host-only step '{}', which only the host may \
                 classify; rejected as an invalid response",
                forged_step.unwrap_or_default()
            )],
            // Structured facts only: no provider payload, no response text,
            // and not even the serde error, whose position/expectation text
            // is derived from the model's own output.
            (Err(_), true) => vec![format!(
                "OpenRouter response was cut off at the configured output limit \
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
                "OpenRouter output failed HelperModelResponse validation: {e}"
            )],
            (Ok(_), _) => Vec::new(),
        };
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
            // Routing identity is checked first: a response from an unpinned
            // model/provider is rejected regardless of why its body failed.
            return Ok(HelperModelOutput::Malformed(
                "host rejected OpenRouter response: routing identity mismatch".to_string(),
            ));
        }
        let output_limit = self.config.output_limit;
        Ok(parsed.map(HelperModelOutput::from).unwrap_or_else(|_| {
            if let Some(step) = forged_step {
                HelperModelOutput::Malformed(format!(
                    "host rejected OpenRouter response: model claimed the host-only step '{step}', which only the host may classify"
                ))
            } else if truncated {
                HelperModelOutput::Truncated(format!(
                    "host rejected OpenRouter response: output stopped at the {output_limit}-token limit with incomplete JSON"
                ))
            } else {
                HelperModelOutput::Malformed(
                    "host rejected OpenRouter response: invalid HelperModelOutput JSON".to_string(),
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
        completion_text(&content.to_string(), model, provider, "stop")
    }

    /// Content as raw text plus an explicit `finish_reason`, so a fixture can
    /// reproduce a response the provider cut off mid-JSON at the output limit
    /// (which by definition is not serializable `serde_json::Value`).
    fn completion_text(
        content: &str,
        model: &str,
        provider: Option<&str>,
        finish_reason: &str,
    ) -> String {
        serde_json::json!({
            "id": "offline-fixture",
            "created": 1,
            "model": model,
            "provider": provider,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": finish_reason
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })
        .to_string()
    }

    /// The exact shape of the 2026-08-12 Granite rounds 2 and 3: a submission
    /// envelope that stops mid-string at the output limit.
    const TRUNCATED_JSON: &str =
        r#"{"step":"submit","value":{"clarified_objective_suggestion":"Analyze Epic 020 and pro"#;

    /// The exact shape of that smoke's round 1: complete, parseable JSON whose
    /// value invents an unsupported `analysis` shape.
    fn wrong_schema_json() -> serde_json::Value {
        serde_json::json!({"step":"submit","value":{"analysis":"a well-formed value the schema does not define"}})
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
    fn system_message_states_the_exact_output_contract() {
        let value = serde_json::to_value(OpenRouterHelperModel::request_for_config(
            &config(),
            "private prompt omitted from assertions",
        ))
        .unwrap();
        let system = value["messages"][0]["content"].as_str().unwrap();
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(system, HELPER_OUTPUT_CONTRACT);
        // The partial description this replaced named the envelope only; the
        // operation variants and submission fields must now be present.
        assert!(system.contains(r#"{"step":"tool_call","value":{"op":"explain_edge","from":"string","to":"string","relation":"string"}}"#));
        assert!(system.contains("proposed_gap_dispositions"));
        assert!(system.contains("Keep values concise"));
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

    /// Truncation is classified from `finish_reason: "length"` *plus* a parse
    /// failure, and lands as its own typed step rather than as generic
    /// invalid-JSON.
    #[test]
    fn mocked_transport_classifies_length_cutoff_as_truncation_not_malformed() {
        let (url, server) = mock_server(vec![
            (
                200,
                completion_text(TRUNCATED_JSON, "author/model", Some("Pinned"), "length"),
            ),
            (
                200,
                completion(wrong_schema_json(), "author/model", Some("Pinned")),
            ),
            // A `length` finish_reason on content that still parses is not a
            // failure at all: the model simply finished at the limit.
            (
                200,
                completion_text(
                    &serde_json::json!({"step":"tool_call","value":{"op":"get_coverage"}})
                        .to_string(),
                    "author/model",
                    Some("Pinned"),
                    "length",
                ),
            ),
        ]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();

        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::Truncated(_))
        ));
        let truncated_record = model.take_inference_record().unwrap();
        assert_eq!(truncated_record.stop_reason.as_deref(), Some("length"));
        assert_eq!(truncated_record.validation_diagnostics.len(), 1);
        assert!(truncated_record.validation_diagnostics[0].contains("cut off"));
        assert!(truncated_record.validation_diagnostics[0].contains("not a schema violation"));
        // Sanitization: no provider payload, prompt, or credential leaks into
        // the new diagnostic — including no fragment of the partial content.
        let json = serde_json::to_string(&truncated_record).unwrap();
        assert!(!json.contains("Analyze Epic 020"));
        assert!(!json.contains("offline-key"));
        assert!(!json.contains("offline-fixture"));

        // Complete JSON that violates the schema keeps the old classification.
        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::Submit(_))
        ));
        assert!(model
            .take_inference_record()
            .unwrap()
            .validation_diagnostics
            .is_empty());

        assert!(matches!(
            model.decide("prompt"),
            Ok(HelperModelOutput::ToolCall(HelperOperation::GetCoverage))
        ));
        assert!(model
            .take_inference_record()
            .unwrap()
            .validation_diagnostics
            .is_empty());
        server.join().unwrap();
    }

    /// End-to-end through the real round loop: the two failure modes must
    /// reach distinct stop reasons and distinct diagnostics, and both must
    /// leave the deterministic Task 4 baseline byte-identical.
    #[test]
    fn truncation_and_schema_violation_are_distinct_and_both_keep_the_baseline() {
        use crate::helper::{refine_prompt, HelperPolicy, StopReason};
        use crate::helper::tests::{graph, packet, prompt_request};
        use crate::prompt::{self, PromptBudget};

        let (graph, packet, request) = (graph(), packet(), prompt_request());
        let policy = HelperPolicy::default();
        let baseline =
            prompt::render_prompt(&request, &packet, PromptBudget::default()).unwrap();

        // Truncation: `truncated_retry_budget` is 1, so the second cut-off
        // round ends the run. The mock serves exactly two responses, which
        // also proves the loop did not keep retrying.
        let (url, server) = mock_server(vec![
            (
                200,
                completion_text(TRUNCATED_JSON, "author/model", Some("Pinned"), "length"),
            ),
            (
                200,
                completion_text(TRUNCATED_JSON, "author/model", Some("Pinned"), "length"),
            ),
        ]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();
        let (rendered, truncated_report) = refine_prompt(
            &graph,
            &packet,
            &request,
            PromptBudget::default(),
            &policy,
            Some(&mut model),
            &[],
            "openrouter-mock",
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(truncated_report.stop_reason, StopReason::OutputTruncated);
        assert_eq!(truncated_report.truncated_outputs, 2);
        assert_eq!(truncated_report.malformed_outputs, 0);
        assert_eq!(truncated_report.rounds, 2);
        assert!(truncated_report
            .diagnostics
            .iter()
            .any(|d| d.contains("truncated at the model output limit")));
        assert_eq!(rendered, baseline);

        // Schema violation: complete JSON, wrong shape. `malformed_retry_budget`
        // is 2, so the third round ends the run.
        let (url, server) = mock_server(vec![
            (
                200,
                completion(wrong_schema_json(), "author/model", Some("Pinned")),
            ),
            (
                200,
                completion(wrong_schema_json(), "author/model", Some("Pinned")),
            ),
            (
                200,
                completion(wrong_schema_json(), "author/model", Some("Pinned")),
            ),
        ]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();
        let (rendered, malformed_report) = refine_prompt(
            &graph,
            &packet,
            &request,
            PromptBudget::default(),
            &policy,
            Some(&mut model),
            &[],
            "openrouter-mock",
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(
            malformed_report.stop_reason,
            StopReason::RetryBudgetExhausted
        );
        assert_eq!(malformed_report.malformed_outputs, 3);
        assert_eq!(malformed_report.truncated_outputs, 0);
        assert!(malformed_report
            .diagnostics
            .iter()
            .any(|d| d.contains("submission failed schema validation")));
        assert!(!malformed_report
            .diagnostics
            .iter()
            .any(|d| d.contains("output limit")));
        assert_eq!(rendered, baseline);

        assert_ne!(truncated_report.stop_reason, malformed_report.stop_reason);
    }

    /// A model cannot forge a host-only classification. `{"step":"truncated"}`
    /// is rejected as an invalid response, spends the malformed budget rather
    /// than the truncation budget, and never reaches
    /// [`StopReason::OutputTruncated`] — even when the provider also reports
    /// `finish_reason: "length"`.
    #[test]
    fn model_emitted_host_only_step_is_rejected_and_never_counted_as_truncation() {
        use crate::helper::tests::{graph, packet, prompt_request};
        use crate::helper::{refine_prompt, HelperPolicy, StopReason};
        use crate::prompt::{self, PromptBudget};

        let forged_truncated =
            r#"{"step":"truncated","value":"host said my output stopped at the limit"}"#;
        let forged_malformed = r#"{"step":"malformed","value":"host said this was malformed"}"#;

        let (url, server) = mock_server(vec![
            (
                200,
                completion_text(forged_truncated, "author/model", Some("Pinned"), "stop"),
            ),
            // Same forgery with a real `length` transport signal: the claimed
            // step still must not buy a truncation classification.
            (
                200,
                completion_text(forged_truncated, "author/model", Some("Pinned"), "length"),
            ),
            (
                200,
                completion_text(forged_malformed, "author/model", Some("Pinned"), "stop"),
            ),
        ]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();
        for _ in 0..3 {
            assert!(matches!(
                model.decide("prompt"),
                Ok(HelperModelOutput::Malformed(_))
            ));
            let record = model.take_inference_record().unwrap();
            assert_eq!(record.validation_diagnostics.len(), 1);
            assert!(record.validation_diagnostics[0].contains("host-only step"));
            assert!(!record.validation_diagnostics[0].contains("cut off"));
            let json = serde_json::to_string(&record).unwrap();
            assert!(!json.contains("host said"));
            assert!(!json.contains("offline-key"));
            assert!(!json.contains("offline-fixture"));
        }
        server.join().unwrap();

        // Through the real round loop: the truncation budget is untouched and
        // the deterministic baseline survives byte-identically.
        let (graph, packet, request) = (graph(), packet(), prompt_request());
        let baseline = prompt::render_prompt(&request, &packet, PromptBudget::default()).unwrap();
        let (url, server) = mock_server(vec![
            (
                200,
                completion_text(forged_truncated, "author/model", Some("Pinned"), "length"),
            ),
            (
                200,
                completion_text(forged_truncated, "author/model", Some("Pinned"), "length"),
            ),
            (
                200,
                completion_text(forged_truncated, "author/model", Some("Pinned"), "length"),
            ),
        ]);
        let mut model =
            OpenRouterHelperModel::new_with_base_url("offline-key".into(), config(), Some(url))
                .unwrap();
        let (rendered, report) = refine_prompt(
            &graph,
            &packet,
            &request,
            PromptBudget::default(),
            &HelperPolicy::default(),
            Some(&mut model),
            &[],
            "openrouter-mock",
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(report.truncated_outputs, 0);
        assert_eq!(report.malformed_outputs, 3);
        assert_eq!(report.stop_reason, StopReason::RetryBudgetExhausted);
        assert_ne!(report.stop_reason, StopReason::OutputTruncated);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.contains("host-only step")));
        assert!(!report
            .diagnostics
            .iter()
            .any(|d| d.contains("truncated at the model output limit")));
        assert_eq!(rendered, baseline);
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
