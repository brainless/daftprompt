use std::path::PathBuf;
use std::time::Duration;

use daftprompt::coordinator::{
    start_coordinator, CoordinatorCommand, CoordinatorConfig, CoordinatorEvent,
};
use daftprompt_acp::{AcpClient, AcpClientConfig, AdapterLaunchProfile};
use daftprompt_indexer::{CommitData, Indexer, IndexerConfig};
use daftprompt_prompt_builder::{RetrievalStatus, SelectionBudget, SourceQuota};
use daftprompt_storage::ConversationStore;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

fn fake_adapter_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest_dir);
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("target"));
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    let path = target_dir.join(&profile).join("acp-fake-adapter");
    if path.exists() {
        return path;
    }

    let status = std::process::Command::new("cargo")
        .args(["build", "-p", "daftprompt-acp", "--bin", "acp-fake-adapter"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to run cargo build");
    assert!(status.success(), "failed to build acp-fake-adapter");
    assert!(path.exists(), "acp-fake-adapter not found at {}", path.display());
    path
}

fn setup_test_indexer() -> (Arc<Mutex<Indexer>>, tempfile::TempDir) {
    let repo_dir = tempfile::tempdir().expect("repo tempdir");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["init"])
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["config", "user.email", "test@test.com"])
        .output()
        .expect("git config email");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["config", "user.name", "Test"])
        .output()
        .expect("git config name");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "--allow-empty", "-m", "init"])
        .output()
        .expect("git commit");

    let cache_dir = tempfile::tempdir().expect("cache tempdir");
    let config = IndexerConfig {
        cache_dir: Some(cache_dir.path().to_path_buf()),
        model_name: String::new(),
    };
    let indexer = Indexer::new(repo_dir.path(), &config).expect("Indexer::new");
    (Arc::new(Mutex::new(indexer)), repo_dir)
}

fn setup_test_indexer_with_commits(
    commits: &[CommitData],
) -> (Arc<Mutex<Indexer>>, tempfile::TempDir, tempfile::TempDir) {
    let repo_dir = tempfile::tempdir().expect("repo tempdir");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["init"])
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["config", "user.email", "test@test.com"])
        .output()
        .expect("git config email");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["config", "user.name", "Test"])
        .output()
        .expect("git config name");
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "--allow-empty", "-m", "init"])
        .output()
        .expect("git commit");

    let cache_dir = tempfile::tempdir().expect("cache tempdir");
    let config = IndexerConfig {
        cache_dir: Some(cache_dir.path().to_path_buf()),
        model_name: String::new(),
    };
    let mut indexer = Indexer::new(repo_dir.path(), &config).expect("Indexer::new");
    indexer
        .index_commits(commits)
        .expect("index deterministic commit candidates");
    (Arc::new(Mutex::new(indexer)), repo_dir, cache_dir)
}

fn launch_fake_adapter() -> (AcpClient, daftprompt_acp::AcpEvents) {
    let profile = AdapterLaunchProfile::new(fake_adapter_path());
    AcpClient::launch(
        profile,
        AcpClientConfig {
            request_timeout: Duration::from_secs(5),
            ..AcpClientConfig::default()
        },
    )
}

fn launch_fake_adapter_with_mode(mode: &str) -> (AcpClient, daftprompt_acp::AcpEvents) {
    let profile = AdapterLaunchProfile::new(fake_adapter_path()).env("FAKE_ADAPTER_MODE", mode);
    AcpClient::launch(
        profile,
        AcpClientConfig {
            request_timeout: Duration::from_secs(5),
            ..AcpClientConfig::default()
        },
    )
}

fn launch_fake_adapter_with_mode_and_timeout(
    mode: &str,
    request_timeout: Duration,
) -> (AcpClient, daftprompt_acp::AcpEvents) {
    let profile = AdapterLaunchProfile::new(fake_adapter_path()).env("FAKE_ADAPTER_MODE", mode);
    AcpClient::launch(
        profile,
        AcpClientConfig {
            request_timeout,
            ..AcpClientConfig::default()
        },
    )
}

/// Like `launch_fake_adapter_with_mode`, but also sets
/// `FAKE_ADAPTER_CLOSE_MARKER` so a test can observe (via the marker file's
/// existence) whether `session/close` was actually sent, without parsing the
/// adapter's own stdio traffic.
fn launch_fake_adapter_with_mode_and_close_marker(
    mode: &str,
    marker_path: &std::path::Path,
) -> (AcpClient, daftprompt_acp::AcpEvents) {
    let profile = AdapterLaunchProfile::new(fake_adapter_path())
        .env("FAKE_ADAPTER_MODE", mode)
        .env("FAKE_ADAPTER_CLOSE_MARKER", marker_path.to_string_lossy());
    AcpClient::launch(
        profile,
        AcpClientConfig {
            request_timeout: Duration::from_secs(5),
            ..AcpClientConfig::default()
        },
    )
}

fn setup_coordinator() -> (
    mpsc::UnboundedSender<CoordinatorCommand>,
    mpsc::UnboundedReceiver<CoordinatorEvent>,
    tempfile::TempDir,
) {
    let (indexer, repo_dir) = setup_test_indexer();
    let store = ConversationStore::open_in_memory().expect("in-memory store");
    let (acp_client, acp_events) = launch_fake_adapter();
    let config = CoordinatorConfig {
        retrieval_limit_per_source: 5,
        budget: SelectionBudget::default(),
        request_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(2),
    };
    let (cmd_tx, evt_rx) = start_coordinator(indexer, store, acp_client, acp_events, config);
    (cmd_tx, evt_rx, repo_dir)
}

