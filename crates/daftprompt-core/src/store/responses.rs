//! Stored agent responses: append-only snapshots of successful Chronicler,
//! Surveyor, and Operator results ([Epic 007](../../../../epics/007-project-paths-and-agent-storage.md)).
//!
//! A row keeps the summary as plain `TEXT` and everything else of the accepted
//! result as versioned JSON. [`StoredAgentResponseV1`] is the frozen storage
//! form of version 1: it is converted to and from the public result types
//! explicitly, so those can evolve without breaking old rows. A row that
//! cannot be decoded is reported as [`ResponseOutcome::Unusable`] and left in
//! place; only real SQLite failures are a [`StoreError`].
//!
//! Nothing here decides whether a stored response is current. This module
//! records repository identity and reuse eligibility at capture and finds
//! reuse *candidates*; the briefing service verifies them.

use daftprompt_types::{
    AgentTask, BriefingNotice, BriefingResult, CitationInfo, CommandsResult, Node, NodeContext,
    Payload, ProjectCommand, ProjectProfile, Run, RunInput, ToolCall,
};
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};

use super::{Store, StoreError};
use crate::scribe::support::{content_hash, new_id, now};

/// The `response_version` written by this build and the only one it reads.
pub const RESPONSE_VERSION: u32 = 1;

const RESPONSE_COLUMNS: &str = "id, project_id, project_path_id, agent, request_key, config_key, \
     commit_hash, repository_subpath, reusable, response_version, summary_text, details_json, \
     created_at";

/// Which agent produced a stored response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredAgent {
    Chronicler,
    Surveyor,
    Operator,
}

impl StoredAgent {
    /// The value of the `agent` column.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chronicler => "chronicler",
            Self::Surveyor => "surveyor",
            Self::Operator => "operator",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "chronicler" => Some(Self::Chronicler),
            "surveyor" => Some(Self::Surveyor),
            "operator" => Some(Self::Operator),
            _ => None,
        }
    }
}

/// Why a stored row cannot be turned back into a result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnusableReason {
    #[error("unsupported response version {0}")]
    UnsupportedVersion(i64),
    #[error("malformed details JSON: {0}")]
    MalformedJson(String),
    #[error("inconsistent response: {0}")]
    Inconsistent(String),
}

/// Everything of an accepted result except its summary text, in the layout of
/// response version 1.
///
/// Compatible additions may use `#[serde(default)]`; a breaking change adds a
/// `StoredAgentResponseV2` and a conversion instead of editing this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDetailsV1 {
    pub project_id: String,
    pub task: AgentTask,
    pub prompt_payload: Option<Payload>,
    /// Hash and time of the summary payload; its content is `summary_text`.
    pub summary_hash: String,
    pub summary_created_at: String,
    pub summary_node_id: String,
    pub nodes: Vec<Node>,
    pub node_contexts: Vec<NodeContext>,
    pub citations: Vec<CitationInfo>,
    pub notices: Vec<BriefingNotice>,
    /// `None` for Chronicler, which has no profile.
    #[serde(default)]
    pub profile: Option<ProjectProfile>,
    pub run: Run,
    pub run_inputs: Vec<RunInput>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_payloads: Vec<Payload>,
    /// Operator only: commands with citation indexes and working directories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<ProjectCommand>>,
    /// What a freshness check needs to repeat (Epic 007). Absent when the
    /// agent has none, and in rows written before reuse existed; a response
    /// without it is never reused. Adding this typed form changed no
    /// previously written byte, so `response_version` stays 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reuse_inputs: Option<ReuseInputsV1>,
}

/// The inputs of an agent's freshness check, recorded at capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReuseInputsV1 {
    /// Chronicler reads history once through `git_log`. The check repeats
    /// that call and compares the output hash.
    Chronicler {
        /// The arguments of the recorded `git_log` call.
        git_log_args: serde_json::Value,
        /// SHA-256 of the call's complete output text.
        git_log_output_hash: String,
    },
    /// Surveyor: the deterministic inputs and the model's observations.
    Surveyor(FileInputsV1),
    /// Operator: Surveyor's inputs plus what Rust derived for the commands.
    Operator {
        files: FileInputsV1,
        /// SHA-256 of the canonical command-candidate list and of the
        /// manifests its evidence cites.
        candidates_hash: String,
        /// Distinct non-root working directories of the stored commands;
        /// each must still be a listable directory.
        working_dirs: Vec<String>,
    },
}

