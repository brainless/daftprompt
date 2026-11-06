use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    FileRead, FsProject, LIST_FILES, ListFilesArgs, ProjectFiles, ProjectToolConfig, READ_FILE,
    ReadFileArgs, RecordedCall, SEARCH, Scope, SearchArgs, SearchQuery, ToolError, ToolRecorder,
    cut_to_bytes, split_lines,
};

/// What one call returns to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResponse {
    /// Text for the model. Errors read `error: <message>`.
    pub text: String,
    pub is_error: bool,
}

/// The tools for one agent run: a backend, the caps, a call counter, and the
/// ledger. Create a new toolbox per run so the call cap and the ledger start
/// empty; share it (`Arc`) with the Rig adapters.
pub struct ProjectToolbox {
    files: Arc<dyn ProjectFiles>,
    config: ProjectToolConfig,
    recorder: ToolRecorder,
    started: AtomicUsize,
}

impl ProjectToolbox {
    pub fn new(files: Arc<dyn ProjectFiles>, config: ProjectToolConfig) -> Self {
        Self {
            files,
            config,
            recorder: ToolRecorder::new(),
            started: AtomicUsize::new(0),
        }
    }

    /// Toolbox over a real directory.
    pub fn for_root(
        root: impl AsRef<std::path::Path>,
        config: ProjectToolConfig,
    ) -> Result<Self, ToolError> {
        Ok(Self::new(Arc::new(FsProject::new(root)?), config))
    }

    pub fn config(&self) -> &ProjectToolConfig {
        &self.config
    }

    /// The ledger of this run's calls and `read_file` ranges.
    pub fn recorder(&self) -> &ToolRecorder {
        &self.recorder
    }

    /// `(name, description, JSON-schema parameters)` for each tool.
    pub fn definitions() -> [(&'static str, &'static str, Value); 3] {
        [
            (
                LIST_FILES,
                "List files in the project, relative to the project root. Optional `directory` \
                 (default: the root) and `glob` (for example `*.rs` or `src/**/*.md`). Hidden, \
                 ignored, binary, and environment files are never listed. Results are capped.",
                json!({"type": "object", "properties": {
                    "directory": {"type": "string", "description": "Directory relative to the project root."},
                    "glob": {"type": "string", "description": "Glob filter on file paths."}
                }}),
            ),
            (
                SEARCH,
                "Search project files for literal text (not a regex). Returns `path:line: text` \
                 lines. Optional `directory`, `glob`, and `case_sensitive` (default false). \
                 Results are capped.",
                json!({"type": "object", "properties": {
                    "query": {"type": "string", "description": "Literal text to find."},
                    "directory": {"type": "string"},
                    "glob": {"type": "string"},
                    "case_sensitive": {"type": "boolean"}
                }, "required": ["query"]}),
            ),
            (
                READ_FILE,
                "Read a project file as numbered lines. Optional 1-based inclusive `start_line` \
                 and `end_line`. Long files are cut off with a marker saying where to continue.",
                json!({"type": "object", "properties": {
                    "path": {"type": "string", "description": "File path relative to the project root."},
                    "start_line": {"type": "integer", "minimum": 1},
                    "end_line": {"type": "integer", "minimum": 1}
                }, "required": ["path"]}),
            ),
        ]
    }

    /// Runs a tool by name with raw JSON arguments. Always returns a result
    /// for the model, records the call, and enforces the call cap.
    pub fn call(&self, name: &str, args: Value) -> ToolResponse {
        let n = self.started.fetch_add(1, Ordering::SeqCst);
        let outcome = if n >= self.config.max_tool_calls {
            Err(ToolError::CallLimit(self.config.max_tool_calls))
        } else {
            match name {
                LIST_FILES => parse(&args).and_then(|a| self.list_files(a)),
                SEARCH => parse(&args).and_then(|a| self.search(a)),
                READ_FILE => parse(&args).and_then(|a| self.read_file(a)),
                other => Err(ToolError::InvalidArgument(format!(
                    "unknown tool `{other}`"
                ))),
            }
        };
        let (text, is_error, truncated, read) = match outcome {
            Ok(out) => (out.text, false, out.truncated, out.read),
            Err(e) => (format!("error: {e}"), true, false, None),
        };
        self.recorder.push(RecordedCall {
            index: 0,
            name: name.to_string(),
            args,
            output: text.clone(),
            is_error,
            truncated,
            read,
            commits: Vec::new(),
        });
        ToolResponse { text, is_error }
    }

