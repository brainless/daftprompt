//! Scribe's configurable model settings and safety limits.

use std::time::Duration;

/// Default maximum input size: 32 KiB of UTF-8.
pub const DEFAULT_MAX_INPUT_BYTES: usize = 32 * 1024;

/// Default maximum number of segments.
pub const DEFAULT_MAX_SEGMENTS: usize = 1_000;

/// Default total attempt budget, including invalid output.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// Default time allowed for one model attempt.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Configuration for a Scribe parser.
///
/// Provider and model identifiers are metadata recorded on the run. The model
/// itself is supplied by the caller, so credentials never pass through this
/// crate's source or configuration. Input limits are checked before the first
/// model call; oversized input is rejected, never truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScribeConfig {
    /// Provider identifier recorded on the run, for example `openai`.
    pub provider: Option<String>,
    /// Model identifier recorded on the run, for example `gpt-4o-mini`.
    pub model: Option<String>,
    /// Maximum accepted input size in UTF-8 bytes.
    pub max_input_bytes: usize,
    /// Maximum accepted number of segments (`\n`-separated lines).
    pub max_segments: usize,
    /// Total attempts, including attempts whose output was invalid. Values
    /// below 1 are treated as 1.
    pub max_attempts: u32,
    /// Time allowed for one model attempt.
    pub timeout: Duration,
}

impl Default for ScribeConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_segments: DEFAULT_MAX_SEGMENTS,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl ScribeConfig {
    /// The default configuration.
    pub fn new() -> Self {
        Self::default()
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

    /// Set the input byte and segment limits.
    pub fn with_input_limits(mut self, max_input_bytes: usize, max_segments: usize) -> Self {
        self.max_input_bytes = max_input_bytes;
        self.max_segments = max_segments;
        self
    }

    /// Set the total attempt budget.
    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// Set the time allowed for one model attempt.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The attempt budget actually used; always at least one attempt.
    pub(crate) fn effective_max_attempts(&self) -> u32 {
        self.max_attempts.max(1)
    }
}
