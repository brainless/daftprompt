//! Surveyor: the files-and-documentation briefing agent of
//! [Epic 006](../../../epics/006-project-briefing.md).
//!
//! Surveyor reads a project with three read-only tools (`list_files`,
//! `search`, `read_file`) and submits a typed
//! [`BriefingExtraction`](daftprompt_types::BriefingExtraction) through a
//! schema-defined output tool: a Markdown summary and the file lines it cites.
//! Rust computes the [`ProjectProfile`](daftprompt_types::ProjectProfile)
//! (languages and key files, no model call) and gives it to the model as
//! context, validates every citation against the lines the tools actually
//! returned in that run, and builds all records. Only file citations are
//! valid.
//!
//! The default task summarizes the project. An optional user prompt replaces
//! the task and becomes a root prompt node. The multi-turn loop, attempts and
//! timeout come from the shared [`project_agent`](crate::project_agent)
//! runner. Tests use [`SurveyorModel`] stubs and an in-memory project and
//! never need model credentials.

mod config;
mod graph;
mod model;
pub mod prompt;
mod validate;

use std::sync::Arc;

use daftprompt_types::{AgentInfo, AgentTaskRequest, BriefingResult, ParseError};

pub use config::{
    DEFAULT_MAX_AGENT_TURNS, DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_CITATIONS, DEFAULT_MAX_PROMPT_BYTES,
    DEFAULT_MAX_SUMMARY_BYTES, DEFAULT_MODEL, DEFAULT_PROVIDER, DEFAULT_TIMEOUT, MODEL_ENV,
    PROVIDER_ENV, SurveyorConfig,
};
pub use model::{BoxFuture, ModelError, RigSurveyor, SurveyorInput, SurveyorModel, SurveyorTools};
pub use prompt::{PROMPT_VERSION, SYSTEM_PROMPT};

use crate::access::ProjectScope;
use crate::languages;
use crate::project_agent::{Briefer, RunLimits, Toolset, run_attempts};
use crate::project_tools::{FsProject, ProjectFiles};
use crate::scribe::support;

/// Builds the read-only file backend for a granted project.
type ProjectFactory =
    dyn Fn(&ProjectScope) -> Result<Arc<dyn ProjectFiles>, ParseError> + Send + Sync;

/// The Surveyor agent, parameterised by its model.
pub struct Surveyor<M: SurveyorModel> {
    config: SurveyorConfig,
    model: M,
    project: Arc<ProjectFactory>,
}

impl<M: SurveyorModel> Surveyor<M> {
    /// A Surveyor that reads the project folder the scope grants.
    pub fn new(config: SurveyorConfig, model: M) -> Self {
        Self {
            config,
            model,
            project: Arc::new(scope_project),
        }
    }

    /// Read the project through `factory` instead of the filesystem, for
    /// example an [`InMemoryProject`](crate::project_tools::InMemoryProject)
    /// in tests.
    pub fn with_project(
        mut self,
        factory: impl Fn(&ProjectScope) -> Arc<dyn ProjectFiles> + Send + Sync + 'static,
    ) -> Self {
        self.project = Arc::new(move |scope| Ok(factory(scope)));
        self
    }

    /// The active configuration.
    pub fn config(&self) -> &SurveyorConfig {
        &self.config
    }

    /// Brief the project `scope` grants.
    ///
    /// The request is validated before any model call: an oversized prompt or
    /// author is rejected, never truncated; a blank prompt means the default
    /// task. The profile is computed once and each attempt then runs the model
    /// with a fresh toolbox. Invalid output (a bad schema, no submission, a
    /// turn-limit overrun, a commit citation, a citation of lines no
    /// `read_file` call returned, or a bad marker) consumes the attempt
    /// budget, and each retry starts from scratch without feedback. A provider
    /// error or an attempt timeout (which covers the whole tool loop) is
    /// returned immediately. A partial result is never returned.
    pub async fn brief(
        &self,
        request: AgentTaskRequest,
        scope: &ProjectScope,
    ) -> Result<BriefingResult, ParseError> {
        check_request(&request, &self.config)?;
        let files = (self.project)(scope)?;
        let profile = languages::profile(files.as_ref());
        let prompt = prompt::render_prompt(request.prompt_text(), &profile);

        let started_at = support::now();
        let run_id = support::new_id("run");
        let limits = RunLimits {
            max_turns: self.config.max_agent_turns,
            max_attempts: self.config.max_attempts,
            timeout: self.config.timeout,
        };
        let completed = run_attempts(
            limits,
            &prompt,
            || Arc::new(SurveyorTools::new(files.clone(), self.config.tools.clone())),
            |input| self.model.brief(input),
            |extraction, tools| validate::validate(extraction, tools.recorder(), &self.config),
        )
        .await?;

        let calls = completed.toolset.files().recorder().calls();
        graph::build(graph::Built {
            request: &request,
            extraction: &completed.extraction,
            citations: &completed.validated,
            profile,
            calls: &calls,
            config: &self.config,
            project_id: scope.project_id.clone(),
            run_id,
            started_at,
            attempts: completed.attempts,
        })
    }

    /// How this agent is configured, for `GET /agents`.
    pub fn info(&self) -> AgentInfo {
        AgentInfo {
            kind: "surveyor".to_owned(),
            provider: self.config.provider.clone(),
            model: self.config.model.clone(),
            prompt_version: PROMPT_VERSION.to_owned(),
        }
    }
}

fn check_request(request: &AgentTaskRequest, config: &SurveyorConfig) -> Result<(), ParseError> {
    if let Some(prompt) = request.prompt_text()
        && prompt.len() > config.max_prompt_bytes
    {
        return Err(ParseError::OversizedInput {
            detail: format!(
                "the prompt is {} bytes, over the limit of {}",
                prompt.len(),
                config.max_prompt_bytes
            ),
        });
    }
    if request.prompt_author().len() > config::MAX_AUTHOR_BYTES {
        return Err(ParseError::InvalidRequest {
            detail: format!(
                "the author is longer than {} bytes",
                config::MAX_AUTHOR_BYTES
            ),
        });
    }
    Ok(())
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

impl Surveyor<RigSurveyor> {
    /// A Surveyor over any Rig completion model. The caller builds the
    /// provider client, for example with [`crate::llm::build_model`].
    pub fn from_rig_model<M>(config: SurveyorConfig, model: M) -> Self
    where
        M: Into<rig::DynModel<rig::operation::Completion>>,
    {
        Self::new(config, RigSurveyor::from_model(model))
    }
}

impl<M> Briefer for Surveyor<M>
where
    M: SurveyorModel + 'static,
{
    fn info(&self) -> AgentInfo {
        Surveyor::info(self)
    }

    fn brief<'a>(
        &'a self,
        request: AgentTaskRequest,
        scope: &ProjectScope,
    ) -> BoxFuture<'a, Result<BriefingResult, ParseError>> {
        let scope = scope.clone();
        Box::pin(async move { Surveyor::brief(self, request, &scope).await })
    }
}
