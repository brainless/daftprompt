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
//!   revision" signal.
//! - [`content_identity`]: deterministic xxh3 content identity for non-Git
//!   fixtures, kept separate from filesystem mtime and observation time.
//! - [`index_coverage`]: read-only inspection of the existing
//!   `daftprompt-indexer` SQLite database for a repository, reusing its
//!   canonical schema and identifiers rather than re-deriving them.
//! - [`run`]: combines the above into one immutable run-identity record.

pub mod content_identity;
pub mod git_snapshot;
pub mod index_coverage;
pub mod run;
