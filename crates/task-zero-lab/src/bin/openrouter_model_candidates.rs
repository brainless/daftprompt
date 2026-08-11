use std::cmp::Ordering;

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use regex::Regex;
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
const PER_TOKEN_TO_PER_MILLION: f64 = 1_000_000.0;

/// Find reproducible OpenRouter candidates for the Epic 014 helper experiment.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Maximum context window in tokens.
    #[arg(long, default_value_t = 131_072)]
    max_context: u64,

    /// Minimum prompt and completion price, in USD per million tokens.
    #[arg(long, default_value_t = 0.0)]
    min_price: f64,

    /// Maximum prompt and completion price, in USD per million tokens.
    #[arg(long, default_value_t = 0.1)]
    max_price: f64,

    /// Largest inferred total parameter count, in billions.
    #[arg(long, default_value_t = 20.0)]
    max_parameters_b: f64,

    /// Keep models whose parameter count cannot be inferred from public text.
    #[arg(long)]
    include_unknown_parameters: bool,

    /// Do not require a non-empty Hugging Face ID as evidence of public weights.
    #[arg(long)]
    allow_missing_hugging_face_id: bool,

    /// Require this OpenRouter request parameter (repeatable).
    #[arg(long, default_value = "response_format")]
    require_parameter: Vec<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,

    /// Models API URL, primarily for an archived response server in reproducible replays.
    #[arg(long, default_value = MODELS_URL)]
    api_url: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

