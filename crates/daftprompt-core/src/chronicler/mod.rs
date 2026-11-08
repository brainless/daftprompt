//! Chronicler: the git-history briefing agent of
//! [Epic 006](../../../epics/006-project-briefing.md).
//!
//! The active path preloads recent history and requests a plain-text summary.
//! The separate, unused `brief_with_tools` path reads history with two read-only tools
//! (`git_log`, `git_show`) and submits a typed
//! [`BriefingExtraction`](daftprompt_types::BriefingExtraction) through a
//! schema-defined output tool: a Markdown summary and the commits it cites.
//! Rust validates every citation against what the tools actually returned in
//! that run and builds all records; the model never proposes IDs, hashes of
//! unseen commits, or labels.
//!
//! The default task summarizes recent work. An optional user prompt replaces
//! the task and becomes a root prompt node. The multi-turn loop, attempts and
//! timeout come from the shared [`project_agent`](crate::project_agent)
//! runner. Tests use [`ChroniclerModel`] stubs and an in-memory git double and
//! never need model credentials.

mod config;
mod graph;
mod model;
pub mod prompt;
mod validate;

use std::sync::Arc;

use daftprompt_types::{AgentInfo, AgentTaskRequest, BriefingResult, ParseError};

pub use config::{
    COMMIT_COUNT_ENV, ChroniclerConfig, DEFAULT_COMMIT_COUNT, DEFAULT_MAX_AGENT_TURNS,
    DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_CITATIONS, DEFAULT_MAX_PROMPT_BYTES,
    DEFAULT_MAX_SUMMARY_BYTES, DEFAULT_MODEL, DEFAULT_PROVIDER, DEFAULT_TIMEOUT, MODEL_ENV,
    PROVIDER_ENV,
};
pub use model::{
    BoxFuture, ChroniclerInput, ChroniclerModel, ChroniclerTools, ModelError, RigChronicler,
};
pub use prompt::{PROMPT_VERSION, SYSTEM_PROMPT};

use crate::access::ProjectScope;
use crate::git_tools::{GitHistory, SubprocessGit};
use crate::project_agent::{Briefer, RunLimits, Toolset, run_attempts};
use crate::scribe::support;

/// Builds the git backend for a granted project.
type HistoryFactory = dyn Fn(&ProjectScope) -> Arc<dyn GitHistory> + Send + Sync;

/// The Chronicler agent, parameterised by its model.
pub struct Chronicler<M: ChroniclerModel> {
    config: ChroniclerConfig,
    model: M,
    history: Arc<HistoryFactory>,
}

impl<M: ChroniclerModel> Chronicler<M> {
    /// A Chronicler that reads history by running `git` in the project root.
    pub fn new(config: ChroniclerConfig, model: M) -> Self {
        let tools = config.tools.clone();
        Self {
            config,
            model,
            history: Arc::new(move |scope| Arc::new(SubprocessGit::new(&scope.root, &tools))),
        }
    }

    /// Read history through `factory` instead of the `git` subprocess, for
    /// example an in-memory double in tests.
    pub fn with_history(
        mut self,
        factory: impl Fn(&ProjectScope) -> Arc<dyn GitHistory> + Send + Sync + 'static,
    ) -> Self {
        self.history = Arc::new(factory);
        self
    }

    /// The active configuration.
    pub fn config(&self) -> &ChroniclerConfig {
        &self.config
    }

    /// Preserved tool-based path for future use; no API route invokes it.
    ///
    /// The request is validated before any model call: an oversized prompt or
    /// author is rejected, never truncated; a blank prompt means the default
    /// task. Each attempt then runs the model with a fresh toolbox. Invalid
    /// output (a bad schema, no submission, a turn-limit overrun, an unseen or
    /// ambiguous commit citation, or a bad marker) consumes the attempt
    /// budget, and each retry gets the rejection reason and fresh tools. A provider
    /// error or an attempt timeout (which covers the whole tool loop) is
    /// returned immediately. A partial result is never returned.
    ///
    /// A folder that is not a repository, or a machine without `git`, is not
    /// an error: the briefing says so and carries a notice.
    pub async fn brief_with_tools(
        &self,
        request: AgentTaskRequest,
        scope: &ProjectScope,
    ) -> Result<BriefingResult, ParseError> {
        check_request(&request, &self.config)?;
        let prompt = prompt::render_prompt(request.prompt_text());
        let history = (self.history)(scope);

        let started_at = support::now();
        let run_id = support::new_id("run");
        let log = crate::agent_log::AgentLog::new("chronicler", &run_id);
        let limits = RunLimits {
            max_turns: self.config.max_agent_turns,
            max_attempts: self.config.max_attempts,
            timeout: self.config.timeout,
        };
        let completed = run_attempts(
            limits,
            &prompt,
            || {
                let tools = Arc::new(ChroniclerTools::new(
                    history.clone(),
                    self.config.tools.clone(),
                ));
                tools.recorder().set_log(log.clone());
                tools
            },
            |input| self.model.brief(input),
            |extraction, tools| validate::validate(extraction, tools.recorder(), &self.config),
        )
        .await?;

        let calls = completed.toolset.git().recorder().calls();
        graph::build(graph::Built {
            tool_based: true,
            request: &request,
            extraction: &completed.extraction,
            citations: &completed.validated,
            git: completed.toolset.git(),
            calls: &calls,
            config: &self.config,
            project_id: scope.project_id.clone(),
            run_id,
            started_at,
            attempts: completed.attempts,
        })
    }

