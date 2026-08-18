//! The ACP runtime boundary: [`AcpClient`].
//!
//! [`AcpClient::launch`] spawns the configured adapter process, drives the
//! `agent-client-protocol` connection on a background task, and exposes a
//! typed request/response API plus a typed event stream. No child-process
//! handle, JSON-RPC id, or raw `serde_json::Value` protocol payload crosses
//! the public API boundary except inside a bounded [`crate::RawDiagnostic`].
//!
//! # Why a command-loop actor
//!
//! `agent-client-protocol`'s incoming dispatch loop processes one message at
//! a time and awaits each notification/request handler before moving to the
//! next message (confirmed by reading the vendored crate's
//! `src/jsonrpc/incoming_actor.rs`). A handler that blocked on a human
//! decision (e.g. a permission choice) would therefore stall delivery of
//! every other incoming message, which Task 1's acceptance criteria
//! explicitly rule out ("Concurrent reverse requests and streaming
//! notifications do not block prompt-response correlation").
//!
//! `Responder<T>` (the type used to answer an incoming request) is a plain
//! `Send + 'static` value that can be stored and invoked later, from a
//! different task, at an arbitrary time -- it is not tied to the handler
//! callback's stack frame. So the request handler here does the minimum
//! amount of work (validate shape, stash the `Responder`, emit an event) and
//! returns immediately; the actual response is sent later, from the command
//! loop, when [`AcpClient::respond_permission`] delivers the caller's
//! decision.
//!
//! The command loop itself runs as the "foreground" future passed to
//! `connect_with`, which is exactly the place the SDK's own docs recommend
//! for `send_request(..).block_task()` linear control flow (see
//! `agent_client_protocol`'s `concepts::callbacks` module).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ClientCapabilities, CloseSessionRequest, ContentBlock, Implementation,
    InitializeRequest, NewSessionRequest, PermissionOptionId, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, StopReason, TextContent,
};
use agent_client_protocol::{
    Agent, AcpAgent, ConnectionTo, LineDirection, Responder, UntypedMessage,
};
use tokio::sync::{mpsc, oneshot, Notify};

use crate::diagnostic::RawDiagnostic;
use crate::error::AcpError;
use crate::events::{
    AcpEvent, AcpSessionId, PermissionOutcome, PermissionRequest, PermissionRequestId,
    SessionUpdateKind,
};
use crate::launch::AdapterLaunchProfile;

/// Negotiated initialization info: protocol version, agent identity, and
/// advertised capabilities/auth methods. A direct alias of the ACP schema
/// response type -- already fully typed, nothing to add.
pub type InitializeInfo = agent_client_protocol::schema::v1::InitializeResponse;

/// The result of a completed (non-cancelled or cancelled) prompt turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptOutcome {
    /// Why the agent stopped processing the turn.
    pub stop_reason: StopReason,
}

/// Configuration for one [`AcpClient::launch`] call.
#[derive(Debug, Clone)]
pub struct AcpClientConfig {
    /// Sent as `clientInfo.name` during `initialize`.
    pub client_name: String,
    /// Sent as `clientInfo.version` during `initialize`.
    pub client_version: String,
    /// Per-request timeout. The ACP wire protocol has no native timeout
    /// signal (Task 0 findings §10); this is enforced client-side.
    pub request_timeout: Duration,
}

impl Default for AcpClientConfig {
    fn default() -> Self {
        Self {
            client_name: "daftprompt".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            request_timeout: Duration::from_secs(60),
        }
    }
}