/// What a file agent's freshness check repeats (Epic 007, Task C2). Nothing
/// here is a machine-absolute path, a generated ID, or a timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInputsV1 {
    /// SHA-256 of the canonical project profile the model was given.
    pub profile_hash: String,
    /// SHA-256 of the complete preload ledger (calls, arguments, outputs,
    /// error and truncation flags, read hashes).
    pub preload_fingerprint: String,
    /// The effective `ProjectToolConfig` limits the model's calls ran under.
    pub tool_limits: serde_json::Value,
    /// The model-initiated `list_files`, `search`, and `read_file` calls in
    /// order. Preload-seeded calls are covered by `preload_fingerprint`.
    pub model_calls: Vec<ObservedCall>,
}

/// One model-initiated tool call and a hash of everything it showed the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCall {
    pub tool: String,
    /// The arguments exactly as received.
    pub args: serde_json::Value,
    /// SHA-256 of the canonical observation: output text (errors and
    /// truncation markers included), error and truncation flags, and the
    /// whole-file hash of a read.
    pub observation_hash: String,
}

/// The storage form of one successful response, version 1.
///
/// `summary_text` is `summary_payload.content` unchanged (Markdown included);
/// `details` holds the rest. The summary body is never duplicated in JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentResponseV1 {
    pub agent: StoredAgent,
    pub summary_text: String,
    pub details: StoredDetailsV1,
}

/// A decoded response in its public form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredResult {
    Briefing(BriefingResult),
    Commands(CommandsResult),
}

impl StoredAgentResponseV1 {
    /// Snapshot a Chronicler result.
    ///
    /// The recorded `git_log` call becomes the reuse inputs, unless it failed
    /// (an `error:` output is not history worth comparing).
    pub fn chronicler(result: &BriefingResult) -> Self {
        let mut stored = Self::from_briefing(StoredAgent::Chronicler, result, None);
        stored.details.reuse_inputs = result
            .tool_calls
            .iter()
            .find(|call| call.tool == "git_log")
            .and_then(|call| {
                let hash = call.output_hash.clone()?;
                let output = result.tool_payloads.iter().find(|p| p.hash == hash)?;
                if output.content.starts_with("error: ") {
                    return None;
                }
                Some(ReuseInputsV1::Chronicler {
                    git_log_args: serde_json::from_str(&call.input).ok()?,
                    git_log_output_hash: hash,
                })
            });
        stored
    }

    /// Snapshot a Surveyor result.
    pub fn surveyor(result: &BriefingResult) -> Self {
        Self::from_briefing(StoredAgent::Surveyor, result, None)
    }

    /// Snapshot an Operator result, including its commands.
    pub fn operator(result: &CommandsResult) -> Self {
        Self::from_briefing(
            StoredAgent::Operator,
            &result.briefing,
            Some(result.commands.clone()),
        )
    }

    /// Record the inputs of the agent's freshness check.
    pub fn with_reuse_inputs(mut self, inputs: Option<ReuseInputsV1>) -> Self {
        self.details.reuse_inputs = inputs;
        self
    }

    fn from_briefing(
        agent: StoredAgent,
        result: &BriefingResult,
        commands: Option<Vec<ProjectCommand>>,
    ) -> Self {
        Self {
            agent,
            summary_text: result.summary_payload.content.clone(),
            details: StoredDetailsV1 {
                project_id: result.project_id.clone(),
                task: result.task,
                prompt_payload: result.prompt_payload.clone(),
                summary_hash: result.summary_payload.hash.clone(),
                summary_created_at: result.summary_payload.created_at.clone(),
                summary_node_id: result.summary_node_id.clone(),
                nodes: result.nodes.clone(),
                node_contexts: result.node_contexts.clone(),
                citations: result.citations.clone(),
                notices: result.notices.clone(),
                profile: result.profile.clone(),
                run: result.run.clone(),
                run_inputs: result.run_inputs.clone(),
                tool_calls: result.tool_calls.clone(),
                tool_payloads: result.tool_payloads.clone(),
                commands,
                reuse_inputs: None,
            },
        }
    }

