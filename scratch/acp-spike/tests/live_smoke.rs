//! Epic 014 Task 0: opt-in LIVE smoke test against the real `codex-acp`
//! adapter (not the scripted `fake_adapter`). This requires:
//!   - the ~/Projects/codex-acp clone with `npm install` already run;
//!   - a `node` on PATH capable of `--import tsx`;
//!   - an authenticated local Codex profile (this test performs a real
//!     Codex turn and will consume API/ChatGPT quota).
//!
//! It does NOT run by default: `cargo test` skips it because it is
//! `#[ignore]`, and it additionally early-returns unless
//! `ACP_SPIKE_LIVE=1` is set, so accidentally passing `--ignored` in some
//! other context still can't make it hit the network. Run explicitly with:
//!
//!   ACP_SPIKE_LIVE=1 cargo test --test live_smoke -- --ignored --nocapture
//!
//! Override the adapter entry point with `ACP_SPIKE_ADAPTER_ENTRY`
//! (absolute path to codex-acp's `src/index.ts`, default
//! `~/Projects/codex-acp/src/index.ts`) and the session cwd with
//! `ACP_SPIKE_SESSION_CWD` (default a scratch temp dir).

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, StopReason, TextContent,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};

#[tokio::test]
#[ignore = "live: spawns the real codex-acp adapter, needs authenticated Codex + network"]
async fn live_initialize_new_session_prompt_close() {
    if std::env::var("ACP_SPIKE_LIVE").as_deref() != Ok("1") {
        eprintln!("skipping: set ACP_SPIKE_LIVE=1 to run this live smoke test");
        return;
    }

    // `AcpAgentConfig` has no `cwd` setter (by design: launch profiles are
    // just argv/env, see `crates/daftprompt-acp`'s later launch-profile
    // design). `node --import tsx <path>` resolves the `--import` loader
    // specifier against the process's cwd, not the entry file's directory,
    // so an absolute path to `src/index.ts` alone is not enough — `npm run
    // start --prefix <dir>` is codex-acp's own documented "run from
    // sources" launch command (readme-dev.md) and lets npm own the cwd
    // instead.
    let adapter_dir = std::env::var("ACP_SPIKE_ADAPTER_DIR").unwrap_or_else(|_| {
        format!(
            "{}/Projects/codex-acp",
            std::env::var("HOME").expect("HOME must be set")
        )
    });
    let session_cwd =
        std::env::var("ACP_SPIKE_SESSION_CWD").unwrap_or_else(|_| std::env::temp_dir().display().to_string());

    let agent = AcpAgent::new(
        AcpAgentConfig::new("npm")
            .arg("run")
            .arg("start")
            .arg("--prefix")
            .arg(adapter_dir),
    );

    agent_client_protocol::Client
        .builder()
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .expect("live initialize should succeed");
            eprintln!("live adapter info: {:?}", init.agent_info);
            eprintln!("live protocol version: {:?}", init.protocol_version);
            eprintln!("live agent capabilities: {:?}", init.agent_capabilities);

            let session = connection
                .send_request(NewSessionRequest::new(session_cwd))
                .block_task()
                .await
                .expect("live session/new should succeed");
            eprintln!("live session id: {}", session.session_id);

            let result = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "Say the single word: hello. Do not use any tools.",
                    ))],
                ))
                .block_task()
                .await
                .expect("live session/prompt should succeed");
            eprintln!("live stop reason: {:?}", result.stop_reason);
            assert_eq!(result.stop_reason, StopReason::EndTurn);

            connection
                .send_request(agent_client_protocol::schema::v1::CloseSessionRequest::new(
                    session.session_id.clone(),
                ))
                .block_task()
                .await
                .expect("live session/close should succeed");

            Ok(())
        })
        .await
        .expect("live connection should shut down cleanly");
}
