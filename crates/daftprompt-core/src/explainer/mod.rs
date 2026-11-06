//! Explainer: the clarification agent.
//!
//! Explainer answers one question about one extracted span. Rust assembles a
//! small context (the subject span, a window of numbered neighbouring
//! segments, the branch's earlier turns, the question, and the grounding
//! mode), the model runs a bounded multi-turn tool loop over read-only
//! project tools, and it ends by submitting a typed
//! [`ExplainerExtraction`](daftprompt_types::ExplainerExtraction). Rust then
//! validates every citation and builds all records; the model never proposes
//! IDs, hashes, offsets, labels, or statuses.
//!
//! The public interface is transport-agnostic: call [`Explainer::clarify`] or
//! hold an [`Arc<dyn Clarifier>`](Clarifier). Tests use [`ExplainerModel`]
//! stubs with an in-memory project and never need model credentials.

mod config;
mod context;
mod graph;
mod model;
pub mod prompt;
mod validate;

use std::sync::Arc;

use daftprompt_types::{AgentInfo, ClarifyRequest, ClarifyResult, Grounding, ParseError};

pub use config::{
    DEFAULT_MAX_AGENT_TURNS, DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_CITATIONS,
    DEFAULT_MAX_EARLIER_TURNS, DEFAULT_MAX_HISTORY_BYTES, DEFAULT_MAX_QUESTION_BYTES,
    DEFAULT_MODEL, DEFAULT_PROVIDER, DEFAULT_TIMEOUT, DEFAULT_WINDOW_SEGMENTS, ExplainerConfig,
    MODEL_ENV, PROVIDER_ENV,
};
pub use model::{BoxFuture, ExplainerInput, ExplainerModel, ModelError, RigExplainer};
pub use prompt::{PROMPT_VERSION, SYSTEM_PROMPT};

use crate::access::ProjectScope;
use crate::project_tools::{FsProject, ProjectFiles, ProjectToolbox};
use crate::scribe::support;

/// An object-safe clarification interface for clients that want to erase the
/// model, such as an HTTP service holding an `Arc<dyn Clarifier>`.
pub trait Clarifier: Send + Sync {
    /// Explain one span for one question.
    fn clarify(&self, request: ClarifyRequest) -> BoxFuture<'_, Result<ClarifyResult, ParseError>>;

    /// Explain one span, reading the project the caller's access policy
    /// granted. `None` keeps the Explainer's default project, if it has one.
    ///
    /// The default ignores the scope.
    fn clarify_in<'a>(
        &'a self,
        request: ClarifyRequest,
        _scope: Option<&ProjectScope>,
    ) -> BoxFuture<'a, Result<ClarifyResult, ParseError>> {
        self.clarify(request)
    }

    /// How this agent is configured, for `GET /agents`. The default reports
    /// nothing.
    fn info(&self) -> Option<AgentInfo> {
        None
    }
}

/// The Explainer agent, parameterised by its model.
pub struct Explainer<M: ExplainerModel> {
    config: ExplainerConfig,
    model: M,
    project: Option<Arc<dyn ProjectFiles>>,
}

impl<M: ExplainerModel> Explainer<M> {
    /// An Explainer in text-only mode: no project tools.
    pub fn new(config: ExplainerConfig, model: M) -> Self {
        Self {
            config,
            model,
            project: None,
        }
    }

    /// Provide a default read-only project so each run gets project tools and
    /// file citations. A project supplied for one call overrides it. Without
    /// either the result is [`Grounding::TextOnly`].
    pub fn with_project(mut self, project: Arc<dyn ProjectFiles>) -> Self {
        self.project = Some(project);
        self
    }

    /// The active configuration.
    pub fn config(&self) -> &ExplainerConfig {
        &self.config
    }

    /// The grounding mode of a run that supplies no project of its own.
    pub fn grounding(&self) -> Grounding {
        Self::grounding_for(self.project.is_some())
    }

    fn grounding_for(has_project: bool) -> Grounding {
        if has_project {
            Grounding::Project
        } else {
            Grounding::TextOnly
        }
    }

    /// Clarify one span.
    ///
    /// The request is validated before any model call: an empty question, a
    /// payload hash mismatch, an invalid target, or an oversized question,
    /// history, or source is rejected, never truncated. Each attempt then runs
    /// the model with a fresh toolbox. Invalid output (a bad schema, no
    /// submission, a turn-limit overrun, an unknown or uncovered citation, or
    /// a bad marker) consumes the attempt budget, and each retry starts from
    /// scratch without feedback. A provider error or an attempt timeout (which
    /// covers the whole tool loop) is returned immediately. A partial result
    /// is never returned.
    ///
    /// The run records the successful attempt's tool calls; `run.attempts`
    /// counts all attempts.
    pub async fn clarify(&self, request: ClarifyRequest) -> Result<ClarifyResult, ParseError> {
        self.run(request, None, None).await
    }

    /// Clarify one span with `project` for this call only.
    ///
    /// The per-call project replaces the default one, and grounding follows
    /// it. Everything else behaves like [`Explainer::clarify`].
    pub async fn clarify_with_project(
        &self,
        request: ClarifyRequest,
        project: Arc<dyn ProjectFiles>,
    ) -> Result<ClarifyResult, ParseError> {
        self.run(request, Some(project), None).await
    }

