//! Construction of Surveyor's records from a validated briefing.
//!
//! The record shape is shared with the other briefing agents; this module
//! fixes what is Surveyor's: the agent name, prompt version, the profile it
//! was given, and the notices its file tools raised.

use daftprompt_types::{
    AgentTaskRequest, BriefingExtraction, BriefingNotice, BriefingNoticeKind, BriefingResult,
    ParseError, ProjectProfile,
};

use super::config::SurveyorConfig;
use super::prompt::PROMPT_VERSION;
use crate::project_agent::{BriefingBuild, PromptRecord, ValidatedCitation, build_briefing};
use crate::project_tools::RecordedCall;

pub(crate) struct Built<'a> {
    pub(crate) request: &'a AgentTaskRequest,
    pub(crate) extraction: &'a BriefingExtraction,
    pub(crate) citations: &'a [ValidatedCitation],
    pub(crate) profile: ProjectProfile,
    pub(crate) calls: &'a [RecordedCall],
    pub(crate) config: &'a SurveyorConfig,
    pub(crate) project_id: String,
    pub(crate) run_id: String,
    pub(crate) started_at: String,
    pub(crate) attempts: u32,
}

pub(crate) fn build(input: Built<'_>) -> Result<BriefingResult, ParseError> {
    let mut notices = Vec::new();
    if input.calls.iter().any(|call| call.truncated)
        || input.profile.languages.iter().any(|l| l.truncated)
    {
        notices.push(BriefingNotice {
            kind: BriefingNoticeKind::Truncated,
            detail: "a file listing or search was cut by a size or count limit, so the \
                     overview may not cover the whole project"
                .to_owned(),
        });
    }
    build_briefing(BriefingBuild {
        agent: "surveyor",
        prompt_version: PROMPT_VERSION,
        provider: input.config.provider.clone(),
        model: input.config.model.clone(),
        project_id: input.project_id,
        prompt: input.request.prompt_text().map(|text| PromptRecord {
            text,
            author: input.request.prompt_author(),
        }),
        summary: &input.extraction.summary,
        citations: input.citations,
        calls: input.calls,
        submitted: input.extraction,
        notices,
        profile: Some(input.profile),
        run_id: input.run_id,
        started_at: input.started_at,
        attempts: input.attempts,
    })
}
