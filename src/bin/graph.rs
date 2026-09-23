use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

/// Build and search the document provenance graph.
#[derive(Debug, Parser)]
#[command(name = "graph", version, about, after_help = "Model: Xiaomi MiMo V2.5 Pro via llm-sdk. Set XIAOMI_API_KEY in the environment or a .env file in the current directory.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Extract and store provenance claims from selected repository documents.
    Build(BuildArgs),
    /// Ask a question using the most recently completed provenance graph build.
    Search(SearchArgs),
}

#[derive(Debug, Args)]
struct BuildArgs {
    /// Repository root (defaults to the current directory).
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// File path or quoted repo-relative glob to include; may be repeated.
    #[arg(long = "source", required = true)]
    sources: Vec<String>,
}

#[derive(Debug, Args)]
struct SearchArgs {
    /// Repository root (defaults to the current directory).
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Natural-language question to ask about the repository.
    question: String,

    /// Print the answer as machine-readable JSON.
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if Path::new(".env").is_file() {
        dotenvy::from_path(".env")?;
    }
    match cli.command {
        Command::Build(args) => {
            let sources = daftprompt_indexer::graph::resolve_sources(&args.repo, &args.sources)?;
            let config = daftprompt_indexer::IndexerConfig::default();
            let db = daftprompt_indexer::graph::open_database(&args.repo, &config)?;
            daftprompt_indexer::graph::init_schema(&db)?;
            let adapter = daftprompt_indexer::graph::model::XiaomiGraphAdapter::from_env()?;
            let build_id = daftprompt_indexer::graph::build(
                &db,
                &args.repo,
                &sources,
                &adapter,
                "graph-extraction-v1",
            )?;
            println!("completed graph build {build_id}");
            Ok(())
        }
        Command::Search(args) => {
            let repo = args.repo.canonicalize()?;
            ensure_query_logs_ignored(&repo)?;
            let config = daftprompt_indexer::IndexerConfig::default();
            let result = (|| -> Result<_> {
                let db = daftprompt_indexer::graph::open_database(&repo, &config)?;
                daftprompt_indexer::graph::init_schema(&db)?;
                let adapter = daftprompt_indexer::graph::model::XiaomiGraphAdapter::from_env()?;
                let prepared = daftprompt_indexer::graph::prepare_search(&db, &args.question, &adapter, 20)?;
                let resolution = daftprompt_indexer::graph::resolve_prepared(&db, &prepared)?;
                daftprompt_indexer::graph::render_answer(&db, &prepared, resolution)
            })();

            let (status, build_id, response, answer, search_error) = match result {
                Ok(answer) => {
                    let response = if args.json {
                        serde_json::to_string_pretty(&answer)?
                    } else {
                        daftprompt_indexer::graph::render_text(&answer)
                    };
                    (answer.outcome.clone(), answer.build_id.clone(), response, Some(answer), None)
                }
                Err(error) => (
                    "error".to_owned(), latest_build_id(&repo).unwrap_or_default(),
                    format!("Error: {}", concise_error(&error)), None, Some(concise_error(&error)),
                ),
            };
            println!("{response}");
            let log = serde_json::json!({
                "schema_version": 1,
                "timestamp_utc": utc_timestamp(),
                "repository_path": repo.to_string_lossy(),
                "build_id": build_id,
                "query": args.question,
                "status": status.clone(),
                "response": response.clone(),
                "citations": answer.as_ref().map(|a| a.citations.iter().map(|c| serde_json::json!({
                    "source_path": c.source_path,
                    "start_line": c.start_line,
                    "end_line": c.end_line,
                })).collect::<Vec<_>>()).unwrap_or_default(),
                "claim_ids": answer.as_ref().map(|a| a.claim_ids.clone()).unwrap_or_default(),
                "error": search_error.clone(),
            });
            if let Err(error) = write_query_log(&repo, &log) {
                anyhow::bail!("could not write graph search log: {error}");
            }
            if let Some(error) = search_error { anyhow::bail!("{error}"); }
            Ok(())
        }
    }
}

fn ensure_query_logs_ignored(repo: &Path) -> Result<()> {
    let checked = ProcessCommand::new("git")
        .arg("-C").arg(repo)
        .args(["check-ignore", "-q", "--", ".daftprompt/graph-search/query.json"])
        .status();
    match checked {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => anyhow::bail!(
            "query logs are not Git-ignored in {}. Add `/.daftprompt/graph-search/` to that repository's .gitignore before searching",
            repo.display()
        ),
        Err(error) => anyhow::bail!("could not verify Git ignore status for {}: {error}", repo.display()),
    }
}

