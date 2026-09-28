use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

use open_vocab_graph::consolidate::{DecisionStatus, MergeTarget, ProposedVocabulary, SeedVocabulary};

use open_vocab_graph::{
    build_consolidation_payload, build_extraction_payload, build_user_payload,
    consolidate_vocabulary, process_block_response, render_prompt_dump, resolve_sources,
    reuse_report, sample_files, split_into_budget_blocks, validate_seed_vocabulary,
    validate_vocabulary, ExtractionBlock, ReuseReport, SampledFile, SelectedSource,
    VocabularyState, XiaomiSchemaAdapter, CONSOLIDATION_MODEL, EXTRACTION_MODEL,
    EXTRACTION_SYSTEM_PROMPT,
};

/// Standalone open-vocabulary graph experiments (Epic 013).
#[derive(Debug, Parser)]
#[command(name = "open-vocab-graph", version, about = "Standalone open-vocabulary graph experiments (Epic 013). Set XIAOMI_API_KEY in the environment or a .env file in the current directory.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Deterministically sample blocks per file and run one batched schema-discovery LLM call.
    Sample(SampleArgs),
    /// Consolidate Task 1's raw proposed vocabulary into a deduplicated seed vocabulary.
    Consolidate(ConsolidateArgs),
    /// Sequential carryover extraction loop over a source (Task 3).
    Extract(ExtractArgs),
}

#[derive(Debug, Args)]
struct ConsolidateArgs {
    /// Path to Task 1's raw proposed vocabulary JSON (proposed-vocabulary.json).
    #[arg(long)]
    input: PathBuf,

    /// Path receiving the consolidated seed vocabulary JSON.
    #[arg(long, default_value = "experiments/open-vocab-graph/output/seed-vocabulary.json")]
    output: PathBuf,

    /// Path receiving the merge-decisions log JSON.
    #[arg(long, default_value = "experiments/open-vocab-graph/output/consolidation-log.json")]
    log: PathBuf,
}

#[derive(Debug, Args)]
struct SampleArgs {
    /// Repository root (defaults to the current directory).
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Repository-relative file path or quoted glob; may be repeated.
    #[arg(long = "source", required = true)]
    sources: Vec<String>,

    /// Directory receiving sampled-blocks.json, prompt.txt, and proposed-vocabulary.json.
    #[arg(long, default_value = "experiments/open-vocab-graph/output")]
    output: PathBuf,

    /// Write the sampling record and prompt dump without calling the LLM.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ExtractArgs {
    /// Repository root (defaults to the current directory).
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Repository-relative file path or quoted glob; may be repeated.
    #[arg(long = "source", required = true)]
    sources: Vec<String>,

    /// Maximum words per extraction block; blocks never break a physical line.
    #[arg(long)]
    word_budget: u32,

    /// Path to Task 2's consolidated seed vocabulary.
    #[arg(long, default_value = "experiments/open-vocab-graph/output/seed-vocabulary.json")]
    seed: PathBuf,

    /// Path to the reference set recorded before any extraction run.
    #[arg(long, default_value = "experiments/open-vocab-graph/output/reference-set.json")]
    reference_set: PathBuf,

    /// Directory receiving this run's outputs (defaults to
    /// experiments/open-vocab-graph/output/runs/budget-<word_budget>).
    #[arg(long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if std::path::Path::new(".env").is_file() {
        dotenvy::from_path(".env")?;
    }
    match cli.command {
        Command::Sample(args) => sample(args),
        Command::Consolidate(args) => consolidate(args),
        Command::Extract(args) => extract(args),
    }
}

