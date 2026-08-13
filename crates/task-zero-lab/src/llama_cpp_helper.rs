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

/// Convert LFM2.5 native tool-call format to JSON.
///
/// LFM2.5 models output tool calls as:
/// `<|tool_call_start|>[operation_name(arg1="value1")]<|tool_call_end|>`
///
/// or in a nested schema-mimicking form:
/// `<|tool_call_start|>[tool_call(step='tool_call', value={'op': '...', ...})]<|tool_call_end|>`
///
/// This function detects that format and converts it to the JSON envelope
/// expected by [`HelperModelResponse`]: `{"step":"tool_call","value":{...}}`.
///
/// Returns `None` if the content is not in native tool-call format (falls
/// through to normal JSON parsing).
fn convert_lfm_tool_call(content: &str) -> Option<String> {
    let trimmed = content.trim();
    let after_start = trimmed.strip_prefix("<|tool_call_start|>")?;
    let after_end = after_start.strip_suffix("<|tool_call_end|>")?;
    let inner = after_end.trim();
    // Expected: [operation_name(arg1="value1", arg2="value2")]
    let inner = inner.strip_prefix('[')?.strip_suffix(']')?;
    // Split at first '(' to get operation name and args.
    let paren_pos = inner.find('(')?;
    let op_name = &inner[..paren_pos];
    let args_str = inner[paren_pos + 1..].strip_suffix(')')?;

    // Handle the nested schema-mimicking form: tool_call(step='...', value={...})
    // In this case, the actual operation is inside the value dict.
    if op_name == "tool_call" {
        // Try to extract the 'value' dict which contains the real operation.
        // Look for value= pattern and extract the dict.
        if let Some(value_start) = args_str.find("value=") {
            let value_part = args_str[value_start + 6..].trim();
            // The value is a Python-style dict like {'op': 'search_graph', 'query': '...'}
            if let Some(dict_inner) = value_part.strip_prefix('{').and_then(|s| {
                // Find matching closing brace
                let mut depth = 1;
                for (i, c) in s.char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(&s[..i]);
                            }
                        }
                        _ => {}
                    }
                }
                None
            }) {
                return parse_python_dict_to_json(dict_inner);
            }
        }
    }

    // Standard direct format: [operation_name(args)]
    let mut value = serde_json::json!({"op": op_name});
    if !args_str.trim().is_empty() {
        parse_args_into_value(args_str, &mut value);
    }
    Some(serde_json::json!({"step": "tool_call", "value": value}).to_string())
}

/// Parse a Python-style dict body (without braces) into a JSON string
/// matching the `HelperModelResponse` envelope.
///
/// Handles: `{'op': 'search_graph', 'query': '...'}`
fn parse_python_dict_to_json(dict_body: &str) -> Option<String> {
    let mut op_name = None;
    let mut value = serde_json::json!({});

    for part in split_python_dict_entries(dict_body) {
        let part = part.trim();
        if let Some(colon_pos) = part.find(':') {
            let key = part[..colon_pos].trim().trim_matches('\'').trim_matches('"');
            let val = part[colon_pos + 1..].trim().trim_matches('\'').trim_matches('"');
            if key == "op" {
                op_name = Some(val.to_string());
            } else {
                value[key] = serde_json::Value::String(val.to_string());
            }
        }
    }

    let op = op_name?;
    value["op"] = serde_json::Value::String(op);
    Some(serde_json::json!({"step": "tool_call", "value": value}).to_string())
}

/// Split a Python dict body by top-level commas (not inside quotes or braces).
fn split_python_dict_entries(body: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut last_start = 0;

    for (i, c) in body.char_indices() {
        match c {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '{' if !in_single_quote && !in_double_quote => depth += 1,
            '}' if !in_single_quote && !in_double_quote => depth -= 1,
            ',' if !in_single_quote && !in_double_quote && depth == 0 => {
                entries.push(&body[last_start..i]);
                last_start = i + 1;
            }
            _ => {}
        }
    }
    if last_start < body.len() {
        entries.push(&body[last_start..]);
    }
    entries
}

/// Parse comma-separated key=value args into a JSON value.
fn parse_args_into_value(args_str: &str, value: &mut serde_json::Value) {
    for arg in args_str.split(',') {
        let arg = arg.trim();
        if let Some(eq_pos) = arg.find('=') {
            let key = arg[..eq_pos].trim();
            let val = arg[eq_pos + 1..].trim();
            // Strip surrounding quotes from string values.
            let val = val
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(val);
            value[key] = serde_json::Value::String(val.to_string());
        }
    }
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
        // Convert LFM2.5 native tool-call format to JSON if detected.
        // LFM2.5 models output `<|tool_call_start|>[op(args)]<|tool_call_end|>`
        // instead of JSON; convert before attempting deserialization.
        let (content, lfm_converted) = match convert_lfm_tool_call(content) {
            Some(json) => (json, true),
            None => (content.to_string(), false),
        };
        // Parse into HelperModelResponse (no host-only variants).
        let parsed = serde_json::from_str::<HelperModelResponse>(&content);
        // A response that claimed a host-only step is rejected as invalid.
        let forged_step = if parsed.is_err() {
            claimed_host_only_step(&content)
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
        if raw_content != content && !lfm_converted {
            validation_diagnostics.push(format!(
                "llama_cpp output wrapped in markdown code fences (stripped before parsing)"
            ));
        }
        // Report if native tool-call format was converted.
        if lfm_converted {
            validation_diagnostics.push(format!(
                "llama_cpp output used native tool-call format (converted to JSON before parsing)"
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
    fn convert_lfm_tool_call_handles_native_format() {
        let native = "<|tool_call_start|>[get_coverage()]<|tool_call_end|>";
        let json = convert_lfm_tool_call(native).unwrap();
        let parsed: crate::helper::HelperModelResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            crate::helper::HelperModelResponse::ToolCall(crate::helper::HelperOperation::GetCoverage)
        ));
    }

    #[test]
    fn convert_lfm_tool_call_handles_search_with_args() {
        let native = "<|tool_call_start|>[search_graph(query=\"Epic 020\")]<|tool_call_end|>";
        let json = convert_lfm_tool_call(native).unwrap();
        let parsed: crate::helper::HelperModelResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            crate::helper::HelperModelResponse::ToolCall(
                crate::helper::HelperOperation::SearchGraph { query },
            ) => assert_eq!(query, "Epic 020"),
            _ => panic!("expected SearchGraph"),
        }
    }

    #[test]
    fn convert_lfm_tool_call_returns_none_for_json() {
        let json = r#"{"step":"tool_call","value":{"op":"get_coverage"}}"#;
        assert!(convert_lfm_tool_call(json).is_none());
    }

    #[test]
    fn convert_lfm_tool_call_handles_nested_schema_form() {
        // LFM2.5 actual output form: tool_call(step='tool_call', value={...})
        let native = r#"<|tool_call_start|>[tool_call(step='tool_call', value={'op': 'search_graph', 'query': 'Epic 020 analysis'})]<|tool_call_end|>"#;
        let json = convert_lfm_tool_call(native).unwrap();
        let parsed: crate::helper::HelperModelResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            crate::helper::HelperModelResponse::ToolCall(
                crate::helper::HelperOperation::SearchGraph { query },
            ) => assert_eq!(query, "Epic 020 analysis"),
            _ => panic!("expected SearchGraph, got {:?}", parsed),
        }
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
