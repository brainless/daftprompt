//! Durable conversation and trace storage for ACP prompt enrichment.
//!
//! This crate provides a separate SQLite database for conversation records
//! (sessions, turns, retrieval runs, ACP events, permission decisions).
//! It is intentionally independent of `daftprompt-indexer` — conversation
//! records survive `--reindex`.
//!
//! The DB file lives under `~/Library/Caches/daftprompt/conversations/`
//! (macOS) and is separate from the per-repo search index DBs.

pub mod redact;
pub mod schema;

use anyhow::{anyhow, Result};
use chrono::Utc;
use redact::redact_secrets;
use rusqlite::{params, Connection};
use schema::run_migrations;
use std::path::{Path, PathBuf};

/// Errors returned by the durable conversation store.
#[derive(Debug)]
pub enum StorageError {
    /// Underlying SQLite failure.
    Db(rusqlite::Error),
    /// The enriched prompt may only be set exactly once per turn.
    EnrichedPromptAlreadySet(i64),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Db(e) => write!(f, "database error: {e}"),
            StorageError::EnrichedPromptAlreadySet(turn_id) => write!(
                f,
                "enriched prompt already set for turn {turn_id}; it can only be set once"
            ),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        StorageError::Db(e)
    }
}

pub struct ConversationStore {
    db: Connection,
}

// --- Query result types ---

pub struct SessionRow {
    pub id: i64,
    pub app_session_id: String,
    pub acp_session_id: Option<String>,
    pub adapter_command: Option<String>,
    pub adapter_name: Option<String>,
    pub adapter_version: Option<String>,
    pub protocol_version: Option<String>,
    pub capabilities_json: Option<String>,
    pub auth_methods_json: Option<String>,
    pub cwd: String,
    pub created_at: String,
    pub closed_at: Option<String>,
}

pub struct TurnRow {
    pub id: i64,
    pub session_id: i64,
    pub state: String,
    pub original_prompt: String,
    pub enriched_prompt: Option<String>,
    pub formatter_version: Option<i64>,
    pub budget_json: Option<String>,
    pub retrieval_status: Option<String>,
    pub stop_reason: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

pub struct RetrievalRunRow {
    pub id: i64,
    pub turn_id: i64,
    pub query: String,
    pub limit_per_source: i64,
    pub status: String,
    pub error_message: Option<String>,
    pub latency_ms: Option<i64>,
    pub created_at: String,
}

pub struct RetrievalCandidateRow {
    pub id: i64,
    pub run_id: i64,
    pub identifier: String,
    pub source: String,
    pub rank: i64,
    pub score: f64,
    pub match_type: String,
    pub text: String,
    pub location_json: String,
    pub included: bool,
    pub truncated: Option<bool>,
    pub truncation_reason: Option<String>,
    pub original_len: Option<i64>,
    pub exclusion_reason: Option<String>,
}

pub struct EventRow {
    pub id: i64,
    pub turn_id: Option<i64>,
    pub session_id: i64,
    pub sequence: i64,
    pub direction: String,
    pub event_kind: String,
    pub method: Option<String>,
    pub correlation_id: Option<String>,
    pub payload_json: String,
    pub timestamp: String,
}

pub struct PermissionDecisionRow {
    pub id: i64,
    pub event_id: i64,
    pub turn_id: i64,
    pub tool_call_json: String,
    pub offered_options_json: String,
    pub chosen_option_id: Option<String>,
    pub outcome: String,
    pub created_at: String,
}

// Valid state transitions: preparing -> running -> completed/cancelled/failed.
// Terminal states (completed, cancelled, failed) cannot transition.
const VALID_TRANSITIONS: &[(&str, &[&str])] = &[
    ("preparing", &["running", "cancelled", "failed"]),
    ("running", &["completed", "cancelled", "failed"]),
    ("completed", &[]),
    ("cancelled", &[]),
    ("failed", &[]),
];

impl ConversationStore {
    /// Open or create the conversation database, running all pending
    /// migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL")?;
        db.execute_batch("PRAGMA foreign_keys=ON")?;
        run_migrations(&db)?;
        Ok(Self { db })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch("PRAGMA foreign_keys=ON")?;
        run_migrations(&db)?;
        Ok(Self { db })
    }

