//! Adapter launch profiles.
//!
//! An [`AdapterLaunchProfile`] describes how to start an ACP adapter process
//! (executable path, argv, environment overrides) without ever going through
//! a shell. This is deliberately adapter-neutral: Epic 014 Design Decision #1
//! requires provider-specific executable discovery to live in a launch
//! profile, not in retrieval or session state, and the acceptance criteria
//! for Task 1 require "Executable path and arguments are configured without
//! shell evaluation."
//!
//! The Task 0 spike (`epics/research/014-acp-adapter-compatibility.md`, §2)
//! found that `node --import tsx <path>` resolves its `--import` loader
//! against the process's cwd, not the entry file's directory, and that the
//! chosen SDK's launch config has no `cwd` setter. `codex_dev` therefore
//! standardizes on codex-acp's own documented "run from sources" launch
//! command, `npm run start --prefix <dir>`, which lets npm own the cwd
//! switch internally without a shell.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use agent_client_protocol::{AcpAgent, AcpAgentConfig};

/// Configuration for launching an ACP adapter subprocess.
///
/// This never goes through a shell: the command and arguments are passed
/// directly to the OS process-creation call. There is intentionally no way
/// to construct this type from a single shell-style command string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterLaunchProfile {
    command: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

impl AdapterLaunchProfile {
    /// Start building a launch profile for the given executable or command
    /// name (resolved via `PATH` if not absolute, exactly like
    /// `std::process::Command::new`).
    #[must_use]
    pub fn new(command: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    /// Append one command-line argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append multiple command-line arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set one environment variable for the child process. Only variables
    /// explicitly listed here are passed; the caller decides whether to seed
    /// this from a permitted-overrides allowlist (Task 6) or leave it empty.
    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Set multiple environment variables for the child process.
    #[must_use]
    pub fn envs<I, K, V>(mut self, env: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env
            .extend(env.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// The executable path or command name.
    #[must_use]
    pub fn command(&self) -> &Path {
        &self.command
    }

    /// Command-line arguments passed to the executable.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.args
    }

    /// Environment variables set for the child process.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// A profile for an installed `codex-acp` executable on `PATH`, taking no
    /// arguments. This is the primary supported path once a packaged binary
    /// is available (see the Task 0 findings' §11 "known gap" note); it has
    /// no cwd dependency at all.
    #[must_use]
    pub fn codex_installed() -> Self {
        Self::new("codex-acp")
    }

    /// A profile for running codex-acp from a local source clone via
    /// `npm run start --prefix <clone_dir>`, codex-acp's own documented
    /// "run from sources" launch command (`readme-dev.md`). Requires `npm
    /// install` to have already been run once in `clone_dir`.
    #[must_use]
    pub fn codex_dev(clone_dir: impl Into<PathBuf>) -> Self {
        Self::new("npm")
            .arg("run")
            .arg("start")
            .arg("--prefix")
            .arg(clone_dir.into().display().to_string())
    }

    pub(crate) fn into_agent(self) -> AcpAgent {
        let mut config = AcpAgentConfig::new(self.command).args(self.args);
        for (k, v) in self.env {
            config = config.env(k, v);
        }
        AcpAgent::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_dev_uses_npm_prefix_not_a_shell() {
        let profile = AdapterLaunchProfile::codex_dev("/tmp/codex-acp");
        assert_eq!(profile.command(), Path::new("npm"));
        assert_eq!(
            profile.arguments(),
            ["run", "start", "--prefix", "/tmp/codex-acp"]
        );
    }

    #[test]
    fn codex_installed_has_no_arguments() {
        let profile = AdapterLaunchProfile::codex_installed();
        assert_eq!(profile.command(), Path::new("codex-acp"));
        assert!(profile.arguments().is_empty());
    }

    #[test]
    fn env_overrides_are_carried() {
        let profile = AdapterLaunchProfile::new("codex-acp").env("CODEX_PATH", "/opt/codex");
        assert_eq!(
            profile.environment().get("CODEX_PATH").map(String::as_str),
            Some("/opt/codex")
        );
    }
}