    fn list_files(&self, args: ListFilesArgs) -> Result<Out, ToolError> {
        let scope = Scope::new(args.directory, args.glob.as_deref())?;
        let listing = self.files.list(&scope, &self.config)?;
        let mut text = String::new();
        let mut shown = 0;
        let mut truncated = listing.truncated;
        for path in &listing.paths {
            if text.len() + path.len() + 1 > self.config.max_output_bytes {
                truncated = true;
                break;
            }
            text.push_str(path);
            text.push('\n');
            shown += 1;
        }
        if shown == 0 {
            text.push_str("(no files)\n");
        }
        if truncated {
            text.push_str(&format!(
                "[truncated: showing {shown} files; narrow the directory or glob to see more]\n"
            ));
        }
        Ok(Out::new(text, truncated))
    }

    fn search(&self, args: SearchArgs) -> Result<Out, ToolError> {
        if args.query.is_empty() {
            return Err(ToolError::InvalidArgument("query must not be empty".into()));
        }
        let scope = Scope::new(args.directory, args.glob.as_deref())?;
        let query = SearchQuery {
            text: args.query,
            case_sensitive: args.case_sensitive.unwrap_or(false),
        };
        let scan = self.files.search(&query, &scope, &self.config)?;
        let mut text = String::new();
        let mut shown = 0;
        let mut truncated = scan.truncated;
        for hit in &scan.hits {
            let (line, cut) = cut_to_bytes(&hit.text, self.config.max_line_bytes);
            let entry = format!(
                "{}:{}: {}{}\n",
                hit.path,
                hit.line,
                line,
                if cut { LINE_CUT } else { "" }
            );
            if text.len() + entry.len() > self.config.max_output_bytes {
                truncated = true;
                break;
            }
            text.push_str(&entry);
            shown += 1;
        }
        if shown == 0 && !truncated {
            text.push_str("(no matches)\n");
        }
        if truncated {
            text.push_str(&format!(
                "[truncated: showing {shown} matches; narrow the query, directory, or glob]\n"
            ));
        }
        Ok(Out::new(text, truncated))
    }

    fn read_file(&self, args: ReadFileArgs) -> Result<Out, ToolError> {
        let start = args.start_line.unwrap_or(1);
        if start == 0 {
            return Err(ToolError::InvalidArgument("start_line is 1-based".into()));
        }
        if let Some(end) = args.end_line
            && end < start
        {
            return Err(ToolError::InvalidArgument(format!(
                "end_line {end} is before start_line {start}"
            )));
        }
        let file = self.files.read(&args.path, &self.config)?;
        let lines = split_lines(&file.text);
        let total = lines.len();
        if total == 0 {
            return Ok(Out::new(
                format!("{} (empty file, 0 lines)\n", file.path),
                false,
            ));
        }
        if start > total {
            return Err(ToolError::InvalidArgument(format!(
                "start_line {start} is past the end of {} ({total} lines)",
                file.path
            )));
        }
        let wanted_end = args.end_line.unwrap_or(usize::MAX).min(total);
        let line_cap_end = start.saturating_add(self.config.max_read_lines.max(1) - 1);
        let mut end = wanted_end.min(line_cap_end);

        let mut body = String::new();
        let mut last = start - 1;
        for n in start..=end {
            let (line, cut) = cut_to_bytes(lines[n - 1], self.config.max_line_bytes);
            let entry = format!("{n}: {line}{}\n", if cut { LINE_CUT } else { "" });
            if n > start && body.len() + entry.len() > self.config.max_output_bytes {
                break;
            }
            body.push_str(&entry);
            last = n;
        }
        end = last;
        let truncated = end < wanted_end;
        let mut text = format!("{} (lines {start}-{end} of {total})\n{body}", file.path);
        if truncated {
            text.push_str(&format!(
                "[truncated: showing lines {start}-{end} of the {} requested; \
                 call read_file with start_line={} to continue]\n",
                wanted_end - start + 1,
                end + 1
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(file.text.as_bytes());
        let read = FileRead {
            path: file.path,
            start_line: start,
            end_line: end,
            total_lines: total,
            file_sha256: format!("{:x}", hasher.finalize()),
        };
        Ok(Out {
            text,
            truncated,
            read: Some(read),
        })
    }
}

const LINE_CUT: &str = "… [line truncated]";

struct Out {
    text: String,
    truncated: bool,
    read: Option<FileRead>,
}

impl Out {
    fn new(text: String, truncated: bool) -> Self {
        Self {
            text,
            truncated,
            read: None,
        }
    }
}

fn parse<T: DeserializeOwned>(args: &Value) -> Result<T, ToolError> {
    // Some models send `null` for a tool without required arguments.
    let value = if args.is_null() {
        json!({})
    } else {
        args.clone()
    };
    serde_json::from_value(value).map_err(|e| ToolError::InvalidArgument(e.to_string()))
}