    /// Clarify one span with the project an access policy granted.
    ///
    /// With a scope, the run reads a fresh [`FsProject`] over the scope's
    /// root instead of the default project and records the project id on
    /// the run. With `None` it is [`Explainer::clarify`].
    pub async fn clarify_in(
        &self,
        request: ClarifyRequest,
        scope: Option<&ProjectScope>,
    ) -> Result<ClarifyResult, ParseError> {
        let Some(scope) = scope else {
            return self.run(request, None, None).await;
        };
        let project = scope_project(scope)?;
        self.run(request, Some(project), Some(scope.project_id.clone()))
            .await
    }

    async fn run(
        &self,
        request: ClarifyRequest,
        call_project: Option<Arc<dyn ProjectFiles>>,
        project_id: Option<String>,
    ) -> Result<ClarifyResult, ParseError> {
        let prepared = context::prepare(&request, &self.config)?;
        let project = call_project.or_else(|| self.project.clone());
        let grounding = Self::grounding_for(project.is_some());
        let prompt = context::render_prompt(&prepared, grounding);

        let started_at = support::now();
        let run_id = support::new_id("run");
        let max_attempts = self.config.effective_max_attempts();
        let mut last_invalid = "no attempt ran".to_owned();

        for attempt in 1..=max_attempts {
            let toolbox = project.as_ref().map(|files| {
                Arc::new(ProjectToolbox::new(
                    files.clone(),
                    self.config.tools.clone(),
                ))
            });
            let input = ExplainerInput {
                prompt: prompt.clone(),
                toolbox: toolbox.clone(),
                max_turns: self.config.max_agent_turns,
            };

            let outcome =
                tokio::time::timeout(self.config.timeout, self.model.explain(input)).await;
            let extraction = match outcome {
                Err(_elapsed) => {
                    return Err(ParseError::Timeout {
                        detail: format!(
                            "the explanation run exceeded {:?} on attempt {attempt} of {max_attempts}",
                            self.config.timeout
                        ),
                    });
                }
                Ok(Err(ModelError::Invalid(detail))) => {
                    last_invalid = detail;
                    continue;
                }
                Ok(Err(ModelError::Provider(detail))) => {
                    return Err(ParseError::Provider { detail });
                }
                Ok(Err(ModelError::Internal(detail))) => {
                    return Err(ParseError::Internal { detail });
                }
                Ok(Ok(extraction)) => extraction,
            };

            let recorder = toolbox.as_ref().map(|toolbox| toolbox.recorder());
            match validate::validate(&extraction, &prepared, recorder, self.config.max_citations) {
                Ok(citations) => {
                    let calls = recorder.map(|r| r.calls()).unwrap_or_default();
                    return graph::build(graph::Built {
                        prepared: &prepared,
                        extraction: &extraction,
                        citations: &citations,
                        calls: &calls,
                        grounding,
                        config: &self.config,
                        run_id,
                        started_at,
                        attempts: attempt,
                        project_id: project_id.clone(),
                    });
                }
                Err(detail) => last_invalid = detail,
            }
        }

        Err(ParseError::InvalidOutput {
            detail: last_invalid,
            attempts: max_attempts,
        })
    }
}

/// A read-only project over the scope's root.
fn scope_project(scope: &ProjectScope) -> Result<Arc<dyn ProjectFiles>, ParseError> {
    FsProject::new(&scope.root)
        .map(|project| Arc::new(project) as Arc<dyn ProjectFiles>)
        .map_err(|error| ParseError::InvalidRequest {
            detail: format!(
                "the folder of project {:?} cannot be read: {error}",
                scope.project_id
            ),
        })
}

impl Explainer<RigExplainer> {
    /// An Explainer over any Rig completion model. The caller builds the
    /// provider client from environment-backed credentials.
    pub fn from_rig_model<M>(config: ExplainerConfig, model: M) -> Self
    where
        M: Into<rig::DynModel<rig::operation::Completion>>,
    {
        Self::new(config, RigExplainer::from_model(model))
    }
}

impl<M> Clarifier for Explainer<M>
where
    M: ExplainerModel + 'static,
{
    fn clarify(&self, request: ClarifyRequest) -> BoxFuture<'_, Result<ClarifyResult, ParseError>> {
        Box::pin(Explainer::clarify(self, request))
    }

    fn info(&self) -> Option<AgentInfo> {
        Some(AgentInfo {
            kind: "explainer".to_owned(),
            provider: self.config.provider.clone(),
            model: self.config.model.clone(),
            prompt_version: PROMPT_VERSION.to_owned(),
        })
    }

    fn clarify_in<'a>(
        &'a self,
        request: ClarifyRequest,
        scope: Option<&ProjectScope>,
    ) -> BoxFuture<'a, Result<ClarifyResult, ParseError>> {
        let project = scope.map(|scope| (scope_project(scope), scope.project_id.clone()));
        Box::pin(async move {
            match project {
                None => self.run(request, None, None).await,
                Some((project, id)) => self.run(request, Some(project?), Some(id)).await,
            }
        })
    }
}
