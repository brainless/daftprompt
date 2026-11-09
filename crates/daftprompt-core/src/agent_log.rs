//! Opt-in, append-only diagnostics shared by agents; never graph data.
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::scribe::support;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LogLevel {
    #[default]
    Summary,
    Full,
}

impl LogLevel {
    fn parse(value: Option<&str>) -> Self {
        match value {
            None | Some("") | Some("summary") => Self::Summary,
            Some("full") => Self::Full,
            Some(value) => {
                eprintln!(
                    "daftprompt agent log: unknown DAFTPROMPT_AGENT_LOG_LEVEL {value:?}; using summary"
                );
                Self::Summary
            }
        }
    }
}

// Project diagnostics only: the provenance ledger always retains complete calls.
fn summary_payload(_kind: &str, mut payload: serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    if let Some(object) = payload.as_object_mut() {
        for field in ["output", "extraction"] {
            if let Some(value) = object.remove(field) {
                let bytes = value
                    .as_str()
                    .map(str::len)
                    .unwrap_or_else(|| value.to_string().len());
                object.insert(format!("{field}_bytes"), json!(bytes));
                let parsed = value
                    .as_str()
                    .and_then(|text| serde_json::from_str(text).ok());
                let structured = parsed.as_ref().unwrap_or(&value);
                for name in ["citations", "commands", "spans", "labels"] {
                    if let Some(items) = structured.get(name).and_then(|value| value.as_array()) {
                        object.insert(format!("{name}_count"), json!(items.len()));
                    }
                }
            }
        }
    }
    if let Some(detail) = payload.get_mut("detail")
        && let Some(text) = detail.as_str()
    {
        *detail = json!(text.chars().take(1000).collect::<String>());
    }
    payload
}

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// A correlation context. Clones retain the same run id and destination.
#[derive(Debug, Clone)]
pub struct AgentLog {
    path: Option<Arc<PathBuf>>,
    run_id: String,
    agent: String,
    attempt: Option<u32>,
    level: LogLevel,
}

impl AgentLog {
    /// Read `DAFTPROMPT_AGENT_LOG_FILE` once for this context. Empty means disabled.
    pub fn new(agent: &str, run_id: &str) -> Self {
        let path = std::env::var_os("DAFTPROMPT_AGENT_LOG_FILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(Arc::new);
        let level = if path.is_some() {
            LogLevel::parse(std::env::var("DAFTPROMPT_AGENT_LOG_LEVEL").ok().as_deref())
        } else {
            LogLevel::Summary
        };
        Self {
            path,
            run_id: run_id.into(),
            agent: agent.into(),
            attempt: None,
            level,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.path.is_some()
    }

    pub fn with_attempt(mut self, attempt: u32) -> Self {
        self.attempt = Some(attempt);
        self
    }

    /// Summarize directly from the ledger without copying source bodies.
    pub(crate) fn tool_call(
        &self,
        call: &crate::project_tools::RecordedCall,
        duration: Option<std::time::Duration>,
    ) {
        if !self.is_enabled() {
            return;
        }
        let mut payload = if self.level == LogLevel::Full {
            serde_json::to_value(call).expect("tool records serialize")
        } else {
            serde_json::json!({
                "index": call.index, "name": call.name, "args": call.args,
                "is_error": call.is_error, "truncated": call.truncated,
                "output_bytes": call.output.len(), "output_lines": call.output.lines().count(),
                "error": call.is_error.then(|| call.output.chars().take(1000).collect::<String>()),
                "read": call.read.as_ref().map(|read| serde_json::json!({
                    "path": read.path, "start_line": read.start_line, "end_line": read.end_line,
                    "total_lines": read.total_lines,
                    "returned_lines": read.end_line.saturating_sub(read.start_line) + 1,
                })),
                "commit_count": call.commits.len(),
            })
        };
        if let Some(duration) = duration {
            payload["duration_ms"] = serde_json::json!(duration.as_secs_f64() * 1000.0);
        }
        self.event("tool_call", payload);
    }

    /// Record a typed model result without formatting its complete Debug representation.
    pub fn model_outcome<T: Serialize>(
        &self,
        kind: &str,
        outcome: &Result<Result<T, crate::scribe::ModelError>, tokio::time::error::Elapsed>,
        duration: std::time::Duration,
    ) {
        if !self.is_enabled() {
            return;
        }
        let mut payload = match outcome {
            Err(_) => serde_json::json!({"status": "timeout"}),
            Ok(Err(error)) => {
                serde_json::json!({"status": "model_error", "detail": error.to_string()})
            }
            Ok(Ok(extraction)) => {
                serde_json::json!({"status": "submitted", "extraction": extraction})
            }
        };
        payload["duration_ms"] = serde_json::json!(duration.as_millis());
        self.event(kind, payload);
    }

    /// Append one JSON object. Failures are visible on stderr and never fail a run.
    /// Payloads can include source text and model output; enable only intentionally.
    pub fn event(&self, kind: &str, payload: impl Serialize) {
        let Some(path) = &self.path else {
            return;
        };
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let payload = serde_json::to_value(payload)?;
            let payload = match self.level {
                LogLevel::Summary => summary_payload(kind, payload),
                LogLevel::Full => payload,
            };
            let mut line = serde_json::to_vec(&serde_json::json!({
                "timestamp": support::now(), "process_id": std::process::id(),
                "run_id": self.run_id, "agent": self.agent, "event": kind,
                "attempt": self.attempt, "level": match self.level { LogLevel::Summary => "summary", LogLevel::Full => "full" }, "payload": payload,
            }))?;
            line.push(b'\n');
            // Serialize all in-process writers, including separate agent contexts.
            let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.as_ref())?
                .write_all(&line)?;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!(
                "daftprompt agent log: could not append to {}: {error}",
                path.display()
            );
        }
    }
}

