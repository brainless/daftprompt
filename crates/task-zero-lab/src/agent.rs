//! Coding agent trait and adapters for Task 6 evaluation.
//!
//! A [`CodingAgent`] takes a prompt and produces an [`AgentResult`] with the
//! model's response. The agent is a capable model (larger than the Task 5
//! helpers) that receives the generated prompt and produces code changes.
//!
//! ## Design
//!
//! The coding agent sends the prompt to a capable model via a chat completion
//! API. The model's response is captured as text. For scoring, the response
//! is then applied to the worktree (if the model produces a diff) or scored
//! as-is (for read-only cases).
//!
//! Unlike the Task 5 helper (which operates through a closed typed interface),
//! the coding agent receives a free-form prompt and produces free-form output.
//! This is deliberate: Task 6 measures what a capable model does with the
//! prompt, not whether it follows a protocol.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::eval::AgentResult;

// ── Trait ──────────────────────────────────────────────────────────────

/// A coding agent that receives a prompt and produces a result.
pub trait CodingAgent {
    /// Execute the prompt and return the agent result.
    fn execute(&self, prompt: &str) -> anyhow::Result<AgentResult>;

    /// Adapter name for reporting.
    fn adapter_name(&self) -> &str;

    /// Model name for reporting.
    fn model_name(&self) -> &str;
}

// ── Scripted agent for replay/testing ──────────────────────────────────

/// A deterministic, credential-free scripted agent for offline replay.
/// Returns pre-recorded responses in order.
pub struct ScriptedCodingAgent {
    responses: Vec<AgentResult>,
    adapter_name: String,
    model_name: String,
}

impl ScriptedCodingAgent {
    pub fn new(
        responses: Vec<AgentResult>,
        adapter_name: String,
        model_name: String,
    ) -> Self {
        Self {
            responses,
            adapter_name,
            model_name,
        }
    }

    pub fn from_fixture(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let responses: Vec<AgentResult> = serde_json::from_str(&text)?;
        Ok(Self::new(
            responses,
            "scripted".into(),
            "scripted".into(),
        ))
    }
}

impl CodingAgent for ScriptedCodingAgent {
    fn execute(&self, _prompt: &str) -> anyhow::Result<AgentResult> {
        self.responses
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("scripted agent has no responses"))
    }

    fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

// ── OpenRouter capable-model agent ─────────────────────────────────────

/// Configuration for an OpenRouter-based coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRouterAgentConfig {
    pub model: String,
    pub provider_order: Vec<String>,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl Default for OpenRouterAgentConfig {
    fn default() -> Self {
        Self {
            model: "ibm-granite/granite-4.1-8b".into(),
            provider_order: vec!["CoreWeave".into()],
            temperature: 0.0,
            max_tokens: 4096,
        }
    }
}

/// OpenRouter-based coding agent using the llm-sdk chat completion API.
pub struct OpenRouterCodingAgent {
    api_key: String,
    config: OpenRouterAgentConfig,
}

impl OpenRouterCodingAgent {
    pub fn new(api_key: String, config: OpenRouterAgentConfig) -> Self {
        Self { api_key, config }
    }
}