fn setup_coordinator_with_mode(mode: &str) -> (
    mpsc::UnboundedSender<CoordinatorCommand>,
    mpsc::UnboundedReceiver<CoordinatorEvent>,
    tempfile::TempDir,
) {
    let (indexer, repo_dir) = setup_test_indexer();
    let store = ConversationStore::open_in_memory().expect("in-memory store");
    let (acp_client, acp_events) = launch_fake_adapter_with_mode(mode);
    let config = CoordinatorConfig {
        retrieval_limit_per_source: 5,
        budget: SelectionBudget::default(),
        request_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(2),
    };
    let (cmd_tx, evt_rx) = start_coordinator(indexer, store, acp_client, acp_events, config);
    (cmd_tx, evt_rx, repo_dir)
}

/// Like `setup_coordinator_with_mode`, but launches the fake adapter with
/// `FAKE_ADAPTER_CLOSE_MARKER` set to `marker_path` so a test can assert
/// whether `session/close` was sent (Task 6: capability-gated `session/close`).
fn setup_coordinator_with_mode_and_close_marker(
    mode: &str,
    marker_path: &std::path::Path,
) -> (
    mpsc::UnboundedSender<CoordinatorCommand>,
    mpsc::UnboundedReceiver<CoordinatorEvent>,
    tempfile::TempDir,
) {
    let (indexer, repo_dir) = setup_test_indexer();
    let store = ConversationStore::open_in_memory().expect("in-memory store");
    let (acp_client, acp_events) =
        launch_fake_adapter_with_mode_and_close_marker(mode, marker_path);
    let config = CoordinatorConfig {
        retrieval_limit_per_source: 5,
        budget: SelectionBudget::default(),
        request_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(2),
    };
    let (cmd_tx, evt_rx) = start_coordinator(indexer, store, acp_client, acp_events, config);
    (cmd_tx, evt_rx, repo_dir)
}

/// Like `setup_coordinator_with_mode`, but with a caller-controlled ACP
/// request timeout (used to force a fast, deterministic dispatch failure
/// against the "hang" fake-adapter mode) and a `ConversationStore` opened at
/// a caller-supplied path rather than in-memory, so a second connection to
/// the same file can be used to simulate a storage write failure.
fn setup_coordinator_with_store_path(
    store_path: &std::path::Path,
    request_timeout: Duration,
) -> (
    mpsc::UnboundedSender<CoordinatorCommand>,
    mpsc::UnboundedReceiver<CoordinatorEvent>,
    tempfile::TempDir,
) {
    let (indexer, repo_dir) = setup_test_indexer();
    let store = ConversationStore::open(store_path).expect("file-backed store");
    let (acp_client, acp_events) = launch_fake_adapter();
    let config = CoordinatorConfig {
        retrieval_limit_per_source: 5,
        budget: SelectionBudget::default(),
        request_timeout,
        shutdown_grace: Duration::from_secs(2),
    };
    let (cmd_tx, evt_rx) = start_coordinator(indexer, store, acp_client, acp_events, config);
    (cmd_tx, evt_rx, repo_dir)
}

async fn collect_until<F>(
    events: &mut mpsc::UnboundedReceiver<CoordinatorEvent>,
    pred: F,
) -> Vec<CoordinatorEvent>
where
    F: Fn(&CoordinatorEvent) -> bool,
{
    let mut collected = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(15), events.recv()).await {
            Ok(Some(event)) => {
                let done = pred(&event);
                collected.push(event);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    collected
}

/// Sends `Shutdown` and waits (bounded) for the coordinator to confirm it
/// via the `done` oneshot, exercising the same real shutdown path
/// `Application::shutdown_coordinator` uses in `src/main.rs` (Task 6).
async fn shutdown_and_wait(cmd_tx: &mpsc::UnboundedSender<CoordinatorCommand>) {
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    if cmd_tx.send(CoordinatorCommand::Shutdown { done: done_tx }).is_err() {
        return;
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), done_rx).await;
}

