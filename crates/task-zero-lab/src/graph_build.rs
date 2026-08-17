//! Epic 014 Task 2: orchestrates Task 1's snapshot primitives and
//! `markdown_extract`'s pure parser into one [`graph::GraphExtraction`].
//!
//! This is the only module that touches the filesystem or Git for graph
//! purposes; everything it produces is handed to `markdown_extract`'s pure
//! functions as plain text plus an already-resolved `source_version`, so the
//! parsing logic itself stays independently testable (see
//! `markdown_extract.rs`'s own unit tests).
//!
//! Content-reading policy (Design Constraint 2): every document is read from
//! the Git object store at the resolved revision first
//! (`git_snapshot::read_blob_at_revision`), never from the working tree. A
//! document that does not exist at the resolved revision is only read from
//! the current worktree when the caller passed `--include-dirty` *and* the
//! resolved revision is `HEAD`; that fallback is always recorded as
//! `read_from_pinned_revision: false` in [`graph::ParsedDocument`] rather
//! than silently blended in.

use std::path::Path;

use serde_json::json;

use crate::graph::{
    DetectorId, Diagnostic, Edge, EdgeProvenance, EvidenceClass, GraphCoverage, GraphExtraction, Node, NodeKind, NodeLocator,
    OrderedF64, ParsedDocument, RelationKind,
};
use crate::markdown_extract::{self, EpicDoc};
use crate::{content_identity, git_snapshot, index_coverage, run};

fn matching_index_identities(
    kind: NodeKind,
    reference: &str,
    code_identifiers: &[String],
    document_identifiers: &[String],
) -> Vec<serde_json::Value> {
    let mut matches = Vec::new();
    match kind {
        NodeKind::FileOrSection => {
            let chunk_prefix = format!("{}::", reference);
            for identifier in document_identifiers {
                if identifier == reference || identifier.starts_with(&chunk_prefix) {
                    matches.push(json!({ "source_type": "document", "identifier": identifier }));
                }
            }
            // A referenced source file can own many indexed symbols. Retain
            // each full canonical identifier rather than replacing it with
            // the non-canonical bare file path.
            for identifier in code_identifiers {
                if identifier.starts_with(&chunk_prefix) {
                    matches.push(json!({ "source_type": "code", "identifier": identifier }));
                }
            }
        }
        NodeKind::CodeSymbol => {
            let suffix = format!("::{}", reference.trim_end_matches("()"));
            for identifier in code_identifiers {
                if identifier == reference || identifier.ends_with(&suffix) {
                    matches.push(json!({ "source_type": "code", "identifier": identifier }));
                }
            }
        }
        _ => {}
    }
    matches.sort_by(|a, b| {
        (a["source_type"].as_str(), a["identifier"].as_str())
            .cmp(&(b["source_type"].as_str(), b["identifier"].as_str()))
    });
    matches.dedup();
    matches
}

/// One resolved document: its text, the exact version it was read at, and
/// whether that version came from the pinned revision or a disclosed dirty
/// fallback.
struct ResolvedDocument {
    text: String,
    source_version: String,
    read_from_pinned_revision: bool,
}

fn read_document(
    repo: &Path,
    resolved_commit: &str,
    resolved_is_head: bool,
    rel_path: &str,
    include_dirty: bool,
) -> anyhow::Result<Option<ResolvedDocument>> {
    // Under the explicit `IncludeDirtyLabelled` policy (Task 1's
    // `run::DirtyInputPolicy`), and only when the resolved revision is the
    // repository's current HEAD, the caller has opted into reading
    // uncommitted worktree content for this run — including a file whose
    // *committed* blob still exists but whose on-disk content has since
    // been edited. That case (edited, not just added) is exactly what
    // `GitSnapshot::worktree_differs_from_revision` warns about, so it must
    // win over the pinned blob here rather than being silently ignored.
    // Every such read is still labelled `read_from_pinned_revision: false`
    // so coverage discloses the mixed-snapshot risk instead of hiding it.
    if include_dirty && resolved_is_head {
        let full_path = repo.join(rel_path);
        if full_path.is_file() {
            let identity = content_identity::inspect_file(&full_path)?;
            let text = std::fs::read_to_string(&full_path)?;
            return Ok(Some(ResolvedDocument {
                text,
                source_version: identity.content_hash,
                read_from_pinned_revision: false,
            }));
        }
    }
    // Default (`ExcludeDirty`) path, and the `IncludeDirtyLabelled` path
    // when the worktree simply doesn't have this file: read the pinned
    // Git blob, never current filesystem content.
    if let Some((blob_id, bytes)) = git_snapshot::read_blob_at_revision(repo, resolved_commit, rel_path)? {
        return Ok(Some(ResolvedDocument {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            source_version: blob_id,
            read_from_pinned_revision: true,
        }));
    }
    Ok(None)
}

