//! Standalone schema-discovery sampler for Epic 013 (open-vocabulary graph
//! experiment). This crate is deliberately independent of
//! `daftprompt-indexer` and the `graph` CLI: it must not change production
//! prompts, validators, or storage. It only samples deterministic text blocks
//! from a `--source`-style file set and asks the model, in one batched call,
//! to propose a starting graph vocabulary.

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    time::Instant,
};

use anyhow::{anyhow, Context, Result};
use llm_sdk::{
    error::LlmError,
    models::xiaomi::MIMO_V2_5_PRO,
    providers::XIAOMI,
    xiaomi::{types::XiaomiChatCompletionRequest, XiaomiClient},
};
use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

pub mod consolidate;
pub use consolidate::{
    build_consolidation_payload, consolidate_vocabulary, validate_vocabulary,
    ConsolidationLog, ConsolidationProposal, ConsolidationReport, MergeDecisionRecord,
    MergeTarget, ProposedVocabulary, SeedVocabulary, CONSOLIDATION_MODEL,
    CONSOLIDATION_SYSTEM_PROMPT,
};

pub mod extract;
pub use extract::{
    build_extraction_payload, evidence_text_matches_snapshot, process_block_response,
    reuse_report, render_labeled_block_text, snapshot_lines, split_into_budget_blocks,
    validate_seed_vocabulary, BlockExtractionOutcome, ExtractedEndpoint, ExtractedEvidence,
    ExtractedRelation, ExtractionBlock, ExtractionResponse, InvalidClaim, MintRejection,
    MintTarget, ReuseReport, ValidClaim, VocabularyState, EXTRACTION_MODEL,
    EXTRACTION_SYSTEM_PROMPT,
};

pub const ENV_API_KEY: &str = "XIAOMI_API_KEY";

/// Maximum number of blocks sampled from one file. Files with fewer paragraph
/// blocks are sampled whole. The sampled count never drops below one, so every
/// matched file contributes at least one block instead of a pooled cross-file
/// subset.
pub const MAX_BLOCKS_PER_FILE: usize = 3;

const SCHEMA_DISCOVERY_MAX_COMPLETION_TOKENS: u32 = 8192;

/// The only framing the model gets: a graph for user-facing search over text
/// documents. Everything project-specific lives in the sampled block text
/// itself, never in this prompt.
pub const SCHEMA_DISCOVERY_SYSTEM_PROMPT: &str = r#"You are helping design a knowledge graph that will power user-facing search over a collection of text documents.

You will receive text blocks sampled from those documents. Read them and propose a starting graph vocabulary:

1. entity_kinds: categories of things the blocks talk about, each with a one-line description.
2. predicates: named relationship types connecting entities, each with a one-line description plus the entity kind expected for its subject and its object.
3. example_relations: two or three example subject/predicate/object relations drawn from the supplied blocks as a sanity check that the vocabulary fits the material.

Rules:
- Ground every entity kind and predicate in concepts the supplied blocks actually mention; do not propose categories or relationships nothing in the blocks supports.
- Prefer a small set of broad, reusable kinds and predicates over many narrow, overlapping ones.
- Each predicate should read as a directed relationship (subject -> object) that could hold across many documents, not a one-off phrase.
- Respond with exactly one JSON object of this shape:
{"entity_kinds":[{"kind":string,"description":string}],"predicates":[{"predicate":string,"description":string,"subject_kind":string,"object_kind":string}],"example_relations":[{"subject":{"kind":string,"name":string},"predicate":string,"object":{"kind":string,"name":string}}]}
- Return JSON only."#;

/// One resolved, readable UTF-8 Markdown/plain-text source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSource {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SampledBlock {
    pub id: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SampledFile {
    pub path: String,
    /// xxh3-64 of the file content as hex. This is the sole sampling seed:
    /// the same content reproduces the same block selection, with no clock,
    /// process, or randomness involved.
    pub content_hash: String,
    /// Number of paragraph blocks in the file before sampling.
    pub block_count: usize,
    pub blocks: Vec<SampledBlock>,
}