#[tokio::test]
async fn submit_prompt_end_to_end() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator();

    // Wait for SessionCreated
    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    // Submit a prompt
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "hello world".to_string(),
        })
        .unwrap();

    // Collect until TurnCompleted
    let turn_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;

    assert!(
        turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::TurnStarted { .. })),
        "expected TurnStarted"
    );
    assert!(
        turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::RetrievalCompleted { .. })),
        "expected RetrievalCompleted"
    );
    assert!(
        turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::EnrichedPromptReady { .. })),
        "expected EnrichedPromptReady"
    );

    let completed = turn_events
        .iter()
        .find_map(|e| match e {
            CoordinatorEvent::TurnCompleted { stop_reason, .. } => Some(stop_reason.as_str()),
            _ => None,
        });
    assert_eq!(completed, Some("EndTurn"), "expected EndTurn stop reason");

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn session_uses_indexed_repo_cwd_and_persists_launch_initialize_metadata() {
    let (indexer, repo_dir) = setup_test_indexer();
    let canonical_repo = std::fs::canonicalize(repo_dir.path()).expect("canonical repo path");

    let trace_dir = tempfile::tempdir().expect("trace tempdir");
    let trace_path = trace_dir.path().join("conversation.db");
    let cwd_marker = trace_dir.path().join("session-new-cwd.txt");
    let store = ConversationStore::open(&trace_path).expect("file-backed store");

    let adapter_path = fake_adapter_path();
    let adapter_arg = "argument with spaces";
    let profile = AdapterLaunchProfile::new(&adapter_path)
        .arg(adapter_arg)
        .env(
            "FAKE_ADAPTER_NEW_SESSION_CWD_MARKER",
            cwd_marker.to_string_lossy(),
        );
    let (acp_client, acp_events) = AcpClient::launch(
        profile,
        AcpClientConfig {
            request_timeout: Duration::from_secs(5),
            ..AcpClientConfig::default()
        },
    );
    let config = CoordinatorConfig {
        retrieval_limit_per_source: 5,
        budget: SelectionBudget::default(),
        request_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(2),
    };
    let (cmd_tx, mut evt_rx) =
        start_coordinator(indexer, store, acp_client, acp_events, config);

    let session_events = collect_until(&mut evt_rx, |event| {
        matches!(event, CoordinatorEvent::SessionCreated { .. })
    })
    .await;
    let session_db_id = session_events
        .iter()
        .find_map(|event| match event {
            CoordinatorEvent::SessionCreated { session_db_id, .. } => Some(*session_db_id),
            _ => None,
        })
        .expect("coordinator should create a durable session");

    let adapter_received_cwd =
        std::fs::read_to_string(&cwd_marker).expect("fake adapter should capture session/new cwd");
    assert_eq!(adapter_received_cwd, canonical_repo.to_string_lossy());

    shutdown_and_wait(&cmd_tx).await;
    drop(cmd_tx);
    drop(evt_rx);

    let store = ConversationStore::open(&trace_path).expect("reopen conversation store");
    let session = store
        .get_session(session_db_id)
        .expect("read durable session")
        .expect("durable session row");
    let expected_command = serde_json::to_string(&[
        adapter_path.to_string_lossy().into_owned(),
        adapter_arg.to_string(),
    ])
    .expect("serialize expected adapter argv");

    assert_eq!(session.cwd, canonical_repo.to_string_lossy());
    assert_eq!(
        session.adapter_command.as_deref(),
        Some(expected_command.as_str())
    );
    assert_eq!(
        session.adapter_name.as_deref(),
        Some("@agentclientprotocol/codex-acp")
    );
    assert_eq!(session.adapter_version.as_deref(), Some("1.4.0"));
    assert_eq!(session.protocol_version.as_deref(), Some("v1"));

    let capabilities: serde_json::Value = serde_json::from_str(
        session
            .capabilities_json
            .as_deref()
            .expect("capabilities should be durable"),
    )
    .expect("capabilities JSON");
    assert_eq!(capabilities["loadSession"], true);
    assert!(capabilities["sessionCapabilities"]["close"].is_object());

    let auth_methods: serde_json::Value = serde_json::from_str(
        session
            .auth_methods_json
            .as_deref()
            .expect("auth methods should be durable"),
    )
    .expect("auth methods JSON");
    assert_eq!(auth_methods[0]["id"], "api-key");
    assert_eq!(auth_methods[1]["id"], "chat-gpt");
}

