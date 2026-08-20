use std::path::PathBuf;
use std::time::Duration;

use daftprompt::coordinator::{
    start_coordinator, CoordinatorCommand, CoordinatorConfig, CoordinatorEvent,
};
use daftprompt_acp::{AcpClient, AcpClientConfig, AdapterLaunchProfile};
use daftprompt_indexer::{Indexer, IndexerConfig};
use daftprompt_prompt_builder::SelectionBudget;
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
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

    let _ = cmd_tx.send(CoordinatorCommand::Shutdown);
}
