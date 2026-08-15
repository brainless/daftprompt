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
    /// Execute the prompt against the disposable worktree and return the
    /// agent result. `worktree_path` is the path of the `EvalWorktree` the
    /// caller created for this run; adapters that do not yet act on the
    /// worktree (patch-apply support lands separately) may ignore it.
    fn execute(&self, prompt: &str, worktree_path: &std::path::Path) -> anyhow::Result<AgentResult>;

    /// Adapter name for reporting.
    fn adapter_name(&self) -> &str;

    /// Model name for reporting.
    fn model_name(&self) -> &str;
}

// ── Scripted agent for replay/testing ──────────────────────────────────

/// A deterministic, credential-free scripted agent for offline replay.
/// Returns pre-recorded responses, cycling through them for multi-repetition
/// replay (e.g., 3 responses for 3 repetitions).
pub struct ScriptedCodingAgent {
    responses: Vec<AgentResult>,
    adapter_name: String,
    model_name: String,
    /// Current index for cycling through responses across repetitions.
    index: std::cell::Cell<usize>,
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
            index: std::cell::Cell::new(0),
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

    /// Reset the cycling index to 0 (for a new case or run).
    pub fn reset(&self) {
        self.index.set(0);
    }
}

impl CodingAgent for ScriptedCodingAgent {
    fn execute(&self, _prompt: &str, _worktree_path: &std::path::Path) -> anyhow::Result<AgentResult> {
        if self.responses.is_empty() {
            return Err(anyhow::anyhow!("scripted agent has no responses"));
        }
        let idx = self.index.get();
        let response = self.responses[idx % self.responses.len()].clone();
        self.index.set(idx + 1);
        Ok(response)
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
    fn execute(&self, prompt: &str, _worktree_path: &std::path::Path) -> anyhow::Result<AgentResult> {
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
    fn execute(&self, prompt: &str, _worktree_path: &std::path::Path) -> anyhow::Result<AgentResult> {
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

// ── Patch-apply coding agent ─────────────────────────────────────────────
//
// Design decision recorded in the 2026-08-14 "Task 6 worktree/agent boundary
// is unwired; patch-apply adapter chosen" experiment note in
// `epics/014-task-zero-prompt-lab.md`: close the worktree/agent gap with a
// single-shot patch-apply adapter, not a fully tool-enabled (multi-turn,
// read/list/edit) agent loop. The model gets one prompt, replies with one
// diff, and the host applies or rejects it. A larger tool-enabled experiment
// is deliberately deferred.

/// Instruction appended to the user-facing prompt telling the model to
/// respond with a single unified diff and nothing else that would break
/// patch parsing.
///
/// ## Diff extraction convention
///
/// The model is asked to wrap its diff in a fenced code block tagged
/// `diff`, e.g.:
///
/// `````text
/// ```diff
/// diff --git a/path/to/file b/path/to/file
/// --- a/path/to/file
/// +++ b/path/to/file
/// @@ -1,3 +1,4 @@
///  context line
/// +added line
///  context line
/// ```
/// `````
///
/// [`extract_diff_from_response`] looks for a ```` ```diff ```` fence first,
/// falls back to any fenced block whose content looks like a diff (contains
/// a `diff --git` line or a `--- `/`+++ ` header), and finally falls back to
/// treating the entire trimmed response as the diff if it starts with a diff
/// header. Prose before/after the fence is discarded. If none of these
/// patterns match, no diff is extracted — for a read-only/analysis request
/// this is the expected, correct outcome, not a parsing failure.
const PATCH_APPLY_INSTRUCTION: &str = "\n\n---\n\nRespond with a single unified diff (git-apply compatible) that implements the requested code change, and nothing else that would break patch parsing. Wrap the diff in a fenced code block tagged `diff`, like:\n\n```diff\ndiff --git a/path/to/file b/path/to/file\n--- a/path/to/file\n+++ b/path/to/file\n@@ -1,3 +1,4 @@\n context line\n+added line\n context line\n```\n\nYou may include brief prose before or after the fenced block, but the diff itself must be the only thing inside the fence. If the request does not require a code change (e.g. it is read-only analysis), do not include a diff fence at all.";

/// Which chat-completion transport backs a [`PatchApplyCodingAgent`].
pub enum PatchApplyBackend {
    OpenRouter {
        api_key: String,
        config: OpenRouterAgentConfig,
    },
    LlamaCpp {
        config: LlamaCppAgentConfig,
    },
}

/// A coding agent that requests a single unified diff from the model and
/// applies it to the disposable worktree via `git apply`, instead of
/// interpreting free-form prose or running a multi-turn tool loop.
///
/// ## Validation and scope
///
/// Before invoking `git apply`, [`validate_diff_paths`] rejects any diff
/// whose headers reference an absolute path or a `..` path segment, so the
/// model (or a prompt-injection attempt inside repository content the model
/// read) cannot make the patch escape `worktree_path`. `git apply` itself is
/// then run with `current_dir(worktree_path)`. This mirrors the closed,
/// host-validated posture `helper.rs`'s `HelperToolExecutor` uses for the
/// Task 5 helper's operation catalog: the model proposes, the host validates
/// and is the only party that executes.
///
/// ## Outcome representation
///
/// A malformed diff, an out-of-scope path, or a `git apply` failure is a
/// valid, informative scoring outcome, not a panic or an `Err`: `execute`
/// still returns `Ok(AgentResult { .. })`, with `diagnostics` describing
/// exactly what happened and why. `AgentResult.touched_files` is left empty
/// by this adapter (no text-heuristic guessing); a caller that wants to know
/// what actually changed on disk should read it from the worktree itself via
/// `EvalWorktree::capture_diff()`/`changed_files()` after `execute` returns,
/// not from this struct's output.
pub struct PatchApplyCodingAgent {
    backend: PatchApplyBackend,
}

impl PatchApplyCodingAgent {
    pub fn new_openrouter(api_key: String, config: OpenRouterAgentConfig) -> Self {
        Self {
            backend: PatchApplyBackend::OpenRouter { api_key, config },
        }
    }

    pub fn new_llama_cpp(config: LlamaCppAgentConfig) -> Self {
        Self {
            backend: PatchApplyBackend::LlamaCpp { config },
        }
    }
}

impl CodingAgent for PatchApplyCodingAgent {
    fn execute(&self, prompt: &str, worktree_path: &std::path::Path) -> anyhow::Result<AgentResult> {
        let augmented_prompt = format!("{prompt}{PATCH_APPLY_INSTRUCTION}");

        let (text, input_tokens, output_tokens, elapsed_ms, stop_reason) = match &self.backend {
            PatchApplyBackend::OpenRouter { api_key, config } => {
                complete_via_openrouter(api_key, config, &augmented_prompt)?
            }
            PatchApplyBackend::LlamaCpp { config } => {
                complete_via_llama_cpp(config, &augmented_prompt)?
            }
        };

        let mut diagnostics = Vec::new();
        let extracted = extract_diff_from_response(&text);

        let diff_for_result = match &extracted {
            None => {
                diagnostics.push(
                    "[no diff extracted] model response contained no fenced or bare unified diff"
                        .to_string(),
                );
                None
            }
            Some(diff) => {
                let (applied, apply_diagnostics) = apply_patch(diff, worktree_path);
                diagnostics.extend(apply_diagnostics);
                diagnostics.push(format!(
                    "[patch apply {}]",
                    if applied { "succeeded" } else { "failed" }
                ));
                Some(diff.clone())
            }
        };

        Ok(AgentResult {
            response_text: text,
            touched_files: vec![],
            diff: diff_for_result,
            input_tokens,
            output_tokens,
            elapsed_ms,
            stop_reason,
            diagnostics,
        })
    }

    fn adapter_name(&self) -> &str {
        match &self.backend {
            PatchApplyBackend::OpenRouter { .. } => "patch_apply_openrouter",
            PatchApplyBackend::LlamaCpp { .. } => "patch_apply_llama_cpp",
        }
    }

    fn model_name(&self) -> &str {
        match &self.backend {
            PatchApplyBackend::OpenRouter { config, .. } => &config.model,
            PatchApplyBackend::LlamaCpp { config } => &config.model,
        }
    }
}

/// Run one non-streaming chat completion against OpenRouter and return
/// `(text, input_tokens, output_tokens, elapsed_ms, stop_reason)`. Shares the
/// request shape used by [`OpenRouterCodingAgent::execute`] but is factored
/// out so [`PatchApplyCodingAgent`] does not have to duplicate the transport
/// wiring inline.
fn complete_via_openrouter(
    api_key: &str,
    config: &OpenRouterAgentConfig,
    prompt: &str,
) -> anyhow::Result<(String, u64, u64, u128, Option<String>)> {
    use llm_sdk::openrouter::{
        OpenRouterChatCompletionRequest, OpenRouterClient, OpenRouterDataCollection,
        OpenRouterMessage, OpenRouterProviderPreferences, OpenRouterRole,
    };

    let routing = OpenRouterProviderPreferences {
        order: Some(config.provider_order.clone()),
        allow_fallbacks: Some(false),
        require_parameters: Some(true),
        data_collection: Some(OpenRouterDataCollection::Deny),
        zdr: Some(true),
    };
    let client = OpenRouterClient::new(api_key)?
        .with_model(&config.model)
        .with_provider_preferences(routing);

    let request = OpenRouterChatCompletionRequest {
        model: config.model.clone(),
        messages: vec![OpenRouterMessage {
            role: OpenRouterRole::User,
            content: prompt.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }],
        max_completion_tokens: Some(config.max_tokens),
        temperature: Some(config.temperature),
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
    let stop_reason = response.choices.first().and_then(|c| c.finish_reason.clone());

    Ok((text, input_tokens, output_tokens, elapsed, stop_reason))
}

/// Run one non-streaming chat completion against a local llama.cpp server
/// and return `(text, input_tokens, output_tokens, elapsed_ms, stop_reason)`.
/// See [`complete_via_openrouter`] for the analogous OpenRouter helper.
fn complete_via_llama_cpp(
    config: &LlamaCppAgentConfig,
    prompt: &str,
) -> anyhow::Result<(String, u64, u64, u128, Option<String>)> {
    use llm_sdk::llama_cpp::{
        LlamaCppChatCompletionRequest, LlamaCppClient, LlamaCppMessage, LlamaCppRole,
    };

    let client = LlamaCppClient::new()?.with_base_url(&config.base_url);

    let request = LlamaCppChatCompletionRequest {
        model: config.model.clone(),
        messages: vec![LlamaCppMessage::new(LlamaCppRole::User, prompt)],
        max_tokens: Some(config.max_tokens),
        temperature: Some(config.temperature),
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
    let stop_reason = response.choices.first().and_then(|c| c.finish_reason.clone());

    Ok((text, input_tokens, output_tokens, elapsed, stop_reason))
}

/// Extract a unified diff from a model's free-form response text, following
/// the convention documented on [`PATCH_APPLY_INSTRUCTION`]. Returns `None`
/// when no diff-shaped content is found — a valid, expected outcome (e.g. a
/// read-only/analysis response), not an error.
fn extract_diff_from_response(text: &str) -> Option<String> {
    // 1. Prefer an explicit ```diff fence.
    if let Some(candidate) = extract_fenced(text, Some("diff")) {
        if looks_like_diff(&candidate) {
            return Some(candidate);
        }
    }
    // 2. Fall back to any fenced block whose content looks like a diff
    //    (models sometimes tag it ```patch, ```text, or leave it untagged).
    if let Some(candidate) = extract_fenced(text, None) {
        if looks_like_diff(&candidate) {
            return Some(candidate);
        }
    }
    // 3. Fall back to the whole trimmed response if it looks like a diff on
    //    its own, with no fence at all.
    let trimmed = text.trim();
    if looks_like_diff(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

/// Find the first fenced code block (```` ``` ````-delimited). If `lang` is
/// `Some`, only a fence whose opening tag matches (case-insensitively) is
/// returned; if `None`, the first fence of any (or no) tag is returned. An
/// unterminated fence (no closing ` ``` `) is treated as absent.
fn extract_fenced(text: &str, lang: Option<&str>) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed_line = lines[i].trim_start();
        if let Some(tag) = trimmed_line.strip_prefix("```") {
            let tag = tag.trim();
            let matches = match lang {
                Some(l) => tag.eq_ignore_ascii_case(l),
                None => true,
            };
            if matches {
                let mut body = Vec::new();
                let mut j = i + 1;
                while j < lines.len() && !lines[j].trim_start().starts_with("```") {
                    body.push(lines[j]);
                    j += 1;
                }
                if j < lines.len() {
                    return Some(body.join("\n"));
                }
                // Unterminated fence: keep scanning in case a later,
                // properly-closed fence exists.
            }
        }
        i += 1;
    }
    None
}

/// Heuristic check for whether `text` looks like a unified diff: one of its
/// first few lines starts with a `diff --git`, `--- `, or `+++ ` header.
fn looks_like_diff(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.lines().take(5).any(|line| {
        line.starts_with("diff --git ") || line.starts_with("--- ") || line.starts_with("+++ ")
    })
}

/// Validate that no path referenced by `diff_text`'s headers escapes the
/// worktree it will be applied against: no absolute paths, no `..` path
/// segments. `/dev/null` (used for new/deleted files) is always allowed.
/// Returns the first violation found, if any.
fn validate_diff_paths(diff_text: &str) -> Result<(), String> {
    for line in diff_text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            for token in rest.split_whitespace() {
                validate_one_path(token)?;
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            validate_one_path(rest.trim())?;
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            validate_one_path(rest.trim())?;
        }
    }
    Ok(())
}

/// Validate a single path token taken from a diff header (`a/…`, `b/…`, or
/// bare). Strips a common `a/`/`b/` prefix and an optional trailing
/// tab-separated timestamp before checking.
fn validate_one_path(raw: &str) -> Result<(), String> {
    let raw = raw.split('\t').next().unwrap_or(raw);
    if raw == "/dev/null" {
        return Ok(());
    }
    let stripped = raw
        .strip_prefix("a/")
        .or_else(|| raw.strip_prefix("b/"))
        .unwrap_or(raw);
    let path = std::path::Path::new(stripped);
    if path.is_absolute() {
        return Err(format!("diff references an absolute path: {stripped}"));
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "diff references a path outside the worktree (contains '..'): {stripped}"
        ));
    }
    Ok(())
}

/// Apply `diff_text` to `worktree_path` via `git apply`, strictly scoped to
/// that directory (`current_dir(worktree_path)`), after validating its paths
/// with [`validate_diff_paths`]. Returns `(applied_cleanly, diagnostics)`.
/// Never panics: a rejected path, a spawn failure, or a `git apply` failure
/// is reported through the returned diagnostics rather than an
/// unwrap/expect/panic, per AGENTS.md's "failures degrade, never panic".
fn apply_patch(diff_text: &str, worktree_path: &std::path::Path) -> (bool, Vec<String>) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut diagnostics = Vec::new();

    if let Err(e) = validate_diff_paths(diff_text) {
        diagnostics.push(format!("[rejected] path validation failed: {e}"));
        return (false, diagnostics);
    }

    let child = Command::new("git")
        .arg("apply")
        .arg("--whitespace=nowarn")
        .current_dir(worktree_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            diagnostics.push(format!("[error] failed to spawn git apply: {e}"));
            return (false, diagnostics);
        }
    };

    if let Some(stdin) = child.stdin.as_mut() {
        if let Err(e) = stdin.write_all(diff_text.as_bytes()) {
            diagnostics.push(format!(
                "[error] failed to write patch to git apply stdin: {e}"
            ));
            return (false, diagnostics);
        }
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            diagnostics.push(format!("[error] git apply did not complete: {e}"));
            return (false, diagnostics);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        diagnostics.push(format!("[git apply stdout] {}", stdout.trim()));
    }
    if !stderr.trim().is_empty() {
        diagnostics.push(format!("[git apply stderr] {}", stderr.trim()));
    }
    diagnostics.push(format!("[git apply exit status] {}", output.status));

    (output.status.success(), diagnostics)
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
        let result = agent.execute("any prompt", std::path::Path::new("/tmp")).unwrap();
        assert_eq!(result.response_text, "done");
        assert_eq!(result.touched_files, vec!["foo.rs"]);
    }

