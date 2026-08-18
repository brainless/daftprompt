//! Scripted tests for the daftprompt-acp public API
//! ([`daftprompt_acp::AcpClient`]) against the `acp-fake-adapter`
//! test-support binary (adapted from the Epic 014 Task 0 spike's
//! `fake_adapter`; see `epics/research/014-acp-adapter-compatibility.md`).
//!
//! No network access or live Codex credentials are required -- everything
//! here talks to a local scripted subprocess over stdio, satisfying Task
//! 1's "Scripted adapter tests run without network access or Codex
//! credentials" acceptance criterion.

use std::time::Duration;

use daftprompt_acp::{
    AcpClient, AcpClientConfig, AcpError, AcpEvent, AdapterLaunchProfile, PermissionOutcome,
    SessionUpdate, SessionUpdateKind, StopReason,
};

fn launch(mode: Option<&str>) -> (AcpClient, daftprompt_acp::AcpEvents) {
    let mut profile = AdapterLaunchProfile::new(env!("CARGO_BIN_EXE_acp-fake-adapter"));
    if let Some(mode) = mode {
        profile = profile.env("FAKE_ADAPTER_MODE", mode);
    }
    AcpClient::launch(
        profile,
        AcpClientConfig {
            request_timeout: Duration::from_secs(5),
            ..AcpClientConfig::default()
        },
    )
}

#[tokio::test]
async fn initialize_and_new_session_round_trip() {
    let (client, _events) = launch(None);

    let init = client.initialize().await.expect("initialize should succeed");
    assert_eq!(
        init.agent_info.as_ref().map(|info| info.name.as_str()),
        Some("@agentclientprotocol/codex-acp")
    );
    assert_eq!(
        init.agent_info.as_ref().map(|info| info.version.as_str()),
        Some("1.4.0")
    );
    assert!(init.agent_capabilities.load_session);

    let session_id = client
        .new_session("/tmp/daftprompt-acp-test-repo")
        .await
        .expect("session/new should succeed");
    assert!(!session_id.to_string().is_empty());

    client
        .shutdown(Duration::from_secs(5))
        .await
        .expect("shutdown should be clean");
}

