//! Explainer's configurable model settings and safety limits.

use std::time::Duration;

use crate::project_tools::ProjectToolConfig;
use crate::scribe::{DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_SEGMENTS};

/// Default provider: Explainer's choice is independent of Scribe's.
pub const DEFAULT_PROVIDER: &str = "neuralwatt";
/// Default model on [`DEFAULT_PROVIDER`].
pub const DEFAULT_MODEL: &str = "glm-5.3-flash";
/// Environment variable overriding the provider.
pub const PROVIDER_ENV: &str = "DAFTPROMPT_EXPLAINER_PROVIDER";
/// Environment variable overriding the model.
pub const MODEL_ENV: &str = "DAFTPROMPT_EXPLAINER_MODEL";

/// Default question size limit: 2 KiB.
pub const DEFAULT_MAX_QUESTION_BYTES: usize = 2 * 1024;
/// Default number of earlier turns accepted.
pub const DEFAULT_MAX_EARLIER_TURNS: usize = 8;
/// Default total size of earlier questions and explanations: 16 KiB.
pub const DEFAULT_MAX_HISTORY_BYTES: usize = 16 * 1024;
/// Default number of neighbouring segments on each side of the subject.
pub const DEFAULT_WINDOW_SEGMENTS: usize = 20;
/// Default model-call budget of one attempt (tool rounds plus the final call).
pub const DEFAULT_MAX_AGENT_TURNS: usize = 12;
/// Default total attempts, including invalid ones.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Default time allowed for one attempt, covering its whole tool loop.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Default number of citations accepted in one explanation.
pub const DEFAULT_MAX_CITATIONS: usize = 20;

/// Configuration for an [`Explainer`](super::Explainer).
///
/// Provider and model are metadata recorded on the run; the model itself is
/// supplied by the caller, so credentials never pass through this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainerConfig {
    /// Provider identifier recorded on the run.
    pub provider: Option<String>,
    /// Model identifier recorded on the run.
    pub model: Option<String>,
    /// Maximum question size in UTF-8 bytes.
    pub max_question_bytes: usize,
    /// Maximum number of earlier turns in a request.
    pub max_earlier_turns: usize,
    /// Maximum combined size of earlier questions and explanations.
    pub max_history_bytes: usize,
    /// Segments included before and after the subject span.
    pub window_segments: usize,
    /// Maximum source text size, as for parsing.
    pub max_input_bytes: usize,
    /// Maximum source segment count, as for parsing.
    pub max_segments: usize,
    /// Model-call budget of one attempt (a "turn" is one model call).
    pub max_agent_turns: usize,
    /// Maximum citations in one explanation.
    pub max_citations: usize,
    /// Total attempts, including invalid ones. Values below 1 count as 1.
    pub max_attempts: u32,
    /// Time allowed for one attempt, covering the whole tool loop.
    pub timeout: Duration,
    /// Caps for the project tools (calls per run, bytes per call, ...).
    pub tools: ProjectToolConfig,
}

impl Default for ExplainerConfig {
    fn default() -> Self {
        Self {
            provider: Some(DEFAULT_PROVIDER.to_owned()),
            model: Some(DEFAULT_MODEL.to_owned()),
            max_question_bytes: DEFAULT_MAX_QUESTION_BYTES,
            max_earlier_turns: DEFAULT_MAX_EARLIER_TURNS,
            max_history_bytes: DEFAULT_MAX_HISTORY_BYTES,
            window_segments: DEFAULT_WINDOW_SEGMENTS,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_segments: DEFAULT_MAX_SEGMENTS,
            max_agent_turns: DEFAULT_MAX_AGENT_TURNS,
            max_citations: DEFAULT_MAX_CITATIONS,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            timeout: DEFAULT_TIMEOUT,
            tools: ProjectToolConfig::default(),
        }
    }
}

impl ExplainerConfig {
    /// The defaults: provider `neuralwatt`, model `glm-5.3-flash`.
    pub fn new() -> Self {
        Self::default()
    }

    /// The defaults with provider and model overridden by
    /// `DAFTPROMPT_EXPLAINER_PROVIDER` and `DAFTPROMPT_EXPLAINER_MODEL`.
    /// Blank values are ignored. No credential is read here.
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Like [`from_env`](Self::from_env) over any variable lookup.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let get = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let mut config = Self::default();
        if let Some(provider) = get(PROVIDER_ENV) {
            config.provider = Some(provider);
        }
        if let Some(model) = get(MODEL_ENV) {
            config.model = Some(model);
        }
        config
    }

    /// Record the provider identifier on the run.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Record the model identifier on the run.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the question size limit.
    pub fn with_max_question_bytes(mut self, bytes: usize) -> Self {
        self.max_question_bytes = bytes;
        self
    }

    /// Set the earlier-turn count and combined size limits.
    pub fn with_history_limits(mut self, max_turns: usize, max_bytes: usize) -> Self {
        self.max_earlier_turns = max_turns;
        self.max_history_bytes = max_bytes;
        self
    }

    /// Set the number of segments included on each side of the subject.
    pub fn with_window_segments(mut self, segments: usize) -> Self {
        self.window_segments = segments;
        self
    }

    /// Set the source text byte and segment limits.
    pub fn with_input_limits(mut self, max_input_bytes: usize, max_segments: usize) -> Self {
        self.max_input_bytes = max_input_bytes;
        self.max_segments = max_segments;
        self
    }

    /// Set the model-call budget of one attempt.
    pub fn with_max_agent_turns(mut self, turns: usize) -> Self {
        self.max_agent_turns = turns;
        self
    }

    /// Set the total attempt budget.
    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// Set the time allowed for one attempt's whole tool loop.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the project tool caps.
    pub fn with_tool_config(mut self, tools: ProjectToolConfig) -> Self {
        self.tools = tools;
        self
    }

    pub(crate) fn effective_max_attempts(&self) -> u32 {
        self.max_attempts.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_independent_of_scribe() {
        let config = ExplainerConfig::default();
        assert_eq!(config.provider.as_deref(), Some("neuralwatt"));
        assert_eq!(config.model.as_deref(), Some("glm-5.3-flash"));
        assert_eq!(config.window_segments, 20);
        assert_eq!(config.max_question_bytes, 2048);
    }

    #[test]
    fn environment_overrides_provider_and_model_and_ignores_blanks() {
        let config = ExplainerConfig::from_lookup(|name| match name {
            PROVIDER_ENV => Some("openai".to_owned()),
            MODEL_ENV => Some("  ".to_owned()),
            _ => None,
        });
        assert_eq!(config.provider.as_deref(), Some("openai"));
        assert_eq!(config.model.as_deref(), Some("glm-5.3-flash"));

        let config = ExplainerConfig::from_lookup(|name| {
            (name == MODEL_ENV).then(|| " other-model ".to_owned())
        });
        assert_eq!(config.model.as_deref(), Some("other-model"));
    }
}
