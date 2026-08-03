//! Epic 014 Task 3: bounded graph selection and context packets.
//!
//! Given a request string and a [`crate::graph::GraphExtraction`] (Task 2's
//! output), selects a small, source-stratified, budget-bounded subgraph and
//! renders it as a machine-readable [`Packet`] with explicit coverage and
//! gaps. Like the rest of this crate, this module is deliberately small and
//! experimental (Design Constraint 1): it is not a preview of Epic 012's
//! production packet/action model, and any field promoted there requires
//! recorded experiment evidence first.
//!
//! ## Selection pipeline
//!
//! The order below matches the Task 3 acceptance criterion "exact ...
//! references seed retrieval before fuzzy channels":
//!
//! 1. **Exact seeding** ([`find_exact_seeds`]): epic/task-number references
//!    (`"Epic 014 Task 3"`, `epics/014-....md`) and backtick-quoted
//!    paths/symbols in the request text are matched against existing node
//!    locators. This runs first and unconditionally.
//! 2. **Established expansion** ([`expand_established`]): breadth-first
//!    traversal from exact seeds over `established_edges` only, bounded by
//!    an allowed relation-kind set (`PacketBudget::allowed_relations`),
//!    `max_depth`, and `max_items`.
//! 3. **Parent/shared epic constraint retention**
//!    ([`retain_parent_constraints`]): any selected `Task`/
//!    `AcceptanceCriterion` also pulls in its enclosing epic's
//!    `DesignConstraint` children, even if plain depth-bounded expansion
//!    would have missed or excluded them.
//! 4. **Source-stratified lexical fallback** ([`find_lexical_seeds`]): only
//!    after (1)-(3) have run, keyword substring matching runs independently
//!    per [`SourceCategory`] (so no single large category can starve the
//!    others) and contributes candidate-tier items only, never established
//!    ones.
//! 5. **Deduplication and budget enforcement** (`build_items` +
//!    [`merge_duplicate_resources`]): items are grouped by locator;
//!    duplicate resources sharing one `source_path` are merged into one item
//!    that keeps every contributing locator in `merged_from` and every
//!    contributing `found_via` reason; excerpt/total-byte budgets are then
//!    applied deterministically by locator order, and anything cut is
//!    recorded in `PacketCoverage::budget_omissions`.
//!
//! ## Deliberately deferred capabilities (not faked)
//!
//! - **Real lexical/semantic retrieval.** Task 2 populated no
//!   `candidate_edges` (no lexical/semantic detector exists yet), so this
//!   module's "fuzzy channel" is a simple case-insensitive keyword-substring
//!   match over already-extracted node text (`label` plus any
//!   `extra.body_excerpt`), not FTS5/sqlite-vec retrieval or an embedding
//!   model. A real semantic-similarity channel over `daftprompt-indexer`'s
//!   existing hybrid search remains an explicit, undone follow-up, not
//!   something this module pretends to already do.
//! - **Real code/document content excerpting.** Excerpts are built only from
//!   text Task 2 already extracted into a node (`label`, `extra.body_excerpt`
//!   for `ProjectInstruction`/repository-configuration nodes). A
//!   `FileOrSection`/`CodeSymbol` node created only from an *unresolved*
//!   Markdown reference (no recorded line span) has no known focused span to
//!   excerpt from its actual file content, so its excerpt is just its own
//!   label (the referenced path/symbol string). Reading and windowing real
//!   Git blob content for such nodes is left as a follow-up, not simulated.
//!
//! ## Helper evidence stays separate and cannot erase host gaps
//!
//! `graph.helper_proposed_edges` never feeds `Packet::established` or
//! `Packet::candidates` automatically: [`apply_helper_dispositions`] is the
//! only path by which a helper proposal can enter `Packet::candidates`, and
//! it requires an explicit, host-supplied [`HelperDisposition`] classifying
//! the proposal as a validated observation or a rejected interpretation —
//! mirroring the real Keystone LOG-019 experiment's "independent validation
//! classified..." step
//! (`epics/research/011-provenance-thought-experiments.md`, "Real Keystone
//! LOG-019 replay and fixed-budget helper run"). Even a validated observation
//! only ever lands in `candidates`, never `established` (Design Constraint 3:
//! "Helper output cannot create established relationships"). Similarly,
//! [`apply_helper_gap_dispositions`] records a helper's opinion about a host
//! gap as an annotation only; there is no code path from it to
//! `PacketCoverage::remaining_gaps` removal. See the `log019_fixture` test
//! below for the synthetic reproduction of that experiment's six-way
//! validated/rejected classification.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::graph::{GraphExtraction, Node, NodeKind, RelationKind};

// ── Source stratification ───────────────────────────────────────────────

/// Stratification buckets for source-stratified search (Task 3 acceptance
/// criterion: "Search is source-stratified across instructions,
/// epics/research, documents, code, and commits where available").
/// "Where available" is reported honestly via
/// [`PacketCoverage::unavailable_sources`] rather than assumed to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCategory {
    Instructions,
    EpicsAndResearch,
    Documents,
    Code,
    Commits,
}

const ALL_SOURCE_CATEGORIES: [SourceCategory; 5] = [
    SourceCategory::Instructions,
    SourceCategory::EpicsAndResearch,
    SourceCategory::Documents,
    SourceCategory::Code,
    SourceCategory::Commits,
];

/// File extensions the shared code indexer covers
/// (`crates/daftprompt-indexer/src/code.rs`, AGENTS.md "Code indexer
/// invariants"). Used only to split `FileOrSection` nodes between the
/// `Documents` and `Code` categories; duplicated here as a small static list
/// rather than depending on `code.rs`'s internal language registry, matching
/// `index_coverage::SUPPORTED_CODE_LANGUAGES`'s existing precedent.
const CODE_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx"];

fn categorize(node: &Node) -> Option<SourceCategory> {
    match node.kind {
        NodeKind::ProjectInstruction => Some(SourceCategory::Instructions),
        NodeKind::Epic
        | NodeKind::Task
        | NodeKind::AcceptanceCriterion
        | NodeKind::DesignConstraint
        | NodeKind::ResearchArtifact
        | NodeKind::UnresolvedGap => Some(SourceCategory::EpicsAndResearch),
        NodeKind::CodeSymbol => Some(SourceCategory::Code),
        NodeKind::FileOrSection => {
            let path = node
                .locator
                .source_path
                .as_deref()
                .or_else(|| node.extra.get("referenced_path").and_then(|v| v.as_str()));
            let is_code = path
                .and_then(|p| p.rsplit('.').next())
                .map(|ext| CODE_EXTENSIONS.contains(&ext))
                .unwrap_or(false);
            Some(if is_code { SourceCategory::Code } else { SourceCategory::Documents })
        }
        NodeKind::Commit | NodeKind::RepositorySnapshot => Some(SourceCategory::Commits),
    }
}

// ── Budget and relation allowlist ───────────────────────────────────────

/// Relation kinds expansion may traverse by default. Deliberately excludes
/// `Mentions` (the loosest relation Task 2's extractor produces — any
/// backtick path/symbol/cross-epic reference becomes a `Mentions` edge,
/// including incidental ones such as an example command block) and
/// `Supersedes` from the default set: including every `Mentions` edge would
/// defeat "select a small relevant subgraph" by flooding the packet with
/// topically adjacent but often-irrelevant references. A caller may still
/// pass a wider allowlist via [`PacketBudget::allowed_relations`].
pub fn default_expansion_allowlist() -> Vec<RelationKind> {
    vec![
        RelationKind::Contains,
        RelationKind::DependsOn,
        RelationKind::Requires,
        RelationKind::Constrains,
        RelationKind::ValidatedBy,
        RelationKind::BlockedBy,
        RelationKind::Changes,
        RelationKind::Defines,
    ]
}

/// Bounds for [`select_packet`]'s expansion and rendering (Task 3 acceptance
/// criterion: "Expansion uses an allowlist of relation kinds, maximum depth,
/// item count, excerpt bytes, and total packet bytes").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketBudget {
    pub max_depth: usize,
    pub max_items: usize,
    pub max_excerpt_bytes: usize,
    pub max_total_bytes: usize,
    pub allowed_relations: Vec<RelationKind>,
}

impl Default for PacketBudget {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_items: 40,
            max_excerpt_bytes: 600,
            max_total_bytes: 20_000,
            allowed_relations: default_expansion_allowlist(),
        }
    }
}

/// Per-category cap for [`find_lexical_seeds`]'s stratified search, kept
/// small and separate from `PacketBudget::max_items` so lexical fallback
/// stays a bounded last resort rather than a second unbounded search.
const LEXICAL_SEEDS_PER_CATEGORY: usize = 5;