    /// Active path: Rust reads history once, then the model returns plain text.
    /// Custom prompts use the same bounded history; no tools are exposed.
    pub async fn brief(
        &self,
        request: AgentTaskRequest,
        scope: &ProjectScope,
    ) -> Result<BriefingResult, ParseError> {
        check_request(&request, &self.config)?;
        let started_at = support::now();
        let run_id = support::new_id("run");
        let log = crate::agent_log::AgentLog::new("chronicler", &run_id);
        let tools = Arc::new(ChroniclerTools::new(
            (self.history)(scope),
            self.config.tools.clone(),
        ));
        tools.recorder().set_log(log.clone());
        let reader = tools.clone();
        let count = self
            .config
            .commit_count
            .max(1)
            .min(self.config.tools.max_log_commits.max(1));
        let history = tokio::task::spawn_blocking(move || {
            reader
                .git()
                .call("git_log", serde_json::json!({"count": count}))
                .text
        })
        .await
        .map_err(|error| ParseError::Internal {
            detail: format!("history read failed: {error}"),
        })?;
        let prompt = prompt::render_summary_prompt(request.prompt_text(), &history);
        let completed = run_attempts(
            RunLimits {
                max_turns: 1,
                max_attempts: self.config.max_attempts,
                timeout: self.config.timeout,
            },
            &prompt,
            || {
                tools.recorder().set_log(log.clone());
                tools.clone()
            },
            |input| {
                Box::pin(async move {
                    let summary = self.model.summarize(input.prompt).await?;
                    Ok(daftprompt_types::BriefingExtraction {
                        summary,
                        citations: vec![],
                    })
                })
            },
            |extraction, _| {
                if extraction.summary.trim().is_empty()
                    || extraction.summary.len() > self.config.max_summary_bytes
                {
                    Err("the summary is empty or exceeds the output limit".to_owned())
                } else {
                    Ok(Vec::new())
                }
            },
        )
        .await?;
        let calls = tools.recorder().calls();
        graph::build(graph::Built {
            tool_based: false,
            request: &request,
            extraction: &completed.extraction,
            citations: &completed.validated,
            git: tools.git(),
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
            kind: "chronicler".to_owned(),
            provider: self.config.provider.clone(),
            model: self.config.model.clone(),
            prompt_version: prompt::SUMMARY_PROMPT_VERSION.to_owned(),
        }
    }
}

fn check_request(request: &AgentTaskRequest, config: &ChroniclerConfig) -> Result<(), ParseError> {
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

impl Chronicler<RigChronicler> {
    /// A Chronicler over any Rig completion model. The caller builds the
    /// provider client, for example with [`crate::llm::build_model`].
    pub fn from_rig_model<M>(config: ChroniclerConfig, model: M) -> Self
    where
        M: Into<rig::DynModel<rig::operation::Completion>>,
    {
        Self::new(config, RigChronicler::from_model(model))
    }
}

impl<M> Briefer for Chronicler<M>
where
    M: ChroniclerModel + 'static,
{
    fn info(&self) -> AgentInfo {
        Chronicler::info(self)
    }

    fn brief<'a>(
        &'a self,
        request: AgentTaskRequest,
        scope: &ProjectScope,
    ) -> BoxFuture<'a, Result<BriefingResult, ParseError>> {
        let scope = scope.clone();
        Box::pin(async move { Chronicler::brief(self, request, &scope).await })
    }
}
