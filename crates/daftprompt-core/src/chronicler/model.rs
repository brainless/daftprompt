//! The briefing-model seam and the Rig-backed implementation.
//!
//! The active `summarize` method accepts preloaded history and returns plain
//! text with no tools or output schema. `brief` preserves the original
//! multi-turn tool implementation for future use. Both run under Chronicler's
//! attempt budget and timeout; credentials are supplied by the caller.

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
    /// Summarize supplied history without tools or a structured output schema.
    fn summarize(&self, prompt: String) -> BoxFuture<'_, Result<String, ModelError>>;

    /// Run one attempt and return the submitted briefing.
    fn brief(
        &self,
        input: ChroniclerInput,
    ) -> BoxFuture<'_, Result<BriefingExtraction, ModelError>>;
}

/// Rig-backed plain-text summaries and the preserved tool-based path.
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
    fn summarize(&self, prompt: String) -> BoxFuture<'_, Result<String, ModelError>> {
        Box::pin(async move {
            let response = AgentBuilder::new(self.model.clone())
                .preamble(super::prompt::SUMMARY_SYSTEM_PROMPT)
                .build()
                .prompt(prompt)
                .max_turns(1)
                .await
                .map_err(crate::project_agent::prompt_error)?;
            Ok(response.output())
        })
    }

    fn brief(
        &self,
        input: ChroniclerInput,
    ) -> BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(run_rig_agent(&self.model, SYSTEM_PROMPT, input))
    }
}
