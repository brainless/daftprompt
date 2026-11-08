//! Surveyor's configurable model settings and safety limits.

use std::time::Duration;

use crate::llm::ModelSettings;
use crate::project_tools::ProjectToolConfig;

/// Default provider: the same as Explainer's default.
pub const DEFAULT_PROVIDER: &str = crate::explainer::DEFAULT_PROVIDER;
/// Default model on [`DEFAULT_PROVIDER`]: the same as Explainer's default.
pub const DEFAULT_MODEL: &str = crate::explainer::DEFAULT_MODEL;
/// Environment variable overriding the provider.
pub const PROVIDER_ENV: &str = "DAFTPROMPT_SURVEYOR_PROVIDER";
/// Environment variable overriding the model.
pub const MODEL_ENV: &str = "DAFTPROMPT_SURVEYOR_MODEL";
/// Environment variable selecting the [`ToolStrategy`] (`chat` or `restate`).
pub const STRATEGY_ENV: &str = "DAFTPROMPT_SURVEYOR_STRATEGY";

/// Default model-call budget of one attempt (tool rounds plus the final call).
pub const DEFAULT_MAX_AGENT_TURNS: usize = 12;
/// Default total attempts, including invalid ones.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Default time allowed for one attempt, covering its whole tool loop.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Default number of citations accepted in one summary.
pub const DEFAULT_MAX_CITATIONS: usize = 20;
/// Default summary size limit: 16 KiB.
pub const DEFAULT_MAX_SUMMARY_BYTES: usize = 16 * 1024;
/// Default user prompt size limit: 2 KiB.
pub const DEFAULT_MAX_PROMPT_BYTES: usize = 2 * 1024;
/// Longest accepted author name.
pub const MAX_AUTHOR_BYTES: usize = 200;

/// How a Rig-backed Surveyor carries tool results between model calls.
///
/// Experimental: the two strategies are compared by measurement, so the
/// strategy is part of the cache identity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolStrategy {
    /// One multi-turn conversation; tool results accumulate as chat messages.
    #[default]
    Chat,
    /// Every model call is a fresh, stateless request: the task prompt plus
    /// an evidence section rebuilt (and merged) from all tool results so far.
    Restate,
}

impl ToolStrategy {
    /// The value accepted by [`STRATEGY_ENV`] and recorded in the cache key.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Restate => "restate",
        }
    }

    /// Parse `chat` or `restate`, ignoring case. `None` for anything else.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chat" => Some(Self::Chat),
            "restate" => Some(Self::Restate),
            _ => None,
        }
    }
}

/// Configuration for a [`Surveyor`](super::Surveyor).
///
/// Provider and model are metadata recorded on the run; the model itself is
/// supplied by the caller, so credentials never pass through this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyorConfig {
    /// Provider identifier recorded on the run.
    pub provider: Option<String>,
    /// Model identifier recorded on the run.
    pub model: Option<String>,
    /// Model-call budget of one attempt (a "turn" is one model call).
    pub max_agent_turns: usize,
    /// Total attempts, including invalid ones. Values below 1 count as 1.
    pub max_attempts: u32,
    /// Time allowed for one attempt, covering the whole tool loop.
    pub timeout: Duration,
    /// Maximum citations in one summary.
    pub max_citations: usize,
    /// Maximum summary size in UTF-8 bytes.
    pub max_summary_bytes: usize,
    /// Maximum user prompt size in UTF-8 bytes.
    pub max_prompt_bytes: usize,
    /// Caps for the file tools (calls per run, bytes per call).
    pub tools: ProjectToolConfig,
    /// How tool results reach the model between calls (Rig models only).
    pub strategy: ToolStrategy,
}

impl Default for SurveyorConfig {
    fn default() -> Self {
        Self {
            provider: Some(DEFAULT_PROVIDER.to_owned()),
            model: Some(DEFAULT_MODEL.to_owned()),
            max_agent_turns: DEFAULT_MAX_AGENT_TURNS,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            timeout: DEFAULT_TIMEOUT,
            max_citations: DEFAULT_MAX_CITATIONS,
            max_summary_bytes: DEFAULT_MAX_SUMMARY_BYTES,
            max_prompt_bytes: DEFAULT_MAX_PROMPT_BYTES,
            tools: ProjectToolConfig::default(),
            strategy: ToolStrategy::default(),
        }
    }
}

