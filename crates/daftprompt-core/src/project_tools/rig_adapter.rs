use std::convert::Infallible;
use std::sync::Arc;

use rig::tool::PortableTool;
use serde_json::Value;

use super::{LIST_FILES, ProjectToolbox, READ_FILE, SEARCH};

macro_rules! adapter {
    ($ty:ident, $name:expr, $idx:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone)]
        pub struct $ty(Arc<ProjectToolbox>);

        impl PortableTool for $ty {
            const NAME: &'static str = $name;
            type Args = Value;
            type Output = String;
            // Failures are tool results, never run failures.
            type Error = Infallible;

            fn description(&self) -> String {
                ProjectToolbox::definitions()[$idx].1.to_string()
            }

            fn parameters(&self) -> Value {
                ProjectToolbox::definitions()[$idx].2.clone()
            }

            async fn call(&self, arguments: Value) -> Result<String, Infallible> {
                Ok(self.0.call($name, arguments).text)
            }
        }
    };
}

adapter!(
    ListFilesTool,
    LIST_FILES,
    0,
    "Rig adapter for `list_files`."
);
adapter!(SearchTool, SEARCH, 1, "Rig adapter for `search`.");
adapter!(ReadFileTool, READ_FILE, 2, "Rig adapter for `read_file`.");

/// The three adapters for one toolbox.
#[derive(Clone)]
pub struct RigProjectTools {
    pub list_files: ListFilesTool,
    pub search: SearchTool,
    pub read_file: ReadFileTool,
}

/// Builds the Rig adapters. Register each on an agent builder, for example
/// `builder.tool(t.list_files).tool(t.search).tool(t.read_file)`. All three
/// share the toolbox's call cap and ledger.
pub fn rig_tools(toolbox: &Arc<ProjectToolbox>) -> RigProjectTools {
    RigProjectTools {
        list_files: ListFilesTool(toolbox.clone()),
        search: SearchTool(toolbox.clone()),
        read_file: ReadFileTool(toolbox.clone()),
    }
}