#[tokio::test]
async fn empty_retrieval_sends_explicit_no_context_envelope() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator();

    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    // The test index contains no indexed items, so an ordinary natural-language
    // request deterministically exercises successful retrieval with zero hits.
    let original = "explain the nonexistent quasar subsystem";
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: original.to_string(),
        })
        .unwrap();

    let turn_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;

    assert!(turn_events.iter().any(|event| matches!(
        event,
        CoordinatorEvent::RetrievalCompleted {
            status,
            candidate_count: 0,
            included_count: 0,
            ..
        } if status == "ok"
    )));

    let enriched = turn_events
        .iter()
        .find_map(|event| match event {
            CoordinatorEvent::EnrichedPromptReady {
                original: recorded_original,
                enriched,
                ..
            } => Some((recorded_original, enriched)),
            _ => None,
        })
        .expect("expected the no-context enriched prompt to be emitted");

    assert_eq!(
        enriched.0, original,
        "the original prompt must remain exact"
    );
    assert!(matches!(
        enriched.1.retrieval_status,
        RetrievalStatus::Empty
    ));
    assert!(enriched.1.text.contains("status=\"empty\""));
    assert!(enriched.1.text.contains("<no-results/>"));
    assert!(enriched.1.included.is_empty());
    assert!(enriched.1.excluded.is_empty());
    assert!(
        turn_events.iter().any(|event| matches!(
            event,
            CoordinatorEvent::TurnCompleted { stop_reason, .. } if stop_reason == "EndTurn"
        )),
        "the adapter should receive and complete the no-context prompt"
    );

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn inbound_updates_and_diagnostics_are_durable_ordered_and_redacted() {
    let trace_dir = tempfile::tempdir().expect("trace tempdir");
    let trace_path = trace_dir.path().join("durable-events.db");
    let (cmd_tx, mut evt_rx, _repo) =
        setup_coordinator_with_store_path(&trace_path, Duration::from_secs(5));

    let session_events = collect_until(&mut evt_rx, |event| {
        matches!(event, CoordinatorEvent::SessionCreated { .. })
    })
    .await;
    let (session_db_id, acp_session_id) = session_events
        .iter()
        .find_map(|event| match event {
            CoordinatorEvent::SessionCreated {
                session_db_id,
                acp_session_id,
                ..
            } => Some((*session_db_id, acp_session_id.clone())),
            _ => None,
        })
        .expect("durable session identity");

    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "TRIGGER_UNKNOWN_UPDATE durable trace".to_string(),
        })
        .unwrap();

    let turn_events = collect_until(&mut evt_rx, |event| {
        matches!(
            event,
            CoordinatorEvent::AcpSessionUpdate { update_json, .. }
                if update_json.contains("Say the single word")
        )
    })
    .await;
    let turn_id = turn_events
        .iter()
        .find_map(|event| match event {
            CoordinatorEvent::TurnStarted { turn_id } => Some(*turn_id),
            _ => None,
        })
        .expect("turn id");

    if !turn_events
        .iter()
        .any(|event| matches!(event, CoordinatorEvent::TurnCompleted { .. }))
    {
        let _ = collect_until(&mut evt_rx, |event| {
            matches!(event, CoordinatorEvent::TurnCompleted { .. })
        })
        .await;
    }

    shutdown_and_wait(&cmd_tx).await;
    drop(cmd_tx);
    drop(evt_rx);

    let store = ConversationStore::open(&trace_path).expect("reopen durable event store");
    let persisted = store
        .get_events_for_session(session_db_id)
        .expect("read durable ACP events");
    assert!(persisted.len() >= 12, "expected the full fixture stream");

    for (index, event) in persisted.iter().enumerate() {
        assert_eq!(event.sequence, index as i64 + 1);
        assert_eq!(event.session_id, session_db_id);
        assert_eq!(event.turn_id, Some(turn_id));
        assert_eq!(event.direction, "inbound");
        assert_eq!(event.correlation_id.as_deref(), Some(acp_session_id.as_str()));
    }

    let unknown_update = persisted
        .iter()
        .find(|event| event.event_kind == "session_update_unknown")
        .expect("unknown session/update should be durable");
    assert_eq!(unknown_update.method.as_deref(), Some("session/update"));
    assert!(unknown_update.payload_json.contains("[REDACTED]"));
    assert!(!unknown_update.payload_json.contains("sk-durable-event-secret"));

    let unknown_notification = persisted
        .iter()
        .find(|event| {
            event.method.as_deref() == Some("session/unknown_notification_kind")
        })
        .expect("unknown notification diagnostic should be durable");
    assert_eq!(unknown_notification.event_kind, "diagnostic");
    assert!(unknown_notification.payload_json.contains("[REDACTED]"));
    assert!(!unknown_notification
        .payload_json
        .contains("durable-diagnostic-secret"));

    let unknown_request = persisted
        .iter()
        .find(|event| event.method.as_deref() == Some("session/unknown_request_kind"))
        .expect("unknown request diagnostic should be durable");
    assert_eq!(unknown_request.event_kind, "diagnostic");

    let agent_chunk = persisted
        .iter()
        .find(|event| event.event_kind == "agent_message_chunk")
        .expect("ordinary streamed agent response should be durable");
    assert_eq!(agent_chunk.method.as_deref(), Some("session/update"));
    let payload: serde_json::Value =
        serde_json::from_str(&agent_chunk.payload_json).expect("known update payload JSON");
    assert_eq!(payload["sessionId"], acp_session_id);
    assert_eq!(payload["update"]["sessionUpdate"], "agent_message_chunk");
}