#[derive(Debug, Deserialize)]
struct Model {
    id: String,
    canonical_slug: String,
    hugging_face_id: Option<String>,
    name: String,
    description: String,
    context_length: Option<u64>,
    architecture: Architecture,
    pricing: Pricing,
    supported_parameters: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Architecture {
    input_modalities: Vec<String>,
    output_modalities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Pricing {
    prompt: String,
    completion: String,
}

#[derive(Debug, Serialize)]
struct Candidate {
    id: String,
    canonical_slug: String,
    hugging_face_id: String,
    name: String,
    context_length: u64,
    prompt_usd_per_million: f64,
    completion_usd_per_million: f64,
    inferred_parameters_b: Option<f64>,
    parameter_evidence: Option<String>,
    supported_parameters: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Report<'a> {
    source_url: &'a str,
    filters: ReportFilters<'a>,
    candidates: Vec<Candidate>,
    suggested_three: Vec<Suggestion>,
    limitations: [&'a str; 2],
}

#[derive(Debug, Serialize)]
struct ReportFilters<'a> {
    exact_input_modalities: [&'a str; 1],
    exact_output_modalities: [&'a str; 1],
    max_context: u64,
    min_prompt_and_completion_usd_per_million: f64,
    max_prompt_and_completion_usd_per_million: f64,
    max_inferred_parameters_b: f64,
    require_hugging_face_id: bool,
    required_parameters: &'a [String],
}

#[derive(Debug, Serialize)]
struct Suggestion {
    tier: &'static str,
    id: String,
    inferred_parameters_b: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    let models = fetch_models(&args).await?;
    let mut candidates = filter_models(models, &args)?;
    candidates.sort_by(candidate_order);
    let suggested_three = suggest_three(&candidates);
    let report = Report {
        source_url: &args.api_url,
        filters: ReportFilters {
            exact_input_modalities: ["text"],
            exact_output_modalities: ["text"],
            max_context: args.max_context,
            min_prompt_and_completion_usd_per_million: args.min_price,
            max_prompt_and_completion_usd_per_million: args.max_price,
            max_inferred_parameters_b: args.max_parameters_b,
            require_hugging_face_id: !args.allow_missing_hugging_face_id,
            required_parameters: &args.require_parameter,
        },
        suggested_three,
        candidates,
        limitations: [
            "OpenRouter does not return parameter count; inferred_parameters_b is parsed from the model ID, name, Hugging Face ID, and description, using the largest B/billion value as the conservative total.",
            "A Hugging Face ID is discovery evidence, not proof of an open-weight license; verify each suggested model card and license before recording the experiment.",
        ],
    };

    match args.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        OutputFormat::Table => print_table(&report),
    }
    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    if !args.min_price.is_finite() || !args.max_price.is_finite() || args.min_price < 0.0 {
        bail!("prices must be finite and non-negative");
    }
    if args.min_price > args.max_price {
        bail!("--min-price cannot exceed --max-price");
    }
    if !args.max_parameters_b.is_finite() || args.max_parameters_b <= 0.0 {
        bail!("--max-parameters-b must be finite and positive");
    }
    Ok(())
}

async fn fetch_models(args: &Args) -> Result<Vec<Model>> {
    let min = args.min_price.to_string();
    let max = args.max_price.to_string();
    let response = reqwest::Client::new()
        .get(&args.api_url)
        .query(&[
            ("input_modalities", "text"),
            ("output_modalities", "text"),
            ("min_price", min.as_str()),
            ("max_price", max.as_str()),
            ("min_output_price", min.as_str()),
            ("max_output_price", max.as_str()),
        ])
        .send()
        .await
        .with_context(|| format!("requesting {}", args.api_url))?
        .error_for_status()
        .context("OpenRouter models API returned an error")?;
    Ok(response
        .json::<ModelsResponse>()
        .await
        .context("decoding OpenRouter models response")?
        .data)
}

fn filter_models(models: Vec<Model>, args: &Args) -> Result<Vec<Candidate>> {
    let parameter_pattern = Regex::new(r"(?i)(\d+(?:\.\d+)?)\s*[-_ ]?(?:b|billion)\b")?;
    Ok(models
        .into_iter()
        .filter_map(|model| {
            if model.architecture.input_modalities.as_slice() != ["text"]
                || model.architecture.output_modalities.as_slice() != ["text"]
                || model.context_length? > args.max_context
                || args.require_parameter.iter().any(|required| {
                    !model
                        .supported_parameters
                        .iter()
                        .any(|value| value == required)
                })
            {
                return None;
            }
            let hugging_face_id = model.hugging_face_id.filter(|id| !id.trim().is_empty());
            if hugging_face_id.is_none() && !args.allow_missing_hugging_face_id {
                return None;
            }
            let prompt = price_per_million(&model.pricing.prompt).ok()?;
            let completion = price_per_million(&model.pricing.completion).ok()?;
            if !(args.min_price..=args.max_price).contains(&prompt)
                || !(args.min_price..=args.max_price).contains(&completion)
            {
                return None;
            }
            let evidence_text = format!(
                "{} | {} | {} | {}",
                model.id,
                model.name,
                hugging_face_id.as_deref().unwrap_or(""),
                model.description
            );
            let inferred = infer_parameter_count(&parameter_pattern, &evidence_text);
            if inferred.is_none() && !args.include_unknown_parameters {
                return None;
            }
            if inferred.is_some_and(|value| value > args.max_parameters_b) {
                return None;
            }
            Some(Candidate {
                id: model.id,
                canonical_slug: model.canonical_slug,
                hugging_face_id: hugging_face_id.unwrap_or_default(),
                name: model.name,
                context_length: model.context_length.unwrap_or_default(),
                prompt_usd_per_million: prompt,
                completion_usd_per_million: completion,
                inferred_parameters_b: inferred,
                parameter_evidence: inferred.map(|_| evidence_text),
                supported_parameters: model.supported_parameters,
            })
        })
        .collect())
}

fn price_per_million(value: &str) -> Result<f64> {
    let per_token: f64 = value
        .parse()
        .with_context(|| format!("invalid price {value:?}"))?;
    Ok(per_token * PER_TOKEN_TO_PER_MILLION)
}

fn infer_parameter_count(pattern: &Regex, text: &str) -> Option<f64> {
    pattern
        .captures_iter(text)
        .filter_map(|capture| capture[1].parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .max_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal))
}

fn candidate_order(left: &Candidate, right: &Candidate) -> Ordering {
    left.inferred_parameters_b
        .partial_cmp(&right.inferred_parameters_b)
        .unwrap_or(Ordering::Greater)
        .then_with(|| left.id.cmp(&right.id))
}

fn suggest_three(candidates: &[Candidate]) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();
    for (index, candidate) in candidates
        .iter()
        .filter(|candidate| {
            candidate
                .inferred_parameters_b
                .is_some_and(|size| size < 10.0)
        })
        .take(2)
        .enumerate()
    {
        suggestions.push(Suggestion {
            tier: if index == 0 {
                "below 10B (first)"
            } else {
                "below 10B (second)"
            },
            id: candidate.id.clone(),
            inferred_parameters_b: candidate.inferred_parameters_b.unwrap_or_default(),
        });
    }
    if let Some(candidate) = candidates.iter().find(|candidate| {
        candidate
            .inferred_parameters_b
            .is_some_and(|size| (10.0..20.0).contains(&size))
    }) {
        suggestions.push(Suggestion {
            tier: "sub-20B tier (>=10B, <20B)",
            id: candidate.id.clone(),
            inferred_parameters_b: candidate.inferred_parameters_b.unwrap_or_default(),
        });
    }
    suggestions
}

fn print_table(report: &Report<'_>) {
    println!("MODEL\tPARAMS_B_INFERRED\tCONTEXT\tPROMPT_$/M\tCOMPLETION_$/M\tHUGGING_FACE_ID");
    for model in &report.candidates {
        println!(
            "{}\t{}\t{}\t{:.6}\t{:.6}\t{}",
            model.id,
            model
                .inferred_parameters_b
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            model.context_length,
            model.prompt_usd_per_million,
            model.completion_usd_per_million,
            model.hugging_face_id,
        );
    }
    println!("\nSuggested tier coverage:");
    for suggestion in &report.suggested_three {
        println!(
            "- {}: {} ({}B inferred)",
            suggestion.tier, suggestion.id, suggestion.inferred_parameters_b
        );
    }
    for limitation in report.limitations {
        eprintln!("note: {limitation}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_largest_parameter_count_as_conservative_total() {
        let pattern = Regex::new(r"(?i)(\d+(?:\.\d+)?)\s*[-_ ]?(?:b|billion)\b").unwrap();
        assert_eq!(
            infer_parameter_count(&pattern, "104B total parameters and 7.4B active parameters"),
            Some(104.0)
        );
        assert_eq!(
            infer_parameter_count(&pattern, "Qwen3-4B Instruct"),
            Some(4.0)
        );
        assert_eq!(infer_parameter_count(&pattern, "no size metadata"), None);
    }

    #[test]
    fn converts_api_per_token_prices_to_per_million() {
        assert!((price_per_million("0.00000005").unwrap() - 0.05).abs() < f64::EPSILON);
        assert_eq!(price_per_million("0").unwrap(), 0.0);
    }

    #[test]
    fn suggestions_match_epic_two_below_ten_and_one_sub_twenty() {
        let candidate = |id: &str, size: f64| Candidate {
            id: id.to_string(),
            canonical_slug: id.to_string(),
            hugging_face_id: id.to_string(),
            name: id.to_string(),
            context_length: 32_768,
            prompt_usd_per_million: 0.05,
            completion_usd_per_million: 0.05,
            inferred_parameters_b: Some(size),
            parameter_evidence: None,
            supported_parameters: vec!["response_format".to_string()],
        };
        let candidates = vec![
            candidate("first-8b", 8.0),
            candidate("second-8b", 8.0),
            candidate("twelve-b", 12.0),
        ];

        let suggestions = suggest_three(&candidates);

        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0].id, "first-8b");
        assert_eq!(suggestions[1].id, "second-8b");
        assert_eq!(suggestions[2].id, "twelve-b");
    }
}
