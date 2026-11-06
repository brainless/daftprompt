//! The generic runner shared by the project-briefing agents.
//!
//! A briefing agent (Chronicler, and later Surveyor and Operator) is one
//! multi-turn Rig agent with a read-only toolset and a schema-defined output
//! tool, run inside an attempt budget with a per-attempt timeout. This module
//! owns that loop; an agent supplies only what differs:
//!
//! - a [`Toolset`]: the tools and the [`ToolRecorder`] ledger of what they
//!   returned;
//! - the system prompt and the user prompt;
//! - the output type `O` the model submits;
//! - a validator that checks `O` against the toolset's ledger and, on
//!   success, resolves it into `V`.
//!
//! [`run_attempts`] is transport-agnostic (the model is a closure, so tests
//! use stubs); [`run_rig_agent`] is the Rig implementation of one attempt.
//! Explainer keeps its own loop and is not migrated.

mod briefing;

use std::sync::Arc;
use std::time::Duration;

use daftprompt_types::{AgentInfo, AgentTaskRequest, BriefingResult, ParseError};
use rig::DynModel;
use rig::agent::{Agent, AgentBuilder};
use rig::completion::PromptError;
use rig::operation::Completion;
use rig::run::OutputMode;
use rig::schemars::JsonSchema;
use serde::de::DeserializeOwned;

pub use crate::scribe::{BoxFuture, ModelError};

pub use briefing::{
    BriefingBuild, BriefingLimits, PromptRecord, ValidatedCitation, build_briefing,
    validate_briefing,
};

use crate::access::ProjectScope;
use crate::project_tools::ToolRecorder;

/// An object-safe briefing agent for clients that want to erase its model,
/// such as an HTTP service holding an `Arc<dyn Briefer>`.
pub trait Briefer: Send + Sync {
    /// How the agent is configured, for `GET /agents`.
    fn info(&self) -> AgentInfo;

    /// Brief the project the caller's access policy granted.
    fn brief<'a>(
        &'a self,
        request: AgentTaskRequest,
        scope: &ProjectScope,
    ) -> BoxFuture<'a, Result<BriefingResult, ParseError>>;
}

/// The tools of one briefing agent for one attempt.
///
/// A toolset is created fresh for every attempt so its call cap and ledger
/// start empty. Every tool invocation must be recorded in
/// [`recorder`](Self::recorder); validation trusts nothing else.
pub trait Toolset: Send + Sync + 'static {
    /// The ledger the validator checks citations against.
    fn recorder(&self) -> &ToolRecorder;

    /// Register the tools on `builder` and build the agent. The runner has
    /// already set the system prompt and the output tool.
    fn build_agent(self: &Arc<Self>, builder: AgentBuilder) -> Agent;
}

/// Attempt budget and timing of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunLimits {
    /// Model-call budget of one attempt (tool rounds plus the final call).
    pub max_turns: usize,
    /// Total attempts including invalid ones. Values below 1 count as 1.
    pub max_attempts: u32,
    /// Time allowed for one attempt, covering its whole tool loop.
    pub timeout: Duration,
}

/// What one attempt receives.
pub struct AttemptInput<T> {
    /// The assembled user message.
    pub prompt: String,
    /// Fresh per-attempt tools. Its recorder is the ledger the validator
    /// checks citations against.
    pub toolset: Arc<T>,
    /// Model-call budget for the attempt.
    pub max_turns: usize,
}

/// A successful run.
pub struct Completed<T, O, V> {
    /// What the model submitted in the accepted attempt.
    pub extraction: O,
    /// The validator's resolved form of it.
    pub validated: V,
    /// The accepted attempt's toolset (its ledger holds the tool calls).
    pub toolset: Arc<T>,
    /// Attempts used, including invalid ones.
    pub attempts: u32,
}

/// Run attempts until one validates or the budget is spent.
///
/// Each attempt gets a fresh toolset from `new_toolset`. Invalid output (a bad
/// schema, no submission, a turn-limit overrun, or a validator rejection)
/// consumes the budget and the next attempt starts from scratch without
/// feedback. A provider error or an attempt timeout (which covers the whole
/// tool loop) is returned immediately. A partial result is never returned.
pub async fn run_attempts<'a, T, O, V>(
    limits: RunLimits,
    prompt: &str,
    new_toolset: impl Fn() -> Arc<T>,
    attempt: impl Fn(AttemptInput<T>) -> BoxFuture<'a, Result<O, ModelError>>,
    validate: impl Fn(&O, &T) -> Result<V, String>,
) -> Result<Completed<T, O, V>, ParseError>
where
    T: Toolset,
{
    let max_attempts = limits.max_attempts.max(1);
    let mut last_invalid = "no attempt ran".to_owned();
    for number in 1..=max_attempts {
        let toolset = new_toolset();
        let input = AttemptInput {
            prompt: prompt.to_owned(),
            toolset: toolset.clone(),
            max_turns: limits.max_turns,
        };
        let outcome = tokio::time::timeout(limits.timeout, attempt(input)).await;
        let extraction = match outcome {
            Err(_elapsed) => {
                return Err(ParseError::Timeout {
                    detail: format!(
                        "the run exceeded {:?} on attempt {number} of {max_attempts}",
                        limits.timeout
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
        match validate(&extraction, &toolset) {
            Ok(validated) => {
                return Ok(Completed {
                    extraction,
                    validated,
                    toolset,
                    attempts: number,
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

/// One attempt on a Rig model: a multi-turn agent with the toolset and a
/// schema-defined output tool (`OutputMode::Tool`, named `final_result` by
/// Rig), bounded by `input.max_turns`. The run ends when the model calls the
/// output tool; its arguments are deserialized as `O`.
///
/// Rig's `Extractor` is single-turn, which is why this builds an agent.
pub async fn run_rig_agent<T, O>(
    model: &DynModel<Completion>,
    system_prompt: &str,
    input: AttemptInput<T>,
) -> Result<O, ModelError>
where
    T: Toolset,
    O: JsonSchema + DeserializeOwned + Send + Sync + 'static,
{
    let builder = AgentBuilder::new(model.clone())
        .preamble(system_prompt)
        .output_schema::<O>()
        .output_mode(OutputMode::Tool);
    let agent = input.toolset.build_agent(builder);
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
}

fn prompt_error(error: PromptError) -> ModelError {
    match error {
        PromptError::MaxTurns { max_turns, .. } => ModelError::Invalid(format!(
            "the model did not submit a result within {max_turns} model call(s)"
        )),
        PromptError::UnknownToolCall { tool_name, .. } => ModelError::Invalid(format!(
            "the model called an unavailable tool `{tool_name}`"
        )),
        other => ModelError::Provider(other.to_string()),
    }
}
