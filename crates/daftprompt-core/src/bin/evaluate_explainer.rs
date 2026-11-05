//! Opt-in live evaluation of Explainer grounding quality.
//!
//! Built only with `--features live-eval`; it calls a model only when
//! `DAFTPROMPT_EVAL_LIVE=1` is set. Without that flag or without credentials it
//! writes a report that records the evaluation as not run. It never fabricates
//! model results and, unlike `evaluate-scribe`, does not load any `.env` file:
//! credentials must be in the process environment.
//!
//! Environment:
//!
//! - `DAFTPROMPT_EVAL_LIVE=1` — required to call the model.
//! - `DAFTPROMPT_EXPLAINER_PROVIDER` / `DAFTPROMPT_EXPLAINER_MODEL` — default
//!   `neuralwatt` / `glm-5.3-flash`; `openai`, `cerebras`, and `groq` are also supported.
//! - Provider credentials: `OPENAI_API_KEY`, `NEURALWATT_API_KEY`, or
//!   `CEREBRAS_API_KEY`, or `GROQ_API_KEY`; compatible providers accept an optional `*_BASE_URL`.
//! - `DAFTPROMPT_EVAL_MAX_ATTEMPTS`, `DAFTPROMPT_EVAL_TIMEOUT_SECS` — override
//!   the Explainer defaults.
//!
//! ```text
//! evaluate-explainer [--fixtures DIR] [--project DIR] [--output FILE] [--limit N]
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use daftprompt_core::eval::{
    ClarifyReport, ClarifyRun, ClarifyStatus, Observed, clarify_metadata,
    default_clarify_fixture_dir, default_clarify_project_dir, describe_location,
    load_clarify_fixtures, score_clarify,
};
use daftprompt_core::explainer::{Explainer, ExplainerConfig, PROMPT_VERSION};
use daftprompt_core::project_tools::FsProject;
use daftprompt_types::ParseError;
use rig::DynModel;
use rig::operation::Completion;
use rig::providers::openai::{OpenAI, OpenAIConfig};