#[tokio::test]
async fn inbound_event_persistence_failures_are_surfaced_without_panicking() {
    let trace_dir = tempfile::tempdir().expect("trace tempdir");
    let trace_path = trace_dir.path().join("broken-events.db");
    let (cmd_tx, mut evt_rx, _repo) =
        setup_coordinator_with_store_path(&trace_path, Duration::from_secs(5));

    let _ = collect_until(&mut evt_rx, |event| {
        matches!(event, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    let breaker = rusqlite::Connection::open(&trace_path).expect("second trace connection");
    breaker
        .execute_batch("DROP TABLE acp_events;")
        .expect("remove event table to force a deterministic write failure");
    drop(breaker);

    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "exercise trace persistence failure".to_string(),
        })
        .unwrap();

    let events = collect_until(&mut evt_rx, |event| {
        matches!(event, CoordinatorEvent::PersistenceError { .. })
    })
    .await;
    assert!(events.iter().any(|event| matches!(
        event,
        CoordinatorEvent::PersistenceError { error }
            if error.contains("Failed to persist inbound ACP event")
                && error.contains("no such table: acp_events")
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, CoordinatorEvent::AcpSessionUpdate { .. })),
        "a trace write failure must not suppress the live streamed update");

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn deterministic_selection_decisions_round_trip_from_file_backed_trace() {
    let commits = (1..=5)
        .map(|rank| CommitData {
            sha: format!("selection-sha-{rank}"),
            short_hash: format!("sel{rank}"),
            author_name: "Trace Tester".to_string(),
            time: "2026-08-20T00:00:00Z".to_string(),
            message_title: format!(
                "durabletrace candidate {rank} with deliberately oversized retrieval text"
            ),
            message_body: "additional deterministic selection evidence".to_string(),
        })
        .collect::<Vec<_>>();
    let (indexer, _repo_dir, _cache_dir) = setup_test_indexer_with_commits(&commits);

    let trace_dir = tempfile::tempdir().expect("trace tempdir");
    let trace_path = trace_dir.path().join("selection-decisions.db");
    let store = ConversationStore::open(&trace_path).expect("file-backed store");
    let (acp_client, acp_events) = launch_fake_adapter();
    let budget = SelectionBudget {
        total_char_budget: 25,
        per_excerpt_char_limit: 10,
        per_source_quota: SourceQuota {
            code: 5,
            document: 5,
            git_log: 5,
        },
    };
    let config = CoordinatorConfig {
        retrieval_limit_per_source: 5,
        budget: budget.clone(),
        request_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(2),
    };
    let (cmd_tx, mut evt_rx) =
        start_coordinator(indexer, store, acp_client, acp_events, config);

    let _ = collect_until(&mut evt_rx, |event| {
        matches!(event, CoordinatorEvent::SessionCreated { .. })
    })
    .await;
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "durabletrace".to_string(),
        })
        .unwrap();
    let turn_events = collect_until(&mut evt_rx, |event| {
        matches!(event, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;
    let turn_id = turn_events
        .iter()
        .find_map(|event| match event {
            CoordinatorEvent::TurnStarted { turn_id } => Some(*turn_id),
            _ => None,
        })
        .expect("turn id");

    shutdown_and_wait(&cmd_tx).await;
    drop(cmd_tx);
    drop(evt_rx);

    let store = ConversationStore::open(&trace_path).expect("reopen selection trace");
    let turn = store
        .get_turn(turn_id)
        .expect("read turn")
        .expect("durable turn");
    let stored_budget: serde_json::Value = serde_json::from_str(
        turn.budget_json.as_deref().expect("selection budget should be durable"),
    )
    .expect("budget JSON");
    assert_eq!(stored_budget["total_char_budget"], budget.total_char_budget);
    assert_eq!(
        stored_budget["per_excerpt_char_limit"],
        budget.per_excerpt_char_limit
    );

    let runs = store
        .get_retrieval_runs_for_turn(turn_id)
        .expect("read retrieval run");
    assert_eq!(runs.len(), 1);
    let candidates = store
        .get_candidates_for_run(runs[0].id)
        .expect("read candidate decisions");
    assert_eq!(candidates.len(), 5);
    for (index, candidate) in candidates.iter().enumerate() {
        assert_eq!(candidate.rank, index as i64 + 1);
        assert_eq!(candidate.source, "git_log");
        assert_eq!(candidate.match_type, "Hybrid");
        assert!(candidate.score > 0.0);
        assert!(candidate.text.contains("durabletrace candidate"));
        let location: serde_json::Value =
            serde_json::from_str(&candidate.location_json).expect("location JSON");
        assert!(location["Commit"]["short_hash"].is_string());
    }

    assert_eq!(candidates[0].rank, 1);
    assert!(candidates[0].included);
    assert_eq!(candidates[0].truncated, Some(true));
    assert_eq!(
        candidates[0].truncation_reason.as_deref(),
        Some("per_excerpt_limit")
    );
    assert!(candidates[0].original_len.unwrap() > 10);
    assert!(candidates[0].exclusion_reason.is_none());

    assert!(candidates[1].included);
    assert_eq!(
        candidates[1].truncation_reason.as_deref(),
        Some("per_excerpt_limit")
    );
    assert!(candidates[2].included);
    assert_eq!(
        candidates[2].truncation_reason.as_deref(),
        Some("total_budget_remaining")
    );
    assert_eq!(candidates[2].truncated, Some(true));

    for candidate in &candidates[3..] {
        assert!(!candidate.included);
        assert_eq!(candidate.truncated, Some(false));
        assert!(candidate.truncation_reason.is_none());
        assert_eq!(
            candidate.exclusion_reason.as_deref(),
            Some("total_budget_exhausted")
        );
        assert!(candidate.original_len.unwrap() > 10);
    }
}

#[tokio::test]
async fn adapter_process_restart_supports_a_new_prompt_round_trip() {
    let (first_cmd_tx, mut first_evt_rx, first_repo) = setup_coordinator();

    let first_session = collect_until(&mut first_evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;
    assert!(
        first_session
            .iter()
            .any(|event| matches!(event, CoordinatorEvent::SessionCreated { .. })),
        "expected the first adapter process to create a session"
    );

    first_cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "before adapter restart".to_string(),
        })
        .unwrap();
    let first_turn = collect_until(&mut first_evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;
    assert!(first_turn.iter().any(|event| matches!(
        event,
        CoordinatorEvent::TurnCompleted { stop_reason, .. } if stop_reason == "EndTurn"
    )));

    // This waits for AcpClient::shutdown to reap the first child before a
    // fresh coordinator launches a replacement adapter process below.
    shutdown_and_wait(&first_cmd_tx).await;
    drop(first_cmd_tx);
    drop(first_evt_rx);
    drop(first_repo);

    let (second_cmd_tx, mut second_evt_rx, _second_repo) = setup_coordinator();
    let second_session = collect_until(&mut second_evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;
    assert!(
        second_session
            .iter()
            .any(|event| matches!(event, CoordinatorEvent::SessionCreated { .. })),
        "expected the restarted adapter process to create a fresh session"
    );

    second_cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "after adapter restart".to_string(),
        })
        .unwrap();
    let second_turn = collect_until(&mut second_evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;
    assert!(
        second_turn.iter().any(|event| matches!(
            event,
            CoordinatorEvent::TurnCompleted { stop_reason, .. } if stop_reason == "EndTurn"
        )),
        "the restarted adapter process should complete a prompt"
    );

    shutdown_and_wait(&second_cmd_tx).await;
}

#[tokio::test]
async fn double_submit_is_rejected() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator();

    // Wait for SessionCreated
    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    // Send both SubmitPrompt commands immediately, back-to-back.
    // The coordinator processes them in order: first spawns the prompt
    // (sets active_turn_id), second is rejected (active_turn_id is Some).
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "first prompt".to_string(),
        })
        .unwrap();
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "second prompt".to_string(),
        })
        .unwrap();

    // Collect until TurnCompleted for the first turn.
    let all_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;

    // Exactly one TurnStarted should have been emitted (second was rejected).
    let start_count = all_events
        .iter()
        .filter(|e| matches!(e, CoordinatorEvent::TurnStarted { .. }))
        .count();
    assert_eq!(
        start_count, 1,
        "only one TurnStarted should be emitted; got {}",
        start_count
    );

    // First turn should complete successfully.
    assert!(
        all_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::TurnCompleted { stop_reason, .. } if stop_reason == "EndTurn")),
        "first turn should complete with EndTurn"
    );

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn cancel_turn_transitions_to_cancelled() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator();

    // Wait for SessionCreated
    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    // Submit prompt and immediately cancel — the CancelTurn arrives while
    // the prompt is in flight because the coordinator processes commands
    // concurrently with the ACP prompt via tokio::select!.
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "long running task".to_string(),
        })
        .unwrap();
    cmd_tx.send(CoordinatorCommand::CancelTurn).unwrap();

    // Collect until TurnCompleted
    let turn_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;

    let completed = turn_events
        .iter()
        .find_map(|e| match e {
            CoordinatorEvent::TurnCompleted { stop_reason, .. } => Some(stop_reason.as_str()),
            _ => None,
        });
    assert_eq!(
        completed,
        Some("Cancelled"),
        "expected Cancelled stop reason, got {:?}",
        completed
    );

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn permission_flow_emits_permission_required_and_completes() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator_with_mode("permission_flow");

    // Wait for SessionCreated
    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    // Submit a prompt — the permission_flow adapter will send a
    // session/request_permission before answering.
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "please do something".to_string(),
        })
        .unwrap();

    // Collect until PermissionRequired is emitted.
    let permission_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::PermissionRequired { .. })
    })
    .await;

    let permission = permission_events
        .iter()
        .find_map(|e| match e {
            CoordinatorEvent::PermissionRequired {
                request_id,
                options_json,
                ..
            } => Some((request_id.clone(), options_json.clone())),
            _ => None,
        })
        .expect("expected PermissionRequired event");

    // Respond with allow_once (one of the options the fixture offers).
    cmd_tx
        .send(CoordinatorCommand::RespondPermission {
            request_id: permission.0,
            outcome: daftprompt_acp::PermissionOutcome::Selected("allow_once".to_string()),
        })
        .unwrap();

    // Collect until TurnCompleted.
    let turn_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;

    let completed = turn_events
        .iter()
        .find_map(|e| match e {
            CoordinatorEvent::TurnCompleted { stop_reason, .. } => Some(stop_reason.as_str()),
            _ => None,
        });
    assert_eq!(
        completed,
        Some("EndTurn"),
        "expected EndTurn after permission granted, got {:?}",
        completed
    );

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn adapter_exit_during_prompt_emits_turn_failed() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator_with_mode("early_exit");

    // The early_exit adapter dies right after initialize, before
    // session/new. The coordinator should emit AdapterError or
    // TurnFailed depending on when the exit is detected.
    let early_events = collect_until(&mut evt_rx, |e| {
        matches!(
            e,
            CoordinatorEvent::AdapterError { .. } | CoordinatorEvent::TurnFailed { .. }
        )
    })
    .await;

    assert!(
        early_events.iter().any(|e| matches!(
            e,
            CoordinatorEvent::AdapterError { .. } | CoordinatorEvent::TurnFailed { .. }
        )),
        "expected an AdapterError or TurnFailed event when the adapter exits early"
    );

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn persistence_failure_blocks_acp_dispatch() {
    let db_dir = tempfile::tempdir().expect("db tempdir");
    let db_path = db_dir.path().join("conversations.db");
    let (cmd_tx, mut evt_rx, _repo) =
        setup_coordinator_with_store_path(&db_path, Duration::from_secs(5));

    // Wait for SessionCreated: the coordinator's own session-creation write
    // has already completed by this point.
    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "hello world".to_string(),
        })
        .unwrap();

    // Wait for TurnStarted: `create_turn` has succeeded, and retrieval +
    // enrichment is about to run in its own spawned task. Only after this
    // point do we hold a competing write lock, so the failure we force below
    // is specifically in `persist_retrieval`, not in turn creation.
    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnStarted { .. })
    })
    .await;

    // Open a second connection to the same database file and hold the
    // write lock, so the coordinator's own retrieval-run/candidate/enriched
    // prompt inserts fail with "database is locked" (rusqlite's default
    // busy timeout is 0 — no retrying). This exercises a real storage
    // failure through the public API rather than a mocked one.
    let locker = rusqlite::Connection::open(&db_path).expect("second connection");
    locker
        .execute_batch("BEGIN IMMEDIATE;")
        .expect("acquire write lock");

    let turn_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnFailed { .. })
    })
    .await;

    drop(locker);

    assert!(
        !turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::EnrichedPromptReady { .. })),
        "persistence failure must block EnrichedPromptReady/dispatch, but it was emitted"
    );
    assert!(
        !turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::RetrievalCompleted { .. })),
        "persistence failure must block RetrievalCompleted, but it was emitted"
    );
    assert!(
        turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::TurnFailed { .. })),
        "expected TurnFailed once persistence failed"
    );

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn cancel_during_retrieval_is_processed() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator();

    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    // Submit and cancel back-to-back, with no await in between: on the
    // single-threaded test runtime, the just-spawned retrieval task cannot
    // have been polled yet (tokio::spawn only schedules it), so the command
    // channel's already-queued CancelTurn is guaranteed to be processed by
    // the coordinator's `tokio::select!` loop before retrieval completes.
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "hello world".to_string(),
        })
        .unwrap();
    cmd_tx.send(CoordinatorCommand::CancelTurn).unwrap();

    let turn_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnCompleted { .. })
    })
    .await;

    assert_eq!(
        turn_events
            .iter()
            .filter(|e| matches!(e, CoordinatorEvent::TurnStarted { .. }))
            .count(),
        1,
        "expected exactly one TurnStarted"
    );
    assert!(
        !turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::RetrievalCompleted { .. })),
        "a turn cancelled while retrieval is in flight must not reach RetrievalCompleted"
    );
    assert!(
        !turn_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::EnrichedPromptReady { .. })),
        "a turn cancelled while retrieval is in flight must not reach EnrichedPromptReady/dispatch"
    );

    let completed = turn_events.iter().find_map(|e| match e {
        CoordinatorEvent::TurnCompleted { stop_reason, .. } => Some(stop_reason.as_str()),
        _ => None,
    });
    assert_eq!(completed, Some("Cancelled"));

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn retry_turn_redispatches_failed_turn() {
    // "hang" mode never answers session/prompt, so a request timeout well
    // below the test's own collection timeout deterministically produces a
    // real ACP dispatch failure (not a retrieval or persistence failure)
    // whose enriched prompt is retained, exactly the state
    // `ConversationStore::retry_turn` requires. Kept generous (not just
    // barely above zero) so it does not itself misfire as a timeout on
    // `initialize`/`session/new` under parallel test-suite load.
    let (acp_client, acp_events) =
        launch_fake_adapter_with_mode_and_timeout("hang", Duration::from_millis(1500));
    let (indexer, _repo) = setup_test_indexer();
    let store = ConversationStore::open_in_memory().expect("in-memory store");
    let config = CoordinatorConfig {
        retrieval_limit_per_source: 5,
        budget: SelectionBudget::default(),
        request_timeout: Duration::from_millis(1500),
        shutdown_grace: Duration::from_secs(2),
    };
    let (cmd_tx, mut evt_rx) = start_coordinator(indexer, store, acp_client, acp_events, config);

    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "hello world".to_string(),
        })
        .unwrap();

    let first_attempt = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnFailed { .. })
    })
    .await;

    assert_eq!(
        first_attempt
            .iter()
            .filter(|e| matches!(e, CoordinatorEvent::TurnStarted { .. }))
            .count(),
        1,
        "expected exactly one TurnStarted before retry"
    );
    let turn_id = first_attempt
        .iter()
        .find_map(|e| match e {
            CoordinatorEvent::TurnStarted { turn_id } => Some(*turn_id),
            _ => None,
        })
        .expect("expected TurnStarted");

    // A retry for a turn that is not retryable (never submitted) must be a
    // silent no-op: no TurnStarted, no state change.
    cmd_tx
        .send(CoordinatorCommand::RetryTurn {
            turn_id: turn_id + 1_000,
        })
        .unwrap();

    cmd_tx
        .send(CoordinatorCommand::RetryTurn { turn_id })
        .unwrap();

    // The retry re-dispatches the same enriched prompt to the still-hanging
    // adapter, which times out again: a second TurnStarted followed by a
    // second TurnFailed for the same turn proves a real new ACP round trip
    // happened rather than the coordinator silently reusing the first
    // attempt's outcome.
    let retry_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::TurnFailed { .. })
    })
    .await;

    assert_eq!(
        retry_events
            .iter()
            .filter(|e| matches!(e, CoordinatorEvent::TurnStarted { turn_id: t } if *t == turn_id))
            .count(),
        1,
        "expected exactly one re-dispatch TurnStarted for the retried turn"
    );
    assert!(
        retry_events
            .iter()
            .any(|e| matches!(e, CoordinatorEvent::TurnFailed { turn_id: t, .. } if *t == turn_id)),
        "expected the retried turn to fail again (still-hanging adapter)"
    );

    shutdown_and_wait(&cmd_tx).await;
}