/// Finds an `epics/{NNN}-*.md` file by listing the current worktree's
/// `epics/` directory. This is a discovery-only convenience, not a content
/// source: the returned relative path's *content* is always read from the
/// pinned revision by [`read_document`] afterward, and if that revision
/// never had a matching blob at this path, the document is honestly
/// reported unavailable rather than substituted. A known limitation: an
/// epic file renamed or removed between the resolved revision and the
/// current worktree will not be discovered by number here; closing that
/// would require a full tree listing at the resolved revision, which this
/// small Task 2 slice does not implement (Design Constraint 1).
fn discover_epic_file_path(repo: &Path, epic_number: u32) -> Option<String> {
    let dir = repo.join("epics");
    let prefix = format!("{:03}-", epic_number);
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(".md") {
            return Some(format!("epics/{}", name));
        }
    }
    None
}

fn git_evidence(resolved_commit: &str, method: &str, detail: String) -> EdgeProvenance {
    EdgeProvenance {
        detector: DetectorId::GitChangedFile,
        method: method.to_string(),
        evidence_locator: format!("commit:{}", resolved_commit),
        source_version: resolved_commit.to_string(),
        evidence_class: EvidenceClass::Structural,
        confidence: OrderedF64(1.0),
        detail,
    }
}

fn repository_configuration_node(source_path: &str, source_version: &str, text: &str) -> Node {
    Node {
        kind: NodeKind::ProjectInstruction,
        locator: NodeLocator::new(format!("instruction:{}", source_path), Some(source_path.to_string()), None),
        label: format!("Repository configuration ({})", source_path),
        source_version: source_version.to_string(),
        checked: None,
        extra: json!({
            "precedence": 2,
            "body_excerpt": text.chars().take(400).collect::<String>(),
        }),
    }
}

/// Every string a node already answers to, for "is this backtick span
/// already covered by an existing node" checks. Mirrors `packet.rs`'s
/// private `reference_strings` (locator, source path, and any recorded
/// `referenced_path`/`referenced_symbol`) so a request span that already
/// matches a real node is never given a second, synthetic one.
fn node_reference_strings(node: &Node) -> Vec<String> {
    let mut out = vec![node.locator.logical_id.clone()];
    if let Some(p) = &node.locator.source_path {
        out.push(p.clone());
    }
    if let Some(p) = node.extra.get("referenced_path").and_then(|v| v.as_str()) {
        out.push(p.to_string());
    }
    if let Some(s) = node.extra.get("referenced_symbol").and_then(|v| v.as_str()) {
        out.push(s.to_string());
    }
    out
}

fn unresolved_reference_node(kind: NodeKind, locator: String, label: String, source_path: Option<String>, extra: serde_json::Value) -> Node {
    Node {
        kind,
        locator: NodeLocator::new(locator, source_path, None),
        label,
        source_version: "unresolved-reference".to_string(),
        checked: None,
        extra,
    }
}