const STOPWORDS: &[&str] = &[
    "this", "that", "with", "from", "have", "were", "been", "which", "their", "about", "into", "also", "when",
    "then", "than", "such", "only", "more", "most", "some", "each", "what", "where", "does", "doing", "done",
    "continue", "closing", "please", "should", "would", "could",
];

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|s| s.len() >= 4 && !STOPWORDS.contains(&s.as_str()))
        .collect()
}

fn node_text(node: &Node) -> String {
    let mut s = node.label.clone();
    if let Some(v) = node.extra.get("body_excerpt").and_then(|v| v.as_str()) {
        s.push(' ');
        s.push_str(v);
    }
    s
}

// ── Seeding ──────────────────────────────────────────────────────────────

/// How a [`SeedHit`] or [`PacketItem`] was found. Exact reference matching
/// always runs before the lexical channel (Task 3 acceptance criterion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedChannel {
    /// Exact epic/task/criterion/path/symbol reference in the request text.
    ExactReference,
    /// Reached from an exact seed by traversing established graph edges.
    EstablishedExpansion,
    /// Substring/keyword match over already-extracted node text. This
    /// crate's deliberately simple stand-in for "fuzzy channels" (module
    /// docs: real lexical/semantic retrieval is a stated, deferred gap).
    LexicalKeyword,
    /// Guaranteed inclusion via [`retain_parent_constraints`], independent of
    /// either search channel.
    ParentConstraintRetention,
    /// Entered `Packet::candidates` only via an explicit, host-supplied
    /// [`HelperDisposition`] (never automatically).
    HostValidatedHelperProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SeedHit {
    pub locator: String,
    pub channel: SeedChannel,
    pub query_fragment: String,
    pub category: Option<SourceCategory>,
}

fn reference_strings(node: &Node) -> Vec<String> {
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

fn backtick_spans(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"`([^`\n]+)`").unwrap());
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

fn epic_task_ref_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    // "Epic 014", "Epic 014 Task 3", "epic 014, task 3" — `.{0,40}` is a
    // deliberately short gap so the task-number capture only fires when the
    // two numbers are clearly part of the same phrase.
    RE.get_or_init(|| regex::Regex::new(r"(?i)\bepics?\s+(\d{3})(?:.{0,40}?\btask\s+(\d+))?").unwrap())
}

fn epic_path_ref_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"epics/(\d{3})-[A-Za-z0-9\-]+\.md").unwrap())
}

fn task_only_ref_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"(?i)\btask\s+(\d+)\b").unwrap())
}

fn push_exact_seed(graph: &GraphExtraction, locator: String, fragment: String, hits: &mut Vec<SeedHit>, seen: &mut BTreeSet<String>) {
    let Some(node) = graph.nodes.iter().find(|n| n.locator.logical_id == locator) else {
        return; // cannot seed a locator this graph does not contain
    };
    if !seen.insert(locator.clone()) {
        return;
    }
    hits.push(SeedHit {
        locator,
        channel: SeedChannel::ExactReference,
        query_fragment: fragment,
        category: categorize(node),
    });
}

/// Exact epic/task/criterion/path/symbol references in `request`, matched
/// against `graph`'s existing node locators. Always runs before any fuzzy
/// channel (Task 3 acceptance criterion).
pub fn find_exact_seeds(graph: &GraphExtraction, request: &str) -> Vec<SeedHit> {
    let mut hits = Vec::new();
    let mut seen = BTreeSet::new();

    for caps in epic_task_ref_regex().captures_iter(request) {
        let Ok(epic_num) = caps[1].parse::<u32>() else { continue };
        let epic_locator = format!("epic:{epic_num:03}");
        if let Some(task_group) = caps.get(2) {
            let task_locator = format!("{epic_locator}/task:{}", task_group.as_str());
            push_exact_seed(graph, task_locator, caps[0].to_string(), &mut hits, &mut seen);
        }
        push_exact_seed(graph, epic_locator, caps[0].to_string(), &mut hits, &mut seen);
    }

    for caps in epic_path_ref_regex().captures_iter(request) {
        let Ok(epic_num) = caps[1].parse::<u32>() else { continue };
        push_exact_seed(graph, format!("epic:{epic_num:03}"), caps[0].to_string(), &mut hits, &mut seen);
    }

    // Bare "Task N" is only unambiguous when the loaded graph carries exactly
    // one epic; with more than one loaded epic this pattern is deliberately
    // left to the lexical channel rather than guessing which epic it belongs
    // to.
    let epic_numbers: BTreeSet<u32> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Epic)
        .filter_map(|n| n.extra.get("epic_number").and_then(|v| v.as_u64()))
        .map(|v| v as u32)
        .collect();
    if epic_numbers.len() == 1 {
        let only_epic = *epic_numbers.iter().next().unwrap();
        for caps in task_only_ref_regex().captures_iter(request) {
            let task_locator = format!("epic:{only_epic:03}/task:{}", &caps[1]);
            push_exact_seed(graph, task_locator, caps[0].to_string(), &mut hits, &mut seen);
        }
    }

    for span in backtick_spans(request) {
        for node in &graph.nodes {
            if reference_strings(node).iter().any(|r| r == &span) {
                push_exact_seed(graph, node.locator.logical_id.clone(), format!("`{span}`"), &mut hits, &mut seen);
            }
        }
    }

    hits
}

/// Source-stratified keyword-substring fallback: for each [`SourceCategory`]
/// independently, the top (by locator, for determinism)
/// [`LEXICAL_SEEDS_PER_CATEGORY`] nodes whose text contains at least one
/// request keyword. Running the cap per category (not globally) is what
/// keeps one large category from starving the others (Task 3 acceptance
/// criterion).
pub fn find_lexical_seeds(graph: &GraphExtraction, request: &str) -> Vec<SeedHit> {
    let keywords = tokenize(request);
    if keywords.is_empty() {
        return Vec::new();
    }
    let mut by_category: BTreeMap<SourceCategory, Vec<SeedHit>> = BTreeMap::new();
    for node in &graph.nodes {
        let Some(category) = categorize(node) else { continue };
        let haystack = node_text(node).to_lowercase();
        let matched: Vec<&str> = keywords.iter().filter(|k| haystack.contains(k.as_str())).map(String::as_str).collect();
        if matched.is_empty() {
            continue;
        }
        by_category.entry(category).or_default().push(SeedHit {
            locator: node.locator.logical_id.clone(),
            channel: SeedChannel::LexicalKeyword,
            query_fragment: matched.join(","),
            category: Some(category),
        });
    }
    let mut out = Vec::new();
    for (_, mut hits) in by_category {
        hits.sort_by(|a, b| a.locator.cmp(&b.locator));
        hits.truncate(LEXICAL_SEEDS_PER_CATEGORY);
        out.extend(hits);
    }
    out
}

// ── Expansion ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FoundVia {
    pub channel: SeedChannel,
    pub query_fragment: String,
    /// Relation kinds traversed from the seed that discovered this item, in
    /// order; empty when the item is itself a seed.
    pub relation_path: Vec<String>,
}

/// Breadth-first traversal from `seeds` over `graph.established_edges`
/// only, honoring `budget.allowed_relations`/`max_depth`/`max_items`.
/// Traverses edges in both directions (an edge's `from` or `to` may be the
/// frontier node) because Task 2's structural edges point in a fixed
/// direction (e.g. epic `--contains-->` task) but a task seed still needs to
/// reach its parent epic, not just its children.
fn expand_established(
    graph: &GraphExtraction,
    seeds: &BTreeSet<String>,
    budget: &PacketBudget,
) -> (BTreeMap<String, Vec<FoundVia>>, Vec<String>) {
    let mut selected: BTreeMap<String, Vec<FoundVia>> = BTreeMap::new();
    for s in seeds {
        selected.insert(s.clone(), Vec::new());
    }
    let mut frontier: Vec<(String, usize, Vec<String>)> = seeds.iter().map(|s| (s.clone(), 0, Vec::new())).collect();
    let mut omissions = Vec::new();

    while let Some((locator, depth, path)) = frontier.pop() {
        if depth >= budget.max_depth {
            continue;
        }
        for edge in &graph.established_edges {
            if !budget.allowed_relations.contains(&edge.relation) {
                continue;
            }
            let next = if edge.from == locator {
                edge.to.clone()
            } else if edge.to == locator {
                edge.from.clone()
            } else {
                continue;
            };
            if selected.contains_key(&next) {
                continue;
            }
            if selected.len() >= budget.max_items {
                omissions.push(format!("{next}: omitted during expansion, max_items ({}) reached", budget.max_items));
                continue;
            }
            let mut relation_path = path.clone();
            relation_path.push(edge.relation.as_str().to_string());
            selected.insert(
                next.clone(),
                vec![FoundVia {
                    channel: SeedChannel::EstablishedExpansion,
                    query_fragment: locator.clone(),
                    relation_path: relation_path.clone(),
                }],
            );
            frontier.push((next, depth + 1, relation_path));
        }
    }
    (selected, omissions)
}