    #[test]
    fn scripted_agent_cycles_through_responses() {
        let agent = ScriptedCodingAgent::new(
            vec![
                AgentResult {
                    response_text: "first".into(),
                    touched_files: vec!["a.rs".into()],
                    diff: None,
                    input_tokens: 10,
                    output_tokens: 5,
                    elapsed_ms: 100,
                    stop_reason: Some("stop".into()),
                    diagnostics: vec![],
                },
                AgentResult {
                    response_text: "second".into(),
                    touched_files: vec!["b.rs".into()],
                    diff: None,
                    input_tokens: 20,
                    output_tokens: 10,
                    elapsed_ms: 200,
                    stop_reason: Some("stop".into()),
                    diagnostics: vec![],
                },
            ],
            "test".into(),
            "test-model".into(),
        );
        // First call returns first response
        let r1 = agent.execute("any", std::path::Path::new("/tmp")).unwrap();
        assert_eq!(r1.response_text, "first");
        // Second call returns second response
        let r2 = agent.execute("any", std::path::Path::new("/tmp")).unwrap();
        assert_eq!(r2.response_text, "second");
        // Third call cycles back to first
        let r3 = agent.execute("any", std::path::Path::new("/tmp")).unwrap();
        assert_eq!(r3.response_text, "first");
        // Reset returns to first
        agent.reset();
        let r4 = agent.execute("any", std::path::Path::new("/tmp")).unwrap();
        assert_eq!(r4.response_text, "first");
    }

