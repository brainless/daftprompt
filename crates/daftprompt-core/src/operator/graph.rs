//! Construction of Operator's records from a validated command list.
//!
//! The briefing records are shared with the other agents. Operator appends
//! the manifest lines of every detected command to the summary node's
//! citation contexts, computed by Rust, so each [`ProjectCommand`] points at
//! real contexts. The model's own citations keep their submitted positions.

use daftprompt_types::{
    AgentTaskRequest, BriefingNotice, BriefingNoticeKind, CommandOrigin, CommandsExtraction,
    CommandsResult, ParseError, ProjectCommand, ProjectProfile,
};

use super::config::OperatorConfig;
use super::prompt::PROMPT_VERSION;
use super::validate::ValidatedCommands;
use crate::languages::CommandCandidate;
use crate::project_agent::{BriefingBuild, PromptRecord, ValidatedCitation, build_briefing};
use crate::project_tools::RecordedCall;
use crate::scribe::support::new_id;

pub(crate) struct Built<'a> {
    pub(crate) request: &'a AgentTaskRequest,
    pub(crate) extraction: &'a CommandsExtraction,
    pub(crate) validated: &'a ValidatedCommands,
    pub(crate) candidates: &'a [CommandCandidate],
    pub(crate) profile: ProjectProfile,
    pub(crate) calls: &'a [RecordedCall],
    pub(crate) config: &'a OperatorConfig,
    pub(crate) project_id: String,
    pub(crate) run_id: String,
    pub(crate) started_at: String,
    pub(crate) attempts: u32,
}

pub(crate) fn build(input: Built<'_>) -> Result<CommandsResult, ParseError> {
    // The model's citations first, then the manifest lines of detected
    // commands that are not already among them.
    let mut citations: Vec<ValidatedCitation> = input.validated.citations.clone();
    let mut placed: Vec<Vec<u32>> = Vec::new();
    for command in &input.validated.commands {
        let mut indexes = command.citations.clone();
        if let Some(evidence) = &command.evidence {
            let position = citations
                .iter()
                .position(|existing| existing == evidence)
                .unwrap_or_else(|| {
                    citations.push(evidence.clone());
                    citations.len() - 1
                });
            indexes.push(position as u32 + 1);
        }
        indexes.sort_unstable();
        indexes.dedup();
        placed.push(indexes);
    }

    let mut notices = Vec::new();
    if input.calls.iter().any(|call| call.truncated)
        || input.profile.languages.iter().any(|l| l.truncated)
    {
        notices.push(BriefingNotice {
            kind: BriefingNoticeKind::Truncated,
            detail: "a file listing or search was cut by a size or count limit, so the \
                     commands may not cover the whole project"
                .to_owned(),
        });
    }
    let briefing = build_briefing(BriefingBuild {
        agent: "operator",
        prompt_version: PROMPT_VERSION,
        provider: input.config.provider.clone(),
        model: input.config.model.clone(),
        project_id: input.project_id,
        prompt: input.request.prompt_text().map(|text| PromptRecord {
            text,
            author: input.request.prompt_author(),
        }),
        summary: &input.extraction.summary,
        citations: &citations,
        calls: input.calls,
        submitted: input.extraction,
        notices,
        profile: Some(input.profile),
        run_id: input.run_id,
        started_at: input.started_at,
        attempts: input.attempts,
    })?;

    let commands = input
        .validated
        .commands
        .iter()
        .zip(placed)
        .map(|(command, citation_indexes)| ProjectCommand {
            id: new_id("cmd"),
            label: command.label.clone(),
            command: command.command.clone(),
            working_dir: command.working_dir.clone(),
            kind: command.kind,
            language: command
                .candidate
                .and_then(|slot| input.candidates.get(slot))
                .map(|candidate| candidate.language),
            origin: if command.candidate.is_some() {
                CommandOrigin::Detected
            } else {
                CommandOrigin::Documented
            },
            citation_indexes,
        })
        .collect();
    Ok(CommandsResult { briefing, commands })
}
