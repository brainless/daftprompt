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