    /// The public result this snapshot was taken from. Lossless.
    pub fn into_result(self) -> StoredResult {
        let details = self.details;
        let briefing = BriefingResult {
            project_id: details.project_id,
            task: details.task,
            prompt_payload: details.prompt_payload,
            summary_payload: Payload {
                hash: details.summary_hash,
                content: self.summary_text,
                created_at: details.summary_created_at,
            },
            summary_node_id: details.summary_node_id,
            nodes: details.nodes,
            node_contexts: details.node_contexts,
            citations: details.citations,
            notices: details.notices,
            profile: details.profile,
            run: details.run,
            run_inputs: details.run_inputs,
            tool_calls: details.tool_calls,
            tool_payloads: details.tool_payloads,
        };
        match details.commands {
            Some(commands) if self.agent == StoredAgent::Operator => {
                StoredResult::Commands(CommandsResult { briefing, commands })
            }
            _ => StoredResult::Briefing(briefing),
        }
    }

    /// The `summary_text` and `details_json` column values.
    fn encode(&self) -> Result<(&str, String), StoreError> {
        let json = serde_json::to_string(&self.details)
            .map_err(|error| StoreError::Storage(format!("could not encode response: {error}")))?;
        Ok((&self.summary_text, json))
    }

    /// Decode the columns of one row by `version`.
    fn decode(
        agent: StoredAgent,
        version: i64,
        summary_text: String,
        details_json: &str,
    ) -> Result<Self, UnusableReason> {
        if version != i64::from(RESPONSE_VERSION) {
            return Err(UnusableReason::UnsupportedVersion(version));
        }
        let details: StoredDetailsV1 = serde_json::from_str(details_json)
            .map_err(|error| UnusableReason::MalformedJson(error.to_string()))?;
        if content_hash(&summary_text) != details.summary_hash {
            return Err(UnusableReason::Inconsistent(
                "the summary text does not match its stored hash".to_owned(),
            ));
        }
        if (agent == StoredAgent::Operator) != details.commands.is_some() {
            return Err(UnusableReason::Inconsistent(format!(
                "{} responses {} commands",
                agent.as_str(),
                if agent == StoredAgent::Operator {
                    "must have"
                } else {
                    "cannot have"
                }
            )));
        }
        Ok(Self {
            agent,
            summary_text,
            details,
        })
    }
}

/// The columns of a row other than the response itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseMeta {
    /// A response id: not a run id.
    pub id: String,
    pub project_id: String,
    pub project_path_id: String,
    pub agent: StoredAgent,
    pub request_key: String,
    pub config_key: String,
    pub commit_hash: Option<String>,
    pub repository_subpath: Option<String>,
    pub reusable: bool,
    pub response_version: i64,
    /// When the snapshot was stored.
    pub created_at: String,
}

/// A decodable row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredResponse {
    pub meta: ResponseMeta,
    pub response: StoredAgentResponseV1,
}

/// A row that exists but cannot be turned back into a result. It is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusableResponse {
    pub meta: ResponseMeta,
    pub reason: UnusableReason,
}

/// What loading one row produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseOutcome {
    Usable(Box<StoredResponse>),
    Unusable(Box<UnusableResponse>),
}

impl ResponseOutcome {
    /// The row's metadata, usable or not.
    pub fn meta(&self) -> &ResponseMeta {
        match self {
            Self::Usable(stored) => &stored.meta,
            Self::Unusable(unusable) => &unusable.meta,
        }
    }
}

/// A response to append.
#[derive(Debug, Clone, Copy)]
pub struct NewResponse<'a> {
    pub project_id: &'a str,
    pub project_path_id: &'a str,
    pub request_key: &'a str,
    pub config_key: &'a str,
    /// Full `HEAD` hash at capture; `None` when unknown.
    pub commit_hash: Option<&'a str>,
    /// Repository-relative subpath of the root at capture; `None` when
    /// unknown.
    pub repository_subpath: Option<&'a str>,
    /// Whether the caller found the response eligible for reuse. Ignored
    /// (stored 0) without a known commit and subpath, so a row without
    /// identity can never be matched.
    pub reusable: bool,
    pub response: &'a StoredAgentResponseV1,
}

/// Which stored responses may be reuse candidates for a request.
#[derive(Debug, Clone, Copy)]
pub struct CandidateQuery<'a> {
    pub project_id: &'a str,
    /// The requested path: its responses sort first.
    pub project_path_id: &'a str,
    pub agent: StoredAgent,
    pub request_key: &'a str,
    pub config_key: &'a str,
    pub commit_hash: &'a str,
    pub repository_subpath: &'a str,
}