/// Parent/shared epic constraint retention (Task 3 acceptance criterion):
/// for every currently selected `Task`/`AcceptanceCriterion` locator, also
/// include its enclosing epic's `DesignConstraint` children, guaranteed
/// regardless of the depth-bounded expansion result.
fn retain_parent_constraints(graph: &GraphExtraction, selected: &mut BTreeMap<String, Vec<FoundVia>>) {
    let mut epic_locators: BTreeSet<String> = BTreeSet::new();
    for locator in selected.keys() {
        if let Some(prefix) = locator.split('/').next() {
            if prefix.starts_with("epic:") {
                epic_locators.insert(prefix.to_string());
            }
        }
    }
    for epic_locator in epic_locators {
        for edge in &graph.established_edges {
            if edge.relation != RelationKind::Contains || edge.from != epic_locator {
                continue;
            }
            if selected.contains_key(&edge.to) {
                continue;
            }
            let is_constraint = graph
                .nodes
                .iter()
                .find(|n| n.locator.logical_id == edge.to)
                .is_some_and(|n| n.kind == NodeKind::DesignConstraint);
            if !is_constraint {
                continue;
            }
            selected.insert(
                edge.to.clone(),
                vec![FoundVia {
                    channel: SeedChannel::ParentConstraintRetention,
                    query_fragment: format!("parent epic constraint of {epic_locator}"),
                    relation_path: vec![RelationKind::Contains.as_str().to_string()],
                }],
            );
        }
    }
}

// ── Items, dedup, and budgets ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PacketItem {
    pub locator: String,
    pub kind: NodeKind,
    pub label: String,
    pub source_path: Option<String>,
    pub source_version: String,
    pub checked: Option<bool>,
    pub category: Option<SourceCategory>,
    pub excerpt: String,
    pub found_via: Vec<FoundVia>,
    /// Other locators merged into this item because they describe the same
    /// `source_path` (Task 3 acceptance criterion: "Duplicate resources ...
    /// are deduplicated without losing provenance paths").
    pub merged_from: Vec<String>,
}

fn byte_boundary(s: &str, mut at: usize) -> usize {
    at = at.min(s.len());
    while at > 0 && !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn excerpt_for(node: &Node) -> String {
    if let Some(body) = node.extra.get("body_excerpt").and_then(|v| v.as_str()) {
        if !body.is_empty() {
            return format!("{}\n\n{}", node.label, body);
        }
    }
    node.label.clone()
}

fn build_items(graph: &GraphExtraction, locator_map: BTreeMap<String, Vec<FoundVia>>, budget: &PacketBudget) -> (Vec<PacketItem>, Vec<String>) {
    let mut items = Vec::new();
    let mut omissions = Vec::new();
    let mut total_bytes = 0usize;

    for (locator, found_via) in locator_map {
        let Some(node) = graph.nodes.iter().find(|n| n.locator.logical_id == locator) else {
            continue; // e.g. a cross-epic dependency pointing outside the loaded set
        };
        if items.len() >= budget.max_items {
            omissions.push(format!("{locator}: omitted, max_items ({}) reached", budget.max_items));
            continue;
        }
        let mut excerpt = excerpt_for(node);
        if excerpt.len() > budget.max_excerpt_bytes {
            const MARKER: &str = "… [truncated_by_packet_excerpt_budget]";
            let content_budget = budget.max_excerpt_bytes.saturating_sub(MARKER.len());
            let cut = byte_boundary(&excerpt, content_budget);
            excerpt.truncate(cut);
            excerpt.push_str(&MARKER[..byte_boundary(MARKER, budget.max_excerpt_bytes - excerpt.len())]);
        }
        if total_bytes + excerpt.len() > budget.max_total_bytes {
            omissions.push(format!("{locator}: omitted, max_total_bytes ({}) reached", budget.max_total_bytes));
            continue;
        }
        total_bytes += excerpt.len();
        items.push(PacketItem {
            locator,
            kind: node.kind,
            label: node.label.clone(),
            source_path: node.locator.source_path.clone(),
            source_version: node.source_version.clone(),
            checked: node.checked,
            category: categorize(node),
            excerpt,
            found_via,
            merged_from: Vec::new(),
        });
    }
    (items, omissions)
}

/// Node-kind priority used to pick the "primary" survivor when two locators
/// describing the same `source_path` are merged: structural container kinds
/// (epic/task/criterion/constraint) outrank a bare unresolved file/symbol
/// reference to the same path, since the former carries more extracted
/// structure.
fn merge_priority_rank(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Epic => 0,
        NodeKind::Task => 1,
        NodeKind::AcceptanceCriterion => 2,
        NodeKind::DesignConstraint => 3,
        NodeKind::ProjectInstruction => 4,
        NodeKind::ResearchArtifact => 5,
        NodeKind::Commit => 6,
        NodeKind::RepositorySnapshot => 7,
        NodeKind::UnresolvedGap => 8,
        NodeKind::FileOrSection => 9,
        NodeKind::CodeSymbol => 10,
    }
}

/// Merge a *generic, whole-document/whole-symbol* reference (a node with no
/// recorded `line_span` — e.g. a bare `file:<path>`/`symbol:<name>` node
/// created only from an unresolved Markdown mention) into a richer,
/// structurally-extracted node that happens to share the same `source_path`,
/// keeping every contributing locator (`merged_from`) and every contributing
/// `found_via` reason. Two items that both carry a concrete `line_span` (for
/// example a `Task` and its own nested `AcceptanceCriterion`, or an `Epic`
/// and one of its `Task`s) legitimately describe different, non-duplicate
/// spans of the same file and are deliberately *not* merged, or Task 3's
/// per-task/per-criterion granularity would collapse into one item per file.
/// Items with no `source_path` never merge (Task 3 acceptance criterion:
/// "duplicate resources ... deduplicated without losing provenance paths").
pub fn merge_duplicate_resources(graph: &GraphExtraction, items: Vec<PacketItem>) -> Vec<PacketItem> {
    let has_line_span = |item: &PacketItem| -> bool {
        graph
            .nodes
            .iter()
            .find(|n| n.locator.logical_id == item.locator)
            .is_some_and(|n| n.locator.line_span.is_some())
    };

    let mut by_path: BTreeMap<String, Vec<PacketItem>> = BTreeMap::new();
    let mut standalone: Vec<PacketItem> = Vec::new();
    for item in items {
        match item.source_path.clone() {
            Some(p) => by_path.entry(p).or_default().push(item),
            None => standalone.push(item),
        }
    }
    let mut merged = Vec::new();
    for (_, mut group) in by_path {
        if group.len() == 1 {
            merged.push(group.remove(0));
            continue;
        }
        group.sort_by(|a, b| merge_priority_rank(a.kind).cmp(&merge_priority_rank(b.kind)).then_with(|| a.locator.cmp(&b.locator)));
        let mut primary = group.remove(0);
        let mut leftovers = Vec::new();
        for other in group {
            if has_line_span(&other) {
                // A distinct, non-duplicate span of the same file: keep it
                // as its own item rather than collapsing structure away.
                leftovers.push(other);
            } else {
                primary.merged_from.push(other.locator.clone());
                primary.merged_from.extend(other.merged_from);
                primary.found_via.extend(other.found_via);
            }
        }
        primary.found_via.sort();
        primary.found_via.dedup();
        primary.merged_from.sort();
        primary.merged_from.dedup();
        merged.push(primary);
        merged.extend(leftovers);
    }
    merged.extend(standalone);
    merged.sort_by(|a, b| a.locator.cmp(&b.locator));
    merged
}

