//! Capture and verification of the deterministic inputs behind Surveyor and
//! Operator responses ([Epic 007](../../../../epics/007-project-paths-and-agent-storage.md),
//! Task C2).
//!
//! A file agent's output depends on everything the model saw: the profile,
//! the preloaded files, the Rust-detected command candidates (Operator), and
//! the results of its own `list_files`, `search`, and `read_file` calls. A
//! matching commit does not establish that (ignored files,
//! local excludes, filters, sparse checkouts), so [`capture_files`] records
//! those observations and [`verify_files`] repeats them against the requested
//! path. Nothing is executed and no model is called; `submit` is never
//! replayed.
//!
//! An observation is the model-visible outcome of one call: the output text
//! (errors and truncation markers included), the error and truncation flags,
//! and the whole-file hash of a successful read. Generated IDs, timestamps,
//! and absolute roots are not part of it, and the tools only ever print
//! root-relative paths.

use std::path::Path;
use std::sync::Arc;

use daftprompt_types::ProjectProfile;
use serde_json::{Value, json};

use super::identity::hash_canonical;
use super::preload::{PreloadedFiles, preload_common_files};
use crate::languages::{self, CommandCandidate};
use crate::project_tools::{
    LIST_FILES, ProjectFiles, ProjectToolConfig, ProjectToolbox, READ_FILE, RecordedCall, SEARCH,
    Scope,
};
use crate::scribe::support::content_hash;
use crate::store::{FileInputsV1, ObservedCall};

/// SHA-256 of the canonical profile.
pub(crate) fn profile_hash(profile: &ProjectProfile) -> String {
    hash_canonical(&serde_json::to_value(profile).expect("a profile serializes"))
}

/// What one call showed the model.
fn observation(call: &RecordedCall) -> Value {
    json!({
        "tool": call.name,
        "args": call.args,
        "output": call.output,
        "is_error": call.is_error,
        "truncated": call.truncated,
        "read": call.read,
    })
}

fn observation_hash(call: &RecordedCall) -> String {
    hash_canonical(&observation(call))
}

/// SHA-256 of a whole ledger, in call order.
pub(crate) fn ledger_fingerprint(calls: &[RecordedCall]) -> String {
    hash_canonical(&Value::Array(calls.iter().map(observation).collect()))
}

/// SHA-256 of the command candidates and of the manifests their evidence
/// cites. The manifests are read as the command validation reads them, so a
/// change outside the cited lines still changes the stored citation hash.
pub(crate) fn candidates_fingerprint(
    files: &dyn ProjectFiles,
    candidates: &[CommandCandidate],
) -> String {
    let mut manifests: Vec<(&str, String)> = Vec::new();
    for candidate in candidates {
        let path = candidate.evidence.path.as_str();
        if manifests.iter().all(|(known, _)| *known != path) {
            let hash = files
                .read(path, &ProjectToolConfig::default())
                .map(|file| content_hash(&file.text))
                .unwrap_or_else(|error| format!("error: {error}"));
            manifests.push((path, hash));
        }
    }
    hash_canonical(&json!({"candidates": candidates, "manifests": manifests}))
}

