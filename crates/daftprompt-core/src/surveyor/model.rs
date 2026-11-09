//! The briefing-model seam and the Rig-backed implementation.
//!
//! [`SurveyorModel`] is the only interface Surveyor uses to obtain a proposed
//! briefing. One call is one attempt: it may run a multi-turn tool loop and
//! ends with the submitted [`BriefingExtraction`]. Tests supply a stub that
//! drives the per-attempt [`ProjectToolbox`] directly; production wraps a Rig
//! agent through the shared runner. Credentials and provider clients are
//! supplied by the caller, never stored here.

use std::sync::Arc;

use daftprompt_types::BriefingExtraction;
use rig::DynModel;
use rig::agent::{Agent, AgentBuilder};
use rig::operation::Completion;

pub use crate::scribe::{BoxFuture, ModelError};

use super::config::ToolStrategy;
use super::prompt::{RESTATE_SYSTEM_PROMPT, SYSTEM_PROMPT};
use crate::project_agent::{AttemptInput, Toolset, run_rig_agent};
use crate::project_tools::{
    ProjectFiles, ProjectToolConfig, ProjectToolbox, ToolRecorder, rig_tools,
};

/// Surveyor's tools for one attempt: `list_files`, `search` and `read_file`
/// over the project.
pub struct SurveyorTools {
    files: Arc<ProjectToolbox>,
}

impl SurveyorTools {
    pub fn new(project: Arc<dyn ProjectFiles>, config: ProjectToolConfig) -> Self {
        Self {
            files: Arc::new(ProjectToolbox::new(project, config)),
        }
    }

    /// The file toolbox. Every tool invocation must go through it so the
    /// ledger sees it.
    pub fn files(&self) -> &ProjectToolbox {
        &self.files
    }
}

impl Toolset for SurveyorTools {
    fn recorder(&self) -> &ToolRecorder {
        self.files.recorder()
    }

    fn build_agent(self: &Arc<Self>, builder: AgentBuilder) -> Agent {
        let tools = rig_tools(&self.files);
        builder
            .tool(tools.list_files)
            .tool(tools.search)
            .tool(tools.read_file)
            .build()
    }
}

/// One attempt's input to the briefing model.
pub type SurveyorInput = AttemptInput<SurveyorTools>;

/// A source of proposed briefings.
///
/// Implementations perform the network and tool-loop work. Surveyor owns the
/// attempt budget, timeout, and all validation, so an implementation runs
/// exactly one attempt per call.
pub trait SurveyorModel: Send + Sync {
    /// Run one attempt and return the submitted briefing.
    fn brief(&self, input: SurveyorInput) -> BoxFuture<'_, Result<BriefingExtraction, ModelError>>;
}

/// Surveyor backed by a Rig model with an output tool. The
/// [`ToolStrategy`] chooses between a multi-turn agent conversation and the
/// stateless, evidence-restating loop of [`restate`](super::restate).
pub struct RigSurveyor {
    model: DynModel<Completion>,
    strategy: ToolStrategy,
}

impl RigSurveyor {
    /// Wrap a Rig completion model built by the caller from environment
    /// credentials.
    pub fn from_model<M>(model: M) -> Self
    where
        M: Into<DynModel<Completion>>,
    {
        Self {
            model: model.into(),
            strategy: ToolStrategy::default(),
        }
    }

    /// Choose how tool results reach the model between calls.
    pub fn with_strategy(mut self, strategy: ToolStrategy) -> Self {
        self.strategy = strategy;
        self
    }
}

impl SurveyorModel for RigSurveyor {
    fn brief(&self, input: SurveyorInput) -> BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        input.toolset.recorder().log_event(
            "tool_strategy",
            serde_json::json!({"strategy": self.strategy.as_str()}),
        );
        match self.strategy {
            ToolStrategy::Chat => Box::pin(run_rig_agent(&self.model, SYSTEM_PROMPT, input)),
            ToolStrategy::Restate => Box::pin(super::restate::run_restate_on(
                &self.model,
                RESTATE_SYSTEM_PROMPT,
                input,
            )),
        }
    }
}
