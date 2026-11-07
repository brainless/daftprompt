//! Validation of a model-proposed overview before any record exists.

use daftprompt_types::{BriefingCitation, BriefingExtraction};

use super::config::SurveyorConfig;
use crate::project_agent::{BriefingLimits, ValidatedCitation, validate_briefing};
use crate::project_tools::ToolRecorder;

/// Check the summary, its `[n]` markers, and every citation against the run's
/// ledger. Only file citations are valid: each must lie within lines a
/// `read_file` call returned in this run. Surveyor has no git tools, so a
/// commit citation is rejected outright. The error is a description of the
/// first problem; it is never fed back to the model.
pub(crate) fn validate(
    extraction: &BriefingExtraction,
    recorder: &ToolRecorder,
    config: &SurveyorConfig,
) -> Result<Vec<ValidatedCitation>, String> {
    if let Some(index) = extraction
        .citations
        .iter()
        .position(|citation| matches!(citation, BriefingCitation::Commit { .. }))
    {
        return Err(format!(
            "citation [{}] is a commit; the Surveyor may cite files only",
            index + 1
        ));
    }
    validate_briefing(
        extraction,
        recorder,
        BriefingLimits {
            max_citations: config.max_citations,
            max_summary_bytes: config.max_summary_bytes,
        },
    )
}
