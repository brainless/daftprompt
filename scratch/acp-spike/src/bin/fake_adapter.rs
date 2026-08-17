//! Scripted stand-in for `codex-acp`'s stdio wire behavior, used by the
//! Epic 014 Task 0 compatibility spike so the Rust-side tests are
//! repeatable without live Codex credentials or network access.
//!
//! Response bodies for `initialize`, `session/new`, `session/prompt`'s
//! `session/update` stream, and the final prompt response are copied
//! (`fixtures/*`) from a real `node --import tsx src/index.ts` run against
//! the local `~/Projects/codex-acp` clone (git revision `97d260e`, package
//! version `1.4.0`) on 2026-08-17. The permission-request fixture is instead
//! reconstructed from `CodexApprovalHandler.ts` because no live run produced
//! one on this workstation (see the fixture file and the findings doc for
//! why). `FAKE_ADAPTER_MODE` selects fault-injection behavior so the same
//! binary can also stand in for malformed-output, early-exit, hang, and
//! stderr-noise scenarios.

use serde_json::{Value, json};
use std::env;
use std::io::Write as _;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

const FIXED_SESSION_ID: &str = "01a01050-a8b2-7a40-ab44-9e6f9400741e";

fn fixture(name: &str) -> &'static str {
    match name {
        "initialize_response" => include_str!("../../fixtures/initialize_response.json"),
        "session_new_response" => include_str!("../../fixtures/session_new_response.json"),
        "session_update_stream" => include_str!("../../fixtures/session_update_stream.jsonl"),
        "prompt_response" => include_str!("../../fixtures/prompt_response.json"),
        "permission_request" => include_str!("../../fixtures/permission_request.json"),
        other => panic!("unknown fixture {other}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Normal,
    MalformedLine,
    StderrNoise,
    EarlyExit,
    Hang,
}

impl Mode {
    fn from_env() -> Self {
        match env::var("FAKE_ADAPTER_MODE").as_deref() {
            Ok("malformed_line") => Mode::MalformedLine,
            Ok("stderr_noise") => Mode::StderrNoise,
            Ok("early_exit") => Mode::EarlyExit,
            Ok("hang") => Mode::Hang,
            _ => Mode::Normal,
        }
    }
}

async fn write_line(stdout: &mut tokio::io::Stdout, value: &Value) {
    let mut s = serde_json::to_string(value).expect("serialize");
    s.push('\n');
    stdout.write_all(s.as_bytes()).await.expect("write stdout");
    stdout.flush().await.expect("flush stdout");
}

fn with_id(mut fixture_value: Value, id: &Value) -> Value {
    fixture_value["id"] = id.clone();
    fixture_value
}

#[tokio::main]
async fn main() {
    let mode = Mode::from_env();

    if mode == Mode::StderrNoise {
        // Concurrent, unrelated stderr traffic. The client's stdout parser
        // must never see this and must never be delayed/corrupted by it.
        tokio::spawn(async {
            let mut i: u64 = 0;
            loop {
                eprintln!("[fake_adapter] diagnostic noise line {i}");
                i += 1;
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            }
        });
    }

    // Reader task: decouples "read next line from stdin" from "am I busy
    // streaming session/update notifications right now", the same way a
    // real client-facing adapter has to interleave inbound cancel
    // notifications with an in-flight prompt turn.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut stdout = tokio::io::stdout();

    while let Some(line) = rx.recv().await {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[fake_adapter] failed to parse inbound line as JSON: {e}");
                continue;
            }
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();

        match method {
            "initialize" => {
                let resp: Value = serde_json::from_str(fixture("initialize_response")).unwrap();
                let resp = with_id(resp, id.as_ref().unwrap());
                write_line(&mut stdout, &resp).await;

                if mode == Mode::MalformedLine {
                    // Adapter emits a non-JSON line on stdout. A conformant
                    // client must log/skip it, not corrupt subsequent
                    // message framing.
                    stdout
                        .write_all(b"this is not valid JSON on the wire {{{\n")
                        .await
                        .unwrap();
                    stdout.flush().await.unwrap();
                }

                if mode == Mode::EarlyExit {
                    // Simulate the adapter process dying right after a
                    // successful initialize, before session/new is ever
                    // answered.
                    std::process::exit(7);
                }
            }
            "session/new" => {
                let resp: Value = serde_json::from_str(fixture("session_new_response")).unwrap();
                let resp = with_id(resp, id.as_ref().unwrap());
                write_line(&mut stdout, &resp).await;
            }
            "session/set_mode" | "session/set_config_option" => {
                write_line(&mut stdout, &json!({"jsonrpc":"2.0","id": id, "result": {}})).await;
            }
            "session/prompt" => {
                if mode == Mode::Hang {
                    // Never respond; the client is expected to enforce its
                    // own request timeout.
                    continue;
                }

                let session_id = msg["params"]["sessionId"]
                    .as_str()
                    .unwrap_or(FIXED_SESSION_ID)
                    .to_string();
                let prompt_text = msg["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                if prompt_text.contains("TRIGGER_PERMISSION") {
                    // Reverse request: agent -> client, interleaved with
                    // ordinary notifications, exactly like codex-acp's
                    // CodexApprovalHandler does mid-turn.
                    let permission_request: Value =
                        serde_json::from_str(fixture("permission_request")).unwrap();
                    let mut permission_request = permission_request;
                    permission_request
                        .as_object_mut()
                        .unwrap()
                        .remove("_comment");
                    permission_request["params"]["sessionId"] = json!(session_id);
                    let permission_request_id = permission_request["id"].clone();
                    write_line(&mut stdout, &permission_request).await;

                    // Wait for the client's response to this specific
                    // reverse request while still being able to observe
                    // other inbound traffic (there is none expected here,
                    // but this proves request correlation, not just
                    // strict turn-taking).
                    let mut chosen_option_id = String::from("reject_once");
                    while let Some(next_line) = rx.recv().await {
                        if next_line.trim().is_empty() {
                            continue;
                        }
                        let Ok(reply): Result<Value, _> = serde_json::from_str(&next_line) else {
                            continue;
                        };
                        if reply.get("id") == Some(&permission_request_id) {
                            if let Some(opt) = reply["result"]["outcome"]["optionId"].as_str() {
                                chosen_option_id = opt.to_string();
                            }
                            break;
                        }
                    }

                    write_line(
                        &mut stdout,
                        &json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {
                                "sessionId": session_id,
                                "update": {
                                    "sessionUpdate": "tool_call_update",
                                    "toolCallId": "exec-fixture-0001",
                                    "status": if chosen_option_id == "reject_once" { "failed" } else { "completed" },
                                    "_meta": { "fixture": { "chosenOptionId": chosen_option_id } }
                                }
                            }
                        }),
                    )
                    .await;

                    write_line(
                        &mut stdout,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "stopReason": "end_turn" }
                        }),
                    )
                    .await;
                    continue;
                }

                let mut turn_cancelled = false;
                for fixture_line in fixture("session_update_stream").lines() {
                    if fixture_line.trim().is_empty() {
                        continue;
                    }
                    // Give an in-flight `session/cancel` notification a
                    // chance to arrive between streamed updates, the same
                    // way a real turn can be interrupted mid-stream.
                    if let Ok(next) = rx.try_recv() {
                        if let Ok(v) = serde_json::from_str::<Value>(&next) {
                            if v.get("method").and_then(Value::as_str) == Some("session/cancel")
                                && v["params"]["sessionId"].as_str() == Some(session_id.as_str())
                            {
                                turn_cancelled = true;
                                break;
                            }
                        }
                    }
                    let mut v: Value = serde_json::from_str(fixture_line).unwrap();
                    v["params"]["sessionId"] = json!(session_id);
                    write_line(&mut stdout, &v).await;
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }

                if turn_cancelled {
                    write_line(
                        &mut stdout,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "stopReason": "cancelled", "usage": null }
                        }),
                    )
                    .await;
                } else {
                    let resp: Value = serde_json::from_str(fixture("prompt_response")).unwrap();
                    let resp = with_id(resp, id.as_ref().unwrap());
                    write_line(&mut stdout, &resp).await;
                }
            }
            "session/cancel" => {
                // Notification: no response, and no effect here because it
                // is only inspected inline while a session/prompt fixture
                // stream is being written (see the `rx.try_recv()` check
                // above). A cancel with no in-flight turn for the session is
                // therefore a true no-op and does not affect a later,
                // unrelated turn — matches the live-observed real-adapter
                // behavior in the Task 0 spike notes.
            }
            "session/close" => {
                write_line(&mut stdout, &json!({"jsonrpc":"2.0","id": id, "result": {}})).await;
            }
            "" => {
                // A response line (has "id" + "result"/"error", no
                // "method") arriving when we were not actively waiting for
                // it (e.g. a stray permission-response after we already
                // moved on). Ignore.
            }
            other => {
                write_line(
                    &mut stdout,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("\"Method not found\": {other}"),
                            "data": { "method": other }
                        }
                    }),
                )
                .await;
            }
        }
    }

    let _ = std::io::stdout().flush();
}