/// Task 2's `extract_epic` only ever turns a backtick path/symbol reference
/// into a node when the reference appears inside an already-loaded epic
/// document (see `markdown_extract::scan_references`'s whole-document
/// pass). A request naming an exact file or symbol that has no matching
/// epic doc and no index coverage — e.g. Epic 014's C07 fixture,
/// `` `email_ranking/mod.rs::contains_date` `` against an unindexed,
/// epic-less target repository — previously produced no node at all: not a
/// label-only placeholder, nothing. `find_exact_seeds` (`packet.rs`) can
/// then never seed on it, and `enrich_with_blob_content` has nothing to
/// enrich because there is no node to begin with.
///
/// This synthesizes the missing node(s) directly from the request text,
/// using the same path/symbol/command shape judgment
/// `markdown_extract::scan_references` already applies (reused via its
/// `pub(crate)` regex helpers, not reimplemented) so a bare command or
/// plain-word backtick span still does not become a spurious file/symbol
/// node. Call this after [`build_graph`] and before
/// `packet::select_packet` so the new node participates in exact seeding;
/// `packet::enrich_with_blob_content` then resolves it conservatively in the
/// pinned tree and fills it with real Git blob content.
pub fn add_request_referenced_nodes(graph: &mut GraphExtraction, request: &str) {
    for caps in markdown_extract::backtick_span_regex().captures_iter(request) {
        let span = caps[1].to_string();
        if markdown_extract::looks_like_command(&span) {
            continue; // e.g. `cargo test` — a command, not a file/symbol reference
        }
        let already_covered = graph.nodes.iter().any(|n| node_reference_strings(n).iter().any(|r| r == &span));
        if already_covered {
            continue;
        }

        if let Some(caps) = markdown_extract::path_symbol_like_regex().captures(&span) {
            // `path::symbol`, e.g. `email_ranking/mod.rs::contains_date`:
            // synthesize both a FileOrSection node for the path and a
            // CodeSymbol node for the symbol, both carrying the file path so
            // `enrich_with_blob_content` can attempt a real blob read for
            // either.
            let path = caps[1].to_string();
            let file_locator = format!("file:{path}");
            if !graph.nodes.iter().any(|n| n.locator.logical_id == file_locator) {
                graph.nodes.push(unresolved_reference_node(
                    NodeKind::FileOrSection,
                    file_locator,
                    path.clone(),
                    Some(path.clone()),
                    json!({ "referenced_path": path }),
                ));
            }
            let symbol_locator = format!("symbol:{span}");
            if !graph.nodes.iter().any(|n| n.locator.logical_id == symbol_locator) {
                graph.nodes.push(unresolved_reference_node(
                    NodeKind::CodeSymbol,
                    symbol_locator,
                    span.clone(),
                    Some(path.clone()),
                    json!({ "referenced_path": path, "referenced_symbol": span }),
                ));
            }
        } else if markdown_extract::path_like_regex().is_match(&span) {
            let locator = format!("file:{span}");
            graph.nodes.push(unresolved_reference_node(
                NodeKind::FileOrSection,
                locator,
                span.clone(),
                Some(span.clone()),
                json!({ "referenced_path": span }),
            ));
        } else if markdown_extract::symbol_like_regex().is_match(&span) {
            let locator = format!("symbol:{span}");
            graph.nodes.push(unresolved_reference_node(
                NodeKind::CodeSymbol,
                locator,
                span.clone(),
                None,
                json!({ "referenced_symbol": span }),
            ));
        }
        // Anything else (a plain word, a sentence fragment, ...) is left
        // alone, same as `scan_references` leaves it out of its own node
        // synthesis.
    }
    graph.normalize();
}