impl Default for AgentLog {
    fn default() -> Self {
        Self::new("project_tools", &support::new_id("tool-run"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_tools::{RecordedCall, ToolRecorder};

    #[test]
    fn append_concurrent_lines_preserves_json_and_existing_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("calls.jsonl");
        std::fs::write(&path, "{\"existing\":true}\n").unwrap();
        let log = AgentLog {
            path: Some(Arc::new(path.clone())),
            run_id: "run-test".into(),
            agent: "test".into(),
            attempt: None,
            level: LogLevel::Summary,
        };
        std::thread::scope(|scope| {
            for worker in 0..8 {
                let log = log.clone();
                scope.spawn(move || {
                    for call in 0..10 {
                        log.event(
                            "test_event",
                            serde_json::json!({"worker": worker, "call": call, "text": "a\nb"}),
                        );
                    }
                });
            }
        });
        let text = std::fs::read_to_string(path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 81);
        assert_eq!(lines[1]["run_id"], "run-test");
        assert_eq!(lines[1]["agent"], "test");
        assert!(lines[1]["timestamp"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn recorder_logs_errors_once_and_does_not_log_seeded_replays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("calls.jsonl");
        let recorder = ToolRecorder::new();
        recorder.set_log(AgentLog {
            path: Some(Arc::new(path.clone())),
            run_id: "run-test".into(),
            agent: "test".into(),
            attempt: None,
            level: LogLevel::Summary,
        });
        let call = RecordedCall {
            index: 99,
            name: "read_file".into(),
            args: serde_json::json!({"path":"missing"}),
            output: "error: missing".into(),
            is_error: true,
            truncated: false,
            read: None,
            commits: vec![],
        };
        recorder.seed(call.clone());
        assert!(!path.exists());
        recorder.push(call);
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(text.lines().count(), 1);
        let event: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(event["payload"]["is_error"], true);
        assert_eq!(event["payload"]["index"], 1);
        assert_eq!(recorder.call_count(), 2);
    }

    #[test]
    fn levels_keep_metrics_and_full_restores_content() {
        assert_eq!(LogLevel::parse(None), LogLevel::Summary);
        assert_eq!(LogLevel::parse(Some("")), LogLevel::Summary);
        assert_eq!(LogLevel::parse(Some("full")), LogLevel::Full);
        assert_eq!(LogLevel::parse(Some("unknown")), LogLevel::Summary);
        let dir = tempfile::tempdir().unwrap();
        for level in [LogLevel::Summary, LogLevel::Full] {
            let path = dir.path().join(format!("{level:?}.jsonl"));
            let log = AgentLog {
                path: Some(Arc::new(path.clone())),
                run_id: "run".into(),
                agent: "surveyor".into(),
                attempt: Some(2),
                level,
            };
            let call = RecordedCall {
                index: 0,
                name: "read_file".into(),
                args: serde_json::json!({"path":"src/lib.rs"}),
                output: "SOURCE_BODY\n".into(),
                is_error: false,
                truncated: true,
                read: Some(crate::project_tools::FileRead {
                    path: "src/lib.rs".into(),
                    start_line: 2,
                    end_line: 4,
                    total_lines: 10,
                    file_sha256: "CONTENT_HASH".into(),
                }),
                commits: vec![],
            };
            log.tool_call(&call, Some(std::time::Duration::from_millis(12)));
            log.event(
                "structured_output",
                serde_json::json!({"output":
                r#"{"summary":"MODEL_BODY","citations":[{},{}]}"#, "output_tool_calls": 1}),
            );
            let text = std::fs::read_to_string(path).unwrap();
            let events: Vec<serde_json::Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(events[0]["attempt"], 2);
            assert_eq!(events[0]["payload"]["duration_ms"], 12.0);
            assert_eq!(events[0]["payload"]["args"]["path"], "src/lib.rs");
            assert_eq!(events[0]["payload"]["truncated"], true);
            if level == LogLevel::Summary {
                for omitted in ["SOURCE_BODY", "CONTENT_HASH", "MODEL_BODY"] {
                    assert!(!text.contains(omitted));
                }
                assert_eq!(events[0]["payload"]["output_bytes"], 12);
                assert_eq!(events[0]["payload"]["read"]["returned_lines"], 3);
                assert_eq!(events[1]["payload"]["citations_count"], 2);
            } else {
                for retained in ["SOURCE_BODY", "CONTENT_HASH", "MODEL_BODY"] {
                    assert!(text.contains(retained));
                }
            }
        }
    }

    #[test]
    fn typed_model_outcomes_omit_bodies_and_keep_counts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.jsonl");
        let log = AgentLog {
            path: Some(Arc::new(path.clone())),
            run_id: "run".into(),
            agent: "scribe".into(),
            attempt: Some(1),
            level: LogLevel::Summary,
        };
        log.model_outcome(
            "model_attempt",
            &Ok(Ok(serde_json::json!({
                "spans": [{"quote": "PRIVATE_MODEL_TEXT"}], "labels": [{}]
            }))),
            std::time::Duration::from_millis(20),
        );
        let text = std::fs::read_to_string(path).unwrap();
        assert!(!text.contains("PRIVATE_MODEL_TEXT"));
        let event: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(event["payload"]["spans_count"], 1);
        assert_eq!(event["payload"]["duration_ms"], 20);
        assert_eq!(event["payload"]["status"], "submitted");
    }

    #[test]
    fn disabled_and_unwritable_logs_do_not_fail_calls() {
        AgentLog {
            path: None,
            run_id: "test".into(),
            agent: "test".into(),
            attempt: None,
            level: LogLevel::Summary,
        }
        .event("test", ());
        let dir = tempfile::tempdir().unwrap();
        AgentLog {
            path: Some(Arc::new(dir.path().to_owned())),
            run_id: "test".into(),
            agent: "test".into(),
            attempt: None,
            level: LogLevel::Summary,
        }
        .event("test", ());
    }
}