/// Which responses to list: one path, agent, and pair of keys.
#[derive(Debug, Clone, Copy)]
pub struct ResponseQuery<'a> {
    pub project_id: &'a str,
    pub project_path_id: &'a str,
    pub agent: StoredAgent,
    pub request_key: &'a str,
    pub config_key: &'a str,
}

/// A row as read from SQLite, before decoding.
struct RawRow {
    meta: Result<ResponseMeta, String>,
    summary_text: String,
    details_json: String,
}

fn raw_row(row: &Row<'_>) -> rusqlite::Result<RawRow> {
    let agent: String = row.get(3)?;
    let meta = StoredAgent::parse(&agent)
        .map(|agent| -> rusqlite::Result<ResponseMeta> {
            Ok(ResponseMeta {
                id: row.get(0)?,
                project_id: row.get(1)?,
                project_path_id: row.get(2)?,
                agent,
                request_key: row.get(4)?,
                config_key: row.get(5)?,
                commit_hash: row.get(6)?,
                repository_subpath: row.get(7)?,
                reusable: row.get(8)?,
                response_version: row.get(9)?,
                created_at: row.get(12)?,
            })
        })
        .ok_or_else(|| format!("unknown agent {agent:?}"));
    Ok(RawRow {
        meta: match meta {
            Ok(meta) => Ok(meta?),
            Err(detail) => Err(detail),
        },
        summary_text: row.get(10)?,
        details_json: row.get(11)?,
    })
}

fn into_outcome(raw: RawRow) -> Result<ResponseOutcome, StoreError> {
    // The CHECK constraint makes an unknown agent impossible; treat one as
    // corruption of the database, not as a usable-or-not row.
    let meta = raw.meta.map_err(StoreError::Storage)?;
    Ok(
        match StoredAgentResponseV1::decode(
            meta.agent,
            meta.response_version,
            raw.summary_text,
            &raw.details_json,
        ) {
            Ok(response) => ResponseOutcome::Usable(Box::new(StoredResponse { meta, response })),
            Err(reason) => ResponseOutcome::Unusable(Box::new(UnusableResponse { meta, reason })),
        },
    )
}

impl Store {
    /// Append a response snapshot and return its metadata.
    ///
    /// The id is generated here and `created_at` is the storage time. The
    /// commit hash, repository subpath, and eligibility are stored as given
    /// (`reusable` only with both identity parts). Fails with
    /// [`StoreError::Storage`] when the project path is not registered.
    pub fn insert_agent_response(&self, new: NewResponse<'_>) -> Result<ResponseMeta, StoreError> {
        let (summary_text, details_json) = new.response.encode()?;
        let meta = ResponseMeta {
            id: new_id("response"),
            project_id: new.project_id.to_owned(),
            project_path_id: new.project_path_id.to_owned(),
            agent: new.response.agent,
            request_key: new.request_key.to_owned(),
            config_key: new.config_key.to_owned(),
            commit_hash: new.commit_hash.map(str::to_owned),
            repository_subpath: new.repository_subpath.map(str::to_owned),
            reusable: new.reusable && new.commit_hash.is_some() && new.repository_subpath.is_some(),
            response_version: i64::from(RESPONSE_VERSION),
            created_at: now(),
        };
        self.lock().execute(
            "INSERT INTO project_agent_response
               (id, project_id, project_path_id, agent, request_key, config_key,
                commit_hash, repository_subpath, reusable, response_version,
                summary_text, details_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                meta.id,
                meta.project_id,
                meta.project_path_id,
                meta.agent.as_str(),
                meta.request_key,
                meta.config_key,
                meta.commit_hash,
                meta.repository_subpath,
                meta.reusable,
                meta.response_version,
                summary_text,
                details_json,
                meta.created_at,
            ],
        )?;
        Ok(meta)
    }