impl CodingAgent for OpenRouterCodingAgent {
    fn execute(&self, prompt: &str) -> anyhow::Result<AgentResult> {
        use llm_sdk::openrouter::{
            OpenRouterChatCompletionRequest, OpenRouterClient, OpenRouterDataCollection,
            OpenRouterMessage, OpenRouterProviderPreferences, OpenRouterRole,
        };

        let routing = OpenRouterProviderPreferences {
            order: Some(self.config.provider_order.clone()),
            allow_fallbacks: Some(false),
            require_parameters: Some(true),
            data_collection: Some(OpenRouterDataCollection::Deny),
            zdr: Some(true),
        };
        let client = OpenRouterClient::new(&self.api_key)?
            .with_model(&self.config.model)
            .with_provider_preferences(routing);

        let request = OpenRouterChatCompletionRequest {
            model: self.config.model.clone(),
            messages: vec![OpenRouterMessage {
                role: OpenRouterRole::User,
                content: prompt.to_string(),
                tool_calls: None,
                tool_call_id: None,
            }],
            max_completion_tokens: Some(self.config.max_tokens),
            temperature: Some(self.config.temperature),
            top_p: None,
            stop: None,
            stream: Some(false),
            tools: None,
            tool_choice: None,
            response_format: None,
            provider: None,
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let start = Instant::now();
        let response = runtime.block_on(client.create_chat_completion(request))?;
        let elapsed = start.elapsed().as_millis();

        let text = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let (input_tokens, output_tokens) = response
            .usage
            .map(|u| (u.prompt_tokens as u64, u.completion_tokens as u64))
            .unwrap_or((0, 0));

        let stop_reason = response
            .choices
            .first()
            .and_then(|c| c.finish_reason.clone());

        let touched = extract_touched_files(&text);

        Ok(AgentResult {
            response_text: text,
            touched_files: touched,
            diff: None,
            input_tokens,
            output_tokens,
            elapsed_ms: elapsed,
            stop_reason,
            diagnostics: vec![],
        })
    }

    fn adapter_name(&self) -> &str {
        "openrouter"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}

// ── llama.cpp capable-model agent ──────────────────────────────────────

/// Configuration for a local llama.cpp coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlamaCppAgentConfig {
    pub model: String,
    pub base_url: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl Default for LlamaCppAgentConfig {
    fn default() -> Self {
        Self {
            model: "qwen3.5-9b".into(),
            base_url: "http://localhost:8080".into(),
            temperature: 0.0,
            max_tokens: 4096,
        }
    }
}

/// Local llama.cpp coding agent.
pub struct LlamaCppCodingAgent {
    config: LlamaCppAgentConfig,
}

impl LlamaCppCodingAgent {
    pub fn new(config: LlamaCppAgentConfig) -> Self {
        Self { config }
    }
}

impl CodingAgent for LlamaCppCodingAgent {
    fn execute(&self, prompt: &str) -> anyhow::Result<AgentResult> {
        use llm_sdk::llama_cpp::{
            LlamaCppChatCompletionRequest, LlamaCppClient, LlamaCppMessage, LlamaCppRole,
        };

        let client = LlamaCppClient::new()?.with_base_url(&self.config.base_url);

        let request = LlamaCppChatCompletionRequest {
            model: self.config.model.clone(),
            messages: vec![LlamaCppMessage::new(LlamaCppRole::User, prompt)],
            max_tokens: Some(self.config.max_tokens),
            temperature: Some(self.config.temperature),
            top_p: None,
            stop: None,
            stream: Some(false),
            tools: None,
            parallel_tool_calls: None,
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let start = Instant::now();
        let response = runtime.block_on(client.create_chat_completion(request))?;
        let elapsed = start.elapsed().as_millis();

        let text = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        let (input_tokens, output_tokens) = response
            .usage
            .map(|u| (u.prompt_tokens as u64, u.completion_tokens as u64))
            .unwrap_or((0, 0));

        let stop_reason = response
            .choices
            .first()
            .and_then(|c| c.finish_reason.clone());

        let touched = extract_touched_files(&text);

        Ok(AgentResult {
            response_text: text,
            touched_files: touched,
            diff: None,
            input_tokens,
            output_tokens,
            elapsed_ms: elapsed,
            stop_reason,
            diagnostics: vec![],
        })
    }

    fn adapter_name(&self) -> &str {
        "llama_cpp"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}

// ── Utilities ──────────────────────────────────────────────────────────

/// Extract file paths mentioned in the agent's response.
/// Looks for common patterns: backtick paths, diff headers, file references.
fn extract_touched_files(text: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in text.lines() {
        // Diff headers: +++ b/path or --- a/path
        if line.starts_with("+++ b/") {
            files.push(line[6..].to_string());
        } else if line.starts_with("--- a/") {
            files.push(line[6..].to_string());
        }
        // "File: path" or "Modified: path" patterns
        let lower = line.to_lowercase();
        if lower.starts_with("file:")
            || lower.starts_with("modified:")
            || lower.starts_with("changed:")
            || lower.starts_with("created:")
        {
            if let Some(path) = line.split(':').nth(1) {
                let path = path.trim();
                if !path.is_empty() && path.contains('.') {
                    files.push(path.to_string());
                }
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_touched_files_from_diff_headers() {
        let text = "Here's the diff:\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n+new line";
        let files = extract_touched_files(text);
        assert!(files.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn extract_touched_files_from_file_references() {
        let text = "Modified: src/prompt.rs\nChanged: crates/lab/src/eval.rs";
        let files = extract_touched_files(text);
        assert!(files.contains(&"src/prompt.rs".to_string()));
        assert!(files.contains(&"crates/lab/src/eval.rs".to_string()));
    }

    #[test]
    fn extract_touched_files_deduplicates() {
        let text = "Modified: src/foo.rs\nModified: src/foo.rs\nModified: src/bar.rs";
        let files = extract_touched_files(text);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn scripted_agent_returns_first_response() {
        let agent = ScriptedCodingAgent::new(
            vec![AgentResult {
                response_text: "done".into(),
                touched_files: vec!["foo.rs".into()],
                diff: None,
                input_tokens: 10,
                output_tokens: 5,
                elapsed_ms: 100,
                stop_reason: Some("stop".into()),
                diagnostics: vec![],
            }],
            "test".into(),
            "test-model".into(),
        );
        let result = agent.execute("any prompt").unwrap();
        assert_eq!(result.response_text, "done");
        assert_eq!(result.touched_files, vec!["foo.rs"]);
    }

    #[test]
    fn scripted_agent_errors_when_empty() {
        let agent = ScriptedCodingAgent::new(vec![], "test".into(), "test".into());
        assert!(agent.execute("prompt").is_err());
    }

    #[test]
    fn scripted_agent_loads_c01_fixture() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/eval/scripted-agent-c01.json");
        let agent = ScriptedCodingAgent::from_fixture(&fixture_path).unwrap();
        let result = agent.execute("Analyze Epic 020").unwrap();
        assert!(result.response_text.contains("Epic 020"));
        assert!(result.touched_files.is_empty());
        assert!(result.diff.is_none());
        assert_eq!(result.stop_reason.as_deref(), Some("stop"));
        assert!(result.input_tokens > 0);
        assert!(result.output_tokens > 0);
        assert_eq!(agent.adapter_name(), "scripted");
        assert_eq!(agent.model_name(), "scripted");
    }
}