#[tokio::test]
async fn late_events_are_not_misattributed_to_a_newer_turn() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator();

    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    // Let the first turn actually reach ACP dispatch (past retrieval) before
    // cancelling it, so it is plausible for its ACP session/update stream to
    // still be delivering events around the moment of cancellation.
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "first task".to_string(),
        })
        .unwrap();
    let mut all_events = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::EnrichedPromptReady { .. })
    })
    .await;

    cmd_tx.send(CoordinatorCommand::CancelTurn).unwrap();
    all_events.extend(
        collect_until(&mut evt_rx, |e| matches!(e, CoordinatorEvent::TurnCompleted { .. })).await,
    );

    // Immediately submit a second turn — while its retrieval/enrichment is
    // preparing, the previous turn's ACP stream identity must not yet have
    // moved on to it.
    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "second task".to_string(),
        })
        .unwrap();
    all_events.extend(
        collect_until(&mut evt_rx, |e| matches!(e, CoordinatorEvent::TurnCompleted { .. })).await,
    );

    // The invariant under test: an `AcpSessionUpdate` for a given turn can
    // only be observed after that turn's own `EnrichedPromptReady` (i.e.
    // after its ACP dispatch actually began). Before the fix, a late event
    // could be tagged with whatever turn was merely *active* (created, not
    // yet dispatched) at arrival time — this would show up here as an
    // AcpSessionUpdate for a turn with no preceding EnrichedPromptReady.
    let mut dispatched: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for event in &all_events {
        match event {
            CoordinatorEvent::EnrichedPromptReady { turn_id, .. } => {
                dispatched.insert(*turn_id);
            }
            CoordinatorEvent::AcpSessionUpdate { turn_id, .. } => {
                assert!(
                    dispatched.contains(turn_id),
                    "AcpSessionUpdate for turn {turn_id} observed before its own EnrichedPromptReady"
                );
            }
            _ => {}
        }
    }

    shutdown_and_wait(&cmd_tx).await;
}