/// Commands accepted by the background connection task. Private: callers
/// only ever see the typed methods on [`AcpClient`].
enum Command {
    Initialize {
        client_info: Implementation,
        reply: oneshot::Sender<Result<InitializeInfo, AcpError>>,
    },
    NewSession {
        cwd: PathBuf,
        reply: oneshot::Sender<Result<AcpSessionId, AcpError>>,
    },
    Prompt {
        session_id: AcpSessionId,
        text: String,
        reply: oneshot::Sender<Result<PromptOutcome, AcpError>>,
    },
    Cancel {
        session_id: AcpSessionId,
    },
    RespondPermission {
        id: PermissionRequestId,
        outcome: PermissionOutcome,
        reply: oneshot::Sender<Result<(), AcpError>>,
    },
    CloseSession {
        session_id: AcpSessionId,
        reply: oneshot::Sender<Result<(), AcpError>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

struct PendingPermission {
    responder: Responder<serde_json::Value>,
    offered: Vec<PermissionOptionId>,
}

type PendingPermissions = Arc<Mutex<HashMap<String, PendingPermission>>>;

/// A typed handle to a running ACP adapter connection.
///
/// Cloning is intentionally not supported: there is exactly one command
/// sender per launched connection, matching the epic's "one daftprompt
/// conversation runtime, one adapter process" model (Design Decision #1).
pub struct AcpClient {
    commands: mpsc::UnboundedSender<Command>,
    task: tokio::task::JoinHandle<Result<(), AcpError>>,
    client_info: Implementation,
    /// The connection task's terminal error, if it has already ended. Read
    /// when a command can no longer be delivered (the `commands` channel's
    /// receiver was dropped) so callers see *why* the connection ended --
    /// e.g. [`AcpError::EarlyExit`] -- instead of a generic
    /// [`AcpError::ShuttingDown`] that would otherwise win a race against
    /// the background task recording its own exit reason.
    terminal_error: Arc<Mutex<Option<AcpError>>>,
    /// Notified once, right after `terminal_error` is populated and the
    /// background task is about to return. Lets [`Self::closed_error`] wait
    /// out the (normally tiny) window between the `commands` channel's
    /// receiver being dropped and the terminal error actually being
    /// recorded, instead of racing it.
    connection_closed: Arc<Notify>,
}

/// The typed event stream for one [`AcpClient`] connection. Returned
/// alongside the client from [`AcpClient::launch`].
pub struct AcpEvents {
    inner: mpsc::UnboundedReceiver<AcpEvent>,
}

impl AcpEvents {
    /// Receive the next event, in wire order. Returns `None` once the
    /// connection has shut down and no further events will arrive.
    pub async fn recv(&mut self) -> Option<AcpEvent> {
        self.inner.recv().await
    }
}

impl AcpClient {
    /// Launch the adapter process described by `profile` and start driving
    /// the ACP connection on a background task. Returns immediately; no
    /// protocol IO happens on the calling task (Design Decision #8).
    #[must_use]
    pub fn launch(profile: AdapterLaunchProfile, config: AcpClientConfig) -> (Self, AcpEvents) {
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let agent = profile.into_agent();
        let timeout = config.request_timeout;
        let client_info = Implementation::new(config.client_name, config.client_version);
        let terminal_error = Arc::new(Mutex::new(None));
        let connection_closed = Arc::new(Notify::new());

        let task = tokio::spawn(run(
            agent,
            commands_rx,
            events_tx,
            timeout,
            terminal_error.clone(),
            connection_closed.clone(),
        ));

        (
            Self {
                commands: commands_tx,
                task,
                client_info,
                terminal_error,
                connection_closed,
            },
            AcpEvents { inner: events_rx },
        )
    }

    /// The connection's recorded terminal error. Waits (briefly, and only
    /// when necessary) for the background task to finish recording its exit
    /// reason if a command was already rejected because the `commands`
    /// channel's receiver was dropped -- so callers see e.g.
    /// [`AcpError::EarlyExit`] rather than a generic
    /// [`AcpError::ShuttingDown`] that would otherwise win a race against
    /// that recording. Falls back to `ShuttingDown` only for an explicit,
    /// clean [`AcpClient::shutdown`] (which does not record a terminal
    /// error at all).
    async fn closed_error(&self) -> AcpError {
        if let Some(error) = self.terminal_error.lock().unwrap().clone() {
            return error;
        }
        let notified = self.connection_closed.notified();
        if let Some(error) = self.terminal_error.lock().unwrap().clone() {
            return error;
        }
        notified.await;
        self.terminal_error
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(AcpError::ShuttingDown)
    }

    /// Send `initialize` and validate/record the negotiated response.
    pub async fn initialize(&self) -> Result<InitializeInfo, AcpError> {
        let (reply, rx) = oneshot::channel();
        if self
            .commands
            .send(Command::Initialize {
                client_info: self.client_info.clone(),
                reply,
            })
            .is_err()
        {
            return Err(self.closed_error().await);
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(self.closed_error().await),
        }
    }

    /// Create a new session rooted at `cwd`.
    pub async fn new_session(&self, cwd: impl Into<PathBuf>) -> Result<AcpSessionId, AcpError> {
        let (reply, rx) = oneshot::channel();
        if self
            .commands
            .send(Command::NewSession {
                cwd: cwd.into(),
                reply,
            })
            .is_err()
        {
            return Err(self.closed_error().await);
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(self.closed_error().await),
        }
    }

    /// Send a plain-text user prompt on `session_id` and await the turn's
    /// outcome. Streamed `session/update` notifications for this turn arrive
    /// on the event stream concurrently, not through this method's return
    /// value.
    pub async fn prompt(
        &self,
        session_id: AcpSessionId,
        text: impl Into<String>,
    ) -> Result<PromptOutcome, AcpError> {
        let (reply, rx) = oneshot::channel();
        if self
            .commands
            .send(Command::Prompt {
                session_id,
                text: text.into(),
                reply,
            })
            .is_err()
        {
            return Err(self.closed_error().await);
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(self.closed_error().await),
        }
    }

    /// Send `session/cancel` for `session_id`. This is a one-way ACP
    /// notification: it does not wait for the turn to actually stop. The
    /// eventual `prompt` response reports `StopReason::Cancelled` if the
    /// cancellation took effect before the turn otherwise completed.
    pub fn cancel(&self, session_id: AcpSessionId) -> Result<(), AcpError> {
        // Best-effort, fire-and-forget notification: unlike the
        // request/response methods above, a send failure here does not
        // await `closed_error()` for the precise terminal reason, since
        // this method is intentionally synchronous (matching "cancellation
        // is an ACP notification", not a request).
        self.commands
            .send(Command::Cancel { session_id })
            .map_err(|_| AcpError::ShuttingDown)
    }

    /// Answer a pending [`crate::AcpEvent::PermissionRequested`] event.
    /// `outcome` must reference an `optionId` the adapter actually offered
    /// for that request (or be [`PermissionOutcome::Cancelled`]); anything
    /// else is rejected with [`AcpError::InvalidPermissionOption`] rather
    /// than silently substituted (Design Decision #7).
    pub async fn respond_permission(
        &self,
        id: PermissionRequestId,
        outcome: PermissionOutcome,
    ) -> Result<(), AcpError> {
        let (reply, rx) = oneshot::channel();
        if self
            .commands
            .send(Command::RespondPermission { id, outcome, reply })
            .is_err()
        {
            return Err(self.closed_error().await);
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(self.closed_error().await),
        }
    }

    /// Send `session/close` for `session_id`.
    pub async fn close_session(&self, session_id: AcpSessionId) -> Result<(), AcpError> {
        let (reply, rx) = oneshot::channel();
        if self
            .commands
            .send(Command::CloseSession { session_id, reply })
            .is_err()
        {
            return Err(self.closed_error().await);
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(self.closed_error().await),
        }
    }

    /// Gracefully shut down: stop accepting new commands, close the
    /// connection (which closes the adapter's stdin and awaits its exit),
    /// and wait up to `grace` for that to finish. If it does not finish in
    /// time, the underlying connection task is aborted, which drops the
    /// adapter's process-group guard and force-terminates it (see
    /// `agent_client_protocol::acp_agent::ChildGuard`).
    pub async fn shutdown(self, grace: Duration) -> Result<(), AcpError> {
        let AcpClient { commands, task, .. } = self;
        let abort_handle = task.abort_handle();
        let (reply, _rx) = oneshot::channel();
        // Best-effort: if the task already ended, the channel may be gone.
        let _ = commands.send(Command::Shutdown { reply });

        match tokio::time::timeout(grace, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_error)) => Err(AcpError::Internal(format!(
                "ACP connection task ended abnormally: {join_error}"
            ))),
            Err(_elapsed) => {
                abort_handle.abort();
                Err(AcpError::Timeout {
                    method: Some("shutdown".to_string()),
                    elapsed: grace,
                })
            }
        }
    }
}