/// Expand repository-relative file paths and quoted globs against the repo
/// root, mirroring the `graph` CLI's `--source` semantics: relative paths only
/// (no `..`, no root), sorted and deduplicated, restricted to readable UTF-8
/// `.md`, `.markdown`, `.txt`, or `.text` files inside the repository.
pub fn resolve_sources(repo_path: &Path, specifications: &[String]) -> Result<Vec<SelectedSource>> {
    anyhow::ensure!(
        !specifications.is_empty(),
        "at least one --source path or glob is required"
    );
    let repo_root = fs::canonicalize(repo_path).map_err(|error| {
        anyhow!(
            "cannot resolve repository root {}: {error}",
            repo_path.display()
        )
    })?;
    anyhow::ensure!(
        repo_root.is_dir(),
        "repository root is not a directory: {}",
        repo_root.display()
    );

    let mut matches = BTreeMap::<String, PathBuf>::new();
    for specification in specifications {
        let relative = Path::new(specification);
        anyhow::ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_) | Component::CurDir)),
            "source must be a repository-relative path or glob without '..' or a root: {specification}"
        );
        let pattern = repo_root.join(relative);
        let pattern = pattern
            .to_str()
            .ok_or_else(|| anyhow!("source path is not valid UTF-8: {specification}"))?;
        let paths = glob::glob(pattern)
            .map_err(|error| anyhow!("invalid source glob {specification:?}: {error}"))?;
        let mut count = 0usize;
        for item in paths {
            let matched = item.map_err(|error| {
                anyhow!("could not expand source glob {specification:?}: {error}")
            })?;
            count += 1;
            let canonical = fs::canonicalize(&matched).map_err(|error| {
                anyhow!(
                    "selected source {} does not exist or cannot be resolved: {error}",
                    matched.display()
                )
            })?;
            let relative = canonical.strip_prefix(&repo_root).map_err(|_| {
                anyhow!(
                    "selected source {} escapes repository root {} (possibly through a symlink)",
                    matched.display(),
                    repo_root.display()
                )
            })?;
            let relative_string = relative
                .to_str()
                .ok_or_else(|| {
                    anyhow!(
                        "selected source path is not valid UTF-8: {}",
                        relative.display()
                    )
                })?
                .replace('\\', "/");
            matches.insert(relative_string, canonical);
        }
        anyhow::ensure!(
            count > 0,
            "source pattern {specification:?} matched no files"
        );
    }

    let mut sources = Vec::with_capacity(matches.len());
    for (relative_path, canonical) in matches {
        anyhow::ensure!(
            canonical.is_file(),
            "selected source is not a regular file: {relative_path}"
        );
        let extension = canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        anyhow::ensure!(
            matches!(extension.as_str(), "md" | "markdown" | "txt" | "text"),
            "selected source {relative_path} is not a Markdown or plain-text file (.md, .markdown, .txt, or .text)"
        );
        let bytes = fs::read(&canonical)
            .map_err(|error| anyhow!("cannot read selected source {relative_path}: {error}"))?;
        let text = String::from_utf8(bytes)
            .map_err(|error| anyhow!("selected source {relative_path} is not valid UTF-8: {error}"))?;
        sources.push(SelectedSource { path: relative_path, text });
    }
    Ok(sources)
}

/// Sample blocks from every source, numbering block ids sequentially across
/// the sorted file set so the outbound payload stays file-agnostic.
pub fn sample_files(sources: &[SelectedSource]) -> Vec<SampledFile> {
    let mut next_block_id = 1usize;
    sources
        .iter()
        .map(|source| {
            let mut sampled = sample_file_blocks(&source.path, &source.text);
            for block in &mut sampled.blocks {
                block.id = format!("b{next_block_id}");
                next_block_id += 1;
            }
            sampled
        })
        .collect()
}