/// Task 6, Design Decision #3: "Session close should be used when
/// advertised." The fixture adapter's normal `initialize_response.json`
/// advertises `agentCapabilities.sessionCapabilities.close`, so `Shutdown`
/// must send `session/close` -- observed here via the fake adapter's
/// marker-file side channel rather than by inspecting protocol traffic
/// directly.
#[tokio::test]
async fn shutdown_calls_close_session_when_advertised() {
    let marker_dir = tempfile::tempdir().expect("marker tempdir");
    let marker_path = marker_dir.path().join("close-called");
    let (cmd_tx, mut evt_rx, _repo) =
        setup_coordinator_with_mode_and_close_marker("normal", &marker_path);

    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    shutdown_and_wait(&cmd_tx).await;

    assert!(
        marker_path.exists(),
        "expected session/close to be sent when the adapter advertises support for it"
    );
}

/// The inverse of `shutdown_calls_close_session_when_advertised`: the
/// `no_session_close` fake-adapter mode omits
/// `agentCapabilities.sessionCapabilities.close` from its `initialize`
/// response entirely, so `Shutdown` must skip `session/close` and go
/// straight to `AcpClient::shutdown` instead.
#[tokio::test]
async fn shutdown_skips_close_session_when_not_advertised() {
    let marker_dir = tempfile::tempdir().expect("marker tempdir");
    let marker_path = marker_dir.path().join("close-called");
    let (cmd_tx, mut evt_rx, _repo) =
        setup_coordinator_with_mode_and_close_marker("no_session_close", &marker_path);

    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    shutdown_and_wait(&cmd_tx).await;

    assert!(
        !marker_path.exists(),
        "session/close must not be sent when the adapter does not advertise support for it"
    );
}