async fn run(
    agent: AcpAgent,
    commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<AcpEvent>,
    timeout: Duration,
    terminal_error: Arc<Mutex<Option<AcpError>>>,
    connection_closed: Arc<Notify>,
) -> Result<(), AcpError> {
    let pending_permissions: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));
    let next_permission_id = Arc::new(AtomicU64::new(1));

    let events_for_debug = events.clone();
    let agent = agent.with_debug(move |line, direction| {
        // The SDK itself already logs-and-skips malformed stdout lines
        // without corrupting subsequent framing (Task 0 findings §10). This
        // callback independently re-validates each stdout line so a
        // malformed message is also visible on daftprompt-acp's own typed
        // event stream, not only in SDK-internal tracing output.
        if direction == LineDirection::Stdout && serde_json::from_str::<serde_json::Value>(line).is_err() {
            let _ = events_for_debug.send(AcpEvent::Diagnostic(RawDiagnostic::from_text(
                "malformed-stdout-line",
                line,
            )));
        }
    });

    let events_for_notification = events.clone();
    let events_for_request = events.clone();
    let pending_permissions_for_request = pending_permissions.clone();
    let next_permission_id_for_request = next_permission_id.clone();

    let result = agent_client_protocol::Client
        .builder()
        .name("daftprompt-acp")
        .on_receive_notification(
            async move |msg: UntypedMessage, _cx: ConnectionTo<Agent>| {
                dispatch_notification(msg, &events_for_notification);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |msg: UntypedMessage,
                         responder: Responder<serde_json::Value>,
                         _cx: ConnectionTo<Agent>| {
                dispatch_request(
                    msg,
                    responder,
                    &events_for_request,
                    &pending_permissions_for_request,
                    &next_permission_id_for_request,
                );
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
            command_loop(cx, commands, timeout, pending_permissions).await;
            Ok(())
        })
        .await;

    let result = result.map_err(AcpError::from_connection_error);
    if let Err(error) = &result {
        *terminal_error.lock().unwrap() = Some(error.clone());
    }
    connection_closed.notify_waiters();
    result
}

