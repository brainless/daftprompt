//! The briefing-model seam and the Rig-backed implementation.
//!
//! [`OperatorModel`] is the only interface Operator uses to obtain a proposed
//! briefing. One call is one attempt: it may run a multi-turn tool loop and
//! ends with the submitted [`CommandsExtraction`]. Tests supply a stub that
//! drives the per-attempt [`ProjectToolbox`] directly; production wraps a Rig
//! agent through the shared runner. Credentials and provider clients are
//! supplied by the caller, never stored here.

use daftprompt_types::CommandsExtraction;
use rig::DynModel;
use rig::operation::Completion;

pub use crate::scribe::{BoxFuture, ModelError};

use super::prompt::SYSTEM_PROMPT;
use crate::project_agent::{AttemptInput, run_rig_agent};

/// Operator's tools for one attempt: `list_files`, `search` and `read_file`
/// over the project, the same read-only set as Surveyor's.
pub type OperatorTools = crate::surveyor::SurveyorTools;

/// One attempt's input to the briefing model.
pub type OperatorInput = AttemptInput<OperatorTools>;

/// A source of proposed briefings.
///
/// Implementations perform the network and tool-loop work. Operator owns the
/// attempt budget, timeout, and all validation, so an implementation runs
/// exactly one attempt per call.
pub trait OperatorModel: Send + Sync {
    /// Run one attempt and return the submitted briefing.
    fn brief(&self, input: OperatorInput) -> BoxFuture<'_, Result<CommandsExtraction, ModelError>>;
}

/// Operator backed by a multi-turn Rig agent with an output tool.
pub struct RigOperator {
    model: DynModel<Completion>,
}

impl RigOperator {
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

impl OperatorModel for RigOperator {
    fn brief(&self, input: OperatorInput) -> BoxFuture<'_, Result<CommandsExtraction, ModelError>> {
        Box::pin(run_rig_agent(&self.model, SYSTEM_PROMPT, input))
    }
}
