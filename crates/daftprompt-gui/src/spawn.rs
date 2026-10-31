//! Starts the compiled `daftprompt-api` binary as a child process.
//!
//! The GUI never links the API or core crates; it only runs the executable.
//! The decision logic and path resolution are pure functions so they can be
//! tested without a subprocess. [`ApiProcess`] owns the child and kills and
//! waits for it when dropped, which the normal window-close path does.

use std::io;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

/// The address the API binds to by default; keep in step with `api::DEFAULT_API_URL`.
pub const DEFAULT_API_ADDR: &str = "127.0.0.1:8080";

/// How long the startup probe waits for a connection. Loopback refusals are
/// immediate, so this only bounds an unusual hang.
const PROBE_TIMEOUT: Duration = Duration::from_millis(200);

/// What to do about the API at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnDecision {
    /// Do not spawn; the reason is shown nowhere unless useful.
    Skip(SkipReason),
    /// Spawn the API binary.
    Spawn,
}

/// Why the GUI does not start the API itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// `DAFTPROMPT_API_URL` is set, so the user chose their own endpoint.
    CustomUrl,
    /// `DAFTPROMPT_GUI_SPAWN_API=0`.
    Disabled,
    /// Something already listens on the default address.
    AlreadyListening,
}

/// Decide whether to spawn from the environment values and a listening probe.
pub fn decide(
    api_url: Option<&str>,
    spawn_flag: Option<&str>,
    already_listening: bool,
) -> SpawnDecision {
    if api_url.is_some_and(|value| !value.trim().is_empty()) {
        SpawnDecision::Skip(SkipReason::CustomUrl)
    } else if spawn_flag.is_some_and(|value| value.trim() == "0") {
        SpawnDecision::Skip(SkipReason::Disabled)
    } else if already_listening {
        SpawnDecision::Skip(SkipReason::AlreadyListening)
    } else {
        SpawnDecision::Spawn
    }
}

/// Locate the API binary: `DAFTPROMPT_API_BIN` if set, otherwise
/// `daftprompt-api` in the directory of the GUI executable (cargo's target dir).
pub fn resolve_binary(bin_env: Option<&str>, current_exe: &Path) -> PathBuf {
    if let Some(path) = bin_env.map(str::trim).filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    let name = format!("daftprompt-api{}", std::env::consts::EXE_SUFFIX);
    match current_exe.parent() {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}

/// Whether something accepts connections on the default API address.
pub fn is_listening() -> bool {
    match DEFAULT_API_ADDR.parse::<SocketAddr>() {
        Ok(addr) => TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok(),
        Err(_) => false,
    }
}

/// The result of the startup attempt, for display.
pub enum Startup {
    /// The GUI did not start an API (custom URL, disabled, or already running).
    NotManaged,
    /// The child is running; dropping the guard stops it.
    Spawned(ApiProcess),
    /// The binary could not be started.
    Failed(String),
}

/// Apply the environment, probe the default address, and spawn if needed.
pub fn start_from_env() -> Startup {
    let api_url = std::env::var("DAFTPROMPT_API_URL").ok();
    let spawn_flag = std::env::var("DAFTPROMPT_GUI_SPAWN_API").ok();
    // Skip the probe when the decision does not depend on it.
    let listening = decide(api_url.as_deref(), spawn_flag.as_deref(), false)
        == SpawnDecision::Spawn
        && is_listening();
    if decide(api_url.as_deref(), spawn_flag.as_deref(), listening) != SpawnDecision::Spawn {
        return Startup::NotManaged;
    }

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            return Startup::Failed(format!("could not locate the GUI executable: {error}"));
        }
    };
    let binary = resolve_binary(
        std::env::var("DAFTPROMPT_API_BIN").ok().as_deref(),
        &current_exe,
    );
    match ApiProcess::spawn(&binary) {
        Ok(process) => Startup::Spawned(process),
        Err(error) => Startup::Failed(missing_binary_message(&binary, &error)),
    }
}

fn missing_binary_message(binary: &Path, error: &io::Error) -> String {
    format!(
        "could not start the API ({}): {error}. Run `cargo build -p daftprompt-api` first, \
         set DAFTPROMPT_API_BIN, or start the API yourself.",
        binary.display()
    )
}

/// A running child API process, stopped when dropped.
pub struct ApiProcess {
    child: Child,
}

impl ApiProcess {
    /// Spawn `binary`, inheriting the environment, working directory, and
    /// stdio so the API's own `.env` loading and logs behave as in a terminal.
    pub fn spawn(binary: &Path) -> io::Result<Self> {
        let child = Command::new(binary).spawn()?;
        Ok(Self { child })
    }

    /// A description of an early exit, or `None` while the child still runs.
    pub fn exit_note(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(format!(
                "the API process exited ({status}); see the terminal output, for example for a missing API key"
            )),
            Ok(None) => None,
            Err(error) => Some(format!("could not check the API process: {error}")),
        }
    }
}

impl Drop for ApiProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_only_when_unconfigured_and_nothing_listens() {
        assert_eq!(decide(None, None, false), SpawnDecision::Spawn);
        assert_eq!(decide(None, Some("1"), false), SpawnDecision::Spawn);
        assert_eq!(decide(Some(" "), None, false), SpawnDecision::Spawn);
    }

    #[test]
    fn a_custom_url_disables_spawning() {
        assert_eq!(
            decide(Some("http://host:1/parse"), None, false),
            SpawnDecision::Skip(SkipReason::CustomUrl)
        );
    }

    #[test]
    fn the_flag_zero_disables_spawning() {
        assert_eq!(
            decide(None, Some("0"), false),
            SpawnDecision::Skip(SkipReason::Disabled)
        );
    }

    #[test]
    fn an_existing_listener_is_reused() {
        assert_eq!(
            decide(None, None, true),
            SpawnDecision::Skip(SkipReason::AlreadyListening)
        );
    }

    #[test]
    fn binary_override_wins() {
        let exe = Path::new("/work/target/debug/daftprompt-gui");
        assert_eq!(
            resolve_binary(Some(" /opt/api "), exe),
            PathBuf::from("/opt/api")
        );
    }

    #[test]
    fn binary_defaults_next_to_the_gui_executable() {
        let exe = Path::new("/work/target/debug/daftprompt-gui");
        let expected = Path::new("/work/target/debug")
            .join(format!("daftprompt-api{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(resolve_binary(None, exe), expected);
        assert_eq!(resolve_binary(Some("  "), exe), expected);
    }

    #[test]
    fn default_address_matches_the_default_url() {
        assert_eq!(
            crate::api::DEFAULT_API_URL,
            format!("http://{DEFAULT_API_ADDR}/parse")
        );
    }
}