/// Deterministically sample one or more paragraph blocks from one file's
/// content. The selection depends only on the content hash: same content in,
/// same blocks out.
pub fn sample_file_blocks(relative_path: &str, content: &str) -> SampledFile {
    let hash = xxh3_64(content.as_bytes());
    let blocks = paragraph_blocks(content);
    let block_count = blocks.len();
    let chosen: Vec<(u32, u32, String)> = if blocks.is_empty() {
        // An empty or all-blank file still yields one block spanning the file
        // so the at-least-one-block-per-file guarantee holds.
        vec![(
            1,
            content.lines().count().max(1) as u32,
            content.to_owned(),
        )]
    } else if block_count <= MAX_BLOCKS_PER_FILE {
        blocks
    } else {
        pick_block_indices(hash, block_count, MAX_BLOCKS_PER_FILE)
            .into_iter()
            .map(|index| blocks[index].clone())
            .collect()
    };
    SampledFile {
        path: relative_path.to_owned(),
        content_hash: format!("{hash:016x}"),
        block_count,
        blocks: chosen
            .into_iter()
            .enumerate()
            .map(|(offset, (start_line, end_line, text))| SampledBlock {
                id: format!("b{}", offset + 1),
                start_line,
                end_line,
                text,
            })
            .collect(),
    }
}

/// Split content into paragraph blocks: runs of consecutive non-blank physical
/// lines, with 1-based inclusive line ranges. Physical lines are never split,
/// so a block boundary always falls on a paragraph break.
fn paragraph_blocks(content: &str) -> Vec<(u32, u32, String)> {
    let mut blocks = Vec::new();
    let mut start = 0u32;
    let mut end = 0u32;
    let mut text = String::new();
    for (offset, line) in content.split_inclusive('\n').enumerate() {
        let line_number = offset as u32 + 1;
        if line.trim().is_empty() {
            if !text.is_empty() {
                blocks.push((start, end, std::mem::take(&mut text)));
            }
        } else {
            if text.is_empty() {
                start = line_number;
            }
            end = line_number;
            text.push_str(line);
        }
    }
    if !text.is_empty() {
        blocks.push((start, end, text));
    }
    blocks
}

/// Pick `count` distinct block indices from a `xxh3`-64 content hash via
/// rehashing plus linear probing. Deterministic (no `rand`, no clock, no
/// process state) and guaranteed to terminate because probing stops at the
/// first unused index and `count <= block_count`.
fn pick_block_indices(content_hash: u64, block_count: usize, count: usize) -> Vec<usize> {
    debug_assert!(count <= block_count);
    let mut seed = content_hash;
    let mut chosen = Vec::with_capacity(count);
    for _ in 0..count {
        let mut index = (seed as usize) % block_count;
        while chosen.contains(&index) {
            index = (index + 1) % block_count;
        }
        chosen.push(index);
        seed = xxh3_64(&seed.to_le_bytes());
    }
    chosen.sort_unstable();
    chosen
}

/// Build the batched user payload: every sampled block in one JSON object,
/// carrying only block ids and text so the request framing stays generic.
pub fn build_user_payload(files: &[SampledFile]) -> serde_json::Value {
    let blocks = files
        .iter()
        .flat_map(|file| file.blocks.iter())
        .map(|block| serde_json::json!({ "id": block.id, "text": block.text }))
        .collect::<Vec<_>>();
    serde_json::json!({ "blocks": blocks })
}

/// Render the full outbound prompt (system + user) for inspection dumps.
pub fn render_prompt_dump(user_payload: &serde_json::Value) -> String {
    format!(
        "=== system message ===\n{}\n\n=== user message ===\n{}",
        SCHEMA_DISCOVERY_SYSTEM_PROMPT,
        serde_json::to_string_pretty(user_payload).expect("user payload is valid JSON"),
    )
}

pub struct XiaomiSchemaAdapter {
    api_key: String,
}

