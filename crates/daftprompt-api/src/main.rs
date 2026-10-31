//! Binary entry point for the HTTP service.
//!
//! Provider credentials and model settings come from the process environment;
//! the server binds to loopback by default. See `docs/api.md` for the full
//! variable list and a sample request. A `.env` file in the working directory
//! is loaded first; real environment variables take precedence over it.

use std::env;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_SEGMENTS, DEFAULT_TIMEOUT, Parser,
    Scribe, ScribeConfig,
};
use rig::DynModel;
use rig::operation::Completion;
use rig::providers::openai::{OpenAI, OpenAIConfig};

/// Loopback only: the experiment has no authentication and no persistence.
const DEFAULT_ADDR: &str = "127.0.0.1:8080";
const PROVIDER_OPENAI: &str = "openai";
const PROVIDER_NEURALWATT: &str = "neuralwatt";
const DEFAULT_PROVIDER: &str = PROVIDER_OPENAI;
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_NEURALWATT_MODEL: &str = "qwen3.6-35b-fast";
/// Neuralwatt's OpenAI-compatible base URL (<https://docs.neuralwatt.com/api/overview.md>).
const DEFAULT_NEURALWATT_BASE_URL: &str = "https://api.neuralwatt.com/v1";

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

    let completion_model = build_model(&settings.provider, &settings.model)?;

    let scribe_config = ScribeConfig::new()
        .with_provider(settings.provider.clone())
        .with_model(settings.model.clone())
        .with_input_limits(settings.max_input_bytes, settings.max_segments)
        .with_max_attempts(settings.max_attempts)
        .with_timeout(settings.timeout);
    let parser: Arc<dyn Parser> = Arc::new(Scribe::from_rig_model(scribe_config, completion_model));

    let api_config = ApiConfig::for_input_limit(settings.max_input_bytes);
    let app = router(AppState::new(parser, api_config));

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

/// Build the completion model for `provider`.
///
/// OpenAI uses `client.completion`, which follows Rig's default OpenAI route.
/// Neuralwatt is OpenAI-compatible but serves Chat Completions, so it uses the
/// explicit chat wire (`POST {base}/chat/completions`) rather than the
/// Responses API. Credentials are read from the environment here, never stored.
fn build_model(provider: &str, model: &str) -> Result<DynModel<Completion>, String> {
    match provider {
        PROVIDER_OPENAI => {
            // The provider client reads OPENAI_API_KEY and OPENAI_BASE_URL itself.
            let client = OpenAI::from_env().map_err(|error| {
                format!("provider credentials are unavailable (set OPENAI_API_KEY): {error}")
            })?;
            Ok(client.completion(model.to_owned()).into())
        }
        PROVIDER_NEURALWATT => {
            let api_key = env_string("NEURALWATT_API_KEY").ok_or_else(|| {
                "provider credentials are unavailable (set NEURALWATT_API_KEY)".to_owned()
            })?;
            let base_url = env_string("NEURALWATT_BASE_URL")
                .unwrap_or_else(|| DEFAULT_NEURALWATT_BASE_URL.to_owned());
            let client = OpenAIConfig::new(api_key).with_base_url(base_url).client();
            Ok(client.chat(model.to_owned()).into())
        }
        other => Err(format!(
            "provider {other:?} is not wired into this service; supported providers are \
             {PROVIDER_OPENAI:?} and {PROVIDER_NEURALWATT:?}"
        )),
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
}

impl Settings {
    fn from_env() -> Result<Self, String> {
        let addr = env_string("DAFTPROMPT_API_ADDR").unwrap_or_else(|| DEFAULT_ADDR.to_owned());
        let addr = addr
            .parse()
            .map_err(|error| format!("DAFTPROMPT_API_ADDR expects an IP:port address: {error}"))?;

        let provider =
            env_string("DAFTPROMPT_API_PROVIDER").unwrap_or_else(|| DEFAULT_PROVIDER.to_owned());

        Ok(Self {
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

fn default_model(provider: &str) -> &'static str {
    if provider == PROVIDER_NEURALWATT {
        DEFAULT_NEURALWATT_MODEL
    } else {
        DEFAULT_OPENAI_MODEL
    }
}

fn env_string(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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
