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
mod consistency;
mod graph;
mod model;
pub mod prompt;
pub(crate) mod support;
mod validate;

use daftprompt_types::{AgentInfo, ParseError, ParseRequest, ParseResult};

pub use config::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_SEGMENTS, DEFAULT_TIMEOUT,
    ScribeConfig,
};
pub use model::{BoxFuture, ExtractionModel, ModelError, ModelInput, RigExtractor};
pub use prompt::{PROMPT_VERSION, SYSTEM_PROMPT, render_segments};

use crate::access::ProjectScope;
use crate::text::{
    segments,
    structure::{resolve_content_type, structure},
};

/// An object-safe parsing interface for clients that want to erase the model.
///
/// The API can hold an `Arc<dyn Parser>` and swap in a stub in tests without
/// knowing which extraction model Scribe uses.
pub trait Parser: Send + Sync {
    /// Parse one submission into a complete validated graph.
    fn parse(&self, request: ParseRequest) -> BoxFuture<'_, Result<ParseResult, ParseError>>;

    /// Parse with the project the caller's access policy granted, if any.
    ///
    /// The default ignores the scope. Scribe records the project id on the
    /// run but gives the model no file access.
    fn parse_in<'a>(
        &'a self,
        request: ParseRequest,
        _scope: Option<&ProjectScope>,
    ) -> BoxFuture<'a, Result<ParseResult, ParseError>> {
        self.parse(request)
    }

    /// How this parser is configured, for `GET /agents`. The default reports
    /// nothing.
    fn info(&self) -> Option<AgentInfo> {
        None
    }
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
        self.run(request, None).await
    }

    /// Parse one submission for a caller whose access policy granted `scope`.
    ///
    /// Behaves exactly like [`Scribe::parse`]; the granted project id is
    /// recorded on [`Run::project_id`](daftprompt_types::Run). Scribe has no
    /// file tools, so the scope's root is never read.
    pub async fn parse_in(
        &self,
        request: ParseRequest,
        scope: Option<&ProjectScope>,
    ) -> Result<ParseResult, ParseError> {
        self.run(request, scope.map(|scope| scope.project_id.clone()))
            .await
    }

    async fn run(
        &self,
        request: ParseRequest,
        project_id: Option<String>,
    ) -> Result<ParseResult, ParseError> {
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
        let (content_type, _) = resolve_content_type(&request);
        let tree = structure(text, content_type);
        let input = ModelInput {
            segments: if self.config.legacy_v3 {
                render_segments(text)
            } else {
                prompt::render_structure(text, &tree)
            },
            content_type,
            feedback: None,
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
                Ok(Ok(extraction)) => match validate::validate(text, &extraction, &tree) {
                    Ok(validated) => {
                        let (extraction, validated, followup) = self
                            .consistency_followup(text, &tree, &input, extraction, validated)
                            .await;
                        return graph::build(
                            &request,
                            &extraction,
                            &validated,
                            &self.config,
                            run_id,
                            started_at,
                            attempt,
                            followup,
                            project_id,
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

impl<M: ExtractionModel> Scribe<M> {
    /// One optional follow-up when sibling blocks of one kind are partly labelled.
    ///
    /// The model proposes every label; Rust only detects the gap and asks again.
    /// The follow-up is outside the attempt budget, uses the per-call timeout,
    /// and can never fail the parse: any error, timeout, or invalid output keeps
    /// the original valid extraction.
    async fn consistency_followup(
        &self,
        text: &str,
        tree: &crate::text::structure::Structure,
        input: &ModelInput,
        extraction: daftprompt_types::Extraction,
        validated: validate::ValidatedExtraction,
    ) -> (
        daftprompt_types::Extraction,
        validate::ValidatedExtraction,
        ConsistencyOutcome,
    ) {
        let none = ConsistencyOutcome::default();
        if !self.config.consistency_followup || self.config.legacy_v3 {
            return (extraction, validated, none);
        }
        let gaps = consistency::sibling_gaps(tree, &extraction, &validated);
        if gaps.is_empty() {
            return (extraction, validated, none);
        }
        let mut followup_input = input.clone();
        followup_input.feedback = Some(consistency::feedback(&gaps));
        let outcome =
            tokio::time::timeout(self.config.timeout, self.model.extract(followup_input)).await;
        if let Ok(Ok(second)) = outcome
            && let Ok(second_validated) = validate::validate(text, &second, tree)
        {
            return (
                second,
                second_validated,
                ConsistencyOutcome {
                    ran: true,
                    used: true,
                },
            );
        }
        (
            extraction,
            validated,
            ConsistencyOutcome {
                ran: true,
                used: false,
            },
        )
    }
}

/// What the sibling-consistency follow-up did, recorded on the run.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ConsistencyOutcome {
    pub(crate) ran: bool,
    pub(crate) used: bool,
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

    fn info(&self) -> Option<AgentInfo> {
        Some(AgentInfo {
            kind: "scribe".to_owned(),
            provider: self.config.provider.clone(),
            model: self.config.model.clone(),
            prompt_version: PROMPT_VERSION.to_owned(),
        })
    }

    fn parse_in<'a>(
        &'a self,
        request: ParseRequest,
        scope: Option<&ProjectScope>,
    ) -> BoxFuture<'a, Result<ParseResult, ParseError>> {
        let project_id = scope.map(|scope| scope.project_id.clone());
        Box::pin(self.run(request, project_id))
    }
}
