//! Immutable snapshots of a `search_all_hybrid` retrieval run.
//!
//! Epic 014 Design Decision #5 requires deterministic ordering "based on the
//! unified combined ranking." [`AllSourceSearchResult::combined`] is already
//! that ranking (RRF over all three sources, see
//! `daftprompt-indexer::Indexer::search_all_hybrid`), so
//! [`RetrievalSnapshot::from_all_source_result`] captures each hit's position
//! in `combined` as its `rank` and does not re-derive or re-sort by score.
//! Task 2 must not change the indexer's schema or ranking algorithm -- this
//! module only projects its output into an immutable, provider-independent
//! shape that the rest of this crate (and, eventually, durable storage) can
//! use without holding a live `Indexer`/DB connection.

use daftprompt_indexer::{AllSourceSearchResult, MatchType, UnifiedSearchHit};

/// Which of the three indexer sources an excerpt came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Code,
    Document,
    GitLog,
}

impl SourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceKind::Code => "code",
            SourceKind::Document => "document",
            SourceKind::GitLog => "git_log",
        }
    }
}

/// Location metadata for an excerpt, shaped per source so code keeps its
/// file path and line range, documents keep their file path, and commits
/// keep their short hash. Epic 014 Design Decision #5 requires "file paths
/// and line ranges for code" and "identifiers and source labels for every
/// excerpt" -- this is the location half of that requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExcerptLocation {
    Code {
        file_path: String,
        line_start: usize,
        line_end: usize,
    },
    Document {
        file_path: String,
    },
    Commit {
        short_hash: String,
    },
}

/// One retrieved candidate, carrying enough metadata to be selected,
/// quota-checked, deduplicated, and formatted without going back to the
/// indexer or the database.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalCandidate {
    /// Stable identifier from the indexer (commit SHA, code symbol
    /// identifier, or document chunk identifier). Used for dedup.
    pub identifier: String,
    pub source: SourceKind,
    /// 1-based position in `AllSourceSearchResult::combined`. Lower is
    /// better; this is the deterministic ordering key.
    pub rank: usize,
    pub score: f32,
    pub match_type: MatchType,
    pub text: String,
    pub location: ExcerptLocation,
}

/// An immutable snapshot of one `search_all_hybrid` call: the query text and
/// every combined-ranked candidate it returned, in rank order.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalSnapshot {
    pub query: String,
    /// Retrieval limit-per-source that was requested, recorded for
    /// reproducibility even though selection operates on `candidates`.
    pub limit_per_source: usize,
    pub candidates: Vec<RetrievalCandidate>,
}

impl RetrievalSnapshot {
    /// Build a snapshot directly from candidates, e.g. in tests or when the
    /// caller already has a provider-independent projection.
    pub fn new(
        query: impl Into<String>,
        limit_per_source: usize,
        candidates: Vec<RetrievalCandidate>,
    ) -> Self {
        Self {
            query: query.into(),
            limit_per_source,
            candidates,
        }
    }

    /// Project an `AllSourceSearchResult` into a snapshot, preserving
    /// `combined`'s order as the `rank` field on each candidate.
    pub fn from_all_source_result(
        query: impl Into<String>,
        limit_per_source: usize,
        result: &AllSourceSearchResult,
    ) -> Self {
        let candidates = result
            .combined
            .iter()
            .enumerate()
            .map(|(idx, hit)| candidate_from_hit(idx + 1, hit))
            .collect();
        Self {
            query: query.into(),
            limit_per_source,
            candidates,
        }
    }
}

fn candidate_from_hit(rank: usize, hit: &UnifiedSearchHit) -> RetrievalCandidate {
    match hit {
        UnifiedSearchHit::GitLog(r) => RetrievalCandidate {
            identifier: r.identifier.clone(),
            source: SourceKind::GitLog,
            rank,
            score: r.score,
            match_type: r.match_type,
            text: r.text.clone(),
            location: ExcerptLocation::Commit {
                short_hash: r.short_hash.clone(),
            },
        },
        UnifiedSearchHit::Code(r) => RetrievalCandidate {
            identifier: r.identifier.clone(),
            source: SourceKind::Code,
            rank,
            score: r.score,
            match_type: r.match_type,
            text: r.text.clone(),
            location: ExcerptLocation::Code {
                file_path: r.file_path.clone(),
                line_start: r.line_start,
                line_end: r.line_end,
            },
        },
        UnifiedSearchHit::Document(r) => RetrievalCandidate {
            identifier: r.identifier.clone(),
            source: SourceKind::Document,
            rank,
            score: r.score,
            match_type: r.match_type,
            text: r.text.clone(),
            location: ExcerptLocation::Document {
                file_path: r.file_path.clone(),
            },
        },
    }
}

/// The outcome of attempting a retrieval run: either a snapshot (which may
/// have zero candidates -- a "no results" run is still a success), or a
/// retrieval error. Epic 014 Design Decision #4: "A failed retrieval run
/// records its error and produces a deterministic no-context envelope
/// rather than silently taking an unrecorded bypass" -- this enum is what
/// makes that error explicit and unavoidable for callers of
/// `format::build_enriched_prompt`, instead of e.g. an `Option` that could
/// be silently treated the same as "no results requested."
#[derive(Debug, Clone, PartialEq)]
pub enum RetrievalOutcome {
    Ok(RetrievalSnapshot),
    Error { query: String, message: String },
}
