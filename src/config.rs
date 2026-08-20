use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use clap::Args as ClapArgs;
use daftprompt_acp::AdapterLaunchProfile;
use daftprompt_prompt_builder::{SelectionBudget, SourceQuota};

use crate::coordinator::CoordinatorConfig;

#[derive(ClapArgs)]
pub struct AdapterArgs {
    /// Adapter executable path or command name.
    #[arg(long, default_value = "codex-acp")]
    pub adapter: PathBuf,

    /// Extra arguments passed to the adapter process.
    #[arg(long)]
    pub adapter_args: Vec<String>,

    /// Environment variables to pass to the adapter (KEY=VALUE format).
    #[arg(long)]
    pub adapter_env: Vec<String>,

    /// Request timeout in seconds.
    #[arg(long, default_value = "60")]
    pub request_timeout: u64,

    /// Shutdown grace period in seconds.
    #[arg(long, default_value = "10")]
    pub shutdown_grace: u64,
}

pub struct DaftpromptConfig {
    pub adapter: AdapterConfig,
    pub retrieval: RetrievalConfig,
    pub trace: TraceConfig,
}

pub struct AdapterConfig {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub env_overrides: BTreeMap<String, String>,
    pub request_timeout_secs: u64,
    pub shutdown_grace_secs: u64,
}

pub struct RetrievalConfig {
    pub limit_per_source: usize,
    pub total_char_budget: usize,
    pub per_excerpt_char_limit: usize,
}

pub struct TraceConfig {
    pub db_path: Option<PathBuf>,
}

impl Default for DaftpromptConfig {
    fn default() -> Self {
        Self {
            adapter: AdapterConfig {
                executable: PathBuf::from("codex-acp"),
                args: Vec::new(),
                env_overrides: BTreeMap::new(),
                request_timeout_secs: 60,
                shutdown_grace_secs: 10,
            },
            retrieval: RetrievalConfig {
                limit_per_source: 10,
                total_char_budget: 8_000,
                per_excerpt_char_limit: 1_500,
            },
            trace: TraceConfig { db_path: None },
        }
    }
}

impl DaftpromptConfig {
    pub fn from_args(adapter_args: &AdapterArgs) -> Self {
        let mut env_overrides = BTreeMap::new();
        for entry in &adapter_args.adapter_env {
            if let Some((key, value)) = entry.split_once('=') {
                env_overrides.insert(key.to_string(), value.to_string());
            } else {
                log::warn!("Ignoring malformed --adapter-env entry (expected KEY=VALUE): {entry}");
            }
        }

        let defaults = Self::default();
        Self {
            adapter: AdapterConfig {
                executable: adapter_args.adapter.clone(),
                args: adapter_args.adapter_args.clone(),
                env_overrides,
                request_timeout_secs: adapter_args.request_timeout,
                shutdown_grace_secs: adapter_args.shutdown_grace,
            },
            retrieval: defaults.retrieval,
            trace: defaults.trace,
        }
    }

    pub fn to_launch_profile(&self) -> AdapterLaunchProfile {
        let mut profile = AdapterLaunchProfile::new(&self.adapter.executable);
        profile = profile.args(&self.adapter.args);
        profile = profile.envs(self.adapter.env_overrides.clone());
        profile
    }

    pub fn to_coordinator_config(&self) -> CoordinatorConfig {
        CoordinatorConfig {
            retrieval_limit_per_source: self.retrieval.limit_per_source,
            budget: SelectionBudget {
                total_char_budget: self.retrieval.total_char_budget,
                per_excerpt_char_limit: self.retrieval.per_excerpt_char_limit,
                per_source_quota: SourceQuota::default(),
            },
            request_timeout: Duration::from_secs(self.adapter.request_timeout_secs),
            shutdown_grace: self.shutdown_grace_duration(),
        }
    }

    pub fn shutdown_grace_duration(&self) -> Duration {
        Duration::from_secs(self.adapter.shutdown_grace_secs)
    }
}

/// Check if an adapter executable can be found. For absolute paths, verifies
/// the file exists. For relative names (like "codex-acp"), checks if the
/// command is available on PATH by attempting a dry-run spawn.
pub fn check_adapter_executable(executable: &std::path::Path) -> Result<(), String> {
    if executable.is_absolute() {
        if executable.is_file() {
            Ok(())
        } else {
            Err(format!(
                "Adapter executable '{}' not found. Verify the path or specify a different one with --adapter.",
                executable.display()
            ))
        }
    } else {
        // For relative names, try to locate on PATH via `which` or similar.
        match std::process::Command::new(executable).arg("--version").output() {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!(
                "Adapter executable '{}' not found on PATH. Install it or specify the full path with --adapter.",
                executable.display()
            )),
            Err(_) => {
                // Command exists but --version may have failed; that's fine.
                Ok(())
            }
        }
    }
}