/// Build the Task 2 evidence graph for `epic_numbers` (e.g. `[11, 12, 13]`)
/// plus `AGENTS.md`, `DEVELOP.md`, root `Cargo.toml`, and revision-pinned
/// Git facts, at `rev` in `repo`.
pub fn build_graph(repo: &Path, rev: &str, epic_numbers: &[u32], include_dirty: bool) -> anyhow::Result<GraphExtraction> {
    let git = git_snapshot::resolve_snapshot(repo, rev)?;
    let index = index_coverage::inspect(repo)?;

    let mut nodes: Vec<Node> = Vec::new();
    let mut established_edges: Vec<Edge> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut parsed_documents: Vec<ParsedDocument> = Vec::new();
    let mut requested_epics_unavailable: Vec<u32> = Vec::new();

    let mut sorted_epic_numbers: Vec<u32> = epic_numbers.to_vec();
    sorted_epic_numbers.sort();
    sorted_epic_numbers.dedup();

    for &num in &sorted_epic_numbers {
        let Some(rel_path) = discover_epic_file_path(repo, num) else {
            requested_epics_unavailable.push(num);
            diagnostics.push(Diagnostic {
                message: format!("epic {:03}: no epics/{:03}-*.md file found under {}", num, num, repo.display()),
            });
            continue;
        };
        let Some(resolved) = read_document(repo, &git.resolved_commit, git.resolved_is_head, &rel_path, include_dirty)? else {
            requested_epics_unavailable.push(num);
            diagnostics.push(Diagnostic {
                message: format!(
                    "epic {:03}: '{}' has no content at resolved revision {} (and dirty-input policy did not apply)",
                    num, rel_path, git.resolved_commit
                ),
            });
            continue;
        };
        let doc = EpicDoc {
            epic_number: num,
            source_path: rel_path.clone(),
            source_version: resolved.source_version.clone(),
            text: &resolved.text,
        };
        let extraction = markdown_extract::extract_epic(&doc);
        parsed_documents.push(ParsedDocument {
            source_path: rel_path,
            source_version: resolved.source_version,
            read_from_pinned_revision: resolved.read_from_pinned_revision,
            node_count: extraction.nodes.len(),
        });
        nodes.extend(extraction.nodes);
        established_edges.extend(extraction.established_edges);
        diagnostics.extend(extraction.diagnostics.into_iter().map(|message| Diagnostic { message }));
    }

    // Project instructions: AGENTS.md (highest precedence), DEVELOP.md.
    // README.md is deliberately excluded here — Task 2's acceptance
    // criterion names only "AGENTS.md, DEVELOP.md, and repository
    // configuration," not README.md.
    for (path, precedence) in [("AGENTS.md", 0u32), ("DEVELOP.md", 1u32)] {
        match read_document(repo, &git.resolved_commit, git.resolved_is_head, path, include_dirty)? {
            Some(resolved) => {
                let instruction_nodes =
                    markdown_extract::extract_project_instructions(path, &resolved.source_version, &resolved.text, precedence);
                parsed_documents.push(ParsedDocument {
                    source_path: path.to_string(),
                    source_version: resolved.source_version,
                    read_from_pinned_revision: resolved.read_from_pinned_revision,
                    node_count: instruction_nodes.len(),
                });
                nodes.extend(instruction_nodes);
            }
            None => diagnostics.push(Diagnostic {
                message: format!("'{}' has no content at resolved revision {}", path, git.resolved_commit),
            }),
        }
    }

    // Repository configuration: root Cargo.toml, lowest of the three
    // explicit precedence tiers.
    if let Some(resolved) = read_document(repo, &git.resolved_commit, git.resolved_is_head, "Cargo.toml", include_dirty)? {
        let node = repository_configuration_node("Cargo.toml", &resolved.source_version, &resolved.text);
        parsed_documents.push(ParsedDocument {
            source_path: "Cargo.toml".to_string(),
            source_version: resolved.source_version,
            read_from_pinned_revision: resolved.read_from_pinned_revision,
            node_count: 1,
        });
        nodes.push(node);
    }

    // Repository snapshot + commit nodes, reusing Task 1's `GitSnapshot`
    // verbatim as the source of Git facts (Task 2 acceptance criterion:
    // "Git commit/file/version facts come from revision-pinned Git
    // evidence").
    let snapshot_locator = format!("snapshot:{}", git.resolved_commit);
    nodes.push(Node {
        kind: NodeKind::RepositorySnapshot,
        locator: NodeLocator::new(snapshot_locator.clone(), None, None),
        label: format!("{} @ {}", git.repo_path, git.short_commit),
        source_version: git.resolved_commit.clone(),
        checked: None,
        extra: json!({
            "requested_rev": git.requested_rev,
            "resolved_is_head": git.resolved_is_head,
            "head_is_dirty": git.head_is_dirty,
            "worktree_differs_from_revision": git.worktree_differs_from_revision,
            "dirty_input_policy": if include_dirty { "include_dirty_labelled" } else { "exclude_dirty" },
        }),
    });

    let commit_locator = format!("commit:{}", git.resolved_commit);
    nodes.push(Node {
        kind: NodeKind::Commit,
        locator: NodeLocator::new(commit_locator.clone(), None, None),
        label: format!("commit {}", git.short_commit),
        source_version: git.resolved_commit.clone(),
        checked: None,
        extra: json!({
            "parents": git.parents,
            "is_root_commit": git.is_root_commit,
            "is_merge_commit": git.is_merge_commit,
        }),
    });
    established_edges.push(Edge {
        relation: RelationKind::Contains,
        from: snapshot_locator.clone(),
        to: commit_locator.clone(),
        provenance: vec![git_evidence(
            &git.resolved_commit,
            "resolved_revision",
            format!("snapshot resolved '{}' to commit {}", git.requested_rev, git.resolved_commit),
        )],
    });

    for change in &git.changed_files {
        let file_locator = format!("file:{}", change.path);
        if !nodes.iter().any(|n| n.locator.logical_id == file_locator) {
            nodes.push(Node {
                kind: NodeKind::FileOrSection,
                locator: NodeLocator::new(file_locator.clone(), Some(change.path.clone()), None),
                label: change.path.clone(),
                source_version: change.blob_id.clone().unwrap_or_else(|| "deleted".to_string()),
                checked: None,
                extra: json!({
                    "change_kind": format!("{:?}", change.kind),
                    "previous_path": change.previous_path,
                    "referenced_path": change.path,
                }),
            });
        }
        established_edges.push(Edge {
            relation: RelationKind::Changes,
            from: commit_locator.clone(),
            to: file_locator,
            provenance: vec![git_evidence(
                &git.resolved_commit,
                "first_parent_diff",
                format!("{:?} in the first-parent diff of {}", change.kind, git.resolved_commit),
            )],
        });
    }

    // Index identity reuse: for FileOrSection/CodeSymbol nodes created from
    // explicit Markdown references, tag membership in the existing index's
    // (source_type, identifier) space without importing any indexed text
    // (Task 2 acceptance criterion).
    let mut index_identifiers_considered = 0usize;
    if index.db_exists {
        let code_identifiers = index_coverage::read_identifiers(repo, "code")?;
        let document_identifiers = index_coverage::read_identifiers(repo, "document")?;
        index_identifiers_considered = code_identifiers.len() + document_identifiers.len();
        for node in nodes.iter_mut() {
            let candidate = match node.kind {
                NodeKind::FileOrSection => node.extra.get("referenced_path").and_then(|v| v.as_str()),
                NodeKind::CodeSymbol => node.extra.get("referenced_symbol").and_then(|v| v.as_str()),
                _ => None,
            };
            let Some(candidate) = candidate.map(str::to_string) else { continue };
            let identities = matching_index_identities(
                node.kind,
                &candidate,
                &code_identifiers,
                &document_identifiers,
            );
            if !identities.is_empty() {
                if let Some(obj) = node.extra.as_object_mut() {
                    obj.insert("indexed_identities".to_string(), json!(identities));
                }
            }
        }
    }

    let gaps: Vec<String> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::UnresolvedGap)
        .map(|n| n.locator.logical_id.clone())
        .collect();

    let mut extraction = GraphExtraction {
        lab_version: run::LAB_VERSION.to_string(),
        repo_path: git.repo_path.clone(),
        resolved_commit: git.resolved_commit.clone(),
        nodes,
        established_edges,
        candidate_edges: Vec::new(),
        helper_proposed_edges: Vec::new(),
        human_assumption_edges: Vec::new(),
        gaps,
        coverage: GraphCoverage {
            parsed_documents,
            requested_epics_unavailable,
            git_available: true,
            index_available: index.db_exists,
            index_identifiers_considered,
        },
        diagnostics,
    };
    extraction.normalize();
    Ok(extraction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .args(args)
            .status()
            .expect("failed to run git");
        assert!(status.success(), "git {:?} failed", args);
    }

    fn init_fixture_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "test@test.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);

        std::fs::create_dir_all(dir.path().join("epics")).unwrap();
        std::fs::write(
            dir.path().join("epics/900-sample.md"),
            "# Epic 900: Sample\n\n## Dependency\n\nRequires Epic 901.\n\n## Tasks\n\n### Task 1: Do it\n\n#### Acceptance Criteria\n\n- [ ] Not done.\n- [x] Done, see `epics/901-other.md`.\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# AGENTS.md\n\n## Conventions\n\nFollow the rules.\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("DEVELOP.md"), "# DEVELOP.md\n\n## Build\n\ncargo check.\n").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = [\"crates/x\"]\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);
        dir
    }

    #[test]
    fn builds_graph_with_epic_instruction_git_and_gap_nodes() {
        let dir = init_fixture_repo();
        let extraction = build_graph(dir.path(), "HEAD", &[900], false).unwrap();

        assert!(extraction.nodes.iter().any(|n| n.locator.logical_id == "epic:900"));
        assert!(extraction.nodes.iter().any(|n| n.locator.logical_id == "epic:900/task:1/criterion:1"));
        assert!(extraction.gaps.contains(&"epic:900/task:1/criterion:1#gap".to_string()));
        assert!(extraction
            .established_edges
            .iter()
            .any(|e| e.relation == RelationKind::DependsOn && e.from == "epic:900" && e.to == "epic:901"));
        assert!(extraction
            .nodes
            .iter()
            .any(|n| n.kind == NodeKind::ProjectInstruction && n.label == "Conventions"));
        assert!(extraction.nodes.iter().any(|n| n.kind == NodeKind::Commit));
        assert!(extraction
            .established_edges
            .iter()
            .any(|e| e.relation == RelationKind::Changes && e.to == "file:epics/900-sample.md"));
        assert_eq!(extraction.coverage.requested_epics_unavailable, Vec::<u32>::new());
    }

    #[test]
    fn index_identity_matching_retains_canonical_code_and_document_ids() {
        let code = vec![
            "src/widget.rs::Widget::render".to_string(),
            "src/widget.rs::__comments__".to_string(),
            "src/other.rs::Widget::render".to_string(),
        ];
        let documents = vec![
            "README.md".to_string(),
            "epics/014-task-zero-prompt-lab.md::0".to_string(),
            "epics/014-task-zero-prompt-lab.md::1".to_string(),
        ];

        let file_matches = matching_index_identities(
            NodeKind::FileOrSection,
            "epics/014-task-zero-prompt-lab.md",
            &code,
            &documents,
        );
        assert_eq!(file_matches.len(), 2);
        assert_eq!(file_matches[0]["identifier"], "epics/014-task-zero-prompt-lab.md::0");

        let code_file_matches = matching_index_identities(
            NodeKind::FileOrSection,
            "src/widget.rs",
            &code,
            &documents,
        );
        assert_eq!(code_file_matches.len(), 2);
        assert!(code_file_matches.iter().all(|m| m["source_type"] == "code"));

        let symbol_matches = matching_index_identities(
            NodeKind::CodeSymbol,
            "Widget::render",
            &code,
            &documents,
        );
        assert_eq!(symbol_matches.len(), 2);
        assert!(symbol_matches
            .iter()
            .all(|m| m["identifier"].as_str().unwrap().ends_with("::Widget::render")));
    }

    #[test]
    fn unrequested_missing_epic_is_reported_as_unavailable_not_fabricated() {
        let dir = init_fixture_repo();
        let extraction = build_graph(dir.path(), "HEAD", &[900, 999], false).unwrap();
        assert_eq!(extraction.coverage.requested_epics_unavailable, vec![999]);
        assert!(!extraction.nodes.iter().any(|n| n.locator.logical_id == "epic:999"));
    }

    #[test]
    fn dirty_worktree_content_is_excluded_by_default_policy() {
        let dir = init_fixture_repo();
        // Uncommitted edit to the epic file: adds a brand-new criterion that
        // must not appear in the graph unless explicitly included.
        std::fs::write(
            dir.path().join("epics/900-sample.md"),
            "# Epic 900: Sample\n\n## Tasks\n\n### Task 1: Do it\n\n#### Acceptance Criteria\n\n- [ ] Not done.\n- [ ] Uncommitted new criterion.\n",
        )
        .unwrap();

        let excluded = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        assert!(!excluded
            .nodes
            .iter()
            .any(|n| n.label.contains("Uncommitted new criterion")));

        let included = build_graph(dir.path(), "HEAD", &[900], true).unwrap();
        assert!(included.nodes.iter().any(|n| n.label.contains("Uncommitted new criterion")));
        let doc = included
            .coverage
            .parsed_documents
            .iter()
            .find(|d| d.source_path == "epics/900-sample.md")
            .unwrap();
        assert!(!doc.read_from_pinned_revision, "dirty fallback must be labelled, not silently pinned");
    }

    #[test]
    fn repeated_build_over_identical_inputs_is_byte_stable() {
        let dir = init_fixture_repo();
        let e1 = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        let e2 = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        assert_eq!(e1.to_normalized_json().unwrap(), e2.to_normalized_json().unwrap());
    }

    /// This crate's own repository root (`crates/task-zero-lab/../..`),
    /// used by the real-fixture tests below. Unlike the manifest-commit
    /// fixtures in `git_snapshot.rs`, this fixture is always present (it is
    /// the repository the test itself is running in), so it never skips.
    fn daftprompt_repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn real_epic_014_extraction_matches_its_own_checkbox_counts() {
        let repo = daftprompt_repo_root();
        let text = std::fs::read_to_string(repo.join("epics/014-task-zero-prompt-lab.md")).unwrap();
        let unchecked = text.lines().filter(|l| l.trim_start().starts_with("- [ ]")).count();
        let checked = text
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with("- [x]") || t.starts_with("- [X]")
            })
            .count();

        let extraction = build_graph(&repo, "HEAD", &[14], false).unwrap();

        let criteria: Vec<&Node> = extraction.nodes.iter().filter(|n| n.kind == NodeKind::AcceptanceCriterion).collect();
        assert_eq!(criteria.len(), unchecked + checked, "extracted criterion count must match the file's own checkbox count");
        let extracted_unchecked = criteria.iter().filter(|n| n.checked == Some(false)).count();
        assert_eq!(extracted_unchecked, unchecked);
        assert_eq!(extraction.gaps.len(), unchecked, "one UnresolvedGap node per unchecked criterion, no more, no fewer");

        // Every gap's criterion is verbatim: no gap label may claim a cause.
        for gap_locator in &extraction.gaps {
            let gap = extraction.nodes.iter().find(|n| &n.locator.logical_id == gap_locator).unwrap();
            assert_eq!(gap.kind, NodeKind::UnresolvedGap);
        }

        assert!(extraction.coverage.requested_epics_unavailable.is_empty());
        assert!(extraction
            .coverage
            .parsed_documents
            .iter()
            .any(|d| d.source_path == "epics/014-task-zero-prompt-lab.md" && d.read_from_pinned_revision));
    }

    #[test]
    fn real_epics_011_and_012_cross_reference_each_other_and_the_dependency_graph_is_acyclic_free_of_self_loops() {
        let repo = daftprompt_repo_root();
        let extraction = build_graph(&repo, "HEAD", &[11, 12], false).unwrap();
        assert!(extraction.coverage.requested_epics_unavailable.is_empty());

        // Epic 012's '## Dependency' section explicitly requires Epic 011
        // (`epics/012-deterministic-task-planner.md`, "Requires Epic 011").
        assert!(extraction
            .established_edges
            .iter()
            .any(|e| e.relation == RelationKind::DependsOn && e.from == "epic:012" && e.to == "epic:011"));

        // No edge should ever point an epic at itself.
        for edge in &extraction.established_edges {
            if edge.relation == RelationKind::DependsOn {
                assert_ne!(edge.from, edge.to);
            }
        }
    }

    #[test]
    fn real_repo_project_instructions_carry_distinct_precedence() {
        let repo = daftprompt_repo_root();
        let extraction = build_graph(&repo, "HEAD", &[14], false).unwrap();
        let agents_precedence = extraction
            .nodes
            .iter()
            .find(|n| n.locator.source_path.as_deref() == Some("AGENTS.md"))
            .and_then(|n| n.extra.get("precedence").and_then(|v| v.as_u64()))
            .unwrap();
        let develop_precedence = extraction
            .nodes
            .iter()
            .find(|n| n.locator.source_path.as_deref() == Some("DEVELOP.md"))
            .and_then(|n| n.extra.get("precedence").and_then(|v| v.as_u64()))
            .unwrap();
        let cargo_toml_precedence = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "instruction:Cargo.toml")
            .and_then(|n| n.extra.get("precedence").and_then(|v| v.as_u64()))
            .unwrap();
        assert!(agents_precedence < develop_precedence);
        assert!(develop_precedence < cargo_toml_precedence);
    }

    #[test]
    fn every_edge_is_explainable_without_rerunning_its_detector() {
        let dir = init_fixture_repo();
        let extraction = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        for edge in extraction.established_edges.iter() {
            let explanation = edge.explain();
            assert!(!explanation.is_empty());
            assert!(!edge.provenance.is_empty(), "edge {} -> {} has no provenance", edge.from, edge.to);
        }
    }

    // ── `add_request_referenced_nodes` (C07 upstream-graph-gap fix) ────────

    /// A fixture with a real source file so the composite `path::symbol`
    /// request-reference test can drive `enrich_with_blob_content` end to
    /// end, mirroring dwata's unindexed, epic-less C07 shape: the epic
    /// requested (900) has nothing to do with the referenced file, and there
    /// is no index, so the only way the file/symbol enters the graph at all
    /// is the request-reference fallback under test.
    fn init_fixture_repo_with_source_file() -> tempfile::TempDir {
        let dir = init_fixture_repo();
        std::fs::create_dir_all(dir.path().join("workspace/src")).unwrap();
        let contents: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        std::fs::write(dir.path().join("workspace/src/email_ranking.rs"), &contents).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "add source file"]);
        // A follow-up commit that does not touch the source file, so `HEAD`'s
        // first-parent diff (`build_graph`'s `changed_files`) does not
        // itself surface `src/email_ranking.rs` as a node. This mirrors the
        // real C07 shape: a file the harness must reach via the request-text
        // fallback, not one it would have gotten for free from the commit
        // diff or an epic doc.
        std::fs::write(dir.path().join("DEVELOP.md"), "# DEVELOP.md\n\n## Build\n\ncargo check --workspace.\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "unrelated follow-up"]);
        dir
    }

    #[test]
    fn request_referenced_path_symbol_is_synthesized_and_enriched_with_real_blob_content() {
        let dir = init_fixture_repo_with_source_file();
        let mut extraction = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        assert!(
            !extraction.nodes.iter().any(|n| n.locator.logical_id == "file:src/email_ranking.rs"),
            "no node should reference the file before the fallback runs"
        );
        assert!(
            !extraction.nodes.iter().any(|n| n.locator.logical_id == "symbol:src/email_ranking.rs::contains_date"),
            "no node should reference the symbol before the fallback runs"
        );

        let request = "Fix the panic at `src/email_ranking.rs::contains_date`.";
        add_request_referenced_nodes(&mut extraction, request);

        let file_node = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "file:src/email_ranking.rs")
            .expect("path half of the composite reference should synthesize a FileOrSection node");
        assert_eq!(file_node.kind, NodeKind::FileOrSection);
        assert_eq!(file_node.source_version, "unresolved-reference");

        let symbol_node = extraction
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == "symbol:src/email_ranking.rs::contains_date")
            .expect("symbol half of the composite reference should synthesize a CodeSymbol node");
        assert_eq!(symbol_node.kind, NodeKind::CodeSymbol);
        assert_eq!(symbol_node.locator.source_path.as_deref(), Some("src/email_ranking.rs"));
        assert_eq!(symbol_node.extra["referenced_path"], "src/email_ranking.rs");

        // find_exact_seeds now matches the request's backtick span as
        // established evidence, exactly as it already does for an
        // epic-doc-derived reference.
        let seeds = crate::packet::find_exact_seeds(&extraction, request);
        assert!(
            seeds.iter().any(|s| s.locator == "symbol:src/email_ranking.rs::contains_date"),
            "the composite symbol node itself must be exact-seeded"
        );

        // enrich_with_blob_content uniquely resolves the request suffix and
        // fills the synthesized node with real Git blob content. This is the
        // end-to-end proof the upstream graph gap is closed, not just that a
        // placeholder node now exists.
        let budget = crate::packet::PacketBudget::default();
        let mut packet = crate::packet::select_packet(&extraction, request, budget).unwrap();
        crate::packet::enrich_with_blob_content(&mut packet, &extraction).unwrap();
        let enriched = packet
            .established
            .iter()
            .chain(packet.candidates.iter())
            .find(|i| i.locator == "file:src/email_ranking.rs")
            .expect("synthesized file node should have been selected into the packet");
        assert!(enriched.excerpt.contains("line 1"), "excerpt should contain real file content, got: {}", enriched.excerpt);
        assert_ne!(enriched.source_version, "unresolved-reference", "source_version should be a real blob id after enrichment");
        assert_eq!(enriched.referenced_path.as_deref(), Some("src/email_ranking.rs"));
        assert_eq!(enriched.source_path.as_deref(), Some("workspace/src/email_ranking.rs"));
        assert_eq!(enriched.path_resolution.as_deref(), Some("unique_tree_suffix"));
    }

    #[test]
    fn request_referenced_nodes_are_not_duplicated_when_no_reference_or_already_covered() {
        let dir = init_fixture_repo_with_source_file();

        // No backtick reference at all: no new nodes.
        let mut extraction = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        let before = extraction.nodes.len();
        add_request_referenced_nodes(&mut extraction, "Please just fix the crash, no specifics.");
        assert_eq!(extraction.nodes.len(), before);

        // A backtick reference that already matches an existing node
        // (epics/901-other.md is referenced by the fixture's own criterion
        // text and so already has a node) must not get a second, duplicate
        // node.
        let mut extraction = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        let before = extraction.nodes.len();
        add_request_referenced_nodes(&mut extraction, "See `epics/901-other.md` for details.");
        assert_eq!(extraction.nodes.len(), before, "an already-covered reference must not create a duplicate node");
    }

    #[test]
    fn non_path_backtick_spans_do_not_become_spurious_nodes() {
        let dir = init_fixture_repo_with_source_file();
        let mut extraction = build_graph(dir.path(), "HEAD", &[900], false).unwrap();
        let before = extraction.nodes.len();
        add_request_referenced_nodes(&mut extraction, "Run `cargo test` and also consider `refactor` carefully.");
        assert_eq!(extraction.nodes.len(), before, "a bare command or plain word backtick span must not synthesize a node");
    }
}
