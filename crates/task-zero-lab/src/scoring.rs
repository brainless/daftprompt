//! Scoring: compare agent output against practical relevance sets.
//!
//! Scores both prompt quality (checkable from prompt text alone) and task
//! outcome (measured from the agent's diff in the worktree), per Design
//! Constraint 7.

use std::collections::HashSet;
use std::path::Path;

use crate::eval::{
    AgentResult, EvalScore, ExpectedArtifact, PracticalRelevanceSet, PromptQualityScore,
    PromptVariant, TaskOutcomeScore,
};

// ── Prompt quality scoring ─────────────────────────────────────────────

/// Score prompt quality from the prompt text and the original request.
/// This is checkable without running an agent (Design Constraint 7).
pub fn score_prompt_quality(
    variant: &PromptVariant,
    original_request: &str,
    negative_constraints: &[String],
) -> PromptQualityScore {
    let text = &variant.text;

    PromptQualityScore {
        intent_preserved: text.contains(original_request),
        constraints_preserved: negative_constraints.iter().all(|c| text.contains(c)),
        has_provenance_labels: text.contains("[E") || text.contains("@ ") || text.contains("provenance"),
        has_gap_disclosure: text.to_lowercase().contains("gap")
            || text.to_lowercase().contains("omission")
            || text.to_lowercase().contains("missing")
            || text.to_lowercase().contains("not available"),
        prompt_bytes: variant.byte_count,
        token_estimate: variant.token_estimate,
        omitted_context: vec![], // populated by caller if prompt renderer reports omissions
    }
}

// ── Task outcome scoring ───────────────────────────────────────────────

/// Score task outcome by comparing the actual worktree diff against the
/// practical relevance set.
///
/// `changed_files` is the ground truth: it must come from
/// `EvalWorktree::changed_files()`, read from the real worktree state after
/// the agent ran and before the worktree guard removes it — never from
/// `agent.touched_files`, which is at best a model self-report (and empty
/// for the patch-apply adapter). Artifact recall/precision and prohibited-
/// change detection are computed from `changed_files`. `agent.touched_files`
/// is still consulted by `count_unsupported_claims` as a secondary,
/// diagnostic signal (see that function's doc comment) — a mismatch between
/// what the model claimed and what actually changed is itself informative,
/// not evidence to score outcome against.
pub fn score_task_outcome(
    agent: &AgentResult,
    changed_files: &[String],
    relevance: &PracticalRelevanceSet,
    worktree_root: &Path,
    prompt_text: &str,
) -> TaskOutcomeScore {
    let touched: HashSet<&str> = changed_files.iter().map(|s| s.as_str()).collect();

    // Artifact recall: fraction of expected changes actually touched.
    let expected_changed: Vec<&ExpectedArtifact> = relevance
        .expected_changes
        .iter()
        .filter(|a| a.should_change)
        .collect();

    let recall = if expected_changed.is_empty() {
        1.0 // no expected changes = trivially satisfied
    } else {
        let hit = expected_changed
            .iter()
            .filter(|a| path_matches_any(&a.path, &touched))
            .count();
        hit as f64 / expected_changed.len() as f64
    };

    // Artifact precision: fraction of touched files that are expected.
    let precision = if touched.is_empty() {
        1.0 // agent touched nothing = no extraneous changes
    } else {
        let all_expected: HashSet<&str> = relevance
            .expected_changes
            .iter()
            .map(|a| a.path.as_str())
            .collect();
        let relevant_touched = touched
            .iter()
            .filter(|t| all_expected.contains(**t) || path_matches_any_set(t, &all_expected))
            .count();
        relevant_touched as f64 / touched.len() as f64
    };

    // Prohibited changes: files that must NOT change but were touched.
    let violated: Vec<String> = relevance
        .prohibited_changes
        .iter()
        .filter(|a| !a.should_change && path_matches_any(&a.path, &touched))
        .map(|a| a.path.clone())
        .collect();

    // Verification: run verification commands if possible.
    let (verification_passed, commands_run, verification_diag) =
        run_verification(worktree_root, &relevance.verification_commands);

    // Unsupported claims: heuristic scan of agent response for file paths
    // and identifiers not traceable to the prompt context or the real diff,
    // plus a check of the model's own touched_files self-report against the
    // real diff.
    let unsupported = count_unsupported_claims(
        &agent.response_text,
        prompt_text,
        &touched,
        &agent.touched_files,
    );

    // Negative constraints: check if the diff violates them.
    // For now, check whether prohibited files appear in the diff.
    let constraints_respected = violated.is_empty();

    TaskOutcomeScore {
        artifact_recall: recall,
        artifact_precision: precision,
        violated_prohibitions: violated,
        verification_passed,
        verification_commands_run: commands_run,
        verification_diagnostics: verification_diag,
        unsupported_claims: unsupported,
        negative_constraints_respected: constraints_respected,
    }
}