#[tokio::test]
async fn prompt_streams_updates_then_completes() {
    let (client, mut events) = launch(None);
    client.initialize().await.unwrap();
    let session_id = client.new_session("/tmp/daftprompt-acp-test-repo").await.unwrap();

    let prompt_fut = client.prompt(
        session_id,
        "Say the single word: hello. Do not use any tools.",
    );

    let drain_fut = async {
        let mut saw_agent_message_chunk = false;
        let mut saw_tool_call = false;
        while !(saw_agent_message_chunk && saw_tool_call) {
            match events.recv().await {
                Some(AcpEvent::SessionUpdate {
                    update: SessionUpdateKind::Known(update),
                    ..
                }) => match update {
                    SessionUpdate::AgentMessageChunk(_) => saw_agent_message_chunk = true,
                    SessionUpdate::ToolCall(_) => saw_tool_call = true,
                    _ => {}
                },
                Some(AcpEvent::Diagnostic(diag)) => panic!("unexpected diagnostic: {diag}"),
                Some(AcpEvent::PermissionRequested(_)) => panic!("unexpected permission request"),
                Some(_) => {}
                None => break,
            }
        }
        (saw_agent_message_chunk, saw_tool_call)
    };

    let (outcome, (saw_agent_message_chunk, saw_tool_call)) = tokio::join!(prompt_fut, drain_fut);

    assert!(saw_agent_message_chunk, "expected an agent message chunk update");
    assert!(saw_tool_call, "expected a tool_call update");
    assert_eq!(outcome.unwrap().stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn permission_request_round_trip_with_harmless_action() {
    let (client, mut events) = launch(None);
    client.initialize().await.unwrap();
    let session_id = client.new_session("/tmp/daftprompt-acp-test-repo").await.unwrap();

    let prompt_fut = client.prompt(
        session_id,
        "TRIGGER_PERMISSION please run the harmless echo command",
    );

    let respond_fut = async {
        // Drain events until the permission request arrives, answering it
        // with an option the adapter actually offered -- never inventing
        // one (Design Decision #7).
        let permission_request = loop {
            match events.recv().await {
                Some(AcpEvent::PermissionRequested(request)) => break request,
                Some(_) => continue,
                None => panic!("connection closed before a permission request arrived"),
            }
        };

        assert_eq!(
            permission_request.tool_call.fields.raw_input,
            Some(serde_json::json!({
                "command": "echo daftprompt-harmless-permission-test",
                "cwd": "/tmp/acp-spike-repo"
            }))
        );
        let allow_once = permission_request
            .options
            .iter()
            .find(|option| option.option_id.to_string() == "allow_once")
            .expect("fixture always offers allow_once")
            .option_id
            .to_string();

        client
            .respond_permission(permission_request.id, PermissionOutcome::Selected(allow_once))
            .await
            .expect("responding with an offered option should succeed");
    };

    let (outcome, ()) = tokio::join!(prompt_fut, respond_fut);
    assert_eq!(
        outcome
            .expect("session/prompt should succeed even with a mid-turn permission request")
            .stop_reason,
        StopReason::EndTurn
    );
}

#[tokio::test]
async fn respond_permission_rejects_an_option_the_adapter_never_offered() {
    let (client, mut events) = launch(None);
    client.initialize().await.unwrap();
    let session_id = client.new_session("/tmp/daftprompt-acp-test-repo").await.unwrap();

    let prompt_fut = client.prompt(
        session_id,
        "TRIGGER_PERMISSION please run the harmless echo command",
    );

    let respond_fut = async {
        let permission_request = loop {
            match events.recv().await {
                Some(AcpEvent::PermissionRequested(request)) => break request,
                Some(_) => continue,
                None => panic!("connection closed before a permission request arrived"),
            }
        };

        let invented_option = "allow_forever_and_ever".to_string();
        let error = client
            .respond_permission(
                permission_request.id.clone(),
                PermissionOutcome::Selected(invented_option.clone()),
            )
            .await
            .expect_err("an invented option id must be rejected, never silently sent to the adapter");
        match error {
            AcpError::InvalidPermissionOption { option_id } => assert_eq!(option_id, invented_option),
            other => panic!("expected InvalidPermissionOption, got {other:?}"),
        }

        // The request must still be answerable after the rejected attempt.
        client
            .respond_permission(permission_request.id, PermissionOutcome::Cancelled)
            .await
            .expect("a valid follow-up response should still succeed");
    };

    let (outcome, ()) = tokio::join!(prompt_fut, respond_fut);
    assert_eq!(
        outcome.expect("session/prompt should still resolve").stop_reason,
        StopReason::EndTurn
    );
}

#[tokio::test]
async fn cancel_before_turn_id_exists_is_a_safe_no_op() {
    let (client, _events) = launch(None);
    client.initialize().await.unwrap();
    let session_id = client.new_session("/tmp/daftprompt-acp-test-repo").await.unwrap();

    // No session/prompt has ever been sent yet: there is no turn id to
    // cancel. Matches the real adapter's observed behavior (Task 0 findings
    // §8): a silent no-op, and the session remains usable afterward.
    client
        .cancel(session_id.clone())
        .expect("cancel notification should send even with no active turn");

    let outcome = client
        .prompt(session_id, "Say hello.")
        .await
        .expect("session should still accept a prompt after a premature cancel");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn cancel_during_active_turn_yields_cancelled_stop_reason() {
    let (client, _events) = launch(None);
    client.initialize().await.unwrap();
    let session_id = client.new_session("/tmp/daftprompt-acp-test-repo").await.unwrap();

    let prompt_fut = client.prompt(session_id.clone(), "Write a long essay.");
    let cancel_fut = async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        client.cancel(session_id).expect("cancel notification should send during an active turn");
    };

    let (outcome, ()) = tokio::join!(prompt_fut, cancel_fut);
    assert_eq!(
        outcome
            .expect("cancelled turns still resolve session/prompt, not an error")
            .stop_reason,
        StopReason::Cancelled
    );
}

#[tokio::test]
async fn session_close_then_shutdown_is_clean() {
    let (client, _events) = launch(None);
    client.initialize().await.unwrap();
    let session_id = client.new_session("/tmp/daftprompt-acp-test-repo").await.unwrap();

    client
        .close_session(session_id)
        .await
        .expect("session/close should succeed");

    client
        .shutdown(Duration::from_secs(5))
        .await
        .expect("shutdown after session/close should be clean");
}

#[tokio::test]
async fn unknown_update_and_notification_and_request_become_diagnostics() {
    let (client, mut events) = launch(None);
    client.initialize().await.unwrap();
    let session_id = client.new_session("/tmp/daftprompt-acp-test-repo").await.unwrap();

    let prompt_fut = client.prompt(session_id, "TRIGGER_UNKNOWN_UPDATE please");

    let drain_fut = async {
        let mut saw_unknown_session_update = false;
        let mut saw_unknown_notification_diagnostic = false;
        let mut saw_unknown_request_diagnostic = false;
        while !(saw_unknown_session_update
            && saw_unknown_notification_diagnostic
            && saw_unknown_request_diagnostic)
        {
            match events.recv().await {
                Some(AcpEvent::SessionUpdate {
                    update: SessionUpdateKind::Unknown(_),
                    ..
                }) => saw_unknown_session_update = true,
                Some(AcpEvent::Diagnostic(diag)) if diag.label.starts_with("unknown-notification:") => {
                    saw_unknown_notification_diagnostic = true;
                }
                Some(AcpEvent::Diagnostic(diag)) if diag.label.starts_with("unknown-request:") => {
                    saw_unknown_request_diagnostic = true;
                }
                Some(_) => {}
                None => break,
            }
        }
        (
            saw_unknown_session_update,
            saw_unknown_notification_diagnostic,
            saw_unknown_request_diagnostic,
        )
    };

    let (outcome, (saw_update, saw_notification, saw_request)) = tokio::join!(prompt_fut, drain_fut);

    assert!(saw_update, "expected an unknown session/update to be preserved");
    assert!(
        saw_notification,
        "expected an unrecognized notification method to be preserved as a diagnostic"
    );
    assert!(
        saw_request,
        "expected an unrecognized reverse-request method to be preserved as a diagnostic"
    );

    // The session must not have crashed: the turn still completes normally.
    assert_eq!(
        outcome.expect("turn should still complete after unknown traffic").stop_reason,
        StopReason::EndTurn
    );
}

#[tokio::test]
async fn malformed_stdout_line_is_reported_as_a_diagnostic_not_a_crash() {
    let (client, mut events) = launch(Some("malformed_line"));

    client.initialize().await.expect("initialize should still succeed");

    let mut saw_malformed_diagnostic = false;
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
            Ok(Some(AcpEvent::Diagnostic(diag))) if diag.label == "malformed-stdout-line" => {
                saw_malformed_diagnostic = true;
                break;
            }
            Ok(Some(_)) => continue,
            _ => break,
        }
    }
    assert!(saw_malformed_diagnostic, "expected a malformed-stdout-line diagnostic");

    // Framing must have recovered: a subsequent request still works.
    let session_id = client
        .new_session("/tmp/daftprompt-acp-test-repo")
        .await
        .expect("framing should have recovered after the malformed line");
    assert!(!session_id.to_string().is_empty());
}