/// Parse and route one incoming `session/update` (or unrecognized)
/// notification. Never panics on unexpected shapes; unknown content becomes
/// a diagnostic event instead (Design Decision #3).
fn dispatch_notification(msg: UntypedMessage, events: &mpsc::UnboundedSender<AcpEvent>) {
    if msg.method() != "session/update" {
        let diag = RawDiagnostic::from_value(format!("unknown-notification:{}", msg.method()), msg.params());
        let _ = events.send(AcpEvent::Diagnostic(diag));
        return;
    }

    match serde_json::from_value::<SessionNotification>(msg.params().clone()) {
        Ok(notification) => {
            let _ = events.send(AcpEvent::SessionUpdate {
                session_id: notification.session_id,
                update: SessionUpdateKind::Known(notification.update),
            });
        }
        Err(_) => {
            let diag = RawDiagnostic::from_value("session/update", msg.params());
            // Best-effort session attribution even when the update body
            // itself did not parse against the pinned schema, so the event
            // can still be routed to the right transcript/session.
            let session_id = msg
                .params()
                .get("sessionId")
                .and_then(|value| value.as_str())
                .map(|s| AcpSessionId::new(s.to_string()));

            match session_id {
                Some(session_id) => {
                    let _ = events.send(AcpEvent::SessionUpdate {
                        session_id,
                        update: SessionUpdateKind::Unknown(diag),
                    });
                }
                None => {
                    let _ = events.send(AcpEvent::Diagnostic(diag));
                }
            }
        }
    }
}

