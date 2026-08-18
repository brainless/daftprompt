//! Provider-independent deterministic context selection and prompt
//! formatting for Epic 014 (ACP Prompt Enrichment Client), Task 2.
//!
//! This crate sits above `daftprompt-indexer` and below every ACP/UI
//! concern. It defines:
//!
//! - [`OriginalPrompt`] -- the user's request, preserved exactly
//!   (`original` module).
//! - [`RetrievalSnapshot`] / [`RetrievalOutcome`] -- an immutable projection
//!   of one `search_all_hybrid` call, or its failure (`retrieval` module).
//! - [`SelectionBudget`] -- total/per-excerpt/per-source limits
//!   (`budget` module).
//! - [`select`] -- deterministic dedup, quota, and budget-truncation
//!   (`selection` module).
//! - [`EnrichedPrompt`] / [`build_enriched_prompt`] -- the versioned,
//!   injection-safe formatter (`format` module).
//!
//! Per the epic's File-change Summary: "`daftprompt-indexer` must not depend
//! on either new crate. The prompt builder may depend on indexer result
//! types or receive a provider-independent projection." This crate takes the
//! first option for its conversion helper
//! ([`RetrievalSnapshot::from_all_source_result`]) but its own public types
//! (everything else) do not require a live `Indexer` or database connection
//! to construct, so callers that already have a provider-independent
//! projection (e.g. from durable storage, once Task 3 exists) can go
//! straight to [`RetrievalCandidate`] without touching `daftprompt-indexer`
//! again.
//!
//! This crate has, and must continue to have, no dependency on the ACP SDK,
//! akar, winit, or any adapter ("provider") crate -- see the epic's Task 2
//! acceptance criteria and Design Decision #1's adapter-neutral boundary.

pub mod budget;
pub mod format;
pub mod original;
pub mod retrieval;
pub mod selection;

pub use budget::{SelectionBudget, SourceQuota};
pub use format::{build_enriched_prompt, EnrichedPrompt, RetrievalStatus, FORMATTER_VERSION};
pub use original::OriginalPrompt;
pub use retrieval::{ExcerptLocation, RetrievalCandidate, RetrievalOutcome, RetrievalSnapshot, SourceKind};
pub use selection::{select, ExcludedCandidate, ExclusionReason, SelectedExcerpt, SelectionResult, TruncationReason};

// Re-export the one indexer type this crate's public API embeds directly,
// so downstream crates do not need a separate `daftprompt-indexer`
// dependency just to name a candidate's match type.
pub use daftprompt_indexer::MatchType;
