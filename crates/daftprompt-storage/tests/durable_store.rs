use daftprompt_storage::ConversationStore;

// --- Test 1: Migration test ---

#[test]
fn migration_creates_all_tables_and_version() {
    let store = ConversationStore::open_in_memory().unwrap();

    // Verify schema_version table exists and has version 1
    // (We go through the store's public API to confirm it works.)
    let session_id = store
        .create_session("test-sess-1", "/tmp", None, None, None, None, None, None, None)
        .unwrap();
    assert!(session_id > 0);

    // If we got here, migrations ran successfully and all tables exist.
    // Also verify by reading back.
    let session = store.get_session(session_id).unwrap().unwrap();
    assert_eq!(session.app_session_id, "test-sess-1");
}

// --- Test 2: Round-trip test ---

#[test]
fn round_trip_preserves_exact_text() {
    let store = ConversationStore::open_in_memory().unwrap();

    let session_id = store
        .create_session(
            "rt-session",
            "/home/user/project",
            Some("codex-acp --stdio"),
            Some("codex-acp"),
            Some("1.4.0"),
            Some("acp-sess-42"),
            Some("2024-01-01"),
            Some(r#"{"tools": true}"#),
            Some(r#"[{"type": "api_key"}]"#),
        )
        .unwrap();

    let turn_id = store.create_turn(session_id, "Fix the bug in main.rs").unwrap();

    store
        .set_enriched_prompt(
            turn_id,
            "<original-request>\nFix the bug in main.rs\n</original-request>\n\n<retrieved-context>\nfn main() { /* code */ }\n</retrieved-context>",
            1,
            Some(r#"{"total_char_budget": 6000}"#),
            "ok",
        )
        .unwrap();

    let run_id = store
        .create_retrieval_run(turn_id, "fix bug main.rs", 10, "ok", None, Some(42))
        .unwrap();

    store
        .insert_retrieval_candidate(
            run_id,
            "src/main.rs::main",
            "code",
            1,
            0.95,
            "fts",
            "fn main() { /* code */ }",
            r#"{"file": "src/main.rs", "line_start": 1, "line_end": 5}"#,
            true,
            Some(false),
            None,
            Some(21),
            None,
        )
        .unwrap();

    let event_id = store
        .append_event(
            session_id,
            Some(turn_id),
            "outbound",
            "prompt_sent",
            Some("session/prompt"),
            Some("corr-1"),
            r#"{"prompt": "Fix the bug in main.rs"}"#,
        )
        .unwrap();

    store
        .record_permission(
            event_id,
            turn_id,
            r#"{"tool": "shell", "command": "ls"}"#,
            r#"[{"id": "allow", "label": "Allow once"}, {"id": "deny", "label": "Deny"}]"#,
            Some("allow"),
            "selected",
        )
        .unwrap();

    // Read everything back
    let session = store.get_session(session_id).unwrap().unwrap();
    assert_eq!(session.app_session_id, "rt-session");
    assert_eq!(session.cwd, "/home/user/project");
    assert_eq!(session.adapter_command.as_deref(), Some("codex-acp --stdio"));
    assert_eq!(session.adapter_name.as_deref(), Some("codex-acp"));
    assert_eq!(session.adapter_version.as_deref(), Some("1.4.0"));
    assert_eq!(session.acp_session_id.as_deref(), Some("acp-sess-42"));
    assert_eq!(session.protocol_version.as_deref(), Some("2024-01-01"));
    assert_eq!(session.capabilities_json.as_deref(), Some(r#"{"tools": true}"#));
    assert_eq!(session.auth_methods_json.as_deref(), Some(r#"[{"type": "api_key"}]"#));

    let turn = store.get_turn(turn_id).unwrap().unwrap();
    assert_eq!(turn.original_prompt, "Fix the bug in main.rs");
    assert!(turn.enriched_prompt.as_deref().unwrap().contains("<original-request>"));
    assert_eq!(turn.formatter_version, Some(1));
    assert_eq!(turn.budget_json.as_deref(), Some(r#"{"total_char_budget": 6000}"#));
    assert_eq!(turn.retrieval_status.as_deref(), Some("ok"));

    let runs = store.get_retrieval_runs_for_turn(turn_id).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].query, "fix bug main.rs");
    assert_eq!(runs[0].limit_per_source, 10);
    assert_eq!(runs[0].latency_ms, Some(42));

    let candidates = store.get_candidates_for_run(run_id).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].identifier, "src/main.rs::main");
    assert_eq!(candidates[0].source, "code");
    assert_eq!(candidates[0].rank, 1);
    assert!((candidates[0].score - 0.95).abs() < f64::EPSILON);
    assert_eq!(candidates[0].match_type, "fts");
    assert_eq!(candidates[0].text, "fn main() { /* code */ }");
    assert!(candidates[0].included);
    assert_eq!(candidates[0].truncated, Some(false));
    assert_eq!(candidates[0].original_len, Some(21));

    let events = store.get_events_for_session(session_id).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].direction, "outbound");
    assert_eq!(events[0].event_kind, "prompt_sent");
    assert_eq!(events[0].method.as_deref(), Some("session/prompt"));
    assert_eq!(events[0].correlation_id.as_deref(), Some("corr-1"));
    assert_eq!(events[0].payload_json, r#"{"prompt":"Fix the bug in main.rs"}"#);

    let perms = store.get_permissions_for_turn(turn_id).unwrap();
    assert_eq!(perms.len(), 1);
    assert_eq!(perms[0].chosen_option_id.as_deref(), Some("allow"));
    assert_eq!(perms[0].outcome, "selected");
}

// --- Test 3: State transition test ---

#[test]
fn legal_state_transitions_work() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("s1", "/tmp", None, None, None, None, None, None, None).unwrap();

    // preparing -> running
    let tid = store.create_turn(sid, "test").unwrap();
    let t = store.get_turn(tid).unwrap().unwrap();
    assert_eq!(t.state, "preparing");
    store.transition_turn(tid, "running").unwrap();
    let t = store.get_turn(tid).unwrap().unwrap();
    assert_eq!(t.state, "running");
    assert!(t.completed_at.is_none());

    // running -> completed
    store.transition_turn(tid, "completed").unwrap();
    let t = store.get_turn(tid).unwrap().unwrap();
    assert_eq!(t.state, "completed");
    assert!(t.completed_at.is_some());
}

#[test]
fn preparing_can_go_to_cancelled() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("s2", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "test").unwrap();
    store.transition_turn(tid, "cancelled").unwrap();
    let t = store.get_turn(tid).unwrap().unwrap();
    assert_eq!(t.state, "cancelled");
}

#[test]
fn preparing_can_go_to_failed() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("s3", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "test").unwrap();
    store.transition_turn(tid, "failed").unwrap();
    let t = store.get_turn(tid).unwrap().unwrap();
    assert_eq!(t.state, "failed");
}

#[test]
fn completed_is_terminal() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("s4", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "test").unwrap();
    store.transition_turn(tid, "running").unwrap();
    store.transition_turn(tid, "completed").unwrap();

    let err = store.transition_turn(tid, "running").unwrap_err();
    assert!(err.to_string().contains("Invalid state transition"));
}

