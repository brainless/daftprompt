//! Validation of a model-proposed history briefing before any record exists.

use daftprompt_types::BriefingExtraction;

use super::config::ChroniclerConfig;
use crate::project_agent::{BriefingLimits, ValidatedCitation, validate_briefing};
use crate::project_tools::ToolRecorder;

/// Check the summary, its `[n]` markers, and every citation against the run's
/// ledger. A commit citation must be a hash, or a prefix of at least 7 hex
/// characters, that resolves to exactly one commit a git tool returned in this
/// run; Chronicler has no file tools, so a file citation never validates.
/// The error is a description of the first problem; it is never fed back to
/// the model.
pub(crate) fn validate(
    extraction: &BriefingExtraction,
    recorder: &ToolRecorder,
    config: &ChroniclerConfig,
) -> Result<Vec<ValidatedCitation>, String> {
    validate_briefing(
        extraction,
        recorder,
        BriefingLimits {
            max_citations: config.max_citations,
            max_summary_bytes: config.max_summary_bytes,
        },
    )
}