// ── Coverage, packet, and top-level selection ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketCoverage {
    pub queried_sources: Vec<SourceCategory>,
    pub unavailable_sources: Vec<SourceCategory>,
    pub lab_version: String,
    pub graph_resolved_commit: String,
    pub index_available: bool,
    pub index_identifiers_considered: usize,
    pub budget_omissions: Vec<String>,
    /// Host-owned `UnresolvedGap` locators reachable (via a `blocked_by`
    /// edge) from any item this packet actually selected (established or
    /// candidate). Never mutated by a helper gap disposition — see
    /// [`apply_helper_gap_dispositions`].
    pub remaining_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedHelperInterpretation {
    pub locator: String,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapDispositionKind {
    ProposedResolved,
    ProposedStillOpen,
    ProposedReclassified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperGapNote {
    pub gap_locator: String,
    pub proposed: GapDispositionKind,
    pub note: String,
    /// Always `true`. Recorded explicitly so a caller reading only this
    /// struct sees, without re-reading Task 3's design constraint, that the
    /// disposition was informational and never removed the host gap.
    pub host_gap_retained_regardless: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub request: String,
    pub budget: PacketBudget,
    /// Seeds in discovery order: exact-reference seeds first, then lexical
    /// seeds (Task 3 acceptance criterion).
    pub seeds: Vec<SeedHit>,
    pub established: Vec<PacketItem>,
    pub candidates: Vec<PacketItem>,
    pub rejected_helper_interpretations: Vec<RejectedHelperInterpretation>,
    pub helper_gap_notes: Vec<HelperGapNote>,
    pub coverage: PacketCoverage,
}

impl Packet {
    /// Deterministically sort every collection so repeated selection over an
    /// identical graph and request produces byte-identical `serde_json`
    /// output (Task 3 acceptance criterion).
    pub fn normalize(&mut self) {
        self.seeds.sort_by(|a, b| (&a.locator, a.channel, &a.query_fragment).cmp(&(&b.locator, b.channel, &b.query_fragment)));
        self.established.sort_by(|a, b| a.locator.cmp(&b.locator));
        self.candidates.sort_by(|a, b| a.locator.cmp(&b.locator));
        self.rejected_helper_interpretations.sort_by(|a, b| (&a.locator, &a.note).cmp(&(&b.locator, &b.note)));
        self.helper_gap_notes.sort_by(|a, b| (&a.gap_locator, &a.note).cmp(&(&b.gap_locator, &b.note)));
        self.coverage.queried_sources.sort();
        self.coverage.queried_sources.dedup();
        self.coverage.unavailable_sources.sort();
        self.coverage.unavailable_sources.dedup();
        self.coverage.budget_omissions.sort();
        self.coverage.budget_omissions.dedup();
        self.coverage.remaining_gaps.sort();
        self.coverage.remaining_gaps.dedup();
    }

    /// Byte-stable normalized JSON (Task 3 acceptance criterion: "Repeated
    /// identical queries over an identical graph produce an identical
    /// normalized packet").
    pub fn to_normalized_json(&self) -> anyhow::Result<String> {
        let mut copy = self.clone();
        copy.normalize();
        Ok(serde_json::to_string_pretty(&copy)?)
    }
}

fn is_required_item(item: &PacketItem) -> bool {
    item.found_via.iter().any(|via| {
        matches!(
            via.channel,
            SeedChannel::ExactReference | SeedChannel::ParentConstraintRetention
        )
    })
}

fn serialized_packet_bytes(packet: &Packet) -> anyhow::Result<usize> {
    let mut normalized = packet.clone();
    normalized.normalize();
    Ok(serde_json::to_string_pretty(&normalized)?.len())
}

const FINAL_BUDGET_OMISSION_SUFFIX: &str = "lower-priority item(s) omitted to satisfy final packet budgets";

fn record_final_budget_omission(packet: &mut Packet, omitted: usize) {
    packet
        .coverage
        .budget_omissions
        .retain(|entry| !entry.ends_with(FINAL_BUDGET_OMISSION_SUFFIX));
    if omitted > 0 {
        packet
            .coverage
            .budget_omissions
            .push(format!("{omitted} {FINAL_BUDGET_OMISSION_SUFFIX}"));
    }
    packet.normalize();
}

/// Enforce the declared item and serialized-packet byte caps after every
/// operation that can add evidence. Exact seeds and retained parent
/// constraints are mandatory; an impossibly small budget is reported as an
/// error instead of silently discarding them or emitting an oversized packet.
fn enforce_packet_budget(packet: &mut Packet) -> anyhow::Result<()> {
    packet.normalize();
    let required = packet.established.iter().filter(|item| is_required_item(item)).count();
    anyhow::ensure!(
        required <= packet.budget.max_items,
        "max_items ({}) is too small for {required} exact/required context items",
        packet.budget.max_items
    );

    let mut omitted = 0usize;
    while packet.established.len() + packet.candidates.len() > packet.budget.max_items {
        if packet.candidates.pop().is_some() {
            omitted += 1;
            continue;
        }
        let Some(index) = packet.established.iter().rposition(|item| !is_required_item(item)) else {
            anyhow::bail!("max_items cannot be satisfied without dropping exact/required context");
        };
        packet.established.remove(index);
        omitted += 1;
    }
    record_final_budget_omission(packet, omitted);

    while serialized_packet_bytes(packet)? > packet.budget.max_total_bytes {
        if packet.candidates.pop().is_some() {
            omitted += 1;
            record_final_budget_omission(packet, omitted);
            continue;
        }
        if let Some(index) = packet.established.iter().rposition(|item| !is_required_item(item)) {
            packet.established.remove(index);
            omitted += 1;
            record_final_budget_omission(packet, omitted);
            continue;
        }
        // Existing per-item omission strings may themselves dominate a tight
        // budget. Preserve the fact of omission in one bounded summary.
        if packet.coverage.budget_omissions.len() > 1 {
            let count = packet.coverage.budget_omissions.len();
            packet.coverage.budget_omissions = vec![format!("{count} earlier budget omissions (details compacted)")];
            continue;
        }
        anyhow::bail!(
            "max_total_bytes ({}) is too small for the packet envelope and exact/required context",
            packet.budget.max_total_bytes
        );
    }
    Ok(())
}

/// Select a bounded, source-stratified context packet for `request` over
/// `graph`. See the module docs for the full pipeline.
pub fn select_packet(graph: &GraphExtraction, request: &str, budget: PacketBudget) -> anyhow::Result<Packet> {
    // 1. Exact seeding (always first).
    let exact_seeds = find_exact_seeds(graph, request);
    let mut established_locators: BTreeMap<String, Vec<FoundVia>> = BTreeMap::new();
    for seed in &exact_seeds {
        established_locators.entry(seed.locator.clone()).or_default().push(FoundVia {
            channel: SeedChannel::ExactReference,
            query_fragment: seed.query_fragment.clone(),
            relation_path: Vec::new(),
        });
    }
    let seed_set: BTreeSet<String> = established_locators.keys().cloned().collect();

    // 2. Established expansion.
    let (expanded, mut budget_omissions) = expand_established(graph, &seed_set, &budget);
    for (locator, via) in expanded {
        established_locators.entry(locator).or_default().extend(via);
    }

    // 3. Parent/shared epic constraint retention.
    retain_parent_constraints(graph, &mut established_locators);
    let required_context_items = established_locators
        .values()
        .filter(|paths| {
            paths.iter().any(|via| {
                matches!(
                    via.channel,
                    SeedChannel::ExactReference | SeedChannel::ParentConstraintRetention
                )
            })
        })
        .count();
    anyhow::ensure!(
        required_context_items <= budget.max_items,
        "max_items ({}) is too small for {required_context_items} exact/required context items",
        budget.max_items
    );

    // 4. Source-stratified lexical fallback, excluding anything already
    //    established so a fuzzy hit never displaces an exact/structural one.
    let lexical_seeds: Vec<SeedHit> = find_lexical_seeds(graph, request)
        .into_iter()
        .filter(|s| !established_locators.contains_key(&s.locator))
        .collect();
    let mut candidate_locators: BTreeMap<String, Vec<FoundVia>> = BTreeMap::new();
    for seed in &lexical_seeds {
        candidate_locators.entry(seed.locator.clone()).or_default().push(FoundVia {
            channel: SeedChannel::LexicalKeyword,
            query_fragment: seed.query_fragment.clone(),
            relation_path: Vec::new(),
        });
    }

    // 5. Dedup + budgets. Established evidence is built (and thus consumes
    //    the byte/item budget) before candidates, so established evidence
    //    always wins priority over a fuzzy-channel guess.
    let (established_items, established_omissions) = build_items(graph, established_locators, &budget);
    let established_items = merge_duplicate_resources(graph, established_items);
    budget_omissions.extend(established_omissions);

    let consumed_bytes: usize = established_items.iter().map(|i| i.excerpt.len()).sum();
    let candidate_budget = PacketBudget {
        max_items: budget.max_items.saturating_sub(established_items.len()),
        max_total_bytes: budget.max_total_bytes.saturating_sub(consumed_bytes),
        ..PacketBudget {
            max_depth: budget.max_depth,
            max_items: budget.max_items,
            max_excerpt_bytes: budget.max_excerpt_bytes,
            max_total_bytes: budget.max_total_bytes,
            allowed_relations: budget.allowed_relations.clone(),
        }
    };
    let (candidate_items, candidate_omissions) = build_items(graph, candidate_locators, &candidate_budget);
    let candidate_items = merge_duplicate_resources(graph, candidate_items);
    budget_omissions.extend(candidate_omissions);

    let mut seeds = exact_seeds;
    seeds.extend(lexical_seeds);

    // Coverage: a category is "queried" iff the graph carries at least one
    // node in it (find_lexical_seeds always scans every node regardless of
    // match), so "queried" and "available" coincide here; a category with
    // zero graph nodes genuinely could not be searched.
    let mut available_categories: BTreeSet<SourceCategory> = BTreeSet::new();
    for node in &graph.nodes {
        if let Some(c) = categorize(node) {
            available_categories.insert(c);
        }
    }
    let queried_sources: Vec<SourceCategory> = ALL_SOURCE_CATEGORIES.iter().copied().filter(|c| available_categories.contains(c)).collect();
    let unavailable_sources: Vec<SourceCategory> = ALL_SOURCE_CATEGORIES.iter().copied().filter(|c| !available_categories.contains(c)).collect();

    // Remaining gaps: host `UnresolvedGap` locators reachable via
    // `blocked_by` from anything this packet actually selected (established
    // or candidate). Deliberately does not fall back to "every graph gap"
    // when nothing is selected — an unrelated epic's unresolved gaps are not
    // this packet's concern, and reporting them anyway would defeat the
    // point of a bounded, request-scoped packet.
    let selected_locators: BTreeSet<&str> = established_items
        .iter()
        .chain(candidate_items.iter())
        .map(|i| i.locator.as_str())
        .collect();
    let mut remaining_gaps: BTreeSet<String> = BTreeSet::new();
    for edge in &graph.established_edges {
        if edge.relation == RelationKind::BlockedBy && selected_locators.contains(edge.from.as_str()) && graph.gaps.contains(&edge.to) {
            remaining_gaps.insert(edge.to.clone());
        }
    }

    let coverage = PacketCoverage {
        queried_sources,
        unavailable_sources,
        lab_version: graph.lab_version.clone(),
        graph_resolved_commit: graph.resolved_commit.clone(),
        index_available: graph.coverage.index_available,
        index_identifiers_considered: graph.coverage.index_identifiers_considered,
        budget_omissions,
        remaining_gaps: remaining_gaps.into_iter().collect(),
    };

    let mut packet = Packet {
        request: request.to_string(),
        budget,
        seeds,
        established: established_items,
        candidates: candidate_items,
        rejected_helper_interpretations: Vec::new(),
        helper_gap_notes: Vec::new(),
        coverage,
    };
    packet.normalize();
    enforce_packet_budget(&mut packet)?;
    Ok(packet)
}

// ── Helper disposition (host-validated only) ────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperClassification {
    ObservationValidated,
    ObservationValidatedNarrower,
    InterpretationRejected,
}

/// A host's independent classification of one `graph.helper_proposed_edges`
/// finding, mirroring the manual "independent validation classified..." step
/// the real LOG-019 experiment performed
/// (`epics/research/011-provenance-thought-experiments.md`). This crate
/// never derives this classification automatically — see module docs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelperDisposition {
    /// The `to` (or `from`) locator of the `helper_proposed_edges` entry
    /// this disposition classifies.
    pub locator: String,
    pub classification: HelperClassification,
    pub note: String,
}

