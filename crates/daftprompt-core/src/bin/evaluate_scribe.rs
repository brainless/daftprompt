//! Opt-in live evaluation of Scribe against the annotated fixture set.
//!
//! This binary is built only with `--features live-eval`, and it calls a model
//! only when `DAFTPROMPT_EVAL_LIVE=1` is set. Without that flag (or without
//! credentials) it writes a baseline report that explicitly records that no
//! live run happened. It never fabricates model results.
//!
//! Environment:
//!
//! - `DAFTPROMPT_EVAL_LIVE=1` — required to call the model.
//! - `DAFTPROMPT_EVAL_PROVIDER` — `openai` (default; OpenAI-compatible
//!   endpoints work through `OPENAI_BASE_URL`).
//! - `DAFTPROMPT_EVAL_MODEL` — model id, default `gpt-4o-mini`.
//! - `DAFTPROMPT_EVAL_PROMPT_VERSION` — optional guard; must equal the
//!   compiled prompt version (currently `scribe-v1`).
//! - `DAFTPROMPT_EVAL_MAX_ATTEMPTS` — total attempts, default 3.
//! - `DAFTPROMPT_EVAL_TIMEOUT_SECS` — per-attempt timeout, default 60.
//! - `OPENAI_API_KEY` — read by the provider client, never by this crate.
//!
//! Usage:
//!
//! ```text
//! evaluate-scribe [--fixtures DIR] [--output FILE] [--limit N]
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use daftprompt_core::eval::{
    FixtureRun, FixtureStatus, Prediction, Report, RunMetadata, default_fixture_dir, evaluate,
    load_fixtures, now_timestamp,
};
use daftprompt_core::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_TIMEOUT, PROMPT_VERSION, Scribe, ScribeConfig,
};
use daftprompt_types::{ParseError, ParseRequest};
use rig::providers::openai::OpenAI;

const DEFAULT_PROVIDER: &str = "openai";
const DEFAULT_MODEL: &str = "gpt-4o-mini";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("evaluate-scribe: {error}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<ExitCode, String> {
    let options = match parse_args(env::args().skip(1))? {
        Some(options) => options,
        None => {
            print_usage();
            return Ok(ExitCode::SUCCESS);
        }
    };

    let mut fixtures = load_fixtures(&options.fixtures)
        .map_err(|error| format!("could not load fixtures: {error}"))?;
    if let Some(limit) = options.limit {
        fixtures.truncate(limit);
    }
    if fixtures.is_empty() {
        return Err("the selected fixture set is empty".to_owned());
    }

    let provider =
        env_string("DAFTPROMPT_EVAL_PROVIDER").unwrap_or_else(|| DEFAULT_PROVIDER.to_owned());
    let model = env_string("DAFTPROMPT_EVAL_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_owned());
    let max_attempts = env_u32("DAFTPROMPT_EVAL_MAX_ATTEMPTS", DEFAULT_MAX_ATTEMPTS)?;
    let timeout_secs = env_u64("DAFTPROMPT_EVAL_TIMEOUT_SECS", DEFAULT_TIMEOUT.as_secs())?;
    let timeout = Duration::from_secs(timeout_secs);
    let requested_prompt = env_string("DAFTPROMPT_EVAL_PROMPT_VERSION");
    let live = env_flag("DAFTPROMPT_EVAL_LIVE");

    let metadata = RunMetadata {
        provider: Some(provider.clone()),
        model: Some(model.clone()),
        prompt_version: PROMPT_VERSION.to_owned(),
        max_attempts,
        timeout,
        fixtures_path: options.fixtures.clone(),
        generated_at: now_timestamp(),
    };
    let mut report = Report::new(metadata, &fixtures);

    if !live {
        report.not_run = Some(
            "DAFTPROMPT_EVAL_LIVE was not set, so no model was called and no model results are recorded."
                .to_owned(),
        );
        return finish(&options.output, &report, ExitCode::SUCCESS);
    }

    if provider != DEFAULT_PROVIDER {
        report.not_run = Some(format!(
            "provider {provider:?} is not wired into this evaluation; only {DEFAULT_PROVIDER:?} is supported, with OpenAI-compatible endpoints selected through OPENAI_BASE_URL."
        ));
        return finish(&options.output, &report, ExitCode::from(2));
    }

    if let Some(requested) = &requested_prompt
        && requested != PROMPT_VERSION
    {
        report.not_run = Some(format!(
            "prompt version {requested:?} was requested, but this build contains only {PROMPT_VERSION:?}. Add a new versioned prompt file and rebuild to evaluate another revision."
        ));
        return finish(&options.output, &report, ExitCode::from(2));
    }

    let client = match OpenAI::from_env() {
        Ok(client) => client,
        Err(error) => {
            report.not_run = Some(format!(
                "the live run could not start because provider credentials were unavailable: {error}"
            ));
            return finish(&options.output, &report, ExitCode::from(2));
        }
    };

    let completion_model = client.completion(model.clone());
    let config = ScribeConfig::new()
        .with_provider(provider)
        .with_model(model)
        .with_max_attempts(max_attempts)
        .with_timeout(timeout);
    let scribe = Scribe::from_rig_model(config, completion_model);

    println!(
        "running {} fixture(s) with prompt {} and {} total attempt(s)",
        fixtures.len(),
        PROMPT_VERSION,
        max_attempts.max(1)
    );

    for fixture in &fixtures {
        let request = match &fixture.source {
            Some(source) => ParseRequest::with_source(fixture.text.clone(), source.clone()),
            None => ParseRequest::new(fixture.text.clone()),
        };

        let started = Instant::now();
        let outcome = scribe.parse(request).await;
        let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;

        match outcome {
            Ok(result) => {
                let attempts = Some(result.run.attempts);
                let prediction = Prediction::from_parse_result(&result);
                let evaluation = evaluate(fixture, &prediction);
                println!(
                    "ok   {:<30} spans {}/{} lenient {}/{} labels {}/{} links {}/{} issues {} attempts {} {:.1} ms",
                    fixture.id,
                    evaluation.span_exact.matched,
                    evaluation.span_exact.expected,
                    evaluation.span_lenient.matched,
                    evaluation.span_lenient.expected,
                    evaluation.label_lenient.matched,
                    evaluation.label_lenient.expected,
                    evaluation.answer_lenient.matched,
                    evaluation.answer_lenient.expected,
                    evaluation.issues.len(),
                    result.run.attempts,
                    latency_ms
                );
                report.push(FixtureRun {
                    fixture_id: fixture.id.clone(),
                    category: fixture.category,
                    attempts,
                    latency_ms,
                    status: FixtureStatus::Parsed(evaluation),
                });
            }
            Err(error) => {
                let attempts = match &error {
                    ParseError::InvalidOutput { attempts, .. } => Some(*attempts),
                    _ => None,
                };
                let kind = error.kind().to_owned();
                println!(
                    "FAIL {:<30} {kind} attempts {} {:.1} ms: {error}",
                    fixture.id,
                    attempts.map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                    latency_ms
                );
                report.push(FixtureRun {
                    fixture_id: fixture.id.clone(),
                    category: fixture.category,
                    attempts,
                    latency_ms,
                    status: FixtureStatus::Failed {
                        kind,
                        detail: error.to_string(),
                        attempts,
                    },
                });
            }
        }
    }

    let failed = report
        .runs
        .iter()
        .any(|run| matches!(run.status, FixtureStatus::Failed { .. }));
    finish(
        &options.output,
        &report,
        if failed {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        },
    )
}

fn finish(path: &Path, report: &Report, code: ExitCode) -> Result<ExitCode, String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    fs::write(path, report.render())
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    println!("report written to {}", path.display());
    Ok(code)
}

