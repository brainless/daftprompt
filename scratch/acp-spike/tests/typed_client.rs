//! Epic 014 Task 0: drive the scripted `fake_adapter` binary (see
//! `src/bin/fake_adapter.rs`) using the maintained `agent-client-protocol`
//! crate (https://github.com/agentclientprotocol/rust-sdk, same GitHub org
//! as codex-acp's TypeScript SDK). This is the primary evidence for the
//! "selected Rust ACP dependency is pinned and proven compatible" acceptance
//! criterion: initialize, session/new, session/prompt, session/update
//! streaming, session/request_permission (reverse request), and
//! cancellation all round-trip through the typed crate API against
//! fixtures captured from the real adapter (see
//! `epics/research/014-acp-adapter-compatibility.md`).
//!
//! No network access or live Codex credentials are required — everything
//! here talks to the local `fake_adapter` binary over stdio.

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, StopReason, TextContent,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn fake_adapter() -> AcpAgent {
    AcpAgent::new(AcpAgentConfig::new(env!("CARGO_BIN_EXE_fake_adapter")))
}

#[tokio::test]
async fn initialize_and_new_session_round_trip() {
    let agent = fake_adapter();

    agent_client_protocol::Client
        .builder()
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .expect("initialize should succeed");

            assert_eq!(init.protocol_version, ProtocolVersion::V1);
            assert_eq!(init.agent_info.as_ref().unwrap().name, "@agentclientprotocol/codex-acp");
            assert_eq!(init.agent_info.as_ref().unwrap().version, "1.4.0");
            assert!(init.agent_capabilities.load_session);

            let session = connection
                .send_request(NewSessionRequest::new("/tmp/acp-spike-repo"))
                .block_task()
                .await
                .expect("session/new should succeed");
            assert!(!session.session_id.to_string().is_empty());

            Ok(())
        })
        .await
        .expect("connection should complete without error");
}

#[tokio::test]
async fn text_prompt_streams_updates_then_completes() {
    let agent = fake_adapter();
    let updates: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let updates_for_handler = updates.clone();

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            move |notification: SessionNotification, _cx| {
                let updates = updates_for_handler.clone();
                async move {
                    updates
                        .lock()
                        .unwrap()
                        .push(format!("{:?}", notification.update));
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .expect("initialize should succeed");
            let session = connection
                .send_request(NewSessionRequest::new("/tmp/acp-spike-repo"))
                .block_task()
                .await
                .expect("session/new should succeed");

            let result = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "Say the single word: hello. Do not use any tools.",
                    ))],
                ))
                .block_task()
                .await
                .expect("session/prompt should succeed");

            assert_eq!(result.stop_reason, StopReason::EndTurn);
            Ok(())
        })
        .await
        .expect("connection should complete without error");

    let updates = updates.lock().unwrap();
    assert!(
        updates
            .iter()
            .any(|u| u.contains("AgentMessageChunk") || u.contains("agent_message_chunk")),
        "expected an agent message chunk update, got: {updates:#?}"
    );
    assert!(
        updates.iter().any(|u| u.contains("ToolCall")),
        "expected a tool_call update, got: {updates:#?}"
    );
    assert!(
        !updates.is_empty(),
        "expected at least one session/update notification"
    );
}

