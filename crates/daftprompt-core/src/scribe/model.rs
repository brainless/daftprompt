//! The extraction-model seam and the Rig-backed implementation.
//!
//! [`ExtractionModel`] is the only interface Scribe uses to obtain a proposed
//! extraction. It is object-safe because its returned future is boxed. Tests
//! supply a deterministic stub; production code wraps Rig's schema-defined
//! `submit` extractor. Credentials and provider clients are supplied by the
//! caller, never stored here.

use std::future::Future;
use std::pin::Pin;

use daftprompt_types::Extraction;

use super::prompt;

/// A boxed, sendable future used by [`ExtractionModel`].
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One attempt's input to the extraction model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInput {
    /// The submitted text as numbered `[n] text` segments.
    pub segments: String,
}

/// Why one extraction attempt produced no usable value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    /// The provider request failed.
    #[error("the model provider failed: {0}")]
    Provider(String),
    /// The model answered but did not submit a valid extraction.
    #[error("the model returned no usable extraction: {0}")]
    Invalid(String),
    /// The adapter itself failed, for example because of a programming error.
    #[error("the extraction model adapter failed: {0}")]
    Internal(String),
}

/// A source of proposed extractions.
///
/// Implementations perform the network or model work. Scribe owns the attempt
/// budget, timeout, and all validation, so an implementation performs exactly
/// one attempt per call.
pub trait ExtractionModel: Send + Sync {
    /// Run one extraction attempt.
    fn extract(&self, input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>>;
}

/// Scribe backed by Rig's typed `submit`-tool extractor.
///
/// Build it with any Rig completion model. The caller constructs the provider
/// from environment-backed credentials (for example, a provider client's
/// `from_env` constructor) and passes the resulting model in, so no credential
/// is read from or stored in this crate.
pub struct RigExtractor {
    extractor: rig::extractor::Extractor<Extraction>,
}

impl RigExtractor {
    /// Wrap a Rig completion model.
    ///
    /// The Scribe system prompt is appended to the extractor's fixed preamble.
    /// Rig's own retry count is left at zero: Scribe's loop owns the single
    /// attempt budget so validation failures and deserialization failures
    /// share it and every retry is a fresh attempt with no feedback.
    pub fn from_model<M>(model: M) -> Self
    where
        M: Into<rig::DynModel<rig::operation::Completion>>,
    {
        let extractor = rig::extractor::ExtractorBuilder::<Extraction>::new(model)
            .append_preamble(prompt::SYSTEM_PROMPT)
            .retries(0)
            .build();
        Self { extractor }
    }
}

impl ExtractionModel for RigExtractor {
    fn extract(&self, input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        Box::pin(async move {
            match self.extractor.extract(input.segments).await {
                Ok(response) => Ok(response.output),
                Err(error) => Err(model_error(error)),
            }
        })
    }
}

fn model_error(error: rig::completion::StructuredOutputError) -> ModelError {
    use rig::completion::StructuredOutputError;

    match error {
        StructuredOutputError::Deserialization { error, .. } => ModelError::Invalid(format!(
            "submit arguments did not match the schema: {error}"
        )),
        StructuredOutputError::EmptyResponse => {
            ModelError::Invalid("the model did not call the submit tool".to_owned())
        }
        StructuredOutputError::Prompt(error) => ModelError::Provider(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_errors_keep_their_category() {
        assert!(matches!(
            ModelError::Provider("quota".into()),
            ModelError::Provider(_)
        ));
        assert!(matches!(
            ModelError::Invalid("bad json".into()),
            ModelError::Invalid(_)
        ));
    }
}