/// Record a run's file observations for later verification.
///
/// `calls` is the accepted attempt's ledger: the preload seed first, then the
/// model's calls. `Err` says why the run cannot be verified later, and the
/// response is then never reused: a seed that is not at the start of the
/// ledger, a call of a kind the verifier cannot replay, or an output that
/// shows a machine-absolute root.
pub(crate) fn capture_files(
    profile: &ProjectProfile,
    preload: &PreloadedFiles,
    calls: &[RecordedCall],
    tools: &ProjectToolConfig,
    root: &Path,
) -> Result<FileInputsV1, String> {
    let seeded = preload.ledger().len();
    let preload_fingerprint = ledger_fingerprint(preload.ledger());
    if calls.len() < seeded || ledger_fingerprint(&calls[..seeded]) != preload_fingerprint {
        return Err("the preload seed is not at the start of the tool ledger".to_owned());
    }
    let roots: Vec<String> = [Some(root.to_path_buf()), root.canonicalize().ok()]
        .into_iter()
        .flatten()
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|path| path.len() > 1)
        .collect();
    let mut model_calls = Vec::new();
    for call in calls {
        if roots.iter().any(|root| call.output.contains(root.as_str())) {
            return Err(format!("a {} output shows an absolute path", call.name));
        }
    }
    for call in &calls[seeded..] {
        if ![LIST_FILES, SEARCH, READ_FILE].contains(&call.name.as_str()) {
            return Err(format!("the {} call kind cannot be replayed", call.name));
        }
        model_calls.push(ObservedCall {
            tool: call.name.clone(),
            args: call.args.clone(),
            observation_hash: observation_hash(call),
        });
    }
    Ok(FileInputsV1 {
        profile_hash: profile_hash(profile),
        preload_fingerprint,
        tool_limits: tools.cache_settings(),
        model_calls,
    })
}

/// The Operator-only checks.
pub(crate) struct OperatorChecks<'a> {
    pub candidates_hash: &'a str,
    pub working_dirs: &'a [String],
}