/// Score both prompt quality and task outcome. `changed_files` must be the
/// ground-truth list from `EvalWorktree::changed_files()` — see
/// [`score_task_outcome`].
pub fn score_run(
    variant: &PromptVariant,
    agent: &AgentResult,
    changed_files: &[String],
    original_request: &str,
    negative_constraints: &[String],
    relevance: &PracticalRelevanceSet,
    worktree_root: &Path,
) -> EvalScore {
    EvalScore {
        prompt_quality: score_prompt_quality(variant, original_request, negative_constraints),
        task_outcome: score_task_outcome(
            agent,
            changed_files,
            relevance,
            worktree_root,
            &variant.text,
        ),
    }
}

// ── Path matching ──────────────────────────────────────────────────────

/// Check if a path pattern matches any of the touched files.
/// Supports prefix matching for directory patterns (paths ending with `/`).
fn path_matches_any(pattern: &str, touched: &HashSet<&str>) -> bool {
    if pattern.ends_with('/') {
        // Directory prefix match
        touched.iter().any(|t| t.starts_with(pattern))
    } else {
        // Exact match or glob-like suffix match
        touched.iter().any(|t| {
            *t == pattern
                || (pattern.contains('*') && glob_match(pattern, t))
        })
    }
}

/// Check if a path matches any pattern in a set.
fn path_matches_any_set(path: &str, patterns: &HashSet<&str>) -> bool {
    patterns.iter().any(|p| {
        *p == path
            || (p.ends_with('/') && path.starts_with(*p))
            || (p.contains('*') && glob_match(p, path))
    })
}

/// Simple glob matching (supports `*` as a wildcard).
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match text[pos..].find(part) {
            Some(found) => {
                if i == 0 && found != 0 {
                    return false; // first part must match at start
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }
    // last part must match at end
    if let Some(last) = parts.last() {
        if !last.is_empty() && !text.ends_with(last) {
            return false;
        }
    }
    true
}

// ── Unsupported claims detection ───────────────────────────────────────

/// Heuristic unsupported-claims counter: scans the agent's response for
/// file paths and backtick-quoted identifiers not present in the prompt
/// context or `changed_files` (the ground-truth worktree diff). This is a
/// heuristic, not a model — it catches file paths, function names, and
/// factual assertions that don't appear in the prompt text.
///
/// Because `changed_files` is ground truth (not the model's self-report),
/// this doubles as a "model describes an edit it never made" check: a path
/// the response mentions that isn't in the prompt and isn't actually in the
/// diff is flagged.
///
/// `claimed_files` is `agent.touched_files` — the model/adapter's own
/// self-reported touched-files list (empty for the patch-apply adapter,
/// regex-extracted for the free-form adapters). Any entry there that is not
/// actually in `changed_files` is also counted: it is a "model claims to
/// have touched X but the real diff shows otherwise" case, symmetric to the
/// response-text scan above but checking the structured self-report rather
/// than prose. This is a small, cheap addition now that ground truth is
/// available — it is not a full claim-by-claim verifier of response prose.
fn count_unsupported_claims(
    response: &str,
    prompt: &str,
    changed_files: &HashSet<&str>,
    claimed_files: &[String],
) -> usize {
    let prompt_lower = prompt.to_lowercase();
    let mut unsupported = 0usize;

    for claimed in claimed_files {
        if !changed_files.contains(claimed.as_str()) {
            unsupported += 1;
        }
    }

    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        // Extract file paths mentioned in backticks or as bare paths
        let paths = extract_paths_from_line(trimmed);
        for p in &paths {
            let p_lower = p.to_lowercase();
            if !prompt_lower.contains(&p_lower)
                && !changed_files.iter().any(|t| t.to_lowercase() == p_lower)
                && !p.starts_with("http")
            {
                unsupported += 1;
            }
        }

        // Extract backtick-quoted identifiers that look like function/method names
        // (contain _ or ::, or are camelCase)
        let idents = extract_code_idents(trimmed);
        for ident in &idents {
            if !prompt_lower.contains(&ident.to_lowercase()) {
                unsupported += 1;
            }
        }
    }

    unsupported
}