    /// Load one response by id. An undecodable row is
    /// [`ResponseOutcome::Unusable`], not an error.
    pub fn get_agent_response(&self, id: &str) -> Result<ResponseOutcome, StoreError> {
        let raw = {
            let connection = self.lock();
            let mut statement = connection.prepare(&format!(
                "SELECT {RESPONSE_COLUMNS} FROM project_agent_response WHERE id = ?1"
            ))?;
            let mut rows = statement.query_map([id], raw_row)?;
            rows.next().transpose()?
        };
        into_outcome(
            raw.ok_or_else(|| StoreError::NotFound(format!("no stored response with id {id:?}")))?,
        )
    }

    /// Up to `limit` reuse candidates: reusable rows of this project, agent,
    /// and key pair whose commit and subpath equal the requested ones, the
    /// requested path's rows first, then the newest by `(created_at, id)`.
    ///
    /// A NULL commit never equals anything, so rows without identity never
    /// appear. The rows come from one short lock; verification happens after
    /// it is released.
    pub fn reuse_candidates(
        &self,
        query: CandidateQuery<'_>,
        limit: usize,
    ) -> Result<Vec<ResponseOutcome>, StoreError> {
        let raws: Vec<RawRow> = {
            let connection = self.lock();
            let mut statement = connection.prepare(&format!(
                "SELECT {RESPONSE_COLUMNS} FROM project_agent_response
                 WHERE project_id = ?1 AND agent = ?2 AND request_key = ?3 AND config_key = ?4
                   AND commit_hash = ?5 AND repository_subpath = ?6 AND reusable = 1
                 ORDER BY (project_path_id = ?7) DESC, created_at DESC, id DESC
                 LIMIT ?8"
            ))?;
            statement
                .query_map(
                    params![
                        query.project_id,
                        query.agent.as_str(),
                        query.request_key,
                        query.config_key,
                        query.commit_hash,
                        query.repository_subpath,
                        query.project_path_id,
                        i64::try_from(limit).unwrap_or(i64::MAX),
                    ],
                    raw_row,
                )?
                .collect::<Result<_, _>>()?
        };
        raws.into_iter().map(into_outcome).collect()
    }

    /// The newest responses for one path, agent, and key pair, newest first
    /// by `(created_at, id)`, at most `limit`.
    pub fn latest_agent_responses(
        &self,
        query: ResponseQuery<'_>,
        limit: usize,
    ) -> Result<Vec<ResponseOutcome>, StoreError> {
        let raws: Vec<RawRow> = {
            let connection = self.lock();
            let mut statement = connection.prepare(&format!(
                "SELECT {RESPONSE_COLUMNS} FROM project_agent_response
                 WHERE project_id = ?1 AND project_path_id = ?2 AND agent = ?3
                   AND request_key = ?4 AND config_key = ?5
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?6"
            ))?;
            statement
                .query_map(
                    params![
                        query.project_id,
                        query.project_path_id,
                        query.agent.as_str(),
                        query.request_key,
                        query.config_key,
                        i64::try_from(limit).unwrap_or(i64::MAX),
                    ],
                    raw_row,
                )?
                .collect::<Result<_, _>>()?
        };
        raws.into_iter().map(into_outcome).collect()
    }
}

#[cfg(test)]
mod tests {
    use daftprompt_types::{
        CitationLocation, CommandKind, CommandOrigin, ContextTarget, RunStatus, SourceRange,
    };

    use super::*;

    fn payload(content: &str) -> Payload {
        Payload {
            hash: content_hash(content),
            content: content.to_owned(),
            created_at: "2024-01-01T00:00:00Z".to_owned(),
        }
    }

