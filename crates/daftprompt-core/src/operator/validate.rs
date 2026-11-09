//! Validation of a model-proposed command list before any record exists.

use std::collections::BTreeSet;

use daftprompt_types::{BriefingCitation, BriefingExtraction, CommandKind, CommandsExtraction};

use super::config::OperatorConfig;
use crate::languages::CommandCandidate;
use crate::project_agent::{BriefingLimits, ValidatedCitation, validate_briefing};
use crate::project_tools::{
    ProjectFiles, ProjectToolConfig, Scope, ToolRecorder, check_relative_path, normalize_path_text,
};
use crate::scribe::support::content_hash;

/// Longest accepted command line, in characters.
pub(crate) const MAX_COMMAND_CHARS: usize = 400;
/// Longest accepted label, in characters.
pub(crate) const MAX_LABEL_CHARS: usize = 24;

/// One command that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedCommand {
    pub label: String,
    pub command: String,
    /// Normalized, root-relative; empty is the root.
    pub working_dir: String,
    pub kind: CommandKind,
    /// Index into the candidate list, for a detected command.
    pub candidate: Option<usize>,
    /// The candidate's manifest lines, hashed by Rust from the file.
    pub evidence: Option<ValidatedCitation>,
    /// 1-based indexes into the model's citations, ascending.
    pub citations: Vec<u32>,
}

/// A validated submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedCommands {
    /// The model's citations, in submitted order.
    pub citations: Vec<ValidatedCitation>,
    pub commands: Vec<ResolvedCommand>,
}

/// Check the summary, markers and citations against the run's ledger, then
/// every command against the candidates, the project folder and each other.
/// Only file citations are valid. The error describes the first problem; it is
/// included as diagnostic data on a retry.
pub(crate) fn validate(
    extraction: &CommandsExtraction,
    recorder: &ToolRecorder,
    files: &dyn ProjectFiles,
    candidates: &[CommandCandidate],
    config: &OperatorConfig,
) -> Result<ValidatedCommands, String> {
    if let Some(index) = extraction
        .citations
        .iter()
        .position(|citation| matches!(citation, BriefingCitation::Commit { .. }))
    {
        return Err(format!(
            "citation [{}] is a commit; the Operator may cite files only",
            index + 1
        ));
    }
    let briefing = BriefingExtraction {
        summary: extraction.summary.clone(),
        citations: extraction.citations.clone(),
    };
    let citations = validate_briefing(
        &briefing,
        recorder,
        BriefingLimits {
            max_citations: config.max_citations,
            max_summary_bytes: config.max_summary_bytes,
        },
    )?;

    if extraction.commands.len() > config.max_commands {
        return Err(format!(
            "{} commands exceed the limit of {}",
            extraction.commands.len(),
            config.max_commands
        ));
    }

    let mut labels = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut commands = Vec::with_capacity(extraction.commands.len());
    for (index, proposed) in extraction.commands.iter().enumerate() {
        let n = index + 1;
        let label = proposed.label.trim();
        if label.is_empty() || label.chars().any(char::is_control) {
            return Err(format!("command {n} has an empty or invalid label"));
        }
        if label.chars().count() > MAX_LABEL_CHARS {
            return Err(format!(
                "command {n} has a label of {} characters, over the limit of {MAX_LABEL_CHARS}",
                label.chars().count()
            ));
        }
        if !labels.insert(label.to_lowercase()) {
            return Err(format!("command {n} repeats the label {label:?}"));
        }

        let command = proposed.command.trim();
        if command.is_empty() || command.chars().any(char::is_control) {
            return Err(format!(
                "command {n} is empty or is not a single line of text"
            ));
        }
        if command.chars().count() > MAX_COMMAND_CHARS {
            return Err(format!(
                "command {n} is longer than {MAX_COMMAND_CHARS} characters"
            ));
        }

        let working_dir = working_dir(proposed.working_dir.as_deref())
            .map_err(|detail| format!("command {n}: {detail}"))?;
        if !working_dir.is_empty() {
            let exists = Scope::new(Some(working_dir.clone()), None)
                .and_then(|scope| files.list(&scope, &ProjectToolConfig::default()))
                .is_ok();
            if !exists {
                return Err(format!(
                    "command {n}: working_dir {working_dir:?} is not a directory of the project"
                ));
            }
        }

        if !seen.insert((working_dir.clone(), command.to_owned())) {
            return Err(format!(
                "command {n} repeats {command:?} in {working_dir:?}"
            ));
        }

        let mut cited = BTreeSet::new();
        for &number in &proposed.citations {
            if number == 0 || number as usize > extraction.citations.len() {
                return Err(format!(
                    "command {n} refers to citation {number}, but there are {} citation(s)",
                    extraction.citations.len()
                ));
            }
            cited.insert(number);
        }

        let (candidate, evidence) = match proposed.candidate {
            Some(number) => {
                let slot = (number as usize).checked_sub(1);
                let Some((slot, found)) =
                    slot.and_then(|slot| candidates.get(slot).map(|found| (slot, found)))
                else {
                    return Err(format!(
                        "command {n} names candidate {number}, but there are {} candidate(s)",
                        candidates.len()
                    ));
                };
                if found.command != command || found.working_dir != working_dir {
                    return Err(format!(
                        "command {n} names candidate {number} but changes its command or \
                         working directory"
                    ));
                }
                let evidence = evidence(files, found)
                    .ok_or_else(|| format!("the manifest of candidate {number} cannot be read"))?;
                (Some(slot), Some(evidence))
            }
            None => {
                if cited.is_empty() {
                    return Err(format!(
                        "command {n} ({command:?}) is not a candidate and cites no file"
                    ));
                }
                (None, None)
            }
        };

        commands.push(ResolvedCommand {
            label: label.to_owned(),
            command: command.to_owned(),
            working_dir,
            kind: proposed.kind,
            candidate,
            evidence,
            citations: cited.into_iter().collect(),
        });
    }
    Ok(ValidatedCommands {
        citations,
        commands,
    })
}

/// A normalized root-relative directory; `.` and empty are the root.
fn working_dir(dir: Option<&str>) -> Result<String, String> {
    let Some(dir) = dir.map(str::trim).filter(|dir| !dir.is_empty()) else {
        return Ok(String::new());
    };
    if dir.chars().any(char::is_control) {
        return Err("working_dir contains control characters".to_owned());
    }
    check_relative_path(dir)
        .map(|path| normalize_path_text(&path))
        .map_err(|error| format!("working_dir {dir:?} is not allowed: {error}"))
}

/// The candidate's manifest lines as a citation Rust computed itself.
fn evidence(files: &dyn ProjectFiles, candidate: &CommandCandidate) -> Option<ValidatedCitation> {
    let file = files
        .read(&candidate.evidence.path, &ProjectToolConfig::default())
        .ok()?;
    Some(ValidatedCitation::File {
        path: file.path,
        start_line: candidate.evidence.start_line,
        end_line: candidate.evidence.end_line,
        content_hash: content_hash(&file.text),
    })
}