    /// Default path for a conversation DB: `~/Library/Caches/daftprompt/conversations/{slug}.db`
    pub fn default_path_for_repo(repo_path: &Path) -> Result<PathBuf> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| anyhow!("Cannot determine cache directory"))?
            .join("daftprompt")
            .join("conversations");
        std::fs::create_dir_all(&cache_dir)?;

        let abs_path = std::fs::canonicalize(repo_path)?;
        let abs_str = abs_path.to_string_lossy();
        let stem: String = abs_str
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let hash = {
            let mut h: u64 = 0;
            for b in abs_str.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u64);
            }
            format!("{:06x}", h & 0xFFFFFF)
        };
        let slug = format!("{}_{}", stem, hash);
        Ok(cache_dir.join(format!("{}.db", slug)))
    }

    // --- Session CRUD ---

    pub fn create_session(
        &self,
        app_session_id: &str,
        cwd: &str,
        adapter_command: Option<&str>,
        adapter_name: Option<&str>,
        adapter_version: Option<&str>,
        acp_session_id: Option<&str>,
        protocol_version: Option<&str>,
        capabilities_json: Option<&str>,
        auth_methods_json: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT INTO acp_sessions(app_session_id, acp_session_id, adapter_command, adapter_name, \
             adapter_version, protocol_version, capabilities_json, auth_methods_json, cwd, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                app_session_id,
                acp_session_id,
                adapter_command,
                adapter_name,
                adapter_version,
                protocol_version,
                capabilities_json,
                auth_methods_json,
                cwd,
                now,
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn close_session(&self, session_id: i64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "UPDATE acp_sessions SET closed_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        Ok(())
    }

    pub fn get_session(&self, session_id: i64) -> Result<Option<SessionRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, app_session_id, acp_session_id, adapter_command, adapter_name, \
             adapter_version, protocol_version, capabilities_json, auth_methods_json, \
             cwd, created_at, closed_at FROM acp_sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![session_id], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                app_session_id: row.get(1)?,
                acp_session_id: row.get(2)?,
                adapter_command: row.get(3)?,
                adapter_name: row.get(4)?,
                adapter_version: row.get(5)?,
                protocol_version: row.get(6)?,
                capabilities_json: row.get(7)?,
                auth_methods_json: row.get(8)?,
                cwd: row.get(9)?,
                created_at: row.get(10)?,
                closed_at: row.get(11)?,
            })
        })?;
        match rows.next() {
            Some(Ok(row)) => Ok(Some(row)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn get_session_by_app_id(&self, app_session_id: &str) -> Result<Option<SessionRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, app_session_id, acp_session_id, adapter_command, adapter_name, \
             adapter_version, protocol_version, capabilities_json, auth_methods_json, \
             cwd, created_at, closed_at FROM acp_sessions WHERE app_session_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![app_session_id], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                app_session_id: row.get(1)?,
                acp_session_id: row.get(2)?,
                adapter_command: row.get(3)?,
                adapter_name: row.get(4)?,
                adapter_version: row.get(5)?,
                protocol_version: row.get(6)?,
                capabilities_json: row.get(7)?,
                auth_methods_json: row.get(8)?,
                cwd: row.get(9)?,
                created_at: row.get(10)?,
                closed_at: row.get(11)?,
            })
        })?;
        match rows.next() {
            Some(Ok(row)) => Ok(Some(row)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    // --- Turn CRUD ---

    pub fn create_turn(&self, session_id: i64, original_prompt: &str) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT INTO turns(session_id, state, original_prompt, created_at) VALUES (?1, 'preparing', ?2, ?3)",
            params![session_id, original_prompt, now],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn transition_turn(&self, turn_id: i64, new_state: &str) -> Result<()> {
        let current_state: String = self.db.query_row(
            "SELECT state FROM turns WHERE id = ?1",
            params![turn_id],
            |row| row.get(0),
        )?;

        let allowed = VALID_TRANSITIONS
            .iter()
            .find(|(s, _)| *s == current_state.as_str())
            .map(|(_, allowed)| *allowed)
            .unwrap_or(&[]);

        if !allowed.contains(&new_state) {
            return Err(anyhow!(
                "Invalid state transition: {} -> {}",
                current_state,
                new_state
            ));
        }

        let completed_at = if new_state == "completed"
            || new_state == "cancelled"
            || new_state == "failed"
        {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        self.db.execute(
            "UPDATE turns SET state = ?1, completed_at = ?2 WHERE id = ?3",
            params![new_state, completed_at, turn_id],
        )?;
        Ok(())
    }

    /// Set the enriched prompt for a turn. The enriched prompt is set exactly
    /// once when enrichment completes; a second call on a turn whose
    /// `enriched_prompt` is already set is rejected rather than overwritten.
    pub fn set_enriched_prompt(
        &self,
        turn_id: i64,
        enriched_prompt: &str,
        formatter_version: i64,
        budget_json: Option<&str>,
        retrieval_status: &str,
    ) -> std::result::Result<(), StorageError> {
        let existing: Option<String> = self.db.query_row(
            "SELECT enriched_prompt FROM turns WHERE id = ?1",
            params![turn_id],
            |row| row.get(0),
        )?;
        if existing.is_some() {
            return Err(StorageError::EnrichedPromptAlreadySet(turn_id));
        }
        self.db.execute(
            "UPDATE turns SET enriched_prompt = ?1, formatter_version = ?2, budget_json = ?3, retrieval_status = ?4 WHERE id = ?5",
            params![enriched_prompt, formatter_version, budget_json, retrieval_status, turn_id],
        )?;
        Ok(())
    }

    pub fn set_turn_stop_reason(&self, turn_id: i64, stop_reason: &str) -> Result<()> {
        self.db.execute(
            "UPDATE turns SET stop_reason = ?1 WHERE id = ?2",
            params![stop_reason, turn_id],
        )?;
        Ok(())
    }

    pub fn set_turn_error(&self, turn_id: i64, error_message: &str) -> Result<()> {
        self.db.execute(
            "UPDATE turns SET error_message = ?1 WHERE id = ?2",
            params![error_message, turn_id],
        )?;
        Ok(())
    }

    pub fn get_turn(&self, turn_id: i64) -> Result<Option<TurnRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, session_id, state, original_prompt, enriched_prompt, formatter_version, \
             budget_json, retrieval_status, stop_reason, error_message, created_at, completed_at \
             FROM turns WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![turn_id], |row| {
            Ok(TurnRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                state: row.get(2)?,
                original_prompt: row.get(3)?,
                enriched_prompt: row.get(4)?,
                formatter_version: row.get(5)?,
                budget_json: row.get(6)?,
                retrieval_status: row.get(7)?,
                stop_reason: row.get(8)?,
                error_message: row.get(9)?,
                created_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        })?;
        match rows.next() {
            Some(Ok(row)) => Ok(Some(row)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn get_turns_for_session(&self, session_id: i64) -> Result<Vec<TurnRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, session_id, state, original_prompt, enriched_prompt, formatter_version, \
             budget_json, retrieval_status, stop_reason, error_message, created_at, completed_at \
             FROM turns WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(TurnRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                state: row.get(2)?,
                original_prompt: row.get(3)?,
                enriched_prompt: row.get(4)?,
                formatter_version: row.get(5)?,
                budget_json: row.get(6)?,
                retrieval_status: row.get(7)?,
                stop_reason: row.get(8)?,
                error_message: row.get(9)?,
                created_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // --- Retrieval run CRUD ---

    pub fn create_retrieval_run(
        &self,
        turn_id: i64,
        query: &str,
        limit_per_source: i64,
        status: &str,
        error_message: Option<&str>,
        latency_ms: Option<i64>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT INTO retrieval_runs(turn_id, query, limit_per_source, status, error_message, latency_ms, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![turn_id, query, limit_per_source, status, error_message, latency_ms, now],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn get_retrieval_run(&self, run_id: i64) -> Result<Option<RetrievalRunRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, turn_id, query, limit_per_source, status, error_message, latency_ms, created_at \
             FROM retrieval_runs WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![run_id], |row| {
            Ok(RetrievalRunRow {
                id: row.get(0)?,
                turn_id: row.get(1)?,
                query: row.get(2)?,
                limit_per_source: row.get(3)?,
                status: row.get(4)?,
                error_message: row.get(5)?,
                latency_ms: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        match rows.next() {
            Some(Ok(row)) => Ok(Some(row)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn get_retrieval_runs_for_turn(&self, turn_id: i64) -> Result<Vec<RetrievalRunRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, turn_id, query, limit_per_source, status, error_message, latency_ms, created_at \
             FROM retrieval_runs WHERE turn_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![turn_id], |row| {
            Ok(RetrievalRunRow {
                id: row.get(0)?,
                turn_id: row.get(1)?,
                query: row.get(2)?,
                limit_per_source: row.get(3)?,
                status: row.get(4)?,
                error_message: row.get(5)?,
                latency_ms: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // --- Retrieval candidate CRUD ---

    pub fn insert_retrieval_candidate(
        &self,
        run_id: i64,
        identifier: &str,
        source: &str,
        rank: i64,
        score: f64,
        match_type: &str,
        text: &str,
        location_json: &str,
        included: bool,
        truncated: Option<bool>,
        truncation_reason: Option<&str>,
        original_len: Option<i64>,
        exclusion_reason: Option<&str>,
    ) -> Result<i64> {
        self.db.execute(
            "INSERT INTO retrieval_candidates(run_id, identifier, source, rank, score, match_type, \
             text, location_json, included, truncated, truncation_reason, original_len, exclusion_reason) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                run_id,
                identifier,
                source,
                rank,
                score,
                match_type,
                text,
                location_json,
                included,
                truncated,
                truncation_reason,
                original_len,
                exclusion_reason,
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn get_candidates_for_run(&self, run_id: i64) -> Result<Vec<RetrievalCandidateRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, run_id, identifier, source, rank, score, match_type, text, location_json, \
             included, truncated, truncation_reason, original_len, exclusion_reason \
             FROM retrieval_candidates WHERE run_id = ?1 ORDER BY rank",
        )?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok(RetrievalCandidateRow {
                id: row.get(0)?,
                run_id: row.get(1)?,
                identifier: row.get(2)?,
                source: row.get(3)?,
                rank: row.get(4)?,
                score: row.get(5)?,
                match_type: row.get(6)?,
                text: row.get(7)?,
                location_json: row.get(8)?,
                included: row.get::<_, i64>(9)? != 0,
                truncated: row.get::<_, Option<i64>>(10)?.map(|v| v != 0),
                truncation_reason: row.get(11)?,
                original_len: row.get(12)?,
                exclusion_reason: row.get(13)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // --- Event CRUD ---

    pub fn append_event(
        &self,
        session_id: i64,
        turn_id: Option<i64>,
        direction: &str,
        event_kind: &str,
        method: Option<&str>,
        correlation_id: Option<&str>,
        payload_json: &str,
    ) -> Result<i64> {
        let redacted = redact_secrets(payload_json);
        let now = Utc::now().to_rfc3339();

        let sequence: i64 = self.db.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM acp_events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;

        self.db.execute(
            "INSERT INTO acp_events(turn_id, session_id, sequence, direction, event_kind, \
             method, correlation_id, payload_json, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                turn_id,
                session_id,
                sequence,
                direction,
                event_kind,
                method,
                correlation_id,
                redacted,
                now,
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn get_events_for_session(&self, session_id: i64) -> Result<Vec<EventRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, turn_id, session_id, sequence, direction, event_kind, method, \
             correlation_id, payload_json, timestamp \
             FROM acp_events WHERE session_id = ?1 ORDER BY sequence",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(EventRow {
                id: row.get(0)?,
                turn_id: row.get(1)?,
                session_id: row.get(2)?,
                sequence: row.get(3)?,
                direction: row.get(4)?,
                event_kind: row.get(5)?,
                method: row.get(6)?,
                correlation_id: row.get(7)?,
                payload_json: row.get(8)?,
                timestamp: row.get(9)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn get_events_for_turn(&self, turn_id: i64) -> Result<Vec<EventRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, turn_id, session_id, sequence, direction, event_kind, method, \
             correlation_id, payload_json, timestamp \
             FROM acp_events WHERE turn_id = ?1 ORDER BY sequence",
        )?;
        let rows = stmt.query_map(params![turn_id], |row| {
            Ok(EventRow {
                id: row.get(0)?,
                turn_id: row.get(1)?,
                session_id: row.get(2)?,
                sequence: row.get(3)?,
                direction: row.get(4)?,
                event_kind: row.get(5)?,
                method: row.get(6)?,
                correlation_id: row.get(7)?,
                payload_json: row.get(8)?,
                timestamp: row.get(9)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // --- Permission decision CRUD ---

    pub fn record_permission(
        &self,
        event_id: i64,
        turn_id: i64,
        tool_call_json: &str,
        offered_options_json: &str,
        chosen_option_id: Option<&str>,
        outcome: &str,
    ) -> Result<i64> {
        let redacted_tool = redact_secrets(tool_call_json);
        let redacted_options = redact_secrets(offered_options_json);
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT INTO permission_decisions(event_id, turn_id, tool_call_json, offered_options_json, \
             chosen_option_id, outcome, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event_id,
                turn_id,
                redacted_tool,
                redacted_options,
                chosen_option_id,
                outcome,
                now,
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn get_permissions_for_turn(&self, turn_id: i64) -> Result<Vec<PermissionDecisionRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, event_id, turn_id, tool_call_json, offered_options_json, \
             chosen_option_id, outcome, created_at \
             FROM permission_decisions WHERE turn_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![turn_id], |row| {
            Ok(PermissionDecisionRow {
                id: row.get(0)?,
                event_id: row.get(1)?,
                turn_id: row.get(2)?,
                tool_call_json: row.get(3)?,
                offered_options_json: row.get(4)?,
                chosen_option_id: row.get(5)?,
                outcome: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_is_not_indexer_path() {
        let tmp = tempfile::tempdir().unwrap();
        let conv_path = ConversationStore::default_path_for_repo(tmp.path()).unwrap();
        // Conversation DB should be under conversations/ subdirectory
        assert!(conv_path.to_string_lossy().contains("conversations"));
    }
}