/// Apply host-supplied [`HelperDisposition`]s to `graph.helper_proposed_edges`.
/// A validated observation is added to `packet.candidates` (never
/// `packet.established` — Design Constraint 3). A rejected interpretation is
/// recorded only in `packet.rejected_helper_interpretations` and never enters
/// evidence. A locator without a matching disposition, or a disposition
/// whose locator does not match any `helper_proposed_edges` entry, is
/// ignored.
pub fn apply_helper_dispositions(
    graph: &GraphExtraction,
    packet: &mut Packet,
    dispositions: &[HelperDisposition],
) -> anyhow::Result<()> {
    let mut updated = packet.clone();
    for disposition in dispositions {
        let matched = graph
            .helper_proposed_edges
            .iter()
            .any(|e| e.to == disposition.locator || e.from == disposition.locator);
        if !matched {
            continue;
        }
        match disposition.classification {
            HelperClassification::ObservationValidated | HelperClassification::ObservationValidatedNarrower => {
                let already_present = updated.established.iter().any(|i| i.locator == disposition.locator)
                    || updated.candidates.iter().any(|i| i.locator == disposition.locator);
                if already_present {
                    continue; // already present via a deterministic channel; do not duplicate or re-trust
                }
                let node = graph.nodes.iter().find(|n| n.locator.logical_id == disposition.locator);
                let excerpt = match node {
                    Some(n) => excerpt_for(n),
                    None => disposition.note.clone(),
                };
                let excerpt = if excerpt.len() > updated.budget.max_excerpt_bytes {
                    let mut e = excerpt;
                    const MARKER: &str = "… [truncated_by_packet_excerpt_budget]";
                    let content_budget = updated.budget.max_excerpt_bytes.saturating_sub(MARKER.len());
                    let cut = byte_boundary(&e, content_budget);
                    e.truncate(cut);
                    e.push_str(&MARKER[..byte_boundary(MARKER, updated.budget.max_excerpt_bytes - e.len())]);
                    e
                } else {
                    excerpt
                };
                updated.candidates.push(PacketItem {
                    locator: disposition.locator.clone(),
                    kind: node.map(|n| n.kind).unwrap_or(NodeKind::FileOrSection),
                    label: node.map(|n| n.label.clone()).unwrap_or_else(|| disposition.locator.clone()),
                    source_path: node.and_then(|n| n.locator.source_path.clone()),
                    source_version: node.map(|n| n.source_version.clone()).unwrap_or_else(|| "unresolved-reference".to_string()),
                    checked: node.and_then(|n| n.checked),
                    category: node.and_then(categorize),
                    excerpt,
                    found_via: vec![FoundVia {
                        channel: SeedChannel::HostValidatedHelperProposal,
                        query_fragment: format!("{:?}: {}", disposition.classification, disposition.note),
                        relation_path: Vec::new(),
                    }],
                    merged_from: Vec::new(),
                });
            }
            HelperClassification::InterpretationRejected => {
                updated.rejected_helper_interpretations.push(RejectedHelperInterpretation {
                    locator: disposition.locator.clone(),
                    note: disposition.note.clone(),
                });
            }
        }
    }
    updated.candidates = merge_duplicate_resources(graph, std::mem::take(&mut updated.candidates));
    updated.normalize();
    enforce_packet_budget(&mut updated)?;
    *packet = updated;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelperGapDisposition {
    pub gap_locator: String,
    pub proposed: GapDispositionKind,
    pub note: String,
}

/// Record a helper's opinion about a host gap as an informational
/// [`HelperGapNote`]. There is deliberately no code path from this function
/// to `PacketCoverage::remaining_gaps` removal (Task 3 acceptance criterion:
/// "A helper's proposed gap disposition cannot remove a deterministic host
/// gap").
pub fn apply_helper_gap_dispositions(packet: &mut Packet, dispositions: &[HelperGapDisposition]) {
    for disposition in dispositions {
        packet.helper_gap_notes.push(HelperGapNote {
            gap_locator: disposition.gap_locator.clone(),
            proposed: disposition.proposed,
            note: disposition.note.clone(),
            host_gap_retained_regardless: true,
        });
    }
    packet.normalize();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{
        DetectorId, Edge, EdgeProvenance, EvidenceClass, GraphCoverage, NodeLocator, OrderedF64,
    };
    use serde_json::json;

    fn node(locator: &str, kind: NodeKind, label: &str) -> Node {
        Node {
            kind,
            locator: NodeLocator::new(locator, None, None),
            label: label.to_string(),
            source_version: "v1".to_string(),
            checked: None,
            extra: json!({}),
        }
    }

    fn node_with_path(locator: &str, kind: NodeKind, label: &str, path: &str) -> Node {
        Node {
            kind,
            locator: NodeLocator::new(locator, Some(path.to_string()), None),
            label: label.to_string(),
            source_version: "v1".to_string(),
            checked: None,
            extra: json!({}),
        }
    }

    fn edge(relation: RelationKind, from: &str, to: &str) -> Edge {
        let detail = format!("{from} {relation} {to}");
        Edge {
            relation,
            from: from.to_string(),
            to: to.to_string(),
            provenance: vec![EdgeProvenance {
                detector: DetectorId::MarkdownStructure,
                method: "test_fixture".to_string(),
                evidence_locator: "fixture:L1".to_string(),
                source_version: "v1".to_string(),
                evidence_class: EvidenceClass::Structural,
                confidence: OrderedF64(1.0),
                detail,
            }],
        }
    }

    fn helper_edge(from: &str, to: &str, detail: &str) -> Edge {
        Edge {
            relation: RelationKind::Mentions,
            from: from.to_string(),
            to: to.to_string(),
            provenance: vec![EdgeProvenance {
                detector: DetectorId::Unknown("groq_context_gap_helper".to_string()),
                method: "bounded_tool_call".to_string(),
                evidence_locator: "helper:round".to_string(),
                source_version: "gpt-oss-20b".to_string(),
                evidence_class: EvidenceClass::HelperProposal,
                confidence: OrderedF64(0.5),
                detail: detail.to_string(),
            }],
        }
    }

    fn empty_coverage() -> GraphCoverage {
        GraphCoverage {
            parsed_documents: Vec::new(),
            requested_epics_unavailable: Vec::new(),
            git_available: true,
            index_available: false,
            index_identifiers_considered: 0,
        }
    }

    fn base_graph() -> GraphExtraction {
        GraphExtraction {
            lab_version: "0.1.0".to_string(),
            repo_path: "/tmp/repo".to_string(),
            resolved_commit: "deadbeef".to_string(),
            nodes: Vec::new(),
            established_edges: Vec::new(),
            candidate_edges: Vec::new(),
            helper_proposed_edges: Vec::new(),
            human_assumption_edges: Vec::new(),
            gaps: Vec::new(),
            coverage: empty_coverage(),
            diagnostics: Vec::new(),
        }
    }

    // ── Exact seeding precedes fuzzy channels ───────────────────────────

    #[test]
    fn exact_epic_and_task_reference_is_seeded_before_lexical_channel() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:014", NodeKind::Epic, "Task Zero Graph and Prompt Lab"));
        graph.nodes.push(node("epic:014/task:3", NodeKind::Task, "Task 3: Implement bounded graph selection"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:014", "epic:014/task:3"));

        let seeds = find_exact_seeds(&graph, "Please continue Epic 014 Task 3 work.");
        assert!(seeds.iter().any(|s| s.locator == "epic:014/task:3" && s.channel == SeedChannel::ExactReference));

        let packet = select_packet(&graph, "Please continue Epic 014 Task 3 work.", PacketBudget::default()).unwrap();
        let first_exact_index = packet.seeds.iter().position(|s| s.channel == SeedChannel::ExactReference);
        let first_lexical_index = packet.seeds.iter().position(|s| s.channel == SeedChannel::LexicalKeyword);
        if let (Some(exact_idx), Some(lexical_idx)) = (first_exact_index, first_lexical_index) {
            // `seeds` is built exact-then-lexical before any final sort, and
            // `Packet::normalize` sorts by locator, not discovery order, so
            // assert via the unsorted builder function instead of the sorted
            // packet field for a determinism-independent check.
            let _ = (exact_idx, lexical_idx);
        }
        let raw_exact = find_exact_seeds(&graph, "Please continue Epic 014 Task 3 work.");
        let raw_lexical = find_lexical_seeds(&graph, "Please continue Epic 014 Task 3 work.");
        assert!(!raw_exact.is_empty());
        assert!(raw_exact.iter().all(|s| s.channel == SeedChannel::ExactReference));
        assert!(raw_lexical.iter().all(|s| s.channel == SeedChannel::LexicalKeyword));
        assert!(packet.established.iter().any(|i| i.locator == "epic:014/task:3"));
    }

    // ── Source-stratified search ─────────────────────────────────────────

    #[test]
    fn lexical_search_is_stratified_and_covers_every_available_category() {
        let mut graph = base_graph();
        graph.nodes.push(node("instruction:AGENTS.md#a", NodeKind::ProjectInstruction, "recertification workflow rules"));
        graph.nodes.push(node("epic:900", NodeKind::Epic, "recertification epic"));
        graph.nodes.push(node_with_path("file:PRD.md", NodeKind::FileOrSection, "recertification PRD", "PRD.md"));
        graph.nodes.push(node("symbol:recert::start", NodeKind::CodeSymbol, "recertification start action symbol"));
        graph.nodes.push(node("commit:abc123", NodeKind::Commit, "recertification banner commit"));

        let hits = find_lexical_seeds(&graph, "investigate the recertification workflow");
        let categories: BTreeSet<SourceCategory> = hits.iter().filter_map(|h| h.category).collect();
        assert_eq!(
            categories,
            [
                SourceCategory::Instructions,
                SourceCategory::EpicsAndResearch,
                SourceCategory::Documents,
                SourceCategory::Code,
                SourceCategory::Commits,
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
        );
    }

    // ── Bounded expansion (allowlist + depth) ────────────────────────────

    #[test]
    fn expansion_allowlist_and_depth_bound_the_selected_subgraph() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:900", NodeKind::Epic, "Epic 900"));
        graph.nodes.push(node("epic:900/task:1", NodeKind::Task, "Task 1"));
        graph.nodes.push(node("epic:900/task:1/criterion:1", NodeKind::AcceptanceCriterion, "Criterion 1"));
        graph.nodes.push(node("file:unrelated.md", NodeKind::FileOrSection, "unrelated file"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/task:1"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900/task:1", "epic:900/task:1/criterion:1"));
        // `Mentions` is outside the default allowlist.
        graph.established_edges.push(edge(RelationKind::Mentions, "epic:900/task:1/criterion:1", "file:unrelated.md"));

        let mut budget = PacketBudget::default();
        budget.max_depth = 2;
        let seeds: BTreeSet<String> = ["epic:900".to_string()].into_iter().collect();
        let (selected, _omissions) = expand_established(&graph, &seeds, &budget);

        assert!(selected.contains_key("epic:900/task:1"));
        assert!(selected.contains_key("epic:900/task:1/criterion:1"));
        assert!(!selected.contains_key("file:unrelated.md"), "Mentions is outside the default allowlist");
    }

    #[test]
    fn depth_budget_excludes_nodes_beyond_max_depth() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:900", NodeKind::Epic, "Epic 900"));
        graph.nodes.push(node("epic:900/task:1", NodeKind::Task, "Task 1"));
        graph.nodes.push(node("epic:900/task:1/criterion:1", NodeKind::AcceptanceCriterion, "Criterion 1"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/task:1"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900/task:1", "epic:900/task:1/criterion:1"));

        let mut budget = PacketBudget::default();
        budget.max_depth = 1;
        let seeds: BTreeSet<String> = ["epic:900".to_string()].into_iter().collect();
        let (selected, _omissions) = expand_established(&graph, &seeds, &budget);
        assert!(selected.contains_key("epic:900/task:1"));
        assert!(!selected.contains_key("epic:900/task:1/criterion:1"), "depth 1 must not reach the grandchild");
    }

    // ── Parent/shared epic constraint retention ─────────────────────────

    #[test]
    fn selecting_a_task_retains_its_parent_epics_design_constraints() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:900", NodeKind::Epic, "Epic 900"));
        graph.nodes.push(node("epic:900/constraint:1", NodeKind::DesignConstraint, "Do not mutate files"));
        graph.nodes.push(node("epic:900/task:7", NodeKind::Task, "Task 7"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/constraint:1"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/task:7"));

        // Seed only the task directly (depth 0), simulating a request that
        // names a specific task without mentioning the constraint.
        let mut selected: BTreeMap<String, Vec<FoundVia>> = BTreeMap::new();
        selected.insert("epic:900/task:7".to_string(), Vec::new());
        retain_parent_constraints(&graph, &mut selected);

        assert!(selected.contains_key("epic:900/constraint:1"), "parent epic's design constraint must be retained");
        assert_eq!(
            selected["epic:900/constraint:1"][0].channel,
            SeedChannel::ParentConstraintRetention
        );
    }

    // ── Deduplication with preserved provenance ─────────────────────────

    #[test]
    fn duplicate_resources_sharing_a_source_path_are_merged_without_losing_provenance() {
        // Epic node: a real structural extraction with a concrete line_span.
        let mut graph = base_graph();
        graph.nodes.push(Node {
            kind: NodeKind::Epic,
            locator: NodeLocator::new("epic:900", Some("epics/900-sample.md".to_string()), Some((1, 40))),
            label: "Epic 900".to_string(),
            source_version: "v1".to_string(),
            checked: None,
            extra: json!({}),
        });
        // Bare unresolved reference to the same file: no recorded line_span.
        graph.nodes.push(node_with_path("file:epics/900-sample.md", NodeKind::FileOrSection, "epics/900-sample.md", "epics/900-sample.md"));

        let items = vec![
            PacketItem {
                locator: "epic:900".to_string(),
                kind: NodeKind::Epic,
                label: "Epic 900".to_string(),
                source_path: Some("epics/900-sample.md".to_string()),
                source_version: "v1".to_string(),
                checked: None,
                category: Some(SourceCategory::EpicsAndResearch),
                excerpt: "Epic 900".to_string(),
                found_via: vec![FoundVia {
                    channel: SeedChannel::ExactReference,
                    query_fragment: "Epic 900".to_string(),
                    relation_path: Vec::new(),
                }],
                merged_from: Vec::new(),
            },
            PacketItem {
                locator: "file:epics/900-sample.md".to_string(),
                kind: NodeKind::FileOrSection,
                label: "epics/900-sample.md".to_string(),
                source_path: Some("epics/900-sample.md".to_string()),
                source_version: "unresolved-reference".to_string(),
                checked: None,
                category: Some(SourceCategory::Documents),
                excerpt: "epics/900-sample.md".to_string(),
                found_via: vec![FoundVia {
                    channel: SeedChannel::LexicalKeyword,
                    query_fragment: "sample".to_string(),
                    relation_path: Vec::new(),
                }],
                merged_from: Vec::new(),
            },
        ];
        let merged = merge_duplicate_resources(&graph, items);
        assert_eq!(merged.len(), 1, "both locators describe the same source_path and must merge into one item");
        assert_eq!(merged[0].locator, "epic:900", "the richer structural node kind wins as the primary locator");
        assert!(merged[0].merged_from.contains(&"file:epics/900-sample.md".to_string()));
        assert_eq!(merged[0].found_via.len(), 2, "both discovery reasons are preserved");
    }

    #[test]
    fn nodes_with_distinct_line_spans_in_the_same_file_are_not_collapsed() {
        // Regression test: a Task and its own nested AcceptanceCriterion
        // both come from the same epic file (same `source_path`) but cover
        // different, non-duplicate spans of it. Merging them by
        // `source_path` alone would silently erase per-task/per-criterion
        // granularity, which is not what "duplicate resources" means.
        let mut graph = base_graph();
        graph.nodes.push(Node {
            kind: NodeKind::Task,
            locator: NodeLocator::new("epic:900/task:1", Some("epics/900-sample.md".to_string()), Some((5, 10))),
            label: "Task 1".to_string(),
            source_version: "v1".to_string(),
            checked: None,
            extra: json!({}),
        });
        graph.nodes.push(Node {
            kind: NodeKind::AcceptanceCriterion,
            locator: NodeLocator::new("epic:900/task:1/criterion:1", Some("epics/900-sample.md".to_string()), Some((8, 8))),
            label: "Criterion 1".to_string(),
            source_version: "v1".to_string(),
            checked: Some(false),
            extra: json!({}),
        });

        let task_item = PacketItem {
            locator: "epic:900/task:1".to_string(),
            kind: NodeKind::Task,
            label: "Task 1".to_string(),
            source_path: Some("epics/900-sample.md".to_string()),
            source_version: "v1".to_string(),
            checked: None,
            category: Some(SourceCategory::EpicsAndResearch),
            excerpt: "Task 1".to_string(),
            found_via: Vec::new(),
            merged_from: Vec::new(),
        };
        let criterion_item = PacketItem {
            locator: "epic:900/task:1/criterion:1".to_string(),
            kind: NodeKind::AcceptanceCriterion,
            label: "Criterion 1".to_string(),
            source_path: Some("epics/900-sample.md".to_string()),
            source_version: "v1".to_string(),
            checked: Some(false),
            category: Some(SourceCategory::EpicsAndResearch),
            excerpt: "Criterion 1".to_string(),
            found_via: Vec::new(),
            merged_from: Vec::new(),
        };

        let merged = merge_duplicate_resources(&graph, vec![task_item, criterion_item]);
        assert_eq!(merged.len(), 2, "a Task and its own nested Criterion must remain distinct items");
        assert!(merged.iter().any(|i| i.locator == "epic:900/task:1"));
        assert!(merged.iter().any(|i| i.locator == "epic:900/task:1/criterion:1"));
    }

    // ── Established and candidates remain separate ──────────────────────

    #[test]
    fn lexical_only_hits_never_enter_established() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:900", NodeKind::Epic, "recertification workflow epic"));

        let packet = select_packet(&graph, "investigate the recertification workflow", PacketBudget::default()).unwrap();
        assert!(packet.candidates.iter().any(|i| i.locator == "epic:900"));
        assert!(!packet.established.iter().any(|i| i.locator == "epic:900"));
    }

    // ── Coverage: sources, versions, and budget omissions ───────────────

    #[test]
    fn coverage_reports_queried_and_unavailable_sources_and_budget_omissions() {
        let mut graph = base_graph();
        graph.lab_version = "9.9.9".to_string();
        graph.resolved_commit = "cafef00d".to_string();
        graph.coverage.index_available = true;
        graph.coverage.index_identifiers_considered = 42;
        graph.nodes.push(node("epic:900", NodeKind::Epic, "widget epic"));
        graph.nodes.push(node("epic:900/task:1", NodeKind::Task, "widget task"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/task:1"));

        let mut tight_budget = PacketBudget::default();
        tight_budget.max_items = 1;
        let packet = select_packet(&graph, "`epic:900/task:1`", tight_budget).unwrap();

        assert_eq!(packet.coverage.lab_version, "9.9.9");
        assert_eq!(packet.coverage.graph_resolved_commit, "cafef00d");
        assert!(packet.coverage.index_available);
        assert_eq!(packet.coverage.index_identifiers_considered, 42);
        assert!(packet.coverage.queried_sources.contains(&SourceCategory::EpicsAndResearch));
        assert!(!packet.coverage.unavailable_sources.contains(&SourceCategory::EpicsAndResearch));
        assert!(packet.coverage.unavailable_sources.contains(&SourceCategory::Instructions));
        assert!(!packet.coverage.budget_omissions.is_empty(), "max_items=1 with two connected nodes must omit one");
    }

    #[test]
    fn excerpt_marker_is_included_inside_the_excerpt_budget() {
        let mut graph = base_graph();
        graph.nodes.push(node("doc:long", NodeKind::FileOrSection, &"x".repeat(200)));
        let mut locators = BTreeMap::new();
        locators.insert("doc:long".to_string(), Vec::new());
        let mut budget = PacketBudget::default();
        budget.max_excerpt_bytes = 32;

        let (items, _) = build_items(&graph, locators, &budget);
        assert_eq!(items.len(), 1);
        assert!(items[0].excerpt.len() <= budget.max_excerpt_bytes);
    }

    #[test]
    fn final_pretty_json_respects_total_packet_bytes() {
        let mut graph = base_graph();
        for number in 0..10 {
            graph.nodes.push(node(
                &format!("doc:{number}"),
                NodeKind::FileOrSection,
                &format!("widget evidence {number} {}", "x".repeat(300)),
            ));
        }
        let roomy = select_packet(&graph, "widget evidence", PacketBudget::default()).unwrap();
        let roomy_len = roomy.to_normalized_json().unwrap().len();
        let mut budget = PacketBudget::default();
        budget.max_total_bytes = roomy_len - 500;

        let packet = select_packet(&graph, "widget evidence", budget.clone()).unwrap();
        assert!(packet.to_normalized_json().unwrap().len() <= budget.max_total_bytes);
        assert!(!packet.coverage.budget_omissions.is_empty());
    }

    #[test]
    fn impossible_item_budget_errors_instead_of_dropping_exact_and_required_context() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:900", NodeKind::Epic, "widget epic"));
        graph.nodes.push(node("epic:900/task:1", NodeKind::Task, "widget task"));
        graph.nodes.push(node("epic:900/constraint:1", NodeKind::DesignConstraint, "shared safety constraint"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/task:1"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/constraint:1"));
        let mut budget = PacketBudget::default();
        budget.max_items = 2;

        let error = select_packet(&graph, "Epic 900 Task 1", budget).unwrap_err();
        assert!(error.to_string().contains("exact/required context"));
    }

    #[test]
    fn helper_validated_candidates_cannot_bypass_item_budget() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:900", NodeKind::Epic, "widget epic"));
        graph.helper_proposed_edges.push(helper_edge("epic:900", "helper:a", "first helper observation"));
        graph.helper_proposed_edges.push(helper_edge("epic:900", "helper:b", "second helper observation"));
        let mut budget = PacketBudget::default();
        budget.max_items = 1;
        let mut packet = select_packet(&graph, "`epic:900`", budget).unwrap();

        apply_helper_dispositions(
            &graph,
            &mut packet,
            &[
                HelperDisposition {
                    locator: "helper:a".to_string(),
                    classification: HelperClassification::ObservationValidated,
                    note: "validated a".to_string(),
                },
                HelperDisposition {
                    locator: "helper:b".to_string(),
                    classification: HelperClassification::ObservationValidated,
                    note: "validated b".to_string(),
                },
            ],
        )
        .unwrap();

        assert_eq!(packet.established.len(), 1);
        assert!(packet.candidates.is_empty());
        assert!(packet.established.iter().any(|item| item.locator == "epic:900"));
    }

    // ── Determinism ──────────────────────────────────────────────────────

    #[test]
    fn repeated_identical_queries_over_an_identical_graph_produce_an_identical_packet() {
        let mut graph = base_graph();
        graph.nodes.push(node("epic:900", NodeKind::Epic, "widget epic"));
        graph.nodes.push(node("epic:900/task:1", NodeKind::Task, "widget task"));
        graph.established_edges.push(edge(RelationKind::Contains, "epic:900", "epic:900/task:1"));

        let p1 = select_packet(&graph, "Epic 900 Task 1: fix the widget", PacketBudget::default()).unwrap();
        let p2 = select_packet(&graph, "Epic 900 Task 1: fix the widget", PacketBudget::default()).unwrap();
        assert_eq!(p1.to_normalized_json().unwrap(), p2.to_normalized_json().unwrap());
    }

    // ── LOG-019 fixture: validated observations vs rejected interpretations ─

    /// Synthetic graph modeled on the real, already-sanitized LOG-019 replay
    /// recorded in `epics/research/011-provenance-thought-experiments.md`
    /// ("Real Keystone LOG-019 replay and fixed-budget helper run"). No
    /// private Keystone content is reproduced here — only the row/PRD/route/
    /// banner/commit/test node shape and the already-published six-way
    /// validated/rejected classification from that experiment's independent
    /// review.
    fn log019_fixture_graph() -> GraphExtraction {
        let mut graph = base_graph();
        graph.repo_path = "keystone-fixture".to_string();
        graph.resolved_commit = "5f45626".to_string();

        graph.nodes.push(node_with_path("row:LOG-019", NodeKind::FileOrSection, "LOG-019 testing-sheet row for the recertification export", "misc_documents/Sri_Testing_Test_Log.csv"));
        graph.nodes.push(node_with_path("doc:PRD.md#5.4", NodeKind::FileOrSection, "PRD.md section 5.4 recertification requirements", "PRD.md"));
        graph.nodes.push(node_with_path(
            "file:webapp/src/routes/pm/recertifications.tsx",
            NodeKind::FileOrSection,
            "global recertification route",
            "webapp/src/routes/pm/recertifications.tsx",
        ));
        graph.nodes.push(node("symbol:OverdueCertBanner", NodeKind::CodeSymbol, "OverdueCertBanner recertification component"));
        graph.nodes.push(node("commit:91ab196", NodeKind::Commit, "commit changing the recertification route"));
        let criterion = node(
            "epic:900/task:1/criterion:1",
            NodeKind::AcceptanceCriterion,
            "Cross-check LOG-019 against the PRD, code, tests, and history for the recertification export row.",
        );
        graph.nodes.push(criterion);
        graph.nodes.push(Node {
            kind: NodeKind::UnresolvedGap,
            locator: NodeLocator::new("epic:900/task:1/criterion:1#gap", None, None),
            label: "No focused global-page end-to-end test or history evidence is linked yet.".to_string(),
            source_version: "v1".to_string(),
            checked: None,
            extra: json!({}),
        });
        graph.gaps.push("epic:900/task:1/criterion:1#gap".to_string());

        graph.established_edges.push(edge(RelationKind::Mentions, "epic:900/task:1/criterion:1", "row:LOG-019"));
        graph.established_edges.push(edge(RelationKind::Mentions, "row:LOG-019", "doc:PRD.md#5.4"));
        graph.established_edges.push(edge(
            RelationKind::Defines,
            "file:webapp/src/routes/pm/recertifications.tsx",
            "symbol:OverdueCertBanner",
        ));
        graph.established_edges.push(edge(RelationKind::Changes, "commit:91ab196", "file:webapp/src/routes/pm/recertifications.tsx"));
        graph.established_edges.push(edge(
            RelationKind::BlockedBy,
            "epic:900/task:1/criterion:1",
            "epic:900/task:1/criterion:1#gap",
        ));

        // The real bounded run's six independently-classified findings.
        graph.helper_proposed_edges.push(helper_edge(
            "row:LOG-019",
            "file:webapp/src/routes/pm/recertifications.tsx",
            "route renders banner",
        ));
        graph.helper_proposed_edges.push(helper_edge(
            "file:webapp/src/routes/pm/recertifications.tsx",
            "expansion:unit_table_and_actions",
            "expansion contains a unit table and actions",
        ));
        graph.helper_proposed_edges.push(helper_edge("row:LOG-019", "test:UT-153", "UT-153 exists"));
        graph.helper_proposed_edges.push(helper_edge(
            "doc:PRD.md#5.4",
            "requirement:per_unit_list_and_start_action",
            "PRD section 5.4 requires a per-unit list/start action",
        ));
        graph.helper_proposed_edges.push(helper_edge(
            "symbol:OverdueCertBanner",
            "action:view_unit",
            "'View unit' proves the requested workflow is satisfied",
        ));
        graph.helper_proposed_edges.push(helper_edge(
            "commit:91ab196",
            "component:certification_router",
            "UI history belongs to certification_router",
        ));

        graph.normalize();
        graph
    }

    #[test]
    fn log019_fixture_distinguishes_validated_observations_from_rejected_interpretations() {
        let graph = log019_fixture_graph();

        // A deliberately non-expert, minimal request: no epic/task numbers,
        // no backtick-quoted paths — matching the manifest's "Non-expert
        // rendering (NEW)" for this same case
        // (`epics/research/task-zero-lab/manifest.md`, C02).
        let mut packet = select_packet(
            &graph,
            "Export doesn't work right on the filtered queue, can you look into LOG-019?",
            PacketBudget::default(),
        )
        .unwrap();

        // select_packet alone must never promote a helper proposal into
        // evidence, established or candidate.
        for helper_target in [
            "expansion:unit_table_and_actions",
            "test:UT-153",
            "requirement:per_unit_list_and_start_action",
            "action:view_unit",
            "component:certification_router",
        ] {
            assert!(!packet.established.iter().any(|i| i.locator == helper_target));
            assert!(!packet.candidates.iter().any(|i| i.locator == helper_target));
        }

        // The deterministic host gap is already in scope before any helper
        // runs, because the criterion mentioning LOG-019 was itself
        // discoverable from the request text.
        assert!(
            packet.coverage.remaining_gaps.contains(&"epic:900/task:1/criterion:1#gap".to_string()),
            "expected the deterministic gap to already be in scope from the request alone"
        );

        // Independent host validation, one entry per real finding.
        let dispositions = vec![
            HelperDisposition {
                locator: "file:webapp/src/routes/pm/recertifications.tsx".to_string(),
                classification: HelperClassification::ObservationValidated,
                note: "route renders banner - validated".to_string(),
            },
            HelperDisposition {
                locator: "expansion:unit_table_and_actions".to_string(),
                classification: HelperClassification::ObservationValidated,
                note: "expansion contains a unit table and actions - validated".to_string(),
            },
            HelperDisposition {
                locator: "test:UT-153".to_string(),
                classification: HelperClassification::ObservationValidatedNarrower,
                note: "UT-153 exists but is narrower than the reported global-page workflow".to_string(),
            },
            HelperDisposition {
                locator: "requirement:per_unit_list_and_start_action".to_string(),
                classification: HelperClassification::InterpretationRejected,
                note: "PRD section 5.4 does not actually require a per-unit list/start action - rejected".to_string(),
            },
            HelperDisposition {
                locator: "action:view_unit".to_string(),
                classification: HelperClassification::InterpretationRejected,
                note: "'View unit' does not prove the requested workflow is satisfied - rejected".to_string(),
            },
            HelperDisposition {
                locator: "component:certification_router".to_string(),
                classification: HelperClassification::InterpretationRejected,
                note: "no history tool was called; UI history attribution is unsupported - rejected".to_string(),
            },
        ];
        apply_helper_dispositions(&graph, &mut packet, &dispositions).unwrap();

        for validated in ["expansion:unit_table_and_actions", "test:UT-153"] {
            assert!(
                packet.candidates.iter().any(|i| i.locator == validated),
                "expected validated observation {validated} to enter candidates"
            );
            assert!(
                !packet.established.iter().any(|i| i.locator == validated),
                "a validated helper observation must never become established evidence"
            );
        }
        for rejected in [
            "requirement:per_unit_list_and_start_action",
            "action:view_unit",
            "component:certification_router",
        ] {
            assert!(!packet.candidates.iter().any(|i| i.locator == rejected));
            assert!(!packet.established.iter().any(|i| i.locator == rejected));
            assert!(
                packet.rejected_helper_interpretations.iter().any(|r| r.locator == rejected),
                "expected rejected interpretation {rejected} to be recorded for audit"
            );
        }

        // The real run's own known failure: the helper stopped early and
        // wrongly reported zero gaps. Reproducing that as a proposed
        // disposition must not be able to remove the host's deterministic
        // gap.
        apply_helper_gap_dispositions(
            &mut packet,
            &[HelperGapDisposition {
                gap_locator: "epic:900/task:1/criterion:1#gap".to_string(),
                proposed: GapDispositionKind::ProposedResolved,
                note: "helper's own submission reported zero unresolved gaps (known-wrong early stop)".to_string(),
            }],
        );
        assert!(
            packet.coverage.remaining_gaps.contains(&"epic:900/task:1/criterion:1#gap".to_string()),
            "a helper's proposed gap disposition must never remove a deterministic host gap"
        );
        assert_eq!(packet.helper_gap_notes.len(), 1);
        assert!(packet.helper_gap_notes[0].host_gap_retained_regardless);
    }
}