    #[test]
    fn scripted_agent_errors_when_empty() {
        let agent = ScriptedCodingAgent::new(vec![], "test".into(), "test".into());
        assert!(agent.execute("prompt", std::path::Path::new("/tmp")).is_err());
    }

    #[test]
    fn scripted_agent_loads_c01_fixture() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/eval/scripted-agent-c01.json");
        let agent = ScriptedCodingAgent::from_fixture(&fixture_path).unwrap();
        let result = agent.execute("Analyze Epic 020", std::path::Path::new("/tmp")).unwrap();
        assert!(result.response_text.contains("Epic 020"));
        assert!(result.touched_files.is_empty());
        assert!(result.diff.is_none());
        assert_eq!(result.stop_reason.as_deref(), Some("stop"));
        assert!(result.input_tokens > 0);
        assert!(result.output_tokens > 0);
        assert_eq!(agent.adapter_name(), "scripted");
        assert_eq!(agent.model_name(), "scripted");
    }

    // ── Patch-apply adapter tests ────────────────────────────────────────

    /// Create a tiny git repo fixture with one committed file, `greeting.txt`
    /// containing "hello\n". Returns the `TempDir` guard (keep it alive for
    /// the duration of the test) and the repo path.
    fn init_tiny_git_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&path)
                .status()
                .expect("git command should spawn");
            assert!(status.success(), "git {:?} failed", args);
        };

        run(&["init", "--quiet"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("greeting.txt"), "hello\n").unwrap();
        run(&["add", "greeting.txt"]);
        run(&["commit", "--quiet", "-m", "initial"]);

        (dir, path)
    }

    #[test]
    fn well_formed_diff_applies_cleanly() {
        let (_dir, repo_path) = init_tiny_git_repo();

        let diff = "diff --git a/greeting.txt b/greeting.txt\n\
--- a/greeting.txt\n\
+++ b/greeting.txt\n\
@@ -1 +1 @@\n\
-hello\n\
+hello, world\n";

        let (applied, diagnostics) = apply_patch(diff, &repo_path);
        assert!(applied, "expected patch to apply cleanly; diagnostics: {diagnostics:?}");

        let content = std::fs::read_to_string(repo_path.join("greeting.txt")).unwrap();
        assert_eq!(content, "hello, world\n");
    }

    #[test]
    fn path_traversal_diff_is_rejected_without_applying() {
        let (_dir, repo_path) = init_tiny_git_repo();

        let diff = "diff --git a/../evil.txt b/../evil.txt\n\
--- a/../evil.txt\n\
+++ b/../evil.txt\n\
@@ -0,0 +1 @@\n\
+pwned\n";

        let (applied, diagnostics) = apply_patch(diff, &repo_path);
        assert!(!applied);
        assert!(
            diagnostics.iter().any(|d| d.contains("rejected")),
            "expected a rejection diagnostic, got: {diagnostics:?}"
        );

        // The escape target must not have been created anywhere.
        assert!(!repo_path.parent().unwrap().join("evil.txt").exists());
        // git apply must never have run against the repo (untouched worktree).
        let content = std::fs::read_to_string(repo_path.join("greeting.txt")).unwrap();
        assert_eq!(content, "hello\n");
    }

    #[test]
    fn absolute_path_diff_is_rejected_without_applying() {
        let (_dir, repo_path) = init_tiny_git_repo();

        let diff = "diff --git a/etc/passwd b/etc/passwd\n\
--- a/etc/passwd\n\
+++ /etc/passwd\n\
@@ -0,0 +1 @@\n\
+pwned\n";

        let (applied, diagnostics) = apply_patch(diff, &repo_path);
        assert!(!applied);
        assert!(diagnostics.iter().any(|d| d.contains("rejected")));
    }

    #[test]
    fn malformed_non_diff_response_is_rejected_gracefully() {
        let (_dir, repo_path) = init_tiny_git_repo();

        // Not a diff at all — should not panic, just fail to extract/apply.
        let text = "I looked at the code and everything seems fine. No changes needed.";
        assert!(extract_diff_from_response(text).is_none());

        // Even if fed straight to apply_patch, git apply must reject it
        // gracefully rather than the harness panicking.
        let (applied, diagnostics) = apply_patch(text, &repo_path);
        assert!(!applied);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn diff_extraction_pulls_diff_out_of_markdown_fence() {
        let response = "Sure, here's the fix:\n\n\
```diff\n\
diff --git a/greeting.txt b/greeting.txt\n\
--- a/greeting.txt\n\
+++ b/greeting.txt\n\
@@ -1 +1 @@\n\
-hello\n\
+hello, world\n\
```\n\n\
Let me know if you'd like anything else.";

        let extracted = extract_diff_from_response(response).expect("should extract a diff");
        assert!(extracted.starts_with("diff --git a/greeting.txt b/greeting.txt"));
        assert!(extracted.contains("+hello, world"));
        assert!(!extracted.contains("Sure, here's"));
        assert!(!extracted.contains("Let me know"));
    }

    #[test]
    fn diff_extraction_falls_back_to_untagged_fence() {
        let response = "```\ndiff --git a/x.txt b/x.txt\n--- a/x.txt\n+++ b/x.txt\n@@ -1 +1 @@\n-old\n+new\n```";
        let extracted = extract_diff_from_response(response).expect("should extract a diff");
        assert!(extracted.starts_with("diff --git a/x.txt b/x.txt"));
    }

    #[test]
    fn diff_extraction_returns_none_for_prose_only_response() {
        let response = "This request is read-only analysis; no code change is required.";
        assert!(extract_diff_from_response(response).is_none());
    }

    #[test]
    fn patch_apply_agent_reports_adapter_and_model_names() {
        let agent = PatchApplyCodingAgent::new_llama_cpp(LlamaCppAgentConfig::default());
        assert_eq!(agent.adapter_name(), "patch_apply_llama_cpp");
        assert_eq!(agent.model_name(), "qwen3.5-9b");

        let agent = PatchApplyCodingAgent::new_openrouter(
            "unused-key".into(),
            OpenRouterAgentConfig::default(),
        );
        assert_eq!(agent.adapter_name(), "patch_apply_openrouter");
        assert_eq!(agent.model_name(), "ibm-granite/granite-4.1-8b");
    }

    // ── End-to-end: worktree + patch-apply + scoring ──────────────────────
    //
    // Covers next-iteration item (5) from the 2026-08-14 "Task 6
    // worktree/agent boundary is unwired; patch-apply adapter chosen"
    // experiment note in `epics/014-task-zero-prompt-lab.md`: "add an
    // end-to-end test with a tiny fixture repo where a scripted patch both
    // fixes the target and makes an extraneous edit, asserting verification
    // catches both". This lives here (not in a new `tests/eval_e2e.rs`)
    // because it needs `apply_patch`, the exact private function
    // `PatchApplyCodingAgent::execute` calls to turn an extracted diff into
    // worktree edits — a `tests/` integration file only sees this crate's
    // public API and would have to reimplement patch application to reach
    // the same coverage. Everything else exercised here (`EvalWorktree`,
    // `scoring::score_task_outcome`, `eval::{AgentResult,
    // PracticalRelevanceSet, ExpectedArtifact}`) is already public and is
    // the same code `eval_runner.rs`'s `run_eval` calls.

    /// Create a tiny git repo fixture with two committed files: `calc.txt`
    /// (holds the bug the intended fix targets) and `notes.txt` (must remain
    /// untouched — the prohibited-change target for this test's extraneous
    /// edit).
    fn init_tiny_bugfix_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&path)
                .status()
                .expect("git command should spawn");
            assert!(status.success(), "git {:?} failed", args);
        };

        run(&["init", "--quiet"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("calc.txt"), "result = 1\n").unwrap();
        std::fs::write(path.join("notes.txt"), "unrelated notes\n").unwrap();
        run(&["add", "calc.txt", "notes.txt"]);
        run(&["commit", "--quiet", "-m", "initial"]);

        (dir, path)
    }

    #[test]
    fn end_to_end_worktree_patch_apply_and_scoring_catches_extraneous_edit() {
        // 1. Tiny fixture repo (disposable `TempDir`, removed on drop) with a
        //    known bug in `calc.txt` and an unrelated file, `notes.txt`,
        //    that the fix must not touch.
        let (_fixture_dir, repo_path) = init_tiny_bugfix_repo();

        // 2. A real `EvalWorktree`, created exactly as `eval_runner.rs`'s
        //    `run_eval` creates it: a disposable worktree at HEAD, auto-
        //    removed on drop (Design Constraint 7).
        let wt = crate::worktree::EvalWorktree::create(&repo_path, "HEAD", "e2e-patch-apply-test")
            .expect("worktree creation should succeed");
        assert!(
            wt.is_clean().unwrap(),
            "freshly created worktree should start clean"
        );

        // 3. A single scripted unified diff, shaped exactly as a model would
        //    return to the patch-apply adapter, doing two things at once:
        //    the intended fix to `calc.txt`, and an extraneous edit to
        //    `notes.txt` — not in the relevance set's expected changes, and
        //    explicitly prohibited.
        let diff_text = "diff --git a/calc.txt b/calc.txt\n\
--- a/calc.txt\n\
+++ b/calc.txt\n\
@@ -1 +1 @@\n\
-result = 1\n\
+result = 2\n\
diff --git a/notes.txt b/notes.txt\n\
--- a/notes.txt\n\
+++ b/notes.txt\n\
@@ -1 +1 @@\n\
-unrelated notes\n\
+unrelated notes changed by mistake\n";

        // Apply it via the exact function `PatchApplyCodingAgent::execute`
        // calls internally after extracting a diff from a model response —
        // no reimplementation of patch application in this test.
        let (applied, apply_diagnostics) = apply_patch(diff_text, wt.path());
        assert!(
            applied,
            "patch should apply cleanly; diagnostics: {apply_diagnostics:?}"
        );

        // 4. Ground truth read from the real worktree, captured before the
        //    guard would remove it — mirrors `run_eval`'s ordering in
        //    `eval_runner.rs`.
        let changed_files = wt.changed_files().expect("changed_files should succeed");
        let diff_captured = wt.capture_diff().expect("capture_diff should succeed");

        assert_eq!(
            changed_files.len(),
            2,
            "expected both files to show as changed: {changed_files:?}"
        );
        assert!(changed_files.contains(&"calc.txt".to_string()));
        assert!(changed_files.contains(&"notes.txt".to_string()));
        assert!(diff_captured.contains("-result = 1"));
        assert!(diff_captured.contains("+result = 2"));
        assert!(diff_captured.contains("unrelated notes changed by mistake"));

        // The model's own self-report deliberately claims something
        // entirely different from what actually happened in the worktree,
        // to prove scoring follows worktree ground truth, not agent
        // self-report (the fix scoring.rs's doc comment on
        // `score_task_outcome` describes).
        let agent_result = AgentResult {
            response_text: "I fixed the calc bug.".into(),
            touched_files: vec!["totally_different_file.txt".into()],
            diff: Some(diff_text.to_string()),
            input_tokens: 123,
            output_tokens: 45,
            elapsed_ms: 10,
            stop_reason: Some("stop".into()),
            diagnostics: apply_diagnostics,
        };

        // The case's known practical relevance set: `calc.txt` is expected
        // to change; `notes.txt` is prohibited from changing; a
        // verification command confirms the intended fix actually landed.
        let relevance = crate::eval::PracticalRelevanceSet {
            expected_changes: vec![crate::eval::ExpectedArtifact {
                path: "calc.txt".into(),
                should_change: true,
                expected_blob_hash: None,
                note: "the intended fix".into(),
            }],
            prohibited_changes: vec![crate::eval::ExpectedArtifact {
                path: "notes.txt".into(),
                should_change: false,
                expected_blob_hash: None,
                note: "must not be touched by this fix".into(),
            }],
            verification_commands: vec!["grep -q 'result = 2' calc.txt".into()],
            evidence_commits: vec![],
            evidence_source: "test fixture".into(),
        };

        // 5. The real scoring path.
        let score = crate::scoring::score_task_outcome(
            &agent_result,
            &changed_files,
            &relevance,
            wt.path(),
            "fix the bug in calc.txt",
        );

        // The intended fix was detected.
        assert_eq!(score.artifact_recall, 1.0);
        // Precision is reduced below 1.0 because `notes.txt` was also
        // touched and is not in the expected set: the harness correctly
        // penalizes the unrequested change.
        assert!(score.artifact_precision < 1.0);
        assert!((score.artifact_precision - 0.5).abs() < f64::EPSILON);
        // The extraneous edit is independently caught as a prohibited-change
        // violation, naming the extraneous file.
        assert_eq!(score.violated_prohibitions, vec!["notes.txt".to_string()]);
        assert!(!score.negative_constraints_respected);
        // Verification of the intended fix passes.
        assert_eq!(score.verification_passed, Some(true));
        // The agent's self-report doesn't match the real diff (it claims a
        // file that was never touched, and omits both files that were) —
        // that mismatch is itself flagged as an unsupported claim, proving
        // ground truth, not model self-report, drove the score.
        assert!(score.unsupported_claims > 0);

        // 6. Cleanup: `wt`'s `Drop` impl removes the worktree when this
        // function returns; `_fixture_dir`'s `Drop` impl removes the
        // fixture repo. Sanity-check the worktree really existed on disk
        // (i.e. this test exercised a real worktree, not a mock).
        assert!(wt.path().exists());
    }

    #[test]
    fn end_to_end_patch_created_file_is_present_in_worktree_ground_truth() {
        let (_fixture_dir, repo_path) = init_tiny_bugfix_repo();
        let wt =
            crate::worktree::EvalWorktree::create(&repo_path, "HEAD", "e2e-untracked-patch-test")
                .expect("worktree creation should succeed");

        let diff_text = "diff --git a/tests/regression.txt b/tests/regression.txt\n\
new file mode 100644\n\
--- /dev/null\n\
+++ b/tests/regression.txt\n\
@@ -0,0 +1 @@\n\
+regression covered\n";
        let (applied, diagnostics) = apply_patch(diff_text, wt.path());
        assert!(applied, "patch should apply cleanly: {diagnostics:?}");

        let changed_files = wt.changed_files().expect("changed_files should succeed");
        let captured_diff = wt.capture_diff().expect("capture_diff should succeed");

        assert_eq!(changed_files, vec!["tests/regression.txt"]);
        assert!(captured_diff.contains("diff --git a/tests/regression.txt b/tests/regression.txt"));
        assert!(captured_diff.contains("new file mode 100644"));
        assert!(captured_diff.contains("+regression covered"));
    }
}
