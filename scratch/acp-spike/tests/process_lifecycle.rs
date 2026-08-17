//! Epic 014 Task 0: process-supervision behavior that the typed
//! `agent-client-protocol` crate mostly hides from callers, exercised here
//! with a small hand-rolled newline-delimited JSON-RPC reader/writer over
//! `tokio::process::Command` so the exact framing and process-exit
//! semantics are directly observable.
//!
//! Covers: adapter stderr never corrupting stdout parsing, malformed stdout
//! lines being skipped without crashing the reader, the child process being
//! reaped after an early/forced exit, and a client-enforced request
//! timeout (the ACP wire has no native timeout notification — a hung
//! adapter is a client-side concern, see the findings doc).

use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

fn spawn_fake_adapter(mode: Option<&str>) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_adapter"));
    if let Some(mode) = mode {
        cmd.env("FAKE_ADAPTER_MODE", mode);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn fake_adapter")
}

async fn send(child: &mut Child, value: &Value) {
    let mut s = serde_json::to_string(value).unwrap();
    s.push('\n');
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(s.as_bytes())
        .await
        .unwrap();
}

fn init_request(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {"readTextFile": true, "writeTextFile": true}, "terminal": true},
            "clientInfo": {"name": "daftprompt-spike", "version": "0.0.1"}
        }
    })
}

/// Malformed adapter stdout output (a non-JSON line interleaved with valid
/// JSON-RPC lines) must be skippable by the reader without losing framing
/// on the messages before and after it.
#[tokio::test]
async fn malformed_stdout_line_does_not_corrupt_subsequent_parsing() {
    let mut child = spawn_fake_adapter(Some("malformed_line"));
    send(&mut child, &init_request(1)).await;

    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    // First line: valid initialize response.
    let first = lines.next_line().await.unwrap().unwrap();
    let first: Result<Value, _> = serde_json::from_str(&first);
    assert!(first.is_ok(), "expected the initialize response to parse");
    assert_eq!(first.unwrap()["id"], 1);

    // Second line: intentionally malformed. A conformant reader must be
    // able to detect this without panicking or desyncing subsequent framing
    // (each message is still exactly one line).
    let second = lines.next_line().await.unwrap().unwrap();
    let second: Result<Value, _> = serde_json::from_str(&second);
    assert!(second.is_err(), "this line is intentionally not valid JSON");

    // The reader must still be positioned correctly: send session/new next
    // and confirm we get a clean, valid response as the very next line.
    send(
        &mut child,
        &json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp/acp-spike-repo","mcpServers":[]}}),
    )
    .await;
    let third = lines.next_line().await.unwrap().unwrap();
    let third: Value = serde_json::from_str(&third).expect("framing recovered after the malformed line");
    assert_eq!(third["id"], 2);
    assert!(third["result"]["sessionId"].is_string());

    let _ = child.start_kill();
}

/// Adapter stderr is a fully separate stream; noisy/unrelated stderr output
/// concurrent with stdout traffic must never appear on stdout or delay
/// stdout message delivery.
#[tokio::test]
async fn stderr_noise_does_not_appear_on_or_block_stdout() {
    let mut child = spawn_fake_adapter(Some("stderr_noise"));
    send(&mut child, &init_request(1)).await;

    let mut stdout_lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let stderr = child.stderr.take().unwrap();

    let stdout_line = tokio::time::timeout(Duration::from_secs(5), stdout_lines.next_line())
        .await
        .expect("stdout response should not be delayed by concurrent stderr writes")
        .unwrap()
        .unwrap();
    let parsed: Value = serde_json::from_str(&stdout_line)
        .expect("stdout must contain only clean JSON-RPC, never stderr diagnostic text");
    assert_eq!(parsed["id"], 1);
    assert_eq!(
        parsed["result"]["agentInfo"]["name"],
        "@agentclientprotocol/codex-acp"
    );

    // Drain a bit of stderr to confirm it is indeed carrying the noise (and
    // therefore that the noise generator is doing its job / this is a
    // meaningful test, not a vacuous one).
    let mut stderr_lines = BufReader::new(stderr).lines();
    let stderr_line = tokio::time::timeout(Duration::from_secs(2), stderr_lines.next_line())
        .await
        .expect("stderr noise should be flowing")
        .unwrap()
        .unwrap();
    assert!(stderr_line.contains("diagnostic noise"));

    let _ = child.start_kill();
}

/// An adapter that exits immediately after `initialize` (before answering
/// `session/new`) must be reaped with an observable, non-hanging exit
/// status, and any request pending against it must not hang the caller
/// forever.
#[tokio::test]
async fn early_process_exit_is_observable_and_does_not_hang_pending_requests() {
    let mut child = spawn_fake_adapter(Some("early_exit"));
    send(&mut child, &init_request(1)).await;

    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = lines.next_line().await.unwrap().unwrap();
    let _: Value = serde_json::from_str(&first).unwrap();

    // No response will ever come for this one — the fixture adapter exits
    // right after answering initialize.
    send(
        &mut child,
        &json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp/acp-spike-repo","mcpServers":[]}}),
    )
    .await;

    let next = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .expect("stdout EOF should be observed promptly after the child exits");
    assert!(
        matches!(next, Ok(None)),
        "stdout should reach EOF (no more lines), got {next:?}"
    );

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("the child process must be reapable promptly, not left as a zombie")
        .expect("wait() should succeed");
    assert_eq!(status.code(), Some(7), "matches the fixture's deliberate exit(7)");
}

/// The ACP wire protocol has no native "this request timed out" signal —
/// codex-acp simply never answers if Codex hangs. A daftprompt-side client
/// must therefore enforce its own timeout budget per pending request. This
/// test proves that pattern works cleanly against a subprocess that never
/// answers `session/prompt`.
#[tokio::test]
async fn client_enforced_timeout_on_a_hung_request() {
    let mut child = spawn_fake_adapter(Some("hang"));
    send(&mut child, &init_request(1)).await;
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let _ = lines.next_line().await.unwrap().unwrap();

    send(
        &mut child,
        &json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp/acp-spike-repo","mcpServers":[]}}),
    )
    .await;
    let _ = lines.next_line().await.unwrap().unwrap();

    send(
        &mut child,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/prompt",
            "params": {"sessionId": "does-not-matter", "prompt": [{"type": "text", "text": "hang forever"}]}
        }),
    )
    .await;

    let result = tokio::time::timeout(Duration::from_millis(300), lines.next_line()).await;
    assert!(
        result.is_err(),
        "the fixture adapter deliberately never answers session/prompt in hang mode; \
         the client's own timeout must fire instead of waiting forever"
    );

    let _ = child.start_kill();
}