/// Parse and route one incoming reverse request. Only `session/request_permission`
/// is supported in the MVP (Epic 014 non-goals exclude fs/terminal support);
/// anything else is answered with a standard JSON-RPC `method_not_found`
/// error and surfaced as a diagnostic, never left unanswered or panicking.
fn dispatch_request(
    msg: UntypedMessage,
    responder: Responder<serde_json::Value>,
    events: &mpsc::UnboundedSender<AcpEvent>,
    pending_permissions: &PendingPermissions,
    next_permission_id: &AtomicU64,
) {
    if msg.method() != "session/request_permission" {
        let diag = RawDiagnostic::from_value(format!("unknown-request:{}", msg.method()), msg.params());
        let _ = responder.respond_with_error(agent_client_protocol::Error::method_not_found());
        let _ = events.send(AcpEvent::Diagnostic(diag));
        return;
    }

    match serde_json::from_value::<RequestPermissionRequest>(msg.params().clone()) {
        Ok(request) => {
            let id = next_permission_id.fetch_add(1, Ordering::Relaxed).to_string();
            let offered: Vec<PermissionOptionId> = request
                .options
                .iter()
                .map(|option| option.option_id.clone())
                .collect();

            let event = PermissionRequest {
                id: PermissionRequestId(id.clone()),
                session_id: request.session_id,
                tool_call: request.tool_call,
                options: request.options,
            };

            pending_permissions
                .lock()
                .unwrap()
                .insert(id, PendingPermission { responder, offered });

            let _ = events.send(AcpEvent::PermissionRequested(event));
        }
        Err(_) => {
            let diag = RawDiagnostic::from_value("session/request_permission", msg.params());
            let _ = responder.respond_with_error(agent_client_protocol::Error::invalid_params());
            let _ = events.send(AcpEvent::Diagnostic(diag));
        }
    }
}

fn respond_permission(
    pending_permissions: &PendingPermissions,
    id: PermissionRequestId,
    outcome: PermissionOutcome,
) -> Result<(), AcpError> {
    let mut guard = pending_permissions.lock().unwrap();
    let Some(pending) = guard.remove(&id.0) else {
        return Err(AcpError::UnknownPermissionRequest);
    };

    let is_valid = match &outcome {
        PermissionOutcome::Cancelled => true,
        PermissionOutcome::Selected(option_id) => {
            pending.offered.iter().any(|offered| offered.to_string() == *option_id)
        }
    };

    if !is_valid {
        let option_id = match &outcome {
            PermissionOutcome::Selected(option_id) => option_id.clone(),
            PermissionOutcome::Cancelled => String::new(),
        };
        // Re-insert so a corrected response can still be sent for this request.
        guard.insert(id.0, pending);
        return Err(AcpError::InvalidPermissionOption { option_id });
    }
    drop(guard);

    let rpc_outcome = match outcome {
        PermissionOutcome::Cancelled => RequestPermissionOutcome::Cancelled,
        PermissionOutcome::Selected(option_id) => {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
        }
    };

    let value = serde_json::to_value(RequestPermissionResponse::new(rpc_outcome))
        .map_err(|e| AcpError::Internal(e.to_string()))?;

    pending.responder.respond(value).map_err(AcpError::from_response_error)
}

/// Validate that the adapter negotiated a protocol version this pinned SDK
/// build actually implements, rather than treating whatever came back as
/// automatically understood. Only `ProtocolVersion::V1` is compiled in here
/// (the `unstable_protocol_v2` SDK feature is not enabled), matching what
/// the Task 0 spike proved codex-acp negotiates.
fn validate_negotiated_protocol_version(info: InitializeInfo) -> Result<InitializeInfo, AcpError> {
    if info.protocol_version == ProtocolVersion::V1 {
        Ok(info)
    } else {
        Err(AcpError::UnsupportedProtocolVersion {
            negotiated: info.protocol_version.as_u16(),
        })
    }
}

