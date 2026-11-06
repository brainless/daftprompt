//! Binary entry point for the HTTP service.
//!
//! Provider credentials and model settings come from the process environment;
//! the server binds to loopback by default. See `docs/api.md` for the full
//! variable list and a sample request. A `.env` file in the working directory
//! is loaded first; real environment variables take precedence over it.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::llm::{
    DEFAULT_PROVIDER, ModelSettings, build_model, default_model, env_string,
};
use daftprompt_core::project_tools::FsProject;
use daftprompt_core::{
    Briefer, Chronicler, ChroniclerConfig, Clarifier, DEFAULT_MAX_ATTEMPTS,
    DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_SEGMENTS, DEFAULT_TIMEOUT, Explainer, ExplainerConfig,
    Parser, Scribe, ScribeConfig, Store,
};

/// Loopback only: the experiment has no authentication and no persistence.
const DEFAULT_ADDR: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("daftprompt-api: {error}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<(), String> {
    load_dotenv()?;
    let settings = Settings::from_env()?;

    let completion_model = build_model(&ModelSettings::new(
        settings.provider.clone(),
        settings.model.clone(),
    ))
    .map_err(|error| error.to_string())?;

    let scribe_config = ScribeConfig::new()
        .with_provider(settings.provider.clone())
        .with_model(settings.model.clone())
        .with_input_limits(settings.max_input_bytes, settings.max_segments)
        .with_max_attempts(settings.max_attempts)
        .with_timeout(settings.timeout);
    let parser: Arc<dyn Parser> = Arc::new(Scribe::from_rig_model(scribe_config, completion_model));

    // The Explainer has its own provider and model; credentials are shared by
    // provider name (e.g. NEURALWATT_API_KEY serves both agents).
    let explainer_config = ExplainerConfig::from_env()
        .with_input_limits(settings.max_input_bytes, settings.max_segments);
    let explainer_provider = explainer_config
        .provider
        .clone()
        .unwrap_or_else(|| DEFAULT_PROVIDER.to_owned());
    let explainer_model = explainer_config.model.clone().unwrap_or_default();
    let explainer_completion = build_model(&ModelSettings::new(
        explainer_provider.clone(),
        explainer_model.clone(),
    ))
    .map_err(|error| error.to_string())?;
    let mut explainer = Explainer::from_rig_model(explainer_config, explainer_completion);
    let grounding = match &settings.project_root {
        Some(root) => {
            let project = FsProject::new(root).map_err(|error| {
                format!(
                    "DAFTPROMPT_PROJECT_ROOT {} is unusable: {error}",
                    root.display()
                )
            })?;
            explainer = explainer.with_project(Arc::new(project));
            format!(
                "project (default root {}, used when a request has no project_id)",
                root.display()
            )
        }
        None => "text_only".to_owned(),
    };
    let clarifier: Arc<dyn Clarifier> = Arc::new(explainer);

    // Chronicler (git history) has its own provider and model too; the
    // credentials are shared by provider name.
    let chronicler_config = ChroniclerConfig::from_env();
    let chronicler_settings = chronicler_config.model_settings();
    let chronicler_completion =
        build_model(&chronicler_settings).map_err(|error| error.to_string())?;
    let chronicler: Arc<dyn Briefer> = Arc::new(Chronicler::from_rig_model(
        chronicler_config,
        chronicler_completion,
    ));

    let api_config = ApiConfig::for_input_limit(settings.max_input_bytes);
    let db_location = settings
        .db_path
        .clone()
        .or_else(daftprompt_core::default_db_path);
    let store = match &settings.db_path {
        Some(path) => Store::open(path),
        None => Store::open_default(),
    }
    .map_err(|error| format!("could not open the database: {error}"))?;
    let app = router(
        AppState::new(parser, api_config)
            .with_clarifier(clarifier)
            .with_chronicler(chronicler)
            .with_store(Arc::new(store)),
    );

    let listener = tokio::net::TcpListener::bind(settings.addr)
        .await
        .map_err(|error| format!("could not bind {}: {error}", settings.addr))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("could not read the bound address: {error}"))?;

    println!(
        "daftprompt-api listening on http://{address} (provider {}, model {}, \
         input limit {} bytes / {} segments, {} attempt(s), timeout {}s)",
        settings.provider,
        settings.model,
        settings.max_input_bytes,
        settings.max_segments,
        settings.max_attempts.max(1),
        settings.timeout.as_secs()
    );

    println!(
        "daftprompt-api POST /clarify: explainer provider {explainer_provider}, model \
         {explainer_model}, grounding {grounding}"
    );

    println!(
        "daftprompt-api POST /projects/{{id}}/history: chronicler provider {}, model {}",
        chronicler_settings.provider, chronicler_settings.model
    );

    if let Some(path) = &db_location {
        println!("daftprompt-api database: {}", path.display());
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("the HTTP server stopped: {error}"))
}

