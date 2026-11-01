//! The explanation-model seam and the Rig-backed implementation.
//!
//! [`ExplainerModel`] is the only interface Explainer uses to obtain a
//! proposed explanation. One call is one attempt: it may run a multi-turn
//! tool loop and ends with the submitted [`ExplainerExtraction`]. Tests supply
//! a stub that drives the per-attempt [`ProjectToolbox`] directly; production
//! wraps a Rig agent. Credentials and provider clients are supplied by the
//! caller, never stored here.

use std::sync::Arc;

use daftprompt_types::ExplainerExtraction;
use rig::DynModel;
use rig::agent::AgentBuilder;
use rig::completion::PromptError;
use rig::operation::Completion;
use rig::run::OutputMode;

pub use crate::scribe::{BoxFuture, ModelError};

use super::prompt::SYSTEM_PROMPT;
use crate::project_tools::{ProjectToolbox, rig_tools};

/// One attempt's input to the explanation model.
#[derive(Clone)]
pub struct ExplainerInput {
    /// The assembled user message: subject, numbered window, earlier turns,
    /// question, and grounding mode.
    pub prompt: String,
    /// Fresh per-attempt tools; `None` in text-only mode. Its recorder is the
    /// ledger Explainer validates file citations against.
    pub toolbox: Option<Arc<ProjectToolbox>>,
    /// Model-call budget for the attempt.
    pub max_turns: usize,
}

/// A source of proposed explanations.
///
/// Implementations perform the network and tool-loop work. Explainer owns the
/// attempt budget, timeout, and all validation, so an implementation runs
/// exactly one attempt per call. Every project tool invocation must go
/// through `input.toolbox` so the recorder sees it.
pub trait ExplainerModel: Send + Sync {
    /// Run one attempt and return the submitted explanation.
    fn explain(
        &self,
        input: ExplainerInput,
    ) -> BoxFuture<'_, Result<ExplainerExtraction, ModelError>>;
}

/// Explainer backed by a multi-turn Rig agent with an output tool.
///
/// Rig's `Extractor` is single-turn, so this builds an agent with the project
/// tools (only when a toolbox is provided) and a schema-defined output tool
/// (`OutputMode::Tool`, named `final_result` by Rig), bounded by
/// `max_turns`. The run ends when the model calls the output tool.
pub struct RigExplainer {
    model: DynModel<Completion>,
}

impl RigExplainer {
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

impl ExplainerModel for RigExplainer {
    fn explain(
        &self,
        input: ExplainerInput,
    ) -> BoxFuture<'_, Result<ExplainerExtraction, ModelError>> {
        Box::pin(async move {
            let builder = AgentBuilder::new(self.model.clone())
                .preamble(SYSTEM_PROMPT)
                .output_schema::<ExplainerExtraction>()
                .output_mode(OutputMode::Tool);
            let agent = match &input.toolbox {
                Some(toolbox) => {
                    let tools = rig_tools(toolbox);
                    builder
                        .tool(tools.list_files)
                        .tool(tools.search)
                        .tool(tools.read_file)
                        .build()
                }
                None => builder.build(),
            };
            let response = agent
                .prompt(input.prompt)
                .max_turns(input.max_turns)
                .await
                .map_err(prompt_error)?;
            if response.output_tool_calls() == 0 {
                return Err(ModelError::Invalid(
                    "the model did not call the structured output tool".to_owned(),
                ));
            }
            serde_json::from_str(&response.output()).map_err(|error| {
                ModelError::Invalid(format!(
                    "submit arguments did not match the schema: {error}"
                ))
            })
        })
    }
}

fn prompt_error(error: PromptError) -> ModelError {
    match error {
        PromptError::MaxTurns { max_turns, .. } => ModelError::Invalid(format!(
            "the model did not submit an explanation within {max_turns} model call(s)"
        )),
        PromptError::UnknownToolCall { tool_name, .. } => ModelError::Invalid(format!(
            "the model called an unavailable tool `{tool_name}`"
        )),
        other => ModelError::Provider(other.to_string()),
    }
}