impl std::fmt::Debug for XiaomiSchemaAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("XiaomiSchemaAdapter")
            .field("provider", &XIAOMI)
            .field("model", &MIMO_V2_5_PRO)
            .field("api_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl XiaomiSchemaAdapter {
    /// Read the Xiaomi API key from the environment (after `.env` loading).
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var(ENV_API_KEY)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("required environment variable {ENV_API_KEY} is not set"))?;
        Ok(Self { api_key })
    }

    /// One JSON-mode chat completion: system prompt plus one user payload in,
    /// raw completion content out. Shared by every experiment stage.
    fn chat_completion(
        &self,
        system_prompt: &str,
        user_payload: &serde_json::Value,
        max_completion_tokens: u32,
        stage: &str,
    ) -> Result<String> {
        let payload = serde_json::json!({
            "model": MIMO_V2_5_PRO,
            "max_completion_tokens": max_completion_tokens,
            "thinking": { "type": "disabled" },
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_payload.to_string() },
            ],
            "response_format": { "type": "json_object" },
        });
        let request: XiaomiChatCompletionRequest = serde_json::from_value(payload)?;
        let client = XiaomiClient::new(&self.api_key)
            .map_err(|error| sanitized_sdk_error(error, stage))?
            .with_model(MIMO_V2_5_PRO);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .with_context(|| format!("could not initialize {stage} runtime"))?;
        eprintln!("{stage}: request started (max completion tokens: {max_completion_tokens})");
        let started = Instant::now();
        // SDK errors can contain provider response text. Do not surface that
        // text in experiment errors or logs.
        let response = runtime
            .block_on(client.create_chat_completion(request))
            .map_err(|error| sanitized_sdk_error(error, stage))?;
        eprintln!(
            "{stage}: request finished after {:.1}s (completion tokens: {})",
            started.elapsed().as_secs_f64(),
            response.usage.completion_tokens
        );
        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("{stage} returned no completion choices"))?;
        if choice.finish_reason.as_deref() == Some("length") {
            anyhow::bail!("{stage} reached its completion token limit ({max_completion_tokens} tokens)");
        }
        choice
            .message
            .content
            .ok_or_else(|| anyhow!("{stage} returned no completion content"))
    }

    /// One batched schema-discovery call: all sampled blocks go out in a
    /// single user message and the raw proposed vocabulary comes back.
    pub fn propose_vocabulary(&self, user_payload: &serde_json::Value) -> Result<String> {
        self.chat_completion(
            SCHEMA_DISCOVERY_SYSTEM_PROMPT,
            user_payload,
            SCHEMA_DISCOVERY_MAX_COMPLETION_TOKENS,
            "schema discovery",
        )
    }

    /// One consolidation call (Task 2): the proposed vocabulary goes out in
    /// a single user message and the raw merge-proposal JSON comes back.
    pub fn propose_merges(&self, user_payload: &serde_json::Value) -> Result<String> {
        self.chat_completion(
            consolidate::CONSOLIDATION_SYSTEM_PROMPT,
            user_payload,
            consolidate::CONSOLIDATION_MAX_COMPLETION_TOKENS,
            "consolidation",
        )
    }

    /// One sequential extraction call (Task 3): the block payload (labeled
    /// block text, running summary, current vocabulary) goes out with the
    /// open-vocabulary extraction prompt and the raw response comes back.
    /// Calls are inherently serial — each depends on the previous call's
    /// summary and vocabulary state.
    pub fn extract_block(&self, user_payload: &serde_json::Value) -> Result<String> {
        self.chat_completion(
            extract::EXTRACTION_SYSTEM_PROMPT,
            user_payload,
            extract::EXTRACTION_MAX_COMPLETION_TOKENS,
            "extraction",
        )
    }
}

