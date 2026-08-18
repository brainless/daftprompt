// Sequential schema migrations for the durable conversation store.
// Each migration is a SQL string; the schema_version table tracks which
// migrations have been applied.

use rusqlite::Connection;

pub const CURRENT_VERSION: i64 = 1;

const MIGRATIONS: &[&str] = &[
    // Version 1: initial schema — acp_sessions, turns, retrieval_runs,
    // retrieval_candidates, acp_events, permission_decisions.
    r#"
    CREATE TABLE IF NOT EXISTS schema_version (
        id INTEGER PRIMARY KEY CHECK(id = 1),
        version INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS acp_sessions (
        id INTEGER PRIMARY KEY,
        app_session_id TEXT UNIQUE NOT NULL,
        acp_session_id TEXT,
        adapter_command TEXT,
        adapter_name TEXT,
        adapter_version TEXT,
        protocol_version TEXT,
        capabilities_json TEXT,
        auth_methods_json TEXT,
        cwd TEXT NOT NULL,
        created_at TEXT NOT NULL,
        closed_at TEXT
    );

    CREATE TABLE IF NOT EXISTS turns (
        id INTEGER PRIMARY KEY,
        session_id INTEGER NOT NULL REFERENCES acp_sessions(id),
        state TEXT NOT NULL CHECK(state IN ('preparing','running','completed','cancelled','failed')),
        original_prompt TEXT NOT NULL,
        enriched_prompt TEXT,
        formatter_version INTEGER,
        budget_json TEXT,
        retrieval_status TEXT,
        stop_reason TEXT,
        error_message TEXT,
        created_at TEXT NOT NULL,
        completed_at TEXT
    );

    CREATE TABLE IF NOT EXISTS retrieval_runs (
        id INTEGER PRIMARY KEY,
        turn_id INTEGER NOT NULL REFERENCES turns(id),
        query TEXT NOT NULL,
        limit_per_source INTEGER NOT NULL,
        status TEXT NOT NULL,
        error_message TEXT,
        latency_ms INTEGER,
        created_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS retrieval_candidates (
        id INTEGER PRIMARY KEY,
        run_id INTEGER NOT NULL REFERENCES retrieval_runs(id),
        identifier TEXT NOT NULL,
        source TEXT NOT NULL,
        rank INTEGER NOT NULL,
        score REAL NOT NULL,
        match_type TEXT NOT NULL,
        text TEXT NOT NULL,
        location_json TEXT NOT NULL,
        included BOOLEAN NOT NULL,
        truncated BOOLEAN,
        truncation_reason TEXT,
        original_len INTEGER,
        exclusion_reason TEXT
    );

    CREATE TABLE IF NOT EXISTS acp_events (
        id INTEGER PRIMARY KEY,
        turn_id INTEGER REFERENCES turns(id),
        session_id INTEGER NOT NULL REFERENCES acp_sessions(id),
        sequence INTEGER NOT NULL,
        direction TEXT NOT NULL CHECK(direction IN ('inbound','outbound')),
        event_kind TEXT NOT NULL,
        method TEXT,
        correlation_id TEXT,
        payload_json TEXT NOT NULL,
        timestamp TEXT NOT NULL,
        UNIQUE(session_id, sequence)
    );

    CREATE TABLE IF NOT EXISTS permission_decisions (
        id INTEGER PRIMARY KEY,
        event_id INTEGER NOT NULL REFERENCES acp_events(id),
        turn_id INTEGER NOT NULL REFERENCES turns(id),
        tool_call_json TEXT NOT NULL,
        offered_options_json TEXT NOT NULL,
        chosen_option_id TEXT,
        outcome TEXT NOT NULL CHECK(outcome IN ('selected','cancelled')),
        created_at TEXT NOT NULL
    );
    "#,
];

pub fn run_migrations(db: &Connection) -> anyhow::Result<()> {
    // Create schema_version if it doesn't exist yet (first migration handles this,
    // but we also check here for the bootstrap case).
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            version INTEGER NOT NULL
        )",
    )?;

    let current: i64 = db
        .query_row(
            "SELECT COALESCE((SELECT version FROM schema_version WHERE id = 1), 0)",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (i, migration_sql) in MIGRATIONS.iter().enumerate() {
        let target_version = (i as i64) + 1;
        if target_version <= current {
            continue;
        }
        db.execute_batch(migration_sql)?;
        if current == 0 {
            db.execute(
                "INSERT INTO schema_version(id, version) VALUES (1, ?)",
                [target_version],
            )?;
        } else {
            db.execute(
                "UPDATE schema_version SET version = ? WHERE id = 1",
                [target_version],
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_creates_tables() {
        let db = Connection::open_in_memory().unwrap();
        run_migrations(&db).unwrap();

        for table in &[
            "schema_version",
            "acp_sessions",
            "turns",
            "retrieval_runs",
            "retrieval_candidates",
            "acp_events",
            "permission_decisions",
        ] {
            let count: i64 = db
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{}'",
                        table
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "table '{}' should exist", table);
        }
    }

    #[test]
    fn test_schema_version_set() {
        let db = Connection::open_in_memory().unwrap();
        run_migrations(&db).unwrap();

        let version: i64 = db
            .query_row("SELECT version FROM schema_version WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    #[test]
    fn test_migrations_idempotent() {
        let db = Connection::open_in_memory().unwrap();
        run_migrations(&db).unwrap();
        run_migrations(&db).unwrap();

        let version: i64 = db
            .query_row("SELECT version FROM schema_version WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }
}