#[tokio::test]
async fn permission_request_round_trip_with_harmless_action() {
    let agent = fake_adapter();
    let chosen_option: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let chosen_option_for_handler = chosen_option.clone();

    agent_client_protocol::Client
        .builder()
        .on_receive_request(
            async move |request: RequestPermissionRequest,
                        responder: agent_client_protocol::Responder<RequestPermissionResponse>,
                        _connection| {
                // A harmless test action: an `echo` command, never
                // auto-approved by rank/kind — the client is expected to
                // choose an offered option explicitly, exactly as Epic 014
                // Design Decision #7 requires.
                assert_eq!(
                    request.tool_call.fields.raw_input,
                    Some(serde_json::json!({
                        "command": "echo daftprompt-harmless-permission-test",
                        "cwd": "/tmp/acp-spike-repo"
                    }))
                );
                let allow_once = request
                    .options
                    .iter()
                    .find(|o| o.option_id.to_string() == "allow_once")
                    .expect("fixture always offers allow_once")
                    .option_id
                    .clone();
                chosen_option_for_handler
                    .lock()
                    .unwrap()
                    .replace(allow_once.to_string());
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(allow_once)),
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .unwrap();
            let session = connection
                .send_request(NewSessionRequest::new("/tmp/acp-spike-repo"))
                .block_task()
                .await
                .unwrap();

            let result = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "TRIGGER_PERMISSION please run the harmless echo command",
                    ))],
                ))
                .block_task()
                .await
                .expect("session/prompt should succeed even with a mid-turn permission request");

            assert_eq!(result.stop_reason, StopReason::EndTurn);
            Ok(())
        })
        .await
        .expect("connection should complete without error");

    assert_eq!(chosen_option.lock().unwrap().as_deref(), Some("allow_once"));
}

#[tokio::test]
async fn cancel_before_turn_id_exists_is_a_safe_no_op() {
    let agent = fake_adapter();

    agent_client_protocol::Client
        .builder()
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .unwrap();
            let session = connection
                .send_request(NewSessionRequest::new("/tmp/acp-spike-repo"))
                .block_task()
                .await
                .unwrap();

            // No session/prompt has ever been sent yet: there is no turn id
            // to cancel. Matches the real adapter's observed behavior
            // (verified live against ~/Projects/codex-acp): this is a
            // silent no-op notification, and the session remains usable
            // afterward.
            connection
                .send_notification(CancelNotification::new(session.session_id.clone()))
                .expect("cancel notification should send even with no active turn");

            let result = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("Say hello."))],
                ))
                .block_task()
                .await
                .expect("session should still accept a prompt after a premature cancel");
            assert_eq!(result.stop_reason, StopReason::EndTurn);
            Ok(())
        })
        .await
        .expect("connection should complete without error");
}

#[tokio::test]
async fn cancel_during_active_turn_yields_cancelled_stop_reason() {
    let agent = fake_adapter();

    agent_client_protocol::Client
        .builder()
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .unwrap();
            let session = connection
                .send_request(NewSessionRequest::new("/tmp/acp-spike-repo"))
                .block_task()
                .await
                .unwrap();

            let sent = connection.send_request(PromptRequest::new(
                session.session_id.clone(),
                vec![ContentBlock::Text(TextContent::new(
                    "Write a long essay.",
                ))],
            ));

            // Give the fixture stream a moment to start, then cancel — this
            // mirrors the live-observed behavior where a cancel notification
            // arriving mid-turn produces stopReason "cancelled" in the
            // eventual session/prompt response.
            tokio::time::sleep(Duration::from_millis(10)).await;
            connection
                .send_notification(CancelNotification::new(session.session_id.clone()))
                .expect("cancel notification should send during an active turn");

            let result = tokio::time::timeout(Duration::from_secs(5), sent.block_task())
                .await
                .expect("prompt should resolve promptly after cancellation")
                .expect("cancelled turns still resolve session/prompt, not an error");
            assert_eq!(result.stop_reason, StopReason::Cancelled);
            Ok(())
        })
        .await
        .expect("connection should complete without error");
}

#[tokio::test]
async fn session_close_then_process_shutdown_is_clean() {
    let agent = fake_adapter();

    agent_client_protocol::Client
        .builder()
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .unwrap();
            let session = connection
                .send_request(NewSessionRequest::new("/tmp/acp-spike-repo"))
                .block_task()
                .await
                .unwrap();

            connection
                .send_request(agent_client_protocol::schema::v1::CloseSessionRequest::new(
                    session.session_id.clone(),
                ))
                .block_task()
                .await
                .expect("session/close should succeed");
            Ok(())
        })
        .await
        .expect("connection (and therefore the child fake_adapter process) should shut down cleanly");
}