static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn write_query_log(repo: &Path, log: &serde_json::Value) -> Result<()> {
    let directory = repo.join(".daftprompt/graph-search");
    fs::create_dir_all(&directory)?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let sequence = LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!("{}-{}-{sequence}.json", nanos, std::process::id());
    let target = directory.join(name);
    let temporary = directory.join(format!(".{}.tmp", target.file_name().unwrap().to_string_lossy()));
    let bytes = serde_json::to_vec_pretty(log)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    fs::rename(&temporary, &target)?;
    Ok(())
}

fn utc_timestamp() -> String {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let days = (duration.as_secs() / 86_400) as i64;
    let seconds = duration.as_secs() % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:09}Z",
        seconds / 3600, seconds / 60 % 60, seconds % 60, duration.subsec_nanos())
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn latest_build_id(repo: &Path) -> Option<String> {
    let config = daftprompt_indexer::IndexerConfig::default();
    let db = daftprompt_indexer::graph::open_database(repo, &config).ok()?;
    db.query_row(
        "SELECT id FROM graph_builds WHERE state='completed' ORDER BY completed_at DESC, rowid DESC LIMIT 1",
        [], |row| row.get(0),
    ).ok()
}

fn concise_error(error: &anyhow::Error) -> String {
    // Display only the top-level message; chained transport errors can contain
    // request details, so they are deliberately excluded from local logs.
    error.to_string()
}

#[cfg(test)]
mod query_log_tests {
    use super::*;

    fn temporary_repo(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "daftprompt-graph-log-{label}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        assert!(ProcessCommand::new("git").arg("-C").arg(&path).args(["init", "-q"]).status().unwrap().success());
        path
    }

    #[test]
    fn writes_distinct_complete_utf8_logs_for_every_outcome() {
        let repo = temporary_repo("outcomes");
        fs::write(repo.join(".gitignore"), "/.daftprompt/graph-search/\n").unwrap();
        ensure_query_logs_ignored(&repo).unwrap();

        for (status, response) in [
            ("yes", "Yes, supported."),
            ("no", "No, explicitly excluded."),
            ("unknown", "Unknown, missing evidence."),
            ("error", "Error: fixture failed."),
        ] {
            let log = serde_json::json!({
                "schema_version": 1,
                "timestamp_utc": utc_timestamp(),
                "repository_path": repo.to_string_lossy(),
                "build_id": "build-fixture",
                "query": "fixture query",
                "status": status,
                "response": response,
                "citations": [],
                "claim_ids": [],
                "error": if status == "error" { Some("fixture failed") } else { None },
            });
            write_query_log(&repo, &log).unwrap();
        }

        let paths = fs::read_dir(repo.join(".daftprompt/graph-search")).unwrap()
            .map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
        assert_eq!(paths.len(), 4);
        let mut statuses = Vec::new();
        for path in paths {
            let bytes = fs::read(path).unwrap();
            let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert!(std::str::from_utf8(&bytes).is_ok());
            for field in ["schema_version", "timestamp_utc", "repository_path", "build_id", "query", "status", "response", "citations", "claim_ids", "error"] {
                assert!(parsed.get(field).is_some(), "missing {field}");
            }
            statuses.push(parsed["status"].as_str().unwrap().to_owned());
        }
        statuses.sort();
        assert_eq!(statuses, ["error", "no", "unknown", "yes"]);
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn refuses_repository_without_ignore_rule_with_actionable_message() {
        let repo = temporary_repo("no-ignore");
        let error = ensure_query_logs_ignored(&repo).unwrap_err().to_string();
        assert!(error.contains("Add `/.daftprompt/graph-search/`"));
        assert!(!repo.join(".daftprompt/graph-search").exists());
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn reports_query_log_write_failure() {
        let repo = temporary_repo("write-failure");
        fs::write(repo.join(".daftprompt"), "blocks directory creation").unwrap();
        let error = write_query_log(&repo, &serde_json::json!({"status": "yes"})).unwrap_err();
        assert!(!error.to_string().is_empty());
        fs::remove_dir_all(repo).unwrap();
    }
}