impl SurveyorConfig {
    /// The defaults: the same provider and model as Explainer's defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// The defaults with provider, model and tool strategy overridden by
    /// `DAFTPROMPT_SURVEYOR_PROVIDER`, `DAFTPROMPT_SURVEYOR_MODEL` and
    /// `DAFTPROMPT_SURVEYOR_STRATEGY`. Blank or unrecognized values are
    /// ignored. No credential is read here.
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
        if let Some(strategy) = get(STRATEGY_ENV).and_then(|value| ToolStrategy::parse(&value)) {
            config.strategy = strategy;
        }
        config
    }

    /// Every setting that can change a response, for the cache key. Generation
    /// options (temperature and the like) are not configurable and use the
    /// provider defaults, so they have nothing to record yet. Credentials are
    /// not part of the config.
    pub(crate) fn cache_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "max_agent_turns": self.max_agent_turns,
            "max_attempts": self.max_attempts,
            "timeout_ms": self.timeout.as_millis() as u64,
            "max_citations": self.max_citations,
            "max_summary_bytes": self.max_summary_bytes,
            "max_prompt_bytes": self.max_prompt_bytes,
            "tools": self.tools.cache_settings(),
            "strategy": self.strategy.as_str(),
        })
    }

    /// The provider and model to build with [`build_model`](crate::llm::build_model).
    pub fn model_settings(&self) -> ModelSettings {
        ModelSettings::new(
            self.provider
                .clone()
                .unwrap_or_else(|| DEFAULT_PROVIDER.to_owned()),
            self.model
                .clone()
                .unwrap_or_else(|| DEFAULT_MODEL.to_owned()),
        )
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

    /// Set the citation and summary size limits.
    pub fn with_output_limits(mut self, max_citations: usize, max_summary_bytes: usize) -> Self {
        self.max_citations = max_citations;
        self.max_summary_bytes = max_summary_bytes;
        self
    }

    /// Set the user prompt size limit.
    pub fn with_max_prompt_bytes(mut self, bytes: usize) -> Self {
        self.max_prompt_bytes = bytes;
        self
    }

    /// Set the tool strategy.
    pub fn with_strategy(mut self, strategy: ToolStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the file tool caps.
    pub fn with_tool_config(mut self, tools: ProjectToolConfig) -> Self {
        self.tools = tools;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_explainers() {
        let config = SurveyorConfig::default();
        assert_eq!(config.provider.as_deref(), Some("neuralwatt"));
        assert_eq!(config.model.as_deref(), Some("glm-5.3-flash"));
        assert_eq!(
            config.model_settings(),
            ModelSettings::new("neuralwatt", "glm-5.3-flash")
        );
    }

    #[test]
    fn environment_overrides_provider_and_model_and_ignores_blanks() {
        let config = SurveyorConfig::from_lookup(|name| match name {
            PROVIDER_ENV => Some("groq".to_owned()),
            MODEL_ENV => Some("  ".to_owned()),
            _ => None,
        });
        assert_eq!(config.provider.as_deref(), Some("groq"));
        assert_eq!(config.model.as_deref(), Some("glm-5.3-flash"));
        assert_eq!(PROVIDER_ENV, "DAFTPROMPT_SURVEYOR_PROVIDER");
        assert_eq!(MODEL_ENV, "DAFTPROMPT_SURVEYOR_MODEL");
    }

    #[test]
    fn strategy_defaults_to_chat_and_parses_from_the_environment() {
        assert_eq!(SurveyorConfig::default().strategy, ToolStrategy::Chat);
        let with = |value: &'static str| {
            SurveyorConfig::from_lookup(|name| (name == STRATEGY_ENV).then(|| value.to_owned()))
                .strategy
        };
        assert_eq!(with(" Restate "), ToolStrategy::Restate);
        assert_eq!(with("chat"), ToolStrategy::Chat);
        assert_eq!(with("bogus"), ToolStrategy::Chat);
        assert_eq!(with(" "), ToolStrategy::Chat);
        assert_eq!(STRATEGY_ENV, "DAFTPROMPT_SURVEYOR_STRATEGY");
    }

    #[test]
    fn strategy_is_part_of_the_cache_settings() {
        let chat = SurveyorConfig::default().cache_settings();
        let restate = SurveyorConfig::default()
            .with_strategy(ToolStrategy::Restate)
            .cache_settings();
        assert_ne!(chat, restate);
    }
}
