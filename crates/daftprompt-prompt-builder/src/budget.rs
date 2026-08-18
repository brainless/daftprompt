//! Deterministic selection budgets.
//!
//! Epic 014 Design Decision #5 requires, at minimum: "a total character or
//! estimated-token budget; a per-excerpt limit; a per-source maximum or
//! reserved quota." These are plain, `PartialEq`-comparable values (not
//! hidden inside the selection algorithm) so a recorded budget can be
//! compared against a later one to detect a policy change, as Task 7 will
//! eventually need.

use crate::retrieval::SourceKind;

/// Maximum number of included excerpts per source. A "reserved quota" per
/// Design Decision #5 -- once a source's quota is exhausted, further
/// candidates from that source are excluded (see [`crate::selection`]) even
/// if the total character budget still has room, so one very active source
/// cannot crowd out the other two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceQuota {
    pub code: usize,
    pub document: usize,
    pub git_log: usize,
}

impl SourceQuota {
    pub fn get(&self, source: SourceKind) -> usize {
        match source {
            SourceKind::Code => self.code,
            SourceKind::Document => self.document,
            SourceKind::GitLog => self.git_log,
        }
    }
}

impl Default for SourceQuota {
    fn default() -> Self {
        // Rejected alternative: one flat quota shared across all sources.
        // Code excerpts tend to be the most directly actionable for a
        // coding agent, so code gets a slightly larger reserved slice;
        // documents and git log still get guaranteed room rather than
        // being crowded out entirely by a code-heavy combined ranking.
        Self {
            code: 4,
            document: 3,
            git_log: 3,
        }
    }
}

/// The full deterministic selection budget: a total character budget, a
/// per-excerpt cap, and a per-source quota. Character counts are used
/// rather than a real tokenizer per Epic 014's "Risks and Follow-ups":
/// "Character budgets only approximate model tokens. Token-aware accounting
/// may follow after the first measurements." The field name spells out
/// that it is an estimate, not a token count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionBudget {
    /// Total estimated-token (character) budget across all included
    /// excerpts combined.
    pub total_char_budget: usize,
    /// Maximum characters kept from any single excerpt before the total
    /// budget is even considered.
    pub per_excerpt_char_limit: usize,
    pub per_source_quota: SourceQuota,
}

impl Default for SelectionBudget {
    fn default() -> Self {
        Self {
            total_char_budget: 6_000,
            per_excerpt_char_limit: 1_200,
            per_source_quota: SourceQuota::default(),
        }
    }
}