/// Repeat a file agent's observations against `files` (the requested,
/// authorized path). `Err` is the miss reason, with no source text.
///
/// In order: the profile, a fresh `preload_common_files` run compared with
/// the saved ledger fingerprint, Operator's candidates and working
/// directories, then the model's own calls replayed under their original
/// limits. Blocking filesystem work runs off the async runtime.
pub(crate) async fn verify_files(
    files: Arc<dyn ProjectFiles>,
    inputs: &FileInputsV1,
    operator: Option<OperatorChecks<'_>>,
) -> Result<(), String> {
    let Some(tools) = ProjectToolConfig::from_cache_settings(&inputs.tool_limits) else {
        return Err("the recorded tool limits are unusable".to_owned());
    };
    let profile = {
        let files = files.clone();
        tokio::task::spawn_blocking(move || languages::profile(files.as_ref()))
            .await
            .map_err(|error| format!("the profile check failed: {error}"))?
    };
    if profile_hash(&profile) != inputs.profile_hash {
        return Err("the project profile changed".to_owned());
    }

    let preload = preload_common_files(files.clone(), &profile, tools.clone(), None)
        .await
        .map_err(|error| format!("the preload check failed: {error}"))?;
    if ledger_fingerprint(preload.ledger()) != inputs.preload_fingerprint {
        return Err("the preloaded files differ".to_owned());
    }

    if let Some(operator) = operator {
        let candidates_hash = {
            let files = files.clone();
            let profile = profile.clone();
            tokio::task::spawn_blocking(move || {
                let candidates = languages::command_candidates(files.as_ref(), &profile);
                candidates_fingerprint(files.as_ref(), &candidates)
            })
            .await
            .map_err(|error| format!("the command check failed: {error}"))?
        };
        if candidates_hash != operator.candidates_hash {
            return Err("the detected commands differ".to_owned());
        }
        let dirs = operator.working_dirs.to_vec();
        let files = files.clone();
        let missing = tokio::task::spawn_blocking(move || {
            dirs.into_iter().find(|dir| {
                Scope::new(Some(dir.clone()), None)
                    .and_then(|scope| files.list(&scope, &ProjectToolConfig::default()))
                    .is_err()
            })
        })
        .await
        .map_err(|error| format!("the working directory check failed: {error}"))?;
        if missing.is_some() {
            return Err("a command's working directory is not available".to_owned());
        }
    }

    let calls = inputs.model_calls.clone();
    tokio::task::spawn_blocking(move || {
        let toolbox = ProjectToolbox::new(files, tools);
        for (number, expected) in calls.iter().enumerate() {
            if ![LIST_FILES, SEARCH, READ_FILE].contains(&expected.tool.as_str()) {
                return Err(format!("call {} is of an unsupported kind", number + 1));
            }
            toolbox.call(&expected.tool, expected.args.clone());
            let replayed = toolbox.recorder().calls().pop();
            if replayed.as_ref().map(observation_hash).as_deref()
                != Some(expected.observation_hash.as_str())
            {
                return Err(format!(
                    "the result of model call {} ({}) differs",
                    number + 1,
                    expected.tool
                ));
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("the replay failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_tools::InMemoryProject;

    fn call(name: &str, output: &str) -> RecordedCall {
        RecordedCall {
            index: 0,
            name: name.to_owned(),
            args: json!({}),
            output: output.to_owned(),
            is_error: false,
            truncated: false,
            read: None,
            commits: Vec::new(),
        }
    }

    async fn setup() -> (ProjectProfile, PreloadedFiles, ProjectToolConfig) {
        let files: Arc<dyn ProjectFiles> =
            Arc::new(InMemoryProject::new().with_file("README.md", "Overview\n"));
        let profile = languages::profile(files.as_ref());
        let tools = ProjectToolConfig::default();
        let preload = preload_common_files(files, &profile, tools.clone(), None)
            .await
            .unwrap();
        (profile, preload, tools)
    }

    #[tokio::test]
    async fn capture_separates_the_seed_from_the_models_calls() {
        let (profile, preload, tools) = setup().await;
        let mut ledger = preload.ledger().to_vec();
        assert!(!ledger.is_empty());
        ledger.push(call(READ_FILE, "x"));
        let inputs = capture_files(&profile, &preload, &ledger, &tools, Path::new("/r")).unwrap();
        assert_eq!(inputs.model_calls.len(), 1);
        assert_eq!(inputs.model_calls[0].tool, READ_FILE);
        assert_eq!(
            inputs.preload_fingerprint,
            ledger_fingerprint(preload.ledger())
        );
        // The seed is not at the start of this ledger.
        let reversed: Vec<_> = ledger.iter().rev().cloned().collect();
        assert!(capture_files(&profile, &preload, &reversed, &tools, Path::new("/r")).is_err());
        assert!(capture_files(&profile, &preload, &[], &tools, Path::new("/r")).is_err());
    }

    #[tokio::test]
    async fn capture_refuses_unreplayable_calls_and_absolute_roots() {
        let (profile, preload, tools) = setup().await;
        let mut ledger = preload.ledger().to_vec();
        ledger.push(call("git_log", "x"));
        let error =
            capture_files(&profile, &preload, &ledger, &tools, Path::new("/r")).unwrap_err();
        assert!(error.contains("git_log"));
        ledger.pop();
        ledger.push(call(READ_FILE, "error: io error: /srv/project/a: denied"));
        let error = capture_files(
            &profile,
            &preload,
            &ledger,
            &tools,
            Path::new("/srv/project"),
        )
        .unwrap_err();
        assert!(error.contains("absolute path"));
    }

    #[test]
    fn observations_ignore_ids_but_not_outcomes() {
        let mut a = call(READ_FILE, "1: a");
        let mut b = a.clone();
        b.index = 7;
        assert_eq!(observation_hash(&a), observation_hash(&b));
        a.truncated = true;
        assert_ne!(observation_hash(&a), observation_hash(&b));
        b.truncated = true;
        b.output = "error: not found".to_owned();
        assert_ne!(observation_hash(&a), observation_hash(&b));
    }

    #[test]
    fn tool_limits_round_trip() {
        let tools = ProjectToolConfig {
            max_read_lines: 7,
            ..Default::default()
        };
        assert_eq!(
            ProjectToolConfig::from_cache_settings(&tools.cache_settings()),
            Some(tools)
        );
        assert!(ProjectToolConfig::from_cache_settings(&json!({"max_tool_calls": 1})).is_none());
    }
}
