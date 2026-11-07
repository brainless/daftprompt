//! Construction of Chronicler's records from a validated briefing.
//!
//! The record shape is shared with the other briefing agents; this module
//! fixes what is Chronicler's: the agent name, prompt version, and the
//! notices its git tools raised.

use daftprompt_types::{
    AgentTaskRequest, BriefingExtraction, BriefingNotice, BriefingNoticeKind, BriefingResult,
    ParseError,
};

use super::config::ChroniclerConfig;
use super::prompt::PROMPT_VERSION;
use crate::git_tools::{GitProblem, GitToolbox};
use crate::project_agent::{BriefingBuild, PromptRecord, ValidatedCitation, build_briefing};
use crate::project_tools::RecordedCall;

pub(crate) struct Built<'a> {
    pub(crate) request: &'a AgentTaskRequest,
    pub(crate) extraction: &'a BriefingExtraction,
    pub(crate) citations: &'a [ValidatedCitation],
    pub(crate) git: &'a GitToolbox,
    pub(crate) calls: &'a [RecordedCall],
    pub(crate) config: &'a ChroniclerConfig,
    pub(crate) project_id: String,
    pub(crate) run_id: String,
    pub(crate) started_at: String,
    pub(crate) attempts: u32,
}

pub(crate) fn build(input: Built<'_>) -> Result<BriefingResult, ParseError> {
    let notices = notices(input.git, input.calls);
    build_briefing(BriefingBuild {
        agent: "chronicler",
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
        profile: None,
        run_id: input.run_id,
        started_at: input.started_at,
        attempts: input.attempts,
    })
}

/// What the reader should know about the history behind the summary.
fn notices(git: &GitToolbox, calls: &[RecordedCall]) -> Vec<BriefingNotice> {
    let mut notices = Vec::new();
    match git.problem() {
        Some(GitProblem::NotARepository) => notices.push(BriefingNotice {
            kind: BriefingNoticeKind::NotAGitRepository,
            detail: "the project folder is not inside a git repository, so there is no \
                     history to summarize"
                .to_owned(),
        }),
        Some(GitProblem::Unavailable(detail)) => notices.push(BriefingNotice {
            kind: BriefingNoticeKind::GitUnavailable,
            detail: format!("git could not be run: {detail}"),
        }),
        None => {}
    }
    if calls.iter().any(|call| call.truncated) {
        notices.push(BriefingNotice {
            kind: BriefingNoticeKind::Truncated,
            detail: "a git result was cut by a size or count limit, so the summary may not \
                     cover all of the history"
                .to_owned(),
        });
    }
    notices
}
