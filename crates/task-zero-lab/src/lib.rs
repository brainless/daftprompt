//! Epic 014 Task 1 — experimental, read-only Task Zero Prompt Lab.
//!
//! This crate is a deliberately small research harness, not a preview of the
//! production `daftprompt-graph` / `daftprompt-planner` / helper-orchestrator
//! crates described in Epics 011-013 (Epic 014 Design Constraint 1). Its
//! types are experimental and may be renamed, merged, or removed as later
//! Task Zero Lab tasks run real experiments; see
//! `epics/014-task-zero-prompt-lab.md`.
//!
//! Task 1 implements only the read-only "run boundary": resolving a
//! requested repository revision, inventorying allowed inputs, and reporting
//! whether the currently indexed data matches that snapshot (Design
//! Constraint 2, "Immutable run identity"). It performs no mutation,
//! checkout, network call, or dependency installation, and it never treats
//! current working-tree content as if it were historic revision content.
//!
//! Modules:
//! - [`git_snapshot`]: resolves a Git revision to its commit/tree/parent
//!   identity and honest changed-file evidence (root/merge/rename/deletion/
//!   modification), plus a conservative "does the worktree represent this
//!   revision" signal. Task 2 extends it with [`git_snapshot::read_blob_at_revision`]
//!   for revision-pinned document content.
//! - [`content_identity`]: deterministic xxh3 content identity for non-Git
//!   fixtures, kept separate from filesystem mtime and observation time.
//! - [`index_coverage`]: read-only inspection of the existing
//!   `daftprompt-indexer` SQLite database for a repository, reusing its
//!   canonical schema and identifiers rather than re-deriving them. Task 2
//!   extends it with [`index_coverage::read_identifiers`] for
//!   `(source_type, identifier)` reuse.
//! - [`run`]: combines the above into one immutable run-identity record.
//! - [`graph`] (Task 2): the experimental evidence-graph node/edge/
//!   provenance/coverage types (Design Constraint 3's minimal vocabulary).
//! - [`markdown_extract`] (Task 2): pure, Git-agnostic Markdown structure
//!   extraction (headings, tasks, checkboxes, criteria, dependencies,
//!   explicit paths/symbols/commands, cross-epic references).
//! - [`graph_build`] (Task 2): orchestrates snapshot resolution and
//!   Markdown extraction into one [`graph::GraphExtraction`].
//! - [`packet`] (Task 3): bounded, source-stratified graph selection and
//!   context-packet rendering over a [`graph::GraphExtraction`], with
//!   explicit coverage/gap reporting and host-validated-only helper
//!   evidence.
//! - [`prompt`] (Task 4): deterministic, model-free Markdown handoff
//!   rendering with provenance, coverage disclosure, and a hard byte budget.
//! - [`helper`] (Task 5): optional, stateless helper refinement behind a
//!   closed operation catalog. The Task 4 baseline stays fully usable when a
//!   helper is disabled or unavailable. [`openrouter_helper`] provides the
//!   explicit credentialed hosted adapter through the local `llm-sdk`;
//!   [`llama_cpp_helper`] provides a local adapter via llama-server
//!   (OpenAI-compatible, no credentials or network required).

pub mod content_identity;
pub mod git_snapshot;
pub mod graph;
pub mod graph_build;
pub mod helper;
pub mod index_coverage;
pub mod llama_cpp_helper;
pub mod markdown_extract;
pub mod openrouter_helper;
pub mod replay;
pub mod packet;
pub mod prompt;
pub mod run;