fn consolidate(args: ConsolidateArgs) -> Result<()> {
    let raw_input = fs::read_to_string(&args.input)
        .with_context(|| format!("cannot read input vocabulary {}", args.input.display()))?;
    let vocabulary: ProposedVocabulary = serde_json::from_str(raw_input.trim()).with_context(|| {
        format!(
            "input vocabulary {} is not a valid proposed-vocabulary JSON object",
            args.input.display()
        )
    })?;
    // Fail before spending the LLM call if the input is malformed.
    validate_vocabulary(&vocabulary)?;

    let adapter = XiaomiSchemaAdapter::from_env()?;
    let payload = build_consolidation_payload(&vocabulary);
    let raw_response = adapter.propose_merges(&payload)?;
    let report = consolidate_vocabulary(&vocabulary, &raw_response);

    if let Some(error) = &report.log.error {
        // Still write the log so the raw response and the failure stay
        // inspectable, then report the failure as an error.
        write_json(&args.log, &report.log)?;
        eprintln!("wrote consolidation log -> {}", args.log.display());
        anyhow::bail!("{error}");
    }
    let seed = report
        .seed
        .as_ref()
        .expect("a report without an error always carries a seed");
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    write_json(&args.output, seed)?;
    write_json(&args.log, &report.log)?;

    let input_counts = report.log.input_counts;
    let output_counts = report.log.output_counts.expect("seed present");
    println!(
        "consolidation ({}): {} entity kinds / {} predicates -> {} entity kinds / {} predicates",
        CONSOLIDATION_MODEL,
        input_counts.entity_kinds,
        input_counts.predicates,
        output_counts.entity_kinds,
        output_counts.predicates,
    );
    for decision in &report.log.decisions {
        match &decision.status {
            DecisionStatus::Applied => println!(
                "  merged {} {:?} <- {:?}: {}",
                match decision.target {
                    MergeTarget::EntityKind => "entity kind",
                    MergeTarget::Predicate => "predicate",
                },
                decision.canonical,
                decision
                    .merged_members
                    .iter()
                    .map(|member| &member.name)
                    .collect::<Vec<_>>(),
                decision.rationale,
            ),
            DecisionStatus::Rejected { reason } => {
                println!("  rejected proposal for {:?}: {}", decision.canonical, reason)
            }
        }
    }
    println!("wrote seed vocabulary -> {}", args.output.display());
    println!("wrote consolidation log -> {}", args.log.display());
    Ok(())
}

fn write_json(path: &std::path::Path, value: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes)
        .with_context(|| format!("could not write {}", path.display()))
}

fn sample(args: SampleArgs) -> Result<()> {
    let sources = resolve_sources(&args.repo, &args.sources)?;
    let sampled = sample_files(&sources);
    fs::create_dir_all(&args.output)
        .with_context(|| format!("could not create output directory {}", args.output.display()))?;

    let blocks_path = args.output.join("sampled-blocks.json");
    fs::write(&blocks_path, serde_json::to_vec_pretty(&sampled)?)?;
    println!(
        "sampled {} block(s) from {} file(s) -> {}",
        total_blocks(&sampled),
        sampled.len(),
        blocks_path.display()
    );

    let user_payload = build_user_payload(&sampled);
    let prompt_path = args.output.join("prompt.txt");
    fs::write(&prompt_path, render_prompt_dump(&user_payload))?;
    println!("wrote prompt dump -> {}", prompt_path.display());

    if args.dry_run {
        println!("dry run: skipped the schema-discovery LLM call");
        return Ok(());
    }

    let adapter = XiaomiSchemaAdapter::from_env()?;
    let raw = adapter.propose_vocabulary(&user_payload)?;
    let vocabulary_path = args.output.join("proposed-vocabulary.json");
    fs::write(&vocabulary_path, &raw)?;
    println!("wrote raw proposed vocabulary -> {}", vocabulary_path.display());

    let value: serde_json::Value = serde_json::from_str(raw.trim()).with_context(|| {
        format!(
            "proposed vocabulary output saved to {} is not valid JSON",
            vocabulary_path.display()
        )
    })?;
    let mut missing = Vec::new();
    for key in ["entity_kinds", "predicates", "example_relations"] {
        if value.get(key).and_then(|entry| entry.as_array()).is_none() {
            missing.push(key);
        }
    }
    anyhow::ensure!(
        missing.is_empty(),
        "proposed vocabulary output is missing {missing:?}; raw output saved to {}",
        vocabulary_path.display()
    );
    for key in ["entity_kinds", "predicates", "example_relations"] {
        let count = value[key].as_array().map(|entries| entries.len()).unwrap_or(0);
        println!("  {key}: {count}");
    }
    Ok(())
}

fn total_blocks(files: &[SampledFile]) -> usize {
    files.iter().map(|file| file.blocks.len()).sum()
}

#[derive(Debug, Serialize)]
struct BlockRecord {
    block_id: String,
    source_path: String,
    start_line: u32,
    end_line: u32,
    word_count: usize,
    relations: usize,
    valid_claims: usize,
    invalid_claims: usize,
    parse_error: bool,
    model_error: bool,
    minted_entity_kinds: Vec<String>,
    minted_predicates: Vec<String>,
}