    /// A result with every optional part filled in.
    fn briefing() -> BriefingResult {
        let summary = payload("# Title\n\n- **item** `code` [1]\n- é日本\n");
        BriefingResult {
            project_id: "project-1".to_owned(),
            task: AgentTask::Prompt,
            prompt_payload: Some(payload("What is this?")),
            summary_payload: summary,
            summary_node_id: "node-2".to_owned(),
            nodes: vec![Node {
                id: "node-2".to_owned(),
                parent_id: Some("node-1".to_owned()),
                author: "agent".to_owned(),
                run_id: Some("run-1".to_owned()),
                created_at: "2024-01-01T00:00:00Z".to_owned(),
            }],
            node_contexts: vec![NodeContext {
                id: "ctx-1".to_owned(),
                node_id: "node-2".to_owned(),
                seq: 1,
                target: ContextTarget::File {
                    path: "README.md".to_owned(),
                    content_hash: Some("abc".to_owned()),
                },
                range: Some(SourceRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 2,
                    end_col: 1,
                }),
            }],
            citations: vec![CitationInfo {
                index: 1,
                context_id: "ctx-1".to_owned(),
                location: CitationLocation::File {
                    path: "README.md".to_owned(),
                    start_line: 1,
                    end_line: 1,
                },
            }],
            notices: vec![],
            profile: Some(ProjectProfile::default()),
            run: Run {
                id: "run-1".to_owned(),
                agent: "surveyor".to_owned(),
                provider: Some("p".to_owned()),
                model: Some("m".to_owned()),
                prompt_version: Some("v".to_owned()),
                started_at: "2024-01-01T00:00:00Z".to_owned(),
                ended_at: Some("2024-01-01T00:00:01Z".to_owned()),
                status: RunStatus::Done,
                attempts: 2,
                consistency_followup: false,
                consistency_used: false,
                project_id: Some("project-1".to_owned()),
            },
            run_inputs: vec![RunInput {
                run_id: "run-1".to_owned(),
                node_id: "node-1".to_owned(),
            }],
            tool_calls: vec![ToolCall {
                id: "call-1".to_owned(),
                run_id: "run-1".to_owned(),
                seq: 0,
                tool: "read_file".to_owned(),
                input: "{\"path\":\"README.md\"}".to_owned(),
                output_hash: Some("out".to_owned()),
                created_at: "2024-01-01T00:00:00Z".to_owned(),
            }],
            tool_payloads: vec![payload("1: # Demo")],
        }
    }

    fn commands() -> CommandsResult {
        CommandsResult {
            briefing: briefing(),
            commands: vec![ProjectCommand {
                id: "cmd-1".to_owned(),
                label: "site".to_owned(),
                command: "make site".to_owned(),
                working_dir: "web".to_owned(),
                kind: CommandKind::Run,
                language: None,
                origin: CommandOrigin::Documented,
                citation_indexes: vec![1],
            }],
        }
    }

    /// A store with one project, returning the ids a response needs.
    fn store() -> (Store, String, String, tempfile::TempDir) {
        let store = Store::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (project, _) = store.add_project(dir.path(), None).unwrap();
        (store, project.id, project.default_path_id, dir)
    }

    fn insert(
        store: &Store,
        project_id: &str,
        path_id: &str,
        response: &StoredAgentResponseV1,
    ) -> ResponseMeta {
        store
            .insert_agent_response(NewResponse {
                project_id,
                project_path_id: path_id,
                request_key: "r",
                config_key: "c",
                commit_hash: None,
                repository_subpath: None,
                reusable: false,
                response,
            })
            .unwrap()
    }

    #[test]
    fn dto_conversion_is_lossless_and_keeps_the_summary_out_of_json() {
        let result = briefing();
        let dto = StoredAgentResponseV1::surveyor(&result);
        assert_eq!(dto.summary_text, result.summary_payload.content);
        let (_, json) = dto.encode().unwrap();
        assert!(!json.contains("item"), "summary body duplicated: {json}");
        assert_eq!(dto.clone().into_result(), StoredResult::Briefing(result));

        let commands = commands();
        let dto = StoredAgentResponseV1::operator(&commands);
        let (_, json) = dto.encode().unwrap();
        assert!(json.contains("\"working_dir\":\"web\"") && json.contains("citation_indexes"));
        assert_eq!(dto.into_result(), StoredResult::Commands(commands));
    }

    #[test]
    fn rows_that_cannot_be_decoded_are_unusable_and_stay() {
        let (store, project, path, _dir) = store();
        let meta = insert(
            &store,
            &project,
            &path,
            &StoredAgentResponseV1::surveyor(&briefing()),
        );
        let operator = insert(
            &store,
            &project,
            &path,
            &StoredAgentResponseV1::operator(&commands()),
        );
        let unusable = |id: &str| match store.get_agent_response(id).unwrap() {
            ResponseOutcome::Unusable(row) => row.reason,
            ResponseOutcome::Usable(_) => panic!("row should be unusable"),
        };
        let reset = |sql: &str, id: &str| {
            store
                .lock()
                .execute(
                    &format!("UPDATE project_agent_response SET {sql} WHERE id = ?1"),
                    [id],
                )
                .unwrap();
        };
        assert!(matches!(
            store.get_agent_response(&meta.id).unwrap(),
            ResponseOutcome::Usable(_)
        ));

        reset("response_version = 2", &meta.id);
        assert_eq!(unusable(&meta.id), UnusableReason::UnsupportedVersion(2));
        reset("response_version = 1", &meta.id);

        // Valid JSON of the wrong shape (the column is `json_valid`-checked).
        reset("details_json = '{\"project_id\": 7}'", &meta.id);
        assert!(matches!(
            unusable(&meta.id),
            UnusableReason::MalformedJson(_)
        ));

        let (_, details) = StoredAgentResponseV1::surveyor(&briefing())
            .encode()
            .unwrap();
        reset(
            &format!("details_json = '{}'", details.replace('\'', "''")),
            &meta.id,
        );
        reset("summary_text = 'edited'", &meta.id);
        assert!(matches!(
            unusable(&meta.id),
            UnusableReason::Inconsistent(_)
        ));

        // An Operator row without commands is inconsistent too.
        reset(
            &format!("details_json = '{}'", details.replace('\'', "''")),
            &operator.id,
        );
        reset(
            &format!(
                "summary_text = '{}'",
                briefing().summary_payload.content.replace('\'', "''")
            ),
            &operator.id,
        );
        assert!(matches!(
            unusable(&operator.id),
            UnusableReason::Inconsistent(_)
        ));

        // Unusable rows are still listed and still present.
        let listed = store
            .latest_agent_responses(
                ResponseQuery {
                    project_id: &project,
                    project_path_id: &path,
                    agent: StoredAgent::Surveyor,
                    request_key: "r",
                    config_key: "c",
                },
                10,
            )
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(matches!(listed[0], ResponseOutcome::Unusable(_)));
        let count: i64 = store
            .lock()
            .query_row("SELECT COUNT(*) FROM project_agent_response", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn latest_orders_by_created_at_then_id_descending_with_a_limit() {
        let (store, project, path, _dir) = store();
        let dto = StoredAgentResponseV1::chronicler(&briefing());
        let ids: Vec<String> = (0..4)
            .map(|_| insert(&store, &project, &path, &dto).id)
            .collect();
        // Two share the newest timestamp, so the id breaks the tie.
        for (id, time) in ids
            .iter()
            .zip(["2024-01-01", "2024-01-03", "2024-01-03", "2024-01-02"])
        {
            store
                .lock()
                .execute(
                    "UPDATE project_agent_response SET created_at = ?2 WHERE id = ?1",
                    [id, &format!("{time}T00:00:00Z")],
                )
                .unwrap();
        }
        let query = ResponseQuery {
            project_id: &project,
            project_path_id: &path,
            agent: StoredAgent::Chronicler,
            request_key: "r",
            config_key: "c",
        };
        let ordered = |limit| -> Vec<String> {
            store
                .latest_agent_responses(query, limit)
                .unwrap()
                .iter()
                .map(|row| row.meta().id.clone())
                .collect()
        };
        let (a, b) = if ids[1] > ids[2] {
            (&ids[1], &ids[2])
        } else {
            (&ids[2], &ids[1])
        };
        assert_eq!(
            ordered(10),
            [a.clone(), b.clone(), ids[3].clone(), ids[0].clone()]
        );
        assert_eq!(ordered(2), [a.clone(), b.clone()]);
        // Other agents and keys never match.
        let other = ResponseQuery {
            agent: StoredAgent::Operator,
            ..query
        };
        assert!(store.latest_agent_responses(other, 10).unwrap().is_empty());
        let other = ResponseQuery {
            config_key: "x",
            ..query
        };
        assert!(store.latest_agent_responses(other, 10).unwrap().is_empty());
    }

    #[test]
    fn an_unregistered_path_is_a_storage_error_not_a_row() {
        let (store, project, _path, _dir) = store();
        let error = store
            .insert_agent_response(NewResponse {
                project_id: &project,
                project_path_id: "path-missing",
                request_key: "r",
                config_key: "c",
                commit_hash: None,
                repository_subpath: None,
                reusable: false,
                response: &StoredAgentResponseV1::chronicler(&briefing()),
            })
            .unwrap_err();
        assert!(matches!(error, StoreError::Storage(_)));
        assert!(matches!(
            store.get_agent_response("response-nope"),
            Err(StoreError::NotFound(_))
        ));
    }
}