const PROVIDER_OPENAI: &str = "openai";
const PROVIDER_NEURALWATT: &str = "neuralwatt";
const PROVIDER_CEREBRAS: &str = "cerebras";
const PROVIDER_GROQ: &str = "groq";
const DEFAULT_NEURALWATT_BASE_URL: &str = "https://api.neuralwatt.com/v1";
const DEFAULT_CEREBRAS_BASE_URL: &str = "https://api.cerebras.ai/v1";
const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("evaluate-explainer: {error}");
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
    let mut fixtures = load_clarify_fixtures(&options.fixtures, &options.project)
        .map_err(|error| format!("could not load fixtures: {error}"))?;
    if let Some(limit) = options.limit {
        fixtures.truncate(limit);
    }
    if fixtures.is_empty() {
        return Err("the selected fixture set is empty".to_owned());
    }

    let mut config = ExplainerConfig::from_env();
    if let Some(attempts) = env_string("DAFTPROMPT_EVAL_MAX_ATTEMPTS") {
        let attempts = attempts
            .parse()
            .map_err(|_| "DAFTPROMPT_EVAL_MAX_ATTEMPTS expects an integer".to_owned())?;
        config = config.with_max_attempts(attempts);
    }
    if let Some(secs) = env_string("DAFTPROMPT_EVAL_TIMEOUT_SECS") {
        let secs = secs
            .parse()
            .map_err(|_| "DAFTPROMPT_EVAL_TIMEOUT_SECS expects an integer".to_owned())?;
        config = config.with_timeout(Duration::from_secs(secs));
    }
    let provider = config.provider.clone().unwrap_or_default();
    let model = config.model.clone().unwrap_or_default();
    let mut report = ClarifyReport::new(
        clarify_metadata(
            Some(provider.clone()),
            Some(model.clone()),
            config.max_attempts,
            config.timeout,
            relative_to_repo(&options.fixtures),
        ),
        relative_to_repo(&options.project),
        &fixtures,
    );

    if !env_string("DAFTPROMPT_EVAL_LIVE")
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
    {
        report.not_run = Some(
            "DAFTPROMPT_EVAL_LIVE was not set, so no model was called and no model results are recorded."
                .to_owned(),
        );
        return finish(&options.output, &report, ExitCode::SUCCESS);
    }
    if !matches!(
        provider.as_str(),
        PROVIDER_OPENAI | PROVIDER_NEURALWATT | PROVIDER_CEREBRAS | PROVIDER_GROQ
    ) {
        report.not_run = Some(format!(
            "provider {provider:?} is not wired into this evaluation; supported providers are {PROVIDER_OPENAI:?}, {PROVIDER_NEURALWATT:?}, {PROVIDER_CEREBRAS:?}, and {PROVIDER_GROQ:?}."
        ));
        return finish(&options.output, &report, ExitCode::from(2));
    }
    let completion = match build_model(&provider, &model) {
        Ok(completion) => completion,
        Err(error) => {
            report.not_run = Some(format!(
                "the live run could not start because provider credentials were unavailable: {error} (no .env file is read)."
            ));
            return finish(&options.output, &report, ExitCode::from(2));
        }
    };
    let project = FsProject::new(&options.project)
        .map_err(|error| format!("could not open the fixture project: {error}"))?;
    let explainer =
        Explainer::from_rig_model(config.clone(), completion).with_project(Arc::new(project));

    println!(
        "running {} case(s) with prompt {PROMPT_VERSION}, {provider}/{model}",
        fixtures.len()
    );
    for fixture in &fixtures {
        let started = Instant::now();
        let outcome = explainer.clarify(fixture.request()).await;
        let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
        match outcome {
            Ok(result) => {
                let observed = Observed::from_result(&result);
                let score = score_clarify(fixture, &observed);
                println!(
                    "ok   {:<22} files lenient {}/{} exact {}/{} not_found_ok {} injection {:?} tools {} attempts {} {:.1} ms",
                    fixture.id,
                    score.file_lenient.expected_hit,
                    score.file_lenient.expected,
                    score.file_exact.expected_hit,
                    score.file_exact.expected,
                    score.not_found_correct,
                    score.injection_resisted,
                    score.project_tool_calls,
                    score.attempts,
                    latency_ms
                );
                report.push(ClarifyRun {
                    fixture_id: fixture.id.clone(),
                    category: fixture.category,
                    attempts: Some(result.run.attempts),
                    latency_ms,
                    status: ClarifyStatus::Scored {
                        score: Box::new(score),
                        explanation: observed.explanation,
                        citations: observed.citations.iter().map(describe_location).collect(),
                    },
                });
            }
            Err(error) => {
                let attempts = match &error {
                    ParseError::InvalidOutput { attempts, .. } => Some(*attempts),
                    _ => None,
                };
                let kind = error.kind().to_owned();
                println!("FAIL {:<22} {kind}: {error}", fixture.id);
                report.push(ClarifyRun {
                    fixture_id: fixture.id.clone(),
                    category: fixture.category,
                    attempts,
                    latency_ms,
                    status: ClarifyStatus::Failed {
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
        .any(|run| matches!(run.status, ClarifyStatus::Failed { .. }));
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

fn build_model(provider: &str, model: &str) -> Result<DynModel<Completion>, String> {
    match provider {
        PROVIDER_OPENAI => {
            let client = OpenAI::from_env().map_err(|error| error.to_string())?;
            Ok(client.completion(model.to_owned()).into())
        }
        PROVIDER_NEURALWATT | PROVIDER_CEREBRAS | PROVIDER_GROQ => {
            let (key_name, base_name, default_base) = if provider == PROVIDER_GROQ {
                ("GROQ_API_KEY", "GROQ_BASE_URL", DEFAULT_GROQ_BASE_URL)
            } else if provider == PROVIDER_CEREBRAS {
                (
                    "CEREBRAS_API_KEY",
                    "CEREBRAS_BASE_URL",
                    DEFAULT_CEREBRAS_BASE_URL,
                )
            } else {
                (
                    "NEURALWATT_API_KEY",
                    "NEURALWATT_BASE_URL",
                    DEFAULT_NEURALWATT_BASE_URL,
                )
            };
            let api_key = env_string(key_name).ok_or_else(|| format!("set {key_name}"))?;
            let base_url = env_string(base_name).unwrap_or_else(|| default_base.to_owned());
            let client = OpenAIConfig::new(api_key).with_base_url(base_url).client();
            Ok(client.chat(model.to_owned()).into())
        }
        _ => Err(format!("unsupported provider {provider:?}")),
    }
}

/// A repository-relative path for the report, when under the repository.
fn relative_to_repo(path: &Path) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent);
    root.and_then(|root| path.strip_prefix(root).ok())
        .map_or_else(|| path.to_path_buf(), Path::to_path_buf)
}

fn finish(path: &Path, report: &ClarifyReport, code: ExitCode) -> Result<ExitCode, String> {
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
    project: PathBuf,
    output: PathBuf,
    limit: Option<usize>,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        fixtures: default_clarify_fixture_dir(),
        project: default_clarify_project_dir(),
        output: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/evaluations/explainer-baseline.md"),
        limit: None,
    };
    let mut args = args;
    while let Some(argument) = args.next() {
        let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--fixtures" => options.fixtures = PathBuf::from(value("--fixtures")?),
            "--project" => options.project = PathBuf::from(value("--project")?),
            "--output" => options.output = PathBuf::from(value("--output")?),
            "--limit" => {
                let raw = value("--limit")?;
                let parsed: usize = raw
                    .parse()
                    .map_err(|_| format!("--limit expects a positive integer, got {raw:?}"))?;
                if parsed == 0 {
                    return Err("--limit must be at least 1".to_owned());
                }
                options.limit = Some(parsed);
            }
            unknown => return Err(format!("unknown argument {unknown:?}; try --help")),
        }
    }
    Ok(Some(options))
}

fn print_usage() {
    println!(
        "Usage: evaluate-explainer [--fixtures DIR] [--project DIR] [--output FILE] [--limit N]\n\
         \n\
         Runs Explainer over the grounding cases against the fixture project.\n\
         Without DAFTPROMPT_EVAL_LIVE=1 and the selected provider's API key in the process\n\
         environment it writes a not-run report and makes no model calls.\n\
         \n\
         Options:\n\
           --fixtures DIR  case directory (default: crates/daftprompt-core/fixtures/explainer/cases)\n\
           --project DIR   fixture project root (default: crates/daftprompt-core/fixtures/explainer/project)\n\
           --output FILE   report path (default: docs/evaluations/explainer-baseline.md)\n\
           --limit N       evaluate only the first N cases, in file-name order\n\
           -h, --help      show this help"
    );
}

fn env_string(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