impl BlockRecord {
    fn failed_call(block: &ExtractionBlock) -> Self {
        Self {
            block_id: block.id.clone(),
            source_path: block.source_path.clone(),
            start_line: block.start_line,
            end_line: block.end_line,
            word_count: block.word_count,
            relations: 0,
            valid_claims: 0,
            invalid_claims: 0,
            parse_error: false,
            model_error: true,
            minted_entity_kinds: Vec::new(),
            minted_predicates: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct CountSummary {
    relations_emitted: usize,
    valid_claims: usize,
    invalid_claims: usize,
    parse_errors: usize,
    model_errors: usize,
    rejected_mints: usize,
}

#[derive(Debug, Serialize)]
struct VocabularySummary {
    seed_entity_kinds: usize,
    seed_predicates: usize,
    minted_entity_kinds: usize,
    minted_predicates: usize,
    final_entity_kinds: usize,
    final_predicates: usize,
    minted_entity_kind_names: Vec<String>,
    minted_predicate_names: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RunManifest {
    epic: String,
    model: String,
    word_budget: u32,
    sources: Vec<String>,
    seed_path: String,
    seed_content_hash: String,
    reference_set_path: String,
    reference_set_content_hash: String,
    snapshot_hashes: BTreeMap<String, String>,
    block_count: usize,
    blocks: Vec<BlockRecord>,
    counts: CountSummary,
    vocabulary: VocabularySummary,
    reuse: ReuseReport,
    final_running_summary: String,
}

/// Sequential carryover extraction loop (Task 3). Blocks are walked in order;
/// each call carries the previous call's running summary and the accumulated
/// open vocabulary, so the loop is inherently serial.
fn extract(args: ExtractArgs) -> Result<()> {
    anyhow::ensure!(args.word_budget >= 1, "word budget must be at least 1");
    let output_dir = args.output.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "experiments/open-vocab-graph/output/runs/budget-{}",
            args.word_budget
        ))
    });

    // Reference set FIRST: a run refuses to start without one that declares
    // it was recorded before any extraction run, so the Task 3 ordering
    // requirement is enforced mechanically rather than by convention.
    let reference_set_bytes = fs::read(&args.reference_set).with_context(|| {
        format!(
            "cannot read reference set {}; record it before running extraction (Task 3 ordering)",
            args.reference_set.display()
        )
    })?;
    let reference_set: serde_json::Value = serde_json::from_slice(&reference_set_bytes)
        .with_context(|| format!("reference set {} is not valid JSON", args.reference_set.display()))?;
    anyhow::ensure!(
        reference_set.get("recorded_before_any_extraction_run") == Some(&serde_json::Value::Bool(true)),
        "reference set {} does not declare \"recorded_before_any_extraction_run\": true",
        args.reference_set.display()
    );

    let sources = resolve_sources(&args.repo, &args.sources)?;

    let seed_raw = fs::read_to_string(&args.seed)
        .with_context(|| format!("cannot read seed vocabulary {}", args.seed.display()))?;
    let seed: SeedVocabulary = serde_json::from_str(seed_raw.trim())
        .with_context(|| format!("seed vocabulary {} is not a valid seed JSON object", args.seed.display()))?;
    validate_seed_vocabulary(&seed)?;

    fs::create_dir_all(&output_dir)
        .with_context(|| format!("could not create {}", output_dir.display()))?;
    let blocks_dir = output_dir.join("blocks");
    let snapshots_dir = output_dir.join("snapshots");
    fs::create_dir_all(&blocks_dir)?;
    fs::create_dir_all(&snapshots_dir)?;
    fs::write(output_dir.join("reference-set.json"), &reference_set_bytes)?;
    fs::write(output_dir.join("seed-vocabulary.json"), serde_json::to_vec_pretty(&seed)?)?;

    let mut snapshot_hashes = BTreeMap::new();
    for source in &sources {
        fs::write(snapshots_dir.join(&source.path), &source.text)?;
        snapshot_hashes.insert(
            source.path.clone(),
            format!("{:016x}", xxh3_64(source.text.as_bytes())),
        );
    }

    let mut per_source: Vec<(&SelectedSource, Vec<ExtractionBlock>)> = Vec::new();
    let mut block_count = 0usize;
    for source in &sources {
        let mut blocks = split_into_budget_blocks(&source.path, &source.text, args.word_budget)?;
        for (offset, block) in blocks.iter_mut().enumerate() {
            block.id = format!("b{}", block_count + offset + 1);
        }
        block_count += blocks.len();
        per_source.push((source, blocks));
    }
    println!(
        "extraction: {} block(s) at {} word budget from {} file(s)",
        block_count,
        args.word_budget,
        sources.len()
    );