/// Extract file-like paths from a line. Looks for backtick-quoted strings
/// containing a dot-slash or common code extensions, and bare paths.
fn extract_paths_from_line(line: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Backtick-quoted paths: `path/to/file.ext`
    let mut in_backtick = false;
    let mut current = String::new();
    for ch in line.chars() {
        if ch == '`' {
            if in_backtick && !current.is_empty() {
                if looks_like_path(&current) {
                    paths.push(current.clone());
                }
            }
            current.clear();
            in_backtick = !in_backtick;
        } else if in_backtick {
            current.push(ch);
        }
    }

    // Bare paths: lines starting with "Modified:", "Changed:", "File:", etc.
    let lower = line.to_lowercase();
    if lower.starts_with("modified:")
        || lower.starts_with("changed:")
        || lower.starts_with("created:")
        || lower.starts_with("file:")
    {
        if let Some(rest) = line.split(':').nth(1) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() && looks_like_path(trimmed) {
                paths.push(trimmed.to_string());
            }
        }
    }

    paths
}

/// Check if a string looks like a file path.
fn looks_like_path(s: &str) -> bool {
    // Must contain a dot and at least one slash, OR end with a common extension
    let code_extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".md", ".toml",
        ".json", ".yaml", ".yml", ".sql", ".html", ".css", ".sh",
    ];
    (s.contains('/') && s.contains('.'))
        || code_extensions.iter().any(|ext| s.ends_with(ext))
}

/// Extract code identifiers from backtick-quoted text that look like
/// function or method names (snake_case, camelCase, or :: qualified).
fn extract_code_idents(line: &str) -> Vec<String> {
    let mut idents = Vec::new();
    let mut in_backtick = false;
    let mut current = String::new();
    for ch in line.chars() {
        if ch == '`' {
            if in_backtick && !current.is_empty() {
                // Skip paths (already handled) and very short tokens
                if current.len() > 3
                    && !looks_like_path(&current)
                    && (current.contains('_')
                        || current.contains("::")
                        || current.chars().any(|c| c.is_uppercase()))
                {
                    idents.push(current.clone());
                }
            }
            current.clear();
            in_backtick = !in_backtick;
        } else if in_backtick {
            current.push(ch);
        }
    }
    idents
}

// ── Verification ───────────────────────────────────────────────────────

/// Run verification commands in the worktree and return (pass, commands_run, diagnostics).
/// Commands are run sequentially; any failure short-circuits.
/// Stdout/stderr are captured for diagnostics. Commands that don't exist
/// are handled gracefully with exit-code 127 reporting.
fn run_verification(
    worktree_root: &Path,
    commands: &[String],
) -> (Option<bool>, Vec<String>, Vec<String>) {
    if commands.is_empty() {
        return (None, vec![], vec![]);
    }

    let mut run = Vec::new();
    let mut diagnostics = Vec::new();
    for cmd in commands {
        run.push(cmd.clone());
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(worktree_root)
            .output();
        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                if !stdout.is_empty() {
                    diagnostics.push(format!("[stdout] {}: {}", cmd, truncate(&stdout, 500)));
                }
                if !stderr.is_empty() {
                    diagnostics.push(format!("[stderr] {}: {}", cmd, truncate(&stderr, 500)));
                }
                if !o.status.success() {
                    let code = o.status.code().unwrap_or(-1);
                    diagnostics.push(format!(
                        "[exit] {}: exit code {}",
                        cmd, code
                    ));
                    return (Some(false), run, diagnostics);
                }
            }
            Err(e) => {
                diagnostics.push(format!("[error] {}: {}", cmd, e));
                return (Some(false), run, diagnostics);
            }
        }
    }
    (Some(true), run, diagnostics)
}

