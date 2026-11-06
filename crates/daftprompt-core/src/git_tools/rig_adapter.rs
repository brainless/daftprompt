use std::convert::Infallible;
use std::sync::Arc;

use rig::tool::PortableTool;
use serde_json::Value;

use super::{GIT_LOG, GIT_SHOW, GitToolbox};

macro_rules! adapter {
    ($ty:ident, $name:expr, $idx:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone)]
        pub struct $ty(Arc<GitToolbox>);

        impl PortableTool for $ty {
            const NAME: &'static str = $name;
            type Args = Value;
            type Output = String;
            // Failures are tool results, never run failures.
            type Error = Infallible;

            fn description(&self) -> String {
                GitToolbox::definitions()[$idx].1.to_string()
            }

            fn parameters(&self) -> Value {
                GitToolbox::definitions()[$idx].2.clone()
            }

            async fn call(&self, arguments: Value) -> Result<String, Infallible> {
                let toolbox = self.0.clone();
                // A git process can take seconds; keep it off the async workers.
                let joined =
                    tokio::task::spawn_blocking(move || toolbox.call($name, arguments).text).await;
                Ok(joined.unwrap_or_else(|error| format!("error: the git call failed: {error}")))
            }
        }
    };
}

adapter!(GitLogTool, GIT_LOG, 0, "Rig adapter for `git_log`.");
adapter!(GitShowTool, GIT_SHOW, 1, "Rig adapter for `git_show`.");

/// The two adapters for one toolbox.
#[derive(Clone)]
pub struct RigGitTools {
    pub git_log: GitLogTool,
    pub git_show: GitShowTool,
}

/// Builds the Rig adapters. Register each on an agent builder, for example
/// `builder.tool(t.git_log).tool(t.git_show)`. Both share the toolbox's call
/// cap and ledger.
pub fn git_rig_tools(toolbox: &Arc<GitToolbox>) -> RigGitTools {
    RigGitTools {
        git_log: GitLogTool(toolbox.clone()),
        git_show: GitShowTool(toolbox.clone()),
    }
}