async fn run_with_timeout<T>(
    timeout: Duration,
    method: &'static str,
    future: impl std::future::Future<Output = Result<T, agent_client_protocol::Error>>,
) -> Result<T, AcpError> {
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(AcpError::from_response_error(error)),
        Err(_elapsed) => Err(AcpError::Timeout {
            method: Some(method.to_string()),
            elapsed: timeout,
        }),
    }
}

/// The "foreground" future driving all outgoing ACP requests/notifications.
/// Runs concurrently with the incoming dispatch loop (see module docs).
///
/// Each request-issuing command is dispatched via [`ConnectionTo::spawn`]
/// rather than awaited inline. `agent-client-protocol`'s incoming dispatch
/// loop already processes one message at a time (see the module docs at the
/// top of this file); if this loop *also* awaited each command to
/// completion before reading the next one, a long-running `session/prompt`
/// would block a concurrently-issued `session/cancel` notification from ever
/// being sent until the prompt already resolved on its own -- exactly the
/// kind of head-of-line blocking Task 1's acceptance criteria rule out for
/// reverse requests, and just as fatal for cancellation. Spawning keeps this
/// loop free to immediately pick up the next command (a `Cancel`, another
/// `Prompt` for a different session, a permission response, ...) while
/// earlier ones are still in flight.
async fn command_loop(
    cx: ConnectionTo<Agent>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    timeout: Duration,
    pending_permissions: PendingPermissions,
) {
    while let Some(command) = commands.recv().await {
        match command {
            Command::Initialize { client_info, reply } => {
                let inner_cx = cx.clone();
                let _ = cx.spawn(async move {
                    let request = InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(ClientCapabilities::default())
                        .client_info(client_info);
                    let outcome = run_with_timeout(
                        timeout,
                        "initialize",
                        inner_cx.send_request(request).block_task(),
                    )
                    .await
                    .and_then(validate_negotiated_protocol_version);
                    let _ = reply.send(outcome);
                    Ok(())
                });
            }
            Command::NewSession { cwd, reply } => {
                let inner_cx = cx.clone();
                let _ = cx.spawn(async move {
                    let request = NewSessionRequest::new(cwd);
                    let outcome = run_with_timeout(
                        timeout,
                        "session/new",
                        inner_cx.send_request(request).block_task(),
                    )
                    .await
                    .map(|response| response.session_id);
                    let _ = reply.send(outcome);
                    Ok(())
                });
            }
            Command::Prompt { session_id, text, reply } => {
                let inner_cx = cx.clone();
                let _ = cx.spawn(async move {
                    let request =
                        PromptRequest::new(session_id, vec![ContentBlock::Text(TextContent::new(text))]);
                    let outcome = run_with_timeout(
                        timeout,
                        "session/prompt",
                        inner_cx.send_request(request).block_task(),
                    )
                    .await
                    .map(|response| PromptOutcome {
                        stop_reason: response.stop_reason,
                    });
                    let _ = reply.send(outcome);
                    Ok(())
                });
            }
            Command::Cancel { session_id } => {
                // A notification, not a request: nothing to spawn or await.
                let _ = cx.send_notification(CancelNotification::new(session_id));
            }
            Command::RespondPermission { id, outcome, reply } => {
                let pending_permissions = pending_permissions.clone();
                let _ = cx.spawn(async move {
                    let result = respond_permission(&pending_permissions, id, outcome);
                    let _ = reply.send(result);
                    Ok(())
                });
            }
            Command::CloseSession { session_id, reply } => {
                let inner_cx = cx.clone();
                let _ = cx.spawn(async move {
                    let request = CloseSessionRequest::new(session_id);
                    let outcome = run_with_timeout(
                        timeout,
                        "session/close",
                        inner_cx.send_request(request).block_task(),
                    )
                    .await
                    .map(|_response| ());
                    let _ = reply.send(outcome);
                    Ok(())
                });
            }
            Command::Shutdown { reply } => {
                let _ = reply.send(());
                break;
            }
        }
    }
}