#[derive(Debug)]
struct Options {
    fixtures: PathBuf,
    output: PathBuf,
    limit: Option<usize>,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut fixtures = default_fixture_dir();
    let mut output = default_output();
    let mut limit = None;
    let mut args = args.peekable();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--fixtures" => {
                fixtures = PathBuf::from(next_value(&mut args, "--fixtures")?);
            }
            "--output" => {
                output = PathBuf::from(next_value(&mut args, "--output")?);
            }
            "--limit" => {
                let value = next_value(&mut args, "--limit")?;
                let parsed: usize = value
                    .parse()
                    .map_err(|_| format!("--limit expects a positive integer, got {value:?}"))?;
                if parsed == 0 {
                    return Err("--limit must be at least 1".to_owned());
                }
                limit = Some(parsed);
            }
            unknown => return Err(format!("unknown argument {unknown:?}; try --help")),
        }
    }

    Ok(Some(Options {
        fixtures,
        output,
        limit,
    }))
}

fn next_value(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    flag: &str,
) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn default_output() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map_or_else(
            || PathBuf::from("docs/evaluations/scribe-baseline.md"),
            |root| root.join("docs/evaluations/scribe-baseline.md"),
        )
}

fn print_usage() {
    println!(
        "Usage: evaluate-scribe [--fixtures DIR] [--output FILE] [--limit N]\n\
         \n\
         Runs Scribe over the annotated fixture set. Without DAFTPROMPT_EVAL_LIVE=1\n\
         it writes a baseline report and makes no model calls.\n\
         \n\
         Options:\n\
           --fixtures DIR  fixture directory (default: crates/daftprompt-core/fixtures/scribe)\n\
           --output FILE   report path (default: docs/evaluations/scribe-baseline.md)\n\
           --limit N       evaluate only the first N fixtures, in file-name order\n\
           -h, --help      show this help"
    );
}

fn env_string(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn env_flag(name: &str) -> bool {
    env_string(name).is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

fn env_u32(name: &str, default: u32) -> Result<u32, String> {
    match env_string(name) {
        Some(value) => value
            .parse()
            .map_err(|_| format!("{name} expects an integer, got {value:?}")),
        None => Ok(default),
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    match env_string(name) {
        Some(value) => value
            .parse()
            .map_err(|_| format!("{name} expects an integer, got {value:?}")),
        None => Ok(default),
    }
}
