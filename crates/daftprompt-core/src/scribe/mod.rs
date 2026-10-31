//! Scribe: the first parsing agent.
//!
//! Scribe asks a model for a typed [`Extraction`](daftprompt_types::Extraction)
//! through Rig's schema-defined `submit` tool. Rust validates the proposal and
//! builds the graph: a payload, a root node, span nodes with range contexts,
//! and labels whose statuses it derives itself.
//!
//! The public interface is transport-agnostic. A GUI, HTTP service, or future
//! CLI calls [`Scribe::parse`] or holds an [`Arc<dyn Parser>`](Parser). Tests
//! use [`ExtractionModel`] stubs and never need model credentials.

mod config;
mod graph;
mod model;
pub mod prompt;
pub(crate) mod support;
mod validate;

use daftprompt_types::{ParseError, ParseRequest, ParseResult};

pub use config::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_SEGMENTS, DEFAULT_TIMEOUT,
    ScribeConfig,
};
pub use model::{BoxFuture, ExtractionModel, ModelError, ModelInput, RigExtractor};
pub use prompt::{PROMPT_VERSION, SYSTEM_PROMPT, render_segments};

use crate::text::segments;

/// An object-safe parsing interface for clients that want to erase the model.
///
/// The API can hold an `Arc<dyn Parser>` and swap in a stub in tests without
/// knowing which extraction model Scribe uses.
pub trait Parser: Send + Sync {
    /// Parse one submission into a complete validated graph.
    fn parse(&self, request: ParseRequest) -> BoxFuture<'_, Result<ParseResult, ParseError>>;
}

/// The Scribe parser, parameterised by its extraction model.
pub struct Scribe<M: ExtractionModel> {
    config: ScribeConfig,
    model: M,
}

impl<M: ExtractionModel> Scribe<M> {
    /// A parser with `config` and `model`.
    pub fn new(config: ScribeConfig, model: M) -> Self {
        Self { config, model }
    }

    /// The active configuration.
    pub fn config(&self) -> &ScribeConfig {
        &self.config
    }

    /// The extraction model behind this parser.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Parse one submission.
    ///
    /// Input limits are checked before the first model call. The attempt
    /// budget covers invalid output only: deserialization and validation
    /// failures are retried from scratch without feedback until the budget is
    /// exhausted, at which point an explicit error is returned. A provider
    /// error or a per-attempt timeout is returned immediately without
    /// retrying, so worst-case latency is one timeout. A partly valid graph is
    /// never returned.
    pub async fn parse(&self, request: ParseRequest) -> Result<ParseResult, ParseError> {
        let text = request.text.as_str();

        if text.is_empty() {
            return Err(ParseError::InvalidRequest {
                detail: "the submitted text is empty".to_owned(),
            });
        }
        if text.len() > self.config.max_input_bytes {
            return Err(ParseError::OversizedInput {
                detail: format!(
                    "the submitted text is {} byte(s); the limit is {} byte(s)",
                    text.len(),
                    self.config.max_input_bytes
                ),
            });
        }
        let segment_count = segments(text).len();
        if segment_count > self.config.max_segments {
            return Err(ParseError::OversizedInput {
                detail: format!(
                    "the submitted text has {segment_count} segment(s); the limit is {}",
                    self.config.max_segments
                ),
            });
        }

        let started_at = support::now();
        let run_id = support::new_id("run");
        let input = ModelInput {
            segments: render_segments(text),
        };
        let max_attempts = self.config.effective_max_attempts();
        let mut last_invalid = "no attempt ran".to_owned();

        for attempt in 1..=max_attempts {
            let outcome =
                tokio::time::timeout(self.config.timeout, self.model.extract(input.clone())).await;

            match outcome {
                Err(_elapsed) => {
                    return Err(ParseError::Timeout {
                        detail: format!(
                            "the model call exceeded {:?} on attempt {attempt} of {max_attempts}",
                            self.config.timeout
                        ),
                    });
                }
                Ok(Err(ModelError::Invalid(detail))) => {
                    last_invalid = detail;
                }
                Ok(Err(ModelError::Provider(detail))) => {
                    return Err(ParseError::Provider { detail });
                }
                Ok(Err(ModelError::Internal(detail))) => {
                    return Err(ParseError::Internal { detail });
                }
                Ok(Ok(extraction)) => match validate::validate(text, &extraction) {
                    Ok(validated) => {
                        return graph::build(
                            &request,
                            &extraction,
                            &validated,
                            &self.config,
                            run_id,
                            started_at,
                            attempt,
                        );
                    }
                    Err(error) => {
                        last_invalid = error.to_string();
                    }
                },
            }
        }

        Err(ParseError::InvalidOutput {
            detail: last_invalid,
            attempts: max_attempts,
        })
    }
}

impl Scribe<RigExtractor> {
    /// A Scribe parser over any Rig completion model.
    ///
    /// The caller builds the provider client from environment-backed
    /// credentials and passes its completion model here.
    pub fn from_rig_model<M>(config: ScribeConfig, model: M) -> Self
    where
        M: Into<rig::DynModel<rig::operation::Completion>>,
    {
        Self::new(config, RigExtractor::from_model(model))
    }
}

impl<M> Parser for Scribe<M>
where
    M: ExtractionModel + 'static,
{
    fn parse(&self, request: ParseRequest) -> BoxFuture<'_, Result<ParseResult, ParseError>> {
        Box::pin(Scribe::parse(self, request))
    }
}