/// Task 6: the real application-exit path must actually call
/// `AcpClient::shutdown` and wait (bounded by the grace period) for the
/// adapter's child process, rather than firing a non-blocking command and
/// racing ahead. This exercises the harder case: a prompt is still
/// in-flight (the "hang" fake-adapter mode never answers `session/prompt`)
/// when `Shutdown` arrives. `Shutdown` must abort that in-flight prompt task
/// -- not merely send a `session/cancel` notification and hope -- so the
/// `Arc<AcpClient>` can be uniquely reclaimed and `.shutdown()` can run
/// promptly, well inside the 2s `shutdown_grace` configured here and nowhere
/// near the fake adapter's request timeout (which "hang" mode would
/// otherwise never resolve within this test's lifetime).
#[tokio::test]
async fn shutdown_completes_promptly_while_a_prompt_is_hung() {
    let (cmd_tx, mut evt_rx, _repo) = setup_coordinator_with_mode("hang");

    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::SessionCreated { .. })
    })
    .await;

    cmd_tx
        .send(CoordinatorCommand::SubmitPrompt {
            original: "will hang forever".to_string(),
        })
        .unwrap();
    // Wait until the prompt has actually been dispatched over ACP (past
    // retrieval), so `prompt_handle` is genuinely `Some` when `Shutdown` is
    // sent below.
    let _ = collect_until(&mut evt_rx, |e| {
        matches!(e, CoordinatorEvent::EnrichedPromptReady { .. })
    })
    .await;

    let start = std::time::Instant::now();
    shutdown_and_wait(&cmd_tx).await;
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(4),
        "Shutdown should abort the hung in-flight prompt and complete within its \
         configured 2s shutdown_grace, not block on the adapter ever responding; took {elapsed:?}"
    );
}