fn sanitized_sdk_error(error: LlmError, stage: &str) -> anyhow::Error {
    match error {
        LlmError::Authentication { .. } => anyhow!("{XIAOMI} {stage} authentication failed"),
        LlmError::RateLimit { .. } => anyhow!("{XIAOMI} {stage} rate limit exceeded"),
        LlmError::InvalidRequest { .. } => anyhow!("{XIAOMI} {stage} rejected the request"),
        LlmError::Api { status, .. } => anyhow!("{XIAOMI} {stage} returned HTTP status {status}"),
        LlmError::Network { .. } => anyhow!("{XIAOMI} {stage} network request failed"),
        LlmError::Parse { .. } | LlmError::Internal { .. } => {
            anyhow!("{XIAOMI} {stage} returned a malformed response")
        }
        _ => anyhow!("{XIAOMI} {stage} request failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_with_blocks(block_count: usize) -> String {
        (1..=block_count)
            .map(|block| {
                format!("Block {block} heading\nBlock {block} body text with supporting detail.\n\n")
            })
            .collect()
    }

    #[test]
    fn sampling_is_deterministic_for_unchanged_content() {
        let content = document_with_blocks(40);
        let first = sample_file_blocks("doc.md", &content);
        let second = sample_file_blocks("doc.md", &content);
        assert_eq!(first, second);
        assert_eq!(
            first.content_hash,
            format!("{:016x}", xxh3_64(content.as_bytes()))
        );
    }

    #[test]
    fn sampling_is_deterministic_across_full_file_set_runs() {
        let sources = vec![
            SelectedSource { path: "a.md".to_owned(), text: document_with_blocks(25) },
            SelectedSource { path: "b.md".to_owned(), text: document_with_blocks(12) },
        ];
        let first = sample_files(&sources);
        let second = sample_files(&sources);
        assert_eq!(first, second);
    }

    #[test]
    fn every_file_yields_at_least_one_block() {
        let sources = vec![
            SelectedSource { path: "big.md".to_owned(), text: document_with_blocks(30) },
            SelectedSource { path: "one.md".to_owned(), text: "only paragraph\n".to_owned() },
            SelectedSource { path: "empty.md".to_owned(), text: String::new() },
            SelectedSource { path: "blank.md".to_owned(), text: "\n \n\n".to_owned() },
        ];
        let sampled = sample_files(&sources);
        assert_eq!(sampled.len(), sources.len());
        for file in &sampled {
            assert!(
                !file.blocks.is_empty(),
                "{} produced no sampled block",
                file.path
            );
        }
        let empty = &sampled[2];
        assert_eq!((empty.blocks[0].start_line, empty.blocks[0].end_line), (1, 1));
        assert_eq!(empty.blocks[0].text, "");
    }

    #[test]
    fn sampled_block_text_matches_its_recorded_line_range() {
        let content = document_with_blocks(30);
        let sampled = sample_file_blocks("doc.md", &content);
        let lines: Vec<&str> = content.split_inclusive('\n').collect();
        for block in &sampled.blocks {
            let expected: String = lines
                [(block.start_line - 1) as usize..block.end_line as usize]
                .concat();
            assert_eq!(block.text, expected);
        }
    }

    #[test]
    fn blocks_keep_paragraph_boundaries_and_physical_lines_intact() {
        let content = "first para line 1\nfirst para line 2\n\nsecond para\n";
        let blocks = paragraph_blocks(content);
        assert_eq!(
            blocks,
            vec![
                (1, 2, "first para line 1\nfirst para line 2\n".to_owned()),
                (4, 4, "second para\n".to_owned()),
            ]
        );
    }

    #[test]
    fn large_files_sample_the_maximum_block_count() {
        let content = document_with_blocks(30);
        let sampled = sample_file_blocks("doc.md", &content);
        assert_eq!(sampled.blocks.len(), MAX_BLOCKS_PER_FILE);
        assert_eq!(sampled.block_count, 30);
        let starts: Vec<u32> = sampled.blocks.iter().map(|b| b.start_line).collect();
        assert_eq!(starts.len(), starts.iter().collect::<std::collections::HashSet<_>>().len());
    }

    #[test]
    fn small_files_sample_every_block() {
        let content = document_with_blocks(2);
        let sampled = sample_file_blocks("doc.md", &content);
        assert_eq!(sampled.blocks.len(), 2);
        assert_eq!(sampled.block_count, 2);
    }

    #[test]
    fn system_prompt_has_only_generic_graph_for_search_framing() {
        let prompt = SCHEMA_DISCOVERY_SYSTEM_PROMPT.to_ascii_lowercase();
        for token in [
            "daftprompt",
            "git",
            "commit",
            "epic",
            "index",
            "akar",
            "wgpu",
            "taffy",
            "glyphon",
            "sqlite",
            "embedding",
            "tree-sitter",
            "rust",
            "cargo",
            "xiaomi",
            "mimo",
            "repo",
            "code",
            "card",
            "canvas",
        ] {
            assert!(!prompt.contains(token), "system prompt must not contain {token:?}");
        }
        for token in ["graph", "search", "entity_kinds", "predicates", "example_relations"] {
            assert!(prompt.contains(token), "system prompt must mention {token:?}");
        }
    }

    #[test]
    fn user_payload_carries_only_block_ids_and_text() {
        let sources = vec![SelectedSource { path: "secret-name.md".to_owned(), text: "alpha\n\nbeta\n".to_owned() }];
        let payload = build_user_payload(&sample_files(&sources));
        let serialized = payload.to_string();
        assert!(serialized.contains("\"text\""));
        assert!(!serialized.contains("secret-name"), "payload must not leak file paths");
    }
}