/// Load `.env` from the working directory (or a parent) if one exists.
///
/// A missing file is fine, variables already set in the process environment
/// win over the file, and a malformed file is a startup error.
fn load_dotenv() -> Result<(), String> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(error) if error.not_found() => Ok(()),
        Err(error) => Err(format!("could not load .env: {error}")),
    }
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("daftprompt-api: could not listen for Ctrl+C: {error}");
    }
}

/// Process settings, all overridable with environment variables.
struct Settings {
    addr: SocketAddr,
    provider: String,
    model: String,
    max_input_bytes: usize,
    max_segments: usize,
    max_attempts: u32,
    timeout: Duration,
    project_root: Option<PathBuf>,
    /// `DAFTPROMPT_DB_PATH`; unset means the per-user default location.
    db_path: Option<PathBuf>,
}

impl Settings {
    fn from_env() -> Result<Self, String> {
        let addr = env_string("DAFTPROMPT_API_ADDR").unwrap_or_else(|| DEFAULT_ADDR.to_owned());
        let addr = addr
            .parse()
            .map_err(|error| format!("DAFTPROMPT_API_ADDR expects an IP:port address: {error}"))?;

        let provider =
            env_string("DAFTPROMPT_API_PROVIDER").unwrap_or_else(|| DEFAULT_PROVIDER.to_owned());

        let project_root = match env_string("DAFTPROMPT_PROJECT_ROOT") {
            Some(value) => {
                let path = PathBuf::from(&value);
                if !path.is_dir() {
                    return Err(format!(
                        "DAFTPROMPT_PROJECT_ROOT {value:?} does not exist or is not a directory"
                    ));
                }
                Some(path)
            }
            None => None,
        };

        Ok(Self {
            project_root,
            db_path: env_string("DAFTPROMPT_DB_PATH").map(PathBuf::from),
            addr,
            model: env_string("DAFTPROMPT_API_MODEL")
                .unwrap_or_else(|| default_model(&provider).to_owned()),
            provider,
            max_input_bytes: env_usize("DAFTPROMPT_API_MAX_INPUT_BYTES", DEFAULT_MAX_INPUT_BYTES)?,
            max_segments: env_usize("DAFTPROMPT_API_MAX_SEGMENTS", DEFAULT_MAX_SEGMENTS)?,
            max_attempts: env_u32("DAFTPROMPT_API_MAX_ATTEMPTS", DEFAULT_MAX_ATTEMPTS)?,
            timeout: Duration::from_secs(env_u64(
                "DAFTPROMPT_API_TIMEOUT_SECS",
                DEFAULT_TIMEOUT.as_secs(),
            )?),
        })
    }
}

fn env_usize(name: &str, default: usize) -> Result<usize, String> {
    match env_string(name) {
        Some(value) => value
            .parse()
            .map_err(|_| format!("{name} expects a non-negative integer, got {value:?}")),
        None => Ok(default),
    }
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
