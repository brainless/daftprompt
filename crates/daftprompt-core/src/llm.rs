//! The provider factory shared by every agent and every client of the library.
//!
//! [`build_model`] turns [`ModelSettings`] (a provider name and a model name)
//! into a Rig completion model. Each provider's credentials and base URLs are
//! read from the process environment at call time, for example
//! `NEURALWATT_API_KEY`; they never enter a config struct or a result.
//!
//! Which provider and model each agent uses is that agent's own configuration
//! (`DAFTPROMPT_<AGENT>_PROVIDER` and `DAFTPROMPT_<AGENT>_MODEL` over the
//! agent's built-in default); this module only builds the model.

use rig::DynModel;
use rig::operation::Completion;
use rig::providers::openai::{OpenAI, OpenAIConfig};

/// Provider names understood by [`build_model`].
pub const PROVIDER_OPENAI: &str = "openai";
pub const PROVIDER_NEURALWATT: &str = "neuralwatt";
pub const PROVIDER_CEREBRAS: &str = "cerebras";
pub const PROVIDER_GROQ: &str = "groq";

/// The provider used by the API process when none is configured.
pub const DEFAULT_PROVIDER: &str = PROVIDER_OPENAI;
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_NEURALWATT_MODEL: &str = "glm-5.3-flash";
const DEFAULT_CEREBRAS_MODEL: &str = "gpt-oss-120b";
const DEFAULT_GROQ_MODEL: &str = "openai/gpt-oss-20b";
/// Neuralwatt's OpenAI-compatible base URL (<https://docs.neuralwatt.com/api/overview.md>).
const DEFAULT_NEURALWATT_BASE_URL: &str = "https://api.neuralwatt.com/v1";
const DEFAULT_CEREBRAS_BASE_URL: &str = "https://api.cerebras.ai/v1";
const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Which provider and model to build. Credentials are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSettings {
    /// One of `openai`, `neuralwatt`, `cerebras`, or `groq`.
    pub provider: String,
    pub model: String,
}

impl ModelSettings {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
        }
    }
}

/// Why a model could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmError {
    /// The provider's API key is not set in the environment.
    #[error("provider credentials are unavailable ({0})")]
    Credentials(String),
    /// The provider name is not wired into the library.
    #[error(
        "provider {0:?} is not wired into this service; supported providers are \"openai\", \
         \"neuralwatt\", \"cerebras\", and \"groq\""
    )]
    UnknownProvider(String),
}

/// The default model for `provider`, used when none is configured.
pub fn default_model(provider: &str) -> &'static str {
    match provider {
        PROVIDER_NEURALWATT => DEFAULT_NEURALWATT_MODEL,
        PROVIDER_CEREBRAS => DEFAULT_CEREBRAS_MODEL,
        PROVIDER_GROQ => DEFAULT_GROQ_MODEL,
        _ => DEFAULT_OPENAI_MODEL,
    }
}

/// Build the completion model for `settings`.
///
/// OpenAI uses `client.completion`, which follows Rig's default OpenAI route.
/// Neuralwatt, Cerebras, and Groq serve OpenAI-compatible Chat Completions,
/// so they use the explicit chat wire (`POST {base}/chat/completions`) rather
/// than the Responses API. Credentials are read from the environment here,
/// never stored.
pub fn build_model(settings: &ModelSettings) -> Result<DynModel<Completion>, LlmError> {
    let model = settings.model.clone();
    match settings.provider.as_str() {
        PROVIDER_OPENAI => {
            // The provider client reads OPENAI_API_KEY and OPENAI_BASE_URL itself.
            let client = OpenAI::from_env()
                .map_err(|error| LlmError::Credentials(format!("set OPENAI_API_KEY: {error}")))?;
            Ok(client.completion(model).into())
        }
        PROVIDER_NEURALWATT => chat_model(
            "NEURALWATT_API_KEY",
            "NEURALWATT_BASE_URL",
            DEFAULT_NEURALWATT_BASE_URL,
            model,
        ),
        PROVIDER_CEREBRAS => chat_model(
            "CEREBRAS_API_KEY",
            "CEREBRAS_BASE_URL",
            DEFAULT_CEREBRAS_BASE_URL,
            model,
        ),
        PROVIDER_GROQ => chat_model(
            "GROQ_API_KEY",
            "GROQ_BASE_URL",
            DEFAULT_GROQ_BASE_URL,
            model,
        ),
        other => Err(LlmError::UnknownProvider(other.to_owned())),
    }
}