#[tokio::test]
async fn client_enforced_timeout_on_a_hung_request() {
    // `hang` mode answers `initialize` and `session/new` normally but never
    // answers `session/prompt` -- the ACP wire has no native timeout signal
    // (Task 0 findings §10), so daftprompt-acp must enforce its own.
    // `initialize`/`session/new` only need to survive ordinary scheduling
    // jitter under a parallel test run, not prove anything about timeouts
    // themselves; a generous shared timeout keeps this deterministic while
    // `session/prompt` (checked below) hangs forever regardless of its
    // value, so the assertion below is still meaningful.
    let timeout = Duration::from_secs(3);
    let (client, _events) = AcpClient::launch(
        AdapterLaunchProfile::new(env!("CARGO_BIN_EXE_acp-fake-adapter")).env("FAKE_ADAPTER_MODE", "hang"),
        AcpClientConfig {
            request_timeout: timeout,
            ..AcpClientConfig::default()
        },
    );

    client.initialize().await.expect("initialize should still succeed in hang mode");
    let session_id = client
        .new_session("/tmp/daftprompt-acp-test-repo")
        .await
        .expect("session/new should still succeed in hang mode");

    let error = client
        .prompt(session_id, "hang forever")
        .await
        .expect_err("the fixture adapter deliberately never answers session/prompt in hang mode");
    match error {
        AcpError::Timeout { elapsed, .. } => assert_eq!(elapsed, timeout),
        other => panic!("expected Timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn early_process_exit_is_reported_distinctly() {
    let (client, _events) = launch(Some("early_exit"));

    // The fixture adapter exits(7) right after writing the `initialize`
    // response, before ever answering `session/new`. Depending on
    // scheduling, `initialize` itself may either observe its own response
    // before the exit is detected, or already surface the early-exit error
    // -- both are legitimate outcomes of the same real race a live adapter
    // crash produces. Either way, every call must end up with a
    // distinguishable early-exit-shaped error, never a hang and never a
    // generic "shutting down" that hides the real reason.
    if let Err(error) = client.initialize().await {
        assert_early_exit_shaped(&error);
        return;
    }

    let error = client
        .new_session("/tmp/daftprompt-acp-test-repo")
        .await
        .expect_err("session/new should fail once the adapter has exited");
    assert_early_exit_shaped(&error);
}

fn assert_early_exit_shaped(error: &AcpError) {
    match error {
        AcpError::EarlyExit { .. } | AcpError::Protocol { .. } => {}
        other => panic!("expected an early-exit-shaped error, got {other:?}"),
    }
}