    let adapter = XiaomiSchemaAdapter::from_env()?;
    let seed_kind_names: Vec<String> =
        seed.entity_kinds.iter().map(|kind| kind.kind.clone()).collect();
    let seed_predicate_names: Vec<String> =
        seed.predicates.iter().map(|predicate| predicate.predicate.clone()).collect();
    let mut vocabulary = VocabularyState::from_seed(&seed);
    let mut running_summary = String::new();
    let mut valid_claims = Vec::new();
    let mut invalid_claims = Vec::new();
    let mut block_records = Vec::new();
    let mut growth_entries = Vec::new();
    let mut model_errors = 0usize;

    for (source, blocks) in &per_source {
        for block in blocks {
            let payload = build_extraction_payload(
                block,
                &source.path,
                &source.text,
                &running_summary,
                &vocabulary,
            );
            fs::write(
                blocks_dir.join(format!("{}.request.json", block.id)),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "system": EXTRACTION_SYSTEM_PROMPT,
                    "user": payload,
                }))?,
            )?;

            let mut raw = None;
            let mut attempts = Vec::new();
            for attempt in 1..=2 {
                match adapter.extract_block(&payload) {
                    Ok(response) => {
                        raw = Some(response);
                        break;
                    }
                    Err(error) => {
                        attempts.push(error.to_string());
                        if attempt < 2 {
                            eprintln!(
                                "extraction: block {}: attempt {attempt} failed, retrying once: {error}",
                                block.id
                            );
                            std::thread::sleep(Duration::from_secs(2));
                        }
                    }
                }
            }
            let Some(raw) = raw else {
                model_errors += 1;
                fs::write(
                    blocks_dir.join(format!("{}.error.json", block.id)),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "block_id": block.id,
                        "attempts": attempts,
                    }))?,
                )?;
                eprintln!(
                    "extraction: block {}: model call failed after {} attempt(s); continuing with the previous summary",
                    block.id,
                    attempts.len()
                );
                block_records.push(BlockRecord::failed_call(block));
                continue;
            };

            // Raw response recorded BEFORE any parsing, validation, or
            // omission so raw output always predates validation decisions.
            fs::write(
                blocks_dir.join(format!("{}.raw.json", block.id)),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "block_id": block.id,
                    "model": EXTRACTION_MODEL,
                    "raw_response": raw,
                }))?,
            )?;

            let outcome = process_block_response(block, &source.text, &mut vocabulary, &raw);
            valid_claims.extend(outcome.valid_claims.iter().cloned());
            invalid_claims.extend(outcome.invalid_claims.iter().cloned());
            if !outcome.running_summary.is_empty() {
                running_summary = outcome.running_summary.clone();
            }
            growth_entries.push(serde_json::json!({
                "block_id": outcome.block_id,
                "minted_entity_kinds": outcome.minted_entity_kinds,
                "minted_predicates": outcome.minted_predicates,
                "rejected_mints": outcome.rejected_mints,
            }));
            fs::write(
                blocks_dir.join(format!("{}.validation.json", block.id)),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "block_id": outcome.block_id,
                    "parse_error": outcome.parse_error,
                    "relation_count": outcome.valid_claims.len() + outcome.invalid_claims.len(),
                    "valid_count": outcome.valid_claims.len(),
                    "invalid_claims": outcome.invalid_claims,
                    "minted_entity_kinds": outcome.minted_entity_kinds.iter().map(|k| k.kind.clone()).collect::<Vec<_>>(),
                    "minted_predicates": outcome.minted_predicates.iter().map(|p| p.predicate.clone()).collect::<Vec<_>>(),
                    "rejected_mints": outcome.rejected_mints,
                    "running_summary": outcome.running_summary,
                }))?,
            )?;
            block_records.push(BlockRecord {
                block_id: block.id.clone(),
                source_path: block.source_path.clone(),
                start_line: block.start_line,
                end_line: block.end_line,
                word_count: block.word_count,
                relations: outcome.valid_claims.len() + outcome.invalid_claims.len(),
                valid_claims: outcome.valid_claims.len(),
                invalid_claims: outcome.invalid_claims.len(),
                parse_error: outcome.parse_error.is_some(),
                model_error: false,
                minted_entity_kinds: outcome.minted_entity_kinds.iter().map(|k| k.kind.clone()).collect(),
                minted_predicates: outcome.minted_predicates.iter().map(|p| p.predicate.clone()).collect(),
            });
        }
    }

    let minted_entity_kind_names: Vec<String> = growth_entries
        .iter()
        .flat_map(|entry| entry["minted_entity_kinds"].as_array().cloned().unwrap_or_default())
        .filter_map(|entry| entry["kind"].as_str().map(str::to_owned))
        .collect();
    let minted_predicate_names: Vec<String> = growth_entries
        .iter()
        .flat_map(|entry| entry["minted_predicates"].as_array().cloned().unwrap_or_default())
        .filter_map(|entry| entry["predicate"].as_str().map(str::to_owned))
        .collect();
    let parse_errors = block_records.iter().filter(|record| record.parse_error).count();
    let rejected_mints: usize = growth_entries
        .iter()
        .map(|entry| entry["rejected_mints"].as_array().map(Vec::len).unwrap_or(0))
        .sum();

    let reuse = reuse_report(&valid_claims, &seed_predicate_names, &minted_predicate_names);
    let final_vocabulary = SeedVocabulary {
        entity_kinds: vocabulary.entity_kinds.clone(),
        predicates: vocabulary.predicates.clone(),
        example_relations: seed.example_relations.clone(),
    };

    write_json(&output_dir.join("validated-claims.json"), &valid_claims)?;
    write_json(&output_dir.join("invalid-claims.json"), &invalid_claims)?;
    write_json(&output_dir.join("final-vocabulary.json"), &final_vocabulary)?;
    write_json(
        &output_dir.join("vocabulary-growth-log.json"),
        &serde_json::json!({
            "seed": {
                "entity_kinds": seed_kind_names,
                "predicates": seed_predicate_names,
            },
            "per_block": growth_entries,
            "totals": {
                "seed_entity_kinds": seed.entity_kinds.len(),
                "seed_predicates": seed.predicates.len(),
                "minted_entity_kinds": minted_entity_kind_names.len(),
                "minted_predicates": minted_predicate_names.len(),
                "rejected_mints": rejected_mints,
                "final_entity_kinds": vocabulary.entity_kinds.len(),
                "final_predicates": vocabulary.predicates.len(),
            },
        }),
    )?;
    let manifest = RunManifest {
        epic: "013-task-3".to_owned(),
        model: EXTRACTION_MODEL.to_owned(),
        word_budget: args.word_budget,
        sources: sources.iter().map(|source| source.path.clone()).collect(),
        seed_path: args.seed.display().to_string(),
        seed_content_hash: format!("{:016x}", xxh3_64(seed_raw.as_bytes())),
        reference_set_path: args.reference_set.display().to_string(),
        reference_set_content_hash: format!("{:016x}", xxh3_64(&reference_set_bytes)),
        snapshot_hashes,
        block_count,
        blocks: block_records,
        counts: CountSummary {
            relations_emitted: valid_claims.len() + invalid_claims.len(),
            valid_claims: valid_claims.len(),
            invalid_claims: invalid_claims.len(),
            parse_errors,
            model_errors,
            rejected_mints,
        },
        vocabulary: VocabularySummary {
            seed_entity_kinds: seed.entity_kinds.len(),
            seed_predicates: seed.predicates.len(),
            minted_entity_kinds: minted_entity_kind_names.len(),
            minted_predicates: minted_predicate_names.len(),
            final_entity_kinds: vocabulary.entity_kinds.len(),
            final_predicates: vocabulary.predicates.len(),
            minted_entity_kind_names,
            minted_predicate_names: minted_predicate_names.clone(),
        },
        reuse,
        final_running_summary: running_summary,
    };
    write_json(&output_dir.join("run-manifest.json"), &manifest)?;

    println!(
        "extraction ({}): budget {} words, {} blocks, {} valid claims, {} invalid claims, {} model error(s), {} parse error(s)",
        EXTRACTION_MODEL,
        args.word_budget,
        block_count,
        valid_claims.len(),
        invalid_claims.len(),
        model_errors,
        parse_errors
    );
    println!(
        "vocabulary: {} seed predicates + {} minted = {} final; minted: {}",
        seed.predicates.len(),
        manifest.vocabulary.minted_predicates,
        vocabulary.predicates.len(),
        if manifest.vocabulary.minted_predicate_names.is_empty() {
            "(none)".to_owned()
        } else {
            manifest.vocabulary.minted_predicate_names.join(", ")
        }
    );
    println!(
        "reuse: {} predicate(s) with valid relations in multiple blocks: {}",
        manifest.reuse.reused_across_blocks.len(),
        if manifest.reuse.reused_across_blocks.is_empty() {
            "(none)".to_owned()
        } else {
            manifest.reuse.reused_across_blocks.join(", ")
        }
    );
    println!(
        "new predicates with valid use: {} of {} minted",
        manifest.reuse.minted_predicates_with_valid_use.len(),
        manifest.reuse.minted_predicates.len()
    );
    println!("wrote run -> {}", output_dir.display());
    Ok(())
}