fn chat_model(
    key_var: &str,
    base_var: &str,
    default_base: &str,
    model: String,
) -> Result<DynModel<Completion>, LlmError> {
    let api_key =
        env_string(key_var).ok_or_else(|| LlmError::Credentials(format!("set {key_var}")))?;
    let base_url = env_string(base_var).unwrap_or_else(|| default_base.to_owned());
    let client = OpenAIConfig::new(api_key).with_base_url(base_url).client();
    Ok(client.chat(model).into())
}

/// The endpoint `provider` would be called at, for cache keys.
///
/// This is the base URL `build_model` uses (an environment override or the
/// built-in default) with any userinfo, query, and fragment removed. No
/// credential is read or returned.
pub fn endpoint_identity(provider: &str) -> String {
    endpoint_identity_with(provider, env_string)
}

fn endpoint_identity_with(provider: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let (base_var, default_base) = match provider {
        PROVIDER_OPENAI => ("OPENAI_BASE_URL", "default"),
        PROVIDER_NEURALWATT => ("NEURALWATT_BASE_URL", DEFAULT_NEURALWATT_BASE_URL),
        PROVIDER_CEREBRAS => ("CEREBRAS_BASE_URL", DEFAULT_CEREBRAS_BASE_URL),
        PROVIDER_GROQ => ("GROQ_BASE_URL", DEFAULT_GROQ_BASE_URL),
        _ => return "unknown".to_owned(),
    };
    let url = lookup(base_var).unwrap_or_else(|| default_base.to_owned());
    let url = url.split(['?', '#']).next().unwrap_or_default();
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = authority.rsplit('@').next().unwrap_or_default();
            format!("{scheme}://{host}/{path}")
        }
        None => url.to_owned(),
    }
}

/// A trimmed, non-empty environment variable.
pub fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_model_defaults() {
        assert_eq!(default_model(PROVIDER_OPENAI), "gpt-4o-mini");
        assert_eq!(default_model(PROVIDER_NEURALWATT), "glm-5.3-flash");
        assert_eq!(default_model(PROVIDER_CEREBRAS), "gpt-oss-120b");
        assert_eq!(default_model(PROVIDER_GROQ), "openai/gpt-oss-20b");
    }

    #[test]
    fn endpoint_identity_uses_the_base_url_without_credentials() {
        let none = |_: &str| None;
        assert_eq!(
            endpoint_identity_with(PROVIDER_GROQ, none),
            "https://api.groq.com/openai/v1"
        );
        assert_eq!(endpoint_identity_with(PROVIDER_OPENAI, none), "default");
        assert_eq!(endpoint_identity_with("nope", none), "unknown");
        let overridden = |name: &str| {
            (name == "GROQ_BASE_URL")
                .then(|| "https://user:secret@proxy.test/v1?key=abc".to_owned())
        };
        let identity = endpoint_identity_with(PROVIDER_GROQ, overridden);
        assert_eq!(identity, "https://proxy.test/v1");
        assert!(!identity.contains("secret") && !identity.contains("abc"));
    }

    #[test]
    fn an_unknown_provider_is_named_in_the_error() {
        let error = build_model(&ModelSettings::new("nope", "m")).err().unwrap();
        assert_eq!(error, LlmError::UnknownProvider("nope".to_owned()));
        assert!(error.to_string().contains("\"nope\""));
    }
}