/// Truncate a string to `max_len` chars, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{AgentResult, PracticalRelevanceSet};

    fn make_variant(text: &str) -> PromptVariant {
        PromptVariant {
            kind: crate::eval::PromptVariantKind::DeterministicBaseline,
            text: text.into(),
            byte_count: text.len(),
            token_estimate: text.len() / 4,
            helper_report_json: None,
        }
    }

    fn make_agent(touched: Vec<&str>) -> AgentResult {
        AgentResult {
            response_text: "done".into(),
            touched_files: touched.into_iter().map(String::from).collect(),
            diff: None,
            input_tokens: 100,
            output_tokens: 50,
            elapsed_ms: 1000,
            stop_reason: Some("stop".into()),
            diagnostics: vec![],
        }
    }

    fn make_relevance(
        expected: Vec<(&str, bool)>,
        prohibited: Vec<(&str, bool)>,
    ) -> PracticalRelevanceSet {
        PracticalRelevanceSet {
            expected_changes: expected
                .into_iter()
                .map(|(p, s)| ExpectedArtifact {
                    path: p.into(),
                    should_change: s,
                    expected_blob_hash: None,
                    note: "test".into(),
                })
                .collect(),
            prohibited_changes: prohibited
                .into_iter()
                .map(|(p, s)| ExpectedArtifact {
                    path: p.into(),
                    should_change: s,
                    expected_blob_hash: None,
                    note: "test".into(),
                })
                .collect(),
            verification_commands: vec![],
            evidence_commits: vec![],
            evidence_source: "test".into(),
        }
    }

    #[test]
    fn prompt_quality_detects_intent_preservation() {
        let v = make_variant("Please analyze Epic 020 and suggest changes.");
        let score = score_prompt_quality(
            &v,
            "analyze Epic 020",
            &["read-only".into()],
        );
        assert!(score.intent_preserved);
    }

    #[test]
    fn prompt_quality_detects_missing_intent() {
        let v = make_variant("Some unrelated prompt.");
        let score = score_prompt_quality(
            &v,
            "analyze Epic 020",
            &["read-only".into()],
        );
        assert!(!score.intent_preserved);
    }

    #[test]
    fn prompt_quality_detects_constraint_preservation() {
        let v = make_variant("Analyze with read-only constraint.");
        let score = score_prompt_quality(&v, "Analyze", &["read-only".into()]);
        assert!(score.constraints_preserved);

        let v2 = make_variant("Analyze.");
        let score2 = score_prompt_quality(&v2, "Analyze", &["read-only".into()]);
        assert!(!score2.constraints_preserved);
    }

    #[test]
    fn artifact_recall_full_when_all_touched() {
        let agent = make_agent(vec!["src/foo.rs", "src/bar.rs"]);
        let rel = make_relevance(vec![("src/foo.rs", true), ("src/bar.rs", true)], vec![]);
        let changed = agent.touched_files.clone();
        let score = score_task_outcome(&agent, &changed, &rel, Path::new("/tmp"), "analyze foo and bar");
        assert!((score.artifact_recall - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn artifact_recall_partial_when_subset_touched() {
        let agent = make_agent(vec!["src/foo.rs"]);
        let rel = make_relevance(vec![("src/foo.rs", true), ("src/bar.rs", true)], vec![]);
        let changed = agent.touched_files.clone();
        let score = score_task_outcome(&agent, &changed, &rel, Path::new("/tmp"), "analyze foo and bar");
        assert!((score.artifact_recall - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn artifact_precision_penalizes_extraneous_touches() {
        let agent = make_agent(vec!["src/foo.rs", "src/unrelated.rs"]);
        let rel = make_relevance(vec![("src/foo.rs", true)], vec![]);
        let changed = agent.touched_files.clone();
        let score = score_task_outcome(&agent, &changed, &rel, Path::new("/tmp"), "analyze foo");
        assert!((score.artifact_precision - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn prohibited_changes_detected() {
        let agent = make_agent(vec!["src/foo.rs", "gui/other.rs"]);
        let rel = make_relevance(
            vec![("src/foo.rs", true)],
            vec![("gui/", false)],
        );
        let changed = agent.touched_files.clone();
        let score = score_task_outcome(&agent, &changed, &rel, Path::new("/tmp"), "analyze foo");
        assert!(!score.violated_prohibitions.is_empty());
        assert!(!score.negative_constraints_respected);
    }

    #[test]
    fn no_violation_when_prohibited_untouched() {
        let agent = make_agent(vec!["src/foo.rs"]);
        let rel = make_relevance(
            vec![("src/foo.rs", true)],
            vec![("gui/", false)],
        );
        let changed = agent.touched_files.clone();
        let score = score_task_outcome(&agent, &changed, &rel, Path::new("/tmp"), "analyze foo");
        assert!(score.violated_prohibitions.is_empty());
        assert!(score.negative_constraints_respected);
    }

    #[test]
    fn directory_prefix_matching_works() {
        let agent = make_agent(vec!["gui/pages/Home.tsx"]);
        let rel = make_relevance(vec![], vec![("gui/", false)]);
        let changed = agent.touched_files.clone();
        let score = score_task_outcome(&agent, &changed, &rel, Path::new("/tmp"), "no gui changes");
        assert!(!score.violated_prohibitions.is_empty());
    }

    #[test]
    fn glob_matching_works() {
        assert!(glob_match("*.rs", "foo.rs"));
        assert!(!glob_match("*.rs", "foo.ts"));
        assert!(glob_match("src/*.rs", "src/foo.rs"));
        assert!(!glob_match("src/*.rs", "lib/foo.rs"));
    }

    #[test]
    fn empty_relevance_set_scores_perfectly() {
        let agent = make_agent(vec!["something.rs"]);
        let rel = make_relevance(vec![], vec![]);
        let changed = agent.touched_files.clone();
        let score = score_task_outcome(&agent, &changed, &rel, Path::new("/tmp"), "do something");
        assert!((score.artifact_recall - 1.0).abs() < f64::EPSILON);
        assert!(score.violated_prohibitions.is_empty());
    }

    #[test]
    fn unsupported_claims_detects_unprompted_paths() {
        let agent = AgentResult {
            response_text: "Modified: src/unknown_file.rs\nChanged: src/another.rs".into(),
            touched_files: vec![],
            diff: None,
            input_tokens: 100,
            output_tokens: 50,
            elapsed_ms: 1000,
            stop_reason: Some("stop".into()),
            diagnostics: vec![],
        };
        let rel = make_relevance(vec![], vec![]);
        let score = score_task_outcome(
            &agent,
            &[],
            &rel,
            Path::new("/tmp"),
            "analyze the epic file",
        );
        // Both paths are in the response but not in the prompt
        assert!(score.unsupported_claims > 0);
    }

    #[test]
    fn unsupported_claims_ignores_paths_in_prompt() {
        let agent = AgentResult {
            response_text: "Modified: src/foo.rs".into(),
            touched_files: vec![],
            diff: None,
            input_tokens: 100,
            output_tokens: 50,
            elapsed_ms: 1000,
            stop_reason: Some("stop".into()),
            diagnostics: vec![],
        };
        let rel = make_relevance(vec![], vec![]);
        let score = score_task_outcome(
            &agent,
            &[],
            &rel,
            Path::new("/tmp"),
            "look at src/foo.rs and tell me about it",
        );
        // Path is in the prompt, so not unsupported
        assert_eq!(score.unsupported_claims, 0);
    }

    #[test]
    fn unsupported_claims_detects_unprompted_code_idents() {
        let agent = AgentResult {
            response_text: "You should refactor `process_data_pipeline` to use a different approach.".into(),
            touched_files: vec![],
            diff: None,
            input_tokens: 100,
            output_tokens: 50,
            elapsed_ms: 1000,
            stop_reason: Some("stop".into()),
            diagnostics: vec![],
        };
        let rel = make_relevance(vec![], vec![]);
        let score = score_task_outcome(
            &agent,
            &[],
            &rel,
            Path::new("/tmp"),
            "analyze the epic",
        );
        assert!(score.unsupported_claims > 0);
    }

    #[test]
    fn verification_captures_diagnostics() {
        let score = score_task_outcome(
            &make_agent(vec![]),
            &[],
            &PracticalRelevanceSet {
                expected_changes: vec![],
                prohibited_changes: vec![],
                verification_commands: vec!["echo hello-world".into()],
                evidence_commits: vec![],
                evidence_source: "test".into(),
            },
            Path::new("/tmp"),
            "test prompt",
        );
        assert_eq!(score.verification_passed, Some(true));
        assert!(score.verification_diagnostics.iter().any(|d| d.contains("hello-world")));
    }

    #[test]
    fn verification_handles_nonexistent_command() {
        let score = score_task_outcome(
            &make_agent(vec![]),
            &[],
            &PracticalRelevanceSet {
                expected_changes: vec![],
                prohibited_changes: vec![],
                verification_commands: vec!["nonexistent_command_xyz_12345".into()],
                evidence_commits: vec![],
                evidence_source: "test".into(),
            },
            Path::new("/tmp"),
            "test prompt",
        );
        assert_eq!(score.verification_passed, Some(false));
        assert!(!score.verification_diagnostics.is_empty());
    }

    // ── Ground-truth vs self-report ────────────────────────────────────────

    #[test]
    fn scoring_uses_ground_truth_not_agent_self_report_for_recall_precision() {
        // The model claims (touched_files self-report) it edited an
        // unrelated file and says nothing about the file that actually
        // changed. Ground truth (changed_files, as if read from
        // EvalWorktree::changed_files()) shows the opposite: only the
        // expected file was actually touched. Scoring must follow ground
        // truth, not the self-report.
        let agent = AgentResult {
            response_text: "done".into(),
            touched_files: vec!["src/decoy.rs".into()],
            diff: None,
            input_tokens: 100,
            output_tokens: 50,
            elapsed_ms: 1000,
            stop_reason: Some("stop".into()),
            diagnostics: vec![],
        };
        let changed_files = vec!["src/foo.rs".to_string()];
        let rel = make_relevance(vec![("src/foo.rs", true)], vec![]);

        let score = score_task_outcome(&agent, &changed_files, &rel, Path::new("/tmp"), "fix foo");

        // Recall is full: the real diff touched the expected file, even
        // though the agent's self-report never mentioned it.
        assert!((score.artifact_recall - 1.0).abs() < f64::EPSILON);
        // Precision is full too: the real diff touched only the expected
        // file. If precision were (wrongly) computed from touched_files, it
        // would be 0.0 because "src/decoy.rs" is not in the expected set.
        assert!((score.artifact_precision - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scoring_uses_ground_truth_not_agent_self_report_for_prohibited_changes() {
        // The model's self-report claims it only touched the allowed file,
        // but the real diff shows it also touched a prohibited path.
        // Scoring must catch the real violation, not trust the self-report.
        let agent = AgentResult {
            response_text: "done".into(),
            touched_files: vec!["src/foo.rs".into()],
            diff: None,
            input_tokens: 100,
            output_tokens: 50,
            elapsed_ms: 1000,
            stop_reason: Some("stop".into()),
            diagnostics: vec![],
        };
        let changed_files = vec!["src/foo.rs".to_string(), "gui/other.rs".to_string()];
        let rel = make_relevance(vec![("src/foo.rs", true)], vec![("gui/", false)]);

        let score = score_task_outcome(&agent, &changed_files, &rel, Path::new("/tmp"), "fix foo");

        assert!(!score.violated_prohibitions.is_empty());
        assert!(!score.negative_constraints_respected);
    }

    #[test]
    fn unsupported_claims_flags_self_reported_file_not_in_real_diff() {
        // The response text doesn't mention any paths, but the agent's
        // structured touched_files self-report claims a file that the real
        // diff (changed_files) does not contain. This must be flagged.
        let agent = AgentResult {
            response_text: "I made the requested change.".into(),
            touched_files: vec!["src/phantom.rs".into()],
            diff: None,
            input_tokens: 100,
            output_tokens: 50,
            elapsed_ms: 1000,
            stop_reason: Some("stop".into()),
            diagnostics: vec![],
        };
        let rel = make_relevance(vec![], vec![]);
        let score = score_task_outcome(&agent, &[], &rel, Path::new("/tmp"), "do something");
        assert!(score.unsupported_claims > 0);
    }
}