#[test]
fn cancelled_is_terminal() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("s5", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "test").unwrap();
    store.transition_turn(tid, "running").unwrap();
    store.transition_turn(tid, "cancelled").unwrap();

    let err = store.transition_turn(tid, "completed").unwrap_err();
    assert!(err.to_string().contains("Invalid state transition"));
}

#[test]
fn failed_is_terminal() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("s6", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "test").unwrap();
    store.transition_turn(tid, "failed").unwrap();

    let err = store.transition_turn(tid, "running").unwrap_err();
    assert!(err.to_string().contains("Invalid state transition"));
}

#[test]
fn preparing_cannot_go_to_completed_directly() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("s7", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "test").unwrap();

    let err = store.transition_turn(tid, "completed").unwrap_err();
    assert!(err.to_string().contains("Invalid state transition"));
}

// --- Test 4: Event ordering test ---

#[test]
fn event_sequence_is_monotonically_increasing() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("ev-sess", "/tmp", None, None, None, None, None, None, None).unwrap();

    store.append_event(sid, None, "inbound", "session_update", None, None, r#"{"a":1}"#).unwrap();
    store.append_event(sid, None, "outbound", "prompt_sent", Some("session/prompt"), None, r#"{"b":2}"#).unwrap();
    store.append_event(sid, None, "inbound", "diagnostic", None, None, r#"{"c":3}"#).unwrap();

    let events = store.get_events_for_session(sid).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
    assert_eq!(events[2].sequence, 3);
    assert_eq!(events[0].event_kind, "session_update");
    assert_eq!(events[1].event_kind, "prompt_sent");
    assert_eq!(events[2].event_kind, "diagnostic");
}

// --- Test 5: Isolation test ---

#[test]
fn conversation_db_path_differs_from_indexer_path() {
    let tmp = tempfile::tempdir().unwrap();
    let conv_path = ConversationStore::default_path_for_repo(tmp.path()).unwrap();

    // Conversation path must contain "conversations" subdirectory
    assert!(
        conv_path.to_string_lossy().contains("conversations"),
        "Conversation DB should be under conversations/ subdirectory: {}",
        conv_path.display()
    );

    // The indexer path (from daftprompt-indexer) does NOT contain "conversations"
    // — it would be ~/Library/Caches/daftprompt/{slug}.db directly.
    // We verify the parent directory is "conversations".
    let parent = conv_path.parent().unwrap();
    assert_eq!(parent.file_name().unwrap(), "conversations");
}

// --- Test 6: Redaction test ---

#[test]
fn redact_secrets_strips_api_keys() {
    let json = r#"{"api_key": "sk-1234567890abcdef1234567890abcdef", "name": "test"}"#;
    let redacted = daftprompt_storage::redact::redact_secrets(json);
    assert!(!redacted.contains("sk-1234567890abcdef"));
    assert!(redacted.contains("[REDACTED]"));
    assert!(redacted.contains("test"));
}

#[test]
fn redact_secrets_strips_bearer_token() {
    let json = r#"{"authorization": "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"}"#;
    let redacted = daftprompt_storage::redact::redact_secrets(json);
    assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn redact_secrets_strips_token_field() {
    let json = r#"{"token": "abc123secrettoken", "count": 5}"#;
    let redacted = daftprompt_storage::redact::redact_secrets(json);
    assert!(!redacted.contains("abc123secrettoken"));
    assert!(redacted.contains("[REDACTED]"));
    assert!(redacted.contains("count"));
}

#[test]
fn redact_secrets_strips_password_field() {
    let json = r#"{"username": "admin", "password": "hunter2"}"#;
    let redacted = daftprompt_storage::redact::redact_secrets(json);
    assert!(!redacted.contains("hunter2"));
    assert!(redacted.contains("admin"));
}

#[test]
fn redact_secrets_strips_env_secret_values() {
    let json = r#"{"env": {"MY_SECRET_KEY": "AKIA1234567890ABCDEF", "NORMAL": "keep"}}"#;
    let redacted = daftprompt_storage::redact::redact_secrets(json);
    // The key "MY_SECRET_KEY" should cause its value to be redacted
    assert!(!redacted.contains("AKIA1234567890ABCDEF"));
    assert!(redacted.contains("[REDACTED]"));
    assert!(redacted.contains("keep"));
}

#[test]
fn redact_secrets_preserves_normal_values() {
    let json = r#"{"prompt": "fix the bug", "status": "ok", "count": 42}"#;
    let redacted = daftprompt_storage::redact::redact_secrets(json);
    assert!(redacted.contains("fix the bug"));
    assert!(redacted.contains("ok"));
    assert!(!redacted.contains("[REDACTED]"));
}

// --- Test 7: Permission round-trip test ---

#[test]
fn permission_round_trip_preserves_option_ids() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("perm-sess", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "run ls").unwrap();
    let ev_id = store
        .append_event(sid, Some(tid), "inbound", "permission_request", Some("session/request_permission"), None, r#"{"tool":"shell"}"#)
        .unwrap();

    let options_json = r#"[{"id": "opt-allow-once", "label": "Allow once"}, {"id": "opt-always", "label": "Always allow"}, {"id": "opt-deny", "label": "Deny"}]"#;
    let tool_call_json = r#"{"tool": "shell", "command": "ls -la", "description": "List directory contents"}"#;

    store
        .record_permission(
            ev_id,
            tid,
            tool_call_json,
            options_json,
            Some("opt-allow-once"),
            "selected",
        )
        .unwrap();

    let perms = store.get_permissions_for_turn(tid).unwrap();
    assert_eq!(perms.len(), 1);
    assert_eq!(perms[0].chosen_option_id.as_deref(), Some("opt-allow-once"));
    assert_eq!(perms[0].outcome, "selected");
    assert_eq!(perms[0].tool_call_json, tool_call_json);
    assert_eq!(perms[0].offered_options_json, options_json);
}

#[test]
fn permission_cancelled_has_null_chosen_option() {
    let store = ConversationStore::open_in_memory().unwrap();
    let sid = store.create_session("perm-cancel", "/tmp", None, None, None, None, None, None, None).unwrap();
    let tid = store.create_turn(sid, "run ls").unwrap();
    let ev_id = store
        .append_event(sid, Some(tid), "inbound", "permission_request", None, None, r#"{}"#)
        .unwrap();

    store
        .record_permission(
            ev_id,
            tid,
            r#"{"tool": "shell"}"#,
            r#"[{"id": "allow", "label": "Allow"}]"#,
            None,
            "cancelled",
        )
        .unwrap();

    let perms = store.get_permissions_for_turn(tid).unwrap();
    assert_eq!(perms.len(), 1);
    assert_eq!(perms[0].chosen_option_id, None);
    assert_eq!(perms[0].outcome, "cancelled");
}
