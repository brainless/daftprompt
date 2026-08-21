//! Deterministic selection: dedup, per-source quotas, and budget-bounded
//! truncation over an ordered list of [`RetrievalCandidate`]s.
//!
//! Epic 014 Design Decision #5 lists the required selection behavior:
//! "stable-identifier deduplication; deterministic ordering based on the
//! unified combined ranking; explicit truncation markers" for excerpts that
//! survive, and (from Task 2's acceptance criteria) "Duplicate, truncated,
//! and excluded results have explicit reasons." [`select`] is a pure
//! function of its inputs -- given the same candidates and budget, in any
//! process, on any machine, it returns the same [`SelectionResult`]: no
//! wall-clock time, randomness, or hash-map iteration order affects the
//! outcome (candidates are explicitly sorted by `rank` first, and
//! duplicate/quota/budget bookkeeping uses ordered scans, not hash-set
//! iteration, to decide what gets excluded).

use std::collections::HashMap;

use crate::budget::SelectionBudget;
use crate::retrieval::{ExcerptLocation, RetrievalCandidate, SourceKind};
use daftprompt_indexer::MatchType;

/// Why an excerpt that *was* included got shortened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    /// The excerpt alone exceeded `per_excerpt_char_limit`.
    PerExcerptLimit,
    /// The excerpt fit under `per_excerpt_char_limit` but the total budget
    /// had less room remaining than that limit.
    TotalBudgetRemaining,
}

impl TruncationReason {
    /// Stable storage/formatting label for this deterministic decision.
    pub fn as_label(self) -> &'static str {
        match self {
            TruncationReason::PerExcerptLimit => "per_excerpt_limit",
            TruncationReason::TotalBudgetRemaining => "total_budget_remaining",
        }
    }
}

/// Why a candidate was excluded entirely (zero characters included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExclusionReason {
    /// Another candidate with the same stable identifier was already kept.
    /// `kept_rank` is that earlier (better-ranked) candidate's rank.
    Duplicate { kept_rank: usize },
    /// This candidate's source already used its full [`SourceQuota`] slot.
    SourceQuotaExceeded,
    /// The total character budget was already exhausted before this
    /// candidate was reached.
    TotalBudgetExhausted,
}

impl ExclusionReason {
    pub fn as_label(&self) -> String {
        match self {
            ExclusionReason::Duplicate { kept_rank } => format!("duplicate_of_rank_{kept_rank}"),
            ExclusionReason::SourceQuotaExceeded => "source_quota_exceeded".to_string(),
            ExclusionReason::TotalBudgetExhausted => "total_budget_exhausted".to_string(),
        }
    }
}

/// An excerpt that made it into the enriched prompt, possibly truncated.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedExcerpt {
    pub identifier: String,
    pub source: SourceKind,
    pub rank: usize,
    pub score: f32,
    pub match_type: MatchType,
    pub location: ExcerptLocation,
    /// The text actually included, already bounded by the budget.
    pub text: String,
    /// Character length of the original, pre-truncation text.
    pub original_len: usize,
    pub truncated: bool,
    pub truncation_reason: Option<TruncationReason>,
}

/// A candidate that was excluded entirely, with the reason recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct ExcludedCandidate {
    pub identifier: String,
    pub source: SourceKind,
    pub rank: usize,
    pub reason: ExclusionReason,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionResult {
    /// In ascending rank order (best first).
    pub included: Vec<SelectedExcerpt>,
    /// In ascending rank order.
    pub excluded: Vec<ExcludedCandidate>,
}

/// Truncate `s` to at most `max_chars` Unicode scalar values, returning the
/// truncated text and whether truncation actually happened. Truncating by
/// `char` (not byte) avoids ever splitting a multi-byte UTF-8 sequence.
fn truncate_chars(s: &str, max_chars: usize) -> (String, bool) {
    if s.chars().count() <= max_chars {
        (s.to_string(), false)
    } else {
        (s.chars().take(max_chars).collect(), true)
    }
}

/// Deterministically select, deduplicate, quota-check, and budget-truncate
/// `candidates` per `budget`.
///
/// Candidates are processed in ascending `rank` order (the unified combined
/// ranking -- Design Decision #5). For each, in order:
/// 1. If its identifier was already kept, it is excluded as a `Duplicate`.
/// 2. If its source's quota is already full, it is excluded as
///    `SourceQuotaExceeded`.
/// 3. If the total budget is already exhausted, it is excluded as
///    `TotalBudgetExhausted`.
/// 4. Otherwise it is included, truncated to whichever is smaller of the
///    per-excerpt limit and the remaining total budget.
pub fn select(candidates: &[RetrievalCandidate], budget: &SelectionBudget) -> SelectionResult {
    let mut ordered: Vec<&RetrievalCandidate> = candidates.iter().collect();
    ordered.sort_by_key(|c| c.rank);

    let mut kept_rank_by_identifier: HashMap<&str, usize> = HashMap::new();
    let mut per_source_count: HashMap<SourceKind, usize> = HashMap::new();
    let mut remaining_budget = budget.total_char_budget;

    let mut result = SelectionResult::default();

    for c in ordered {
        if let Some(&kept_rank) = kept_rank_by_identifier.get(c.identifier.as_str()) {
            result.excluded.push(ExcludedCandidate {
                identifier: c.identifier.clone(),
                source: c.source,
                rank: c.rank,
                reason: ExclusionReason::Duplicate { kept_rank },
            });
            continue;
        }

        let quota = budget.per_source_quota.get(c.source);
        let used = *per_source_count.get(&c.source).unwrap_or(&0);
        if used >= quota {
            result.excluded.push(ExcludedCandidate {
                identifier: c.identifier.clone(),
                source: c.source,
                rank: c.rank,
                reason: ExclusionReason::SourceQuotaExceeded,
            });
            continue;
        }

        if remaining_budget == 0 {
            result.excluded.push(ExcludedCandidate {
                identifier: c.identifier.clone(),
                source: c.source,
                rank: c.rank,
                reason: ExclusionReason::TotalBudgetExhausted,
            });
            continue;
        }

        let original_len = c.text.chars().count();
        let cap = budget.per_excerpt_char_limit.min(remaining_budget);
        let (text, truncated) = truncate_chars(&c.text, cap);
        let truncation_reason = if truncated {
            if remaining_budget < budget.per_excerpt_char_limit {
                Some(TruncationReason::TotalBudgetRemaining)
            } else {
                Some(TruncationReason::PerExcerptLimit)
            }
        } else {
            None
        };

        kept_rank_by_identifier.insert(c.identifier.as_str(), c.rank);
        *per_source_count.entry(c.source).or_insert(0) += 1;
        remaining_budget = remaining_budget.saturating_sub(text.chars().count());

        result.included.push(SelectedExcerpt {
            identifier: c.identifier.clone(),
            source: c.source,
            rank: c.rank,
            score: c.score,
            match_type: c.match_type,
            location: c.location.clone(),
            text,
            original_len,
            truncated,
            truncation_reason,
        });
    }

    result.included.sort_by_key(|e| e.rank);
    result.excluded.sort_by_key(|e| e.rank);
    result
}
