//! The briefing-model seam and the Rig-backed implementation.
//!
//! [`ChroniclerModel`] is the only interface Chronicler uses to obtain a
//! proposed briefing. One call is one attempt: it may run a multi-turn tool
//! loop and ends with the submitted [`BriefingExtraction`]. Tests supply a
//! stub that drives the per-attempt [`GitToolbox`] directly; production wraps
//! a Rig agent through the shared runner. Credentials and provider clients are
//! supplied by the caller, never stored here.

use std::sync::Arc;

use daftprompt_types::BriefingExtraction;
use rig::DynModel;
use rig::agent::{Agent, AgentBuilder};
use rig::operation::Completion;

pub use crate::scribe::{BoxFuture, ModelError};

use super::prompt::SYSTEM_PROMPT;
use crate::git_tools::{GitHistory, GitToolConfig, GitToolbox, git_rig_tools};
use crate::project_agent::{AttemptInput, Toolset, run_rig_agent};
use crate::project_tools::ToolRecorder;

/// Chronicler's tools for one attempt: the git tools over the project.
pub struct ChroniclerTools {
    git: Arc<GitToolbox>,
}

impl ChroniclerTools {
    pub fn new(history: Arc<dyn GitHistory>, config: GitToolConfig) -> Self {
        Self {
            git: Arc::new(GitToolbox::new(history, config)),
        }
    }

    /// The git toolbox. Every tool invocation must go through it so the
    /// ledger sees it.
    pub fn git(&self) -> &GitToolbox {
        &self.git
    }
}

impl Toolset for ChroniclerTools {
    fn recorder(&self) -> &ToolRecorder {
        self.git.recorder()
    }

    fn build_agent(self: &Arc<Self>, builder: AgentBuilder) -> Agent {
        let tools = git_rig_tools(&self.git);
        builder.tool(tools.git_log).tool(tools.git_show).build()
    }
}

/// One attempt's input to the briefing model.
pub type ChroniclerInput = AttemptInput<ChroniclerTools>;

/// A source of proposed briefings.
///
/// Implementations perform the network and tool-loop work. Chronicler owns
/// the attempt budget, timeout, and all validation, so an implementation runs
/// exactly one attempt per call.
pub trait ChroniclerModel: Send + Sync {
    /// Run one attempt and return the submitted briefing.
    fn brief(
        &self,
        input: ChroniclerInput,
    ) -> BoxFuture<'_, Result<BriefingExtraction, ModelError>>;
}

/// Chronicler backed by a multi-turn Rig agent with an output tool.
pub struct RigChronicler {
    model: DynModel<Completion>,
}

impl RigChronicler {
    /// Wrap a Rig completion model built by the caller from environment
    /// credentials.
    pub fn from_model<M>(model: M) -> Self
    where
        M: Into<DynModel<Completion>>,
    {
        Self {
            model: model.into(),
        }
    }
}

impl ChroniclerModel for RigChronicler {
    fn brief(
        &self,
        input: ChroniclerInput,
    ) -> BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(run_rig_agent(&self.model, SYSTEM_PROMPT, input))
    }
}
