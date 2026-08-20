use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use daftprompt_acp::{
    AcpClient, AcpEvent, AcpEvents, AcpSessionId,
    PermissionOutcome, PermissionRequestId, PromptOutcome, SessionUpdateKind,
};
use daftprompt_acp::StopReason;
use daftprompt_indexer::Indexer;
use daftprompt_prompt_builder::{
    build_enriched_prompt, EnrichedPrompt, ExcerptLocation, OriginalPrompt, RetrievalOutcome,
    RetrievalSnapshot, RetrievalStatus, SelectionBudget,
};
use daftprompt_storage::ConversationStore;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;

// ── Types ──

pub struct CoordinatorConfig {
    pub retrieval_limit_per_source: usize,
    pub budget: SelectionBudget,
    pub request_timeout: Duration,
    /// Bound applied to [`AcpClient::shutdown`] on the real application exit
    /// path (Design Decision #8, Task 6). Not consulted by anything else.
    pub shutdown_grace: Duration,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            retrieval_limit_per_source: 10,
            budget: SelectionBudget::default(),
            request_timeout: Duration::from_secs(60),
            shutdown_grace: Duration::from_secs(10),
        }
    }
}

pub enum CoordinatorCommand {
    SubmitPrompt { original: String },
    /// Re-dispatch a turn left in the terminal 'failed' state with a
    /// retained enriched prompt (see `ConversationStore::retry_turn`).
    /// Not yet sent by the UI (that wiring is a separate task); covered by
    /// coordinator integration tests.
    #[allow(dead_code)]
    RetryTurn { turn_id: i64 },
    CancelTurn,
    RespondPermission {
        request_id: PermissionRequestId,
        outcome: PermissionOutcome,
    },
    /// Cancels in-flight work, closes the session (if advertised), and calls
    /// [`AcpClient::shutdown`] to reap or force-kill the adapter child
    /// process. `done` fires once that is confirmed, bounded by
    /// [`CoordinatorConfig::shutdown_grace`], so a real caller can block
    /// application exit on it (Task 6).
    Shutdown { done: oneshot::Sender<()> },
}

pub enum CoordinatorEvent {
    SessionCreated {
        session_db_id: i64,
        acp_session_id: String,
        adapter_name: String,
        adapter_version: String,
        protocol_version: String,
    },
    TurnStarted {
        turn_id: i64,
    },
    RetrievalCompleted {
        turn_id: i64,
        status: String,
        candidate_count: usize,
        included_count: usize,
    },
    EnrichedPromptReady {
        turn_id: i64,
        original: String,
        /// The full enriched prompt, not just its rendered text, so the UI
        /// can also show which candidates were included/excluded and why
        /// (Task 5 acceptance: "Retrieval omissions and truncation are
        /// visible, not silently discarded").
        enriched: EnrichedPrompt,
    },
    AcpSessionUpdate {
        turn_id: i64,
        update_json: String,
    },
    PermissionRequired {
        request_id: PermissionRequestId,
        tool_call_json: String,
        options_json: String,
    },
    TurnCompleted {
        turn_id: i64,
        stop_reason: String,
    },
    TurnFailed {
        turn_id: i64,
        error: String,
    },
    AdapterError {
        error: String,
        kind: AdapterErrorKind,
    },
}

/// A coarse, UI-renderable classification of an [`CoordinatorEvent::AdapterError`],
/// distinguishing the [`daftprompt_acp::AcpError`] variants that already carry
/// distinguishable structure instead of collapsing everything to `Display`
/// text. Errors that do not originate from the ACP runtime boundary (e.g. a
/// conversation-storage failure while creating a session) use `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterErrorKind {
    Timeout,
    MalformedMessage,
    BrokenPipe,
    EarlyExit,
    UnsupportedProtocolVersion,
    Protocol,
    InvalidPermissionOption,
    UnknownPermissionRequest,
    ShuttingDown,
    AuthenticationRequired,
    Other,
}

impl AdapterErrorKind {
    fn from_acp_error(error: &daftprompt_acp::AcpError) -> Self {
        match error {
            daftprompt_acp::AcpError::Timeout { .. } => Self::Timeout,
            daftprompt_acp::AcpError::MalformedMessage { .. } => Self::MalformedMessage,
            daftprompt_acp::AcpError::BrokenPipe { .. } => Self::BrokenPipe,
            daftprompt_acp::AcpError::EarlyExit { .. } => Self::EarlyExit,
            daftprompt_acp::AcpError::UnsupportedProtocolVersion { .. } => {
                Self::UnsupportedProtocolVersion
            }
            daftprompt_acp::AcpError::Protocol { .. } => Self::Protocol,
            daftprompt_acp::AcpError::InvalidPermissionOption { .. } => {
                Self::InvalidPermissionOption
            }
            daftprompt_acp::AcpError::UnknownPermissionRequest => Self::UnknownPermissionRequest,
            daftprompt_acp::AcpError::ShuttingDown => Self::ShuttingDown,
            daftprompt_acp::AcpError::AuthenticationRequired { .. } => {
                Self::AuthenticationRequired
            }
            daftprompt_acp::AcpError::Internal(_) => Self::Other,
        }
    }

    /// Short label used as a transcript-entry prefix, e.g. "[Timeout]".
    pub fn label(&self) -> &'static str {
        match self {
            Self::Timeout => "Timeout",
            Self::MalformedMessage => "Malformed message",
            Self::BrokenPipe => "Broken pipe",
            Self::EarlyExit => "Process exited",
            Self::UnsupportedProtocolVersion => "Unsupported protocol version",
            Self::Protocol => "Protocol error",
            Self::InvalidPermissionOption => "Invalid permission option",
            Self::UnknownPermissionRequest => "Unknown permission request",
            Self::ShuttingDown => "Shutting down",
            Self::AuthenticationRequired => "Authentication required",
            Self::Other => "Adapter error",
        }
    }
}

/// Builds the transcript-facing error message for an ACP failure, adding an
/// actionable authentication hint for [`daftprompt_acp::AcpError::AuthenticationRequired`]
/// (the ACP wire protocol's `auth_required` JSON-RPC error). daftprompt has
/// no interactive authentication UI (Epic 014 non-goals), so the hint points
/// at codex-acp's documented non-interactive auth flow instead: the
/// `CODEX_API_KEY`/`OPENAI_API_KEY` environment variables, or a
/// `DEFAULT_AUTH_REQUEST` configured for the adapter process (see
/// `~/Projects/codex-acp/README.md`'s "Authentication" section). Every other
/// error kind falls back to the plain `Display` message.
fn authentication_error_message(error: &daftprompt_acp::AcpError, context: &str) -> String {
    match error {
        daftprompt_acp::AcpError::AuthenticationRequired { .. } => format!(
            "{context}: {error}. This adapter requires authentication before it can \
             process prompts. Set CODEX_API_KEY or OPENAI_API_KEY in the adapter's \
             environment (--adapter-env), or configure DEFAULT_AUTH_REQUEST, before \
             starting a conversation."
        ),
        _ => format!("{context}: {error}"),
    }
}

type PromptHandle = JoinHandle<Result<PromptOutcome, daftprompt_acp::AcpError>>;

/// The result of the off-loop retrieval + enrichment step (Task 4 item 3):
/// running in its own spawned task so the main command loop stays free to
/// process a concurrent `CancelTurn` while retrieval is in flight.
struct PreparedPrompt {
    original: String,
    limit: usize,
    outcome: RetrievalOutcome,
    enriched: EnrichedPrompt,
    retrieval_latency_ms: i64,
}

type PrepareHandle = JoinHandle<PreparedPrompt>;

/// Context recorded for a pending `session/request_permission` reverse
/// request so `RespondPermission` can persist the decision (Task 4 item 5)
/// once it is known, correlated back to the turn and event that produced it.
struct PendingPermission {
    turn_id: i64,
    event_id: i64,
    tool_call_json: String,
    options_json: String,
}

// ── Public entry point ──

pub fn start_coordinator(
    indexer: Arc<Mutex<Indexer>>,
    store: ConversationStore,
    acp_client: AcpClient,
    acp_events: AcpEvents,
    config: CoordinatorConfig,
) -> (
    mpsc::UnboundedSender<CoordinatorCommand>,
    mpsc::UnboundedReceiver<CoordinatorEvent>,
) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel();

    tokio::spawn(run(indexer, store, acp_client, acp_events, config, cmd_rx, evt_tx));

    (cmd_tx, evt_rx)
}

// ── Coordinator loop ──

async fn run(
    indexer: Arc<Mutex<Indexer>>,
    store: ConversationStore,
    acp_client: AcpClient,
    mut acp_events: AcpEvents,
    config: CoordinatorConfig,
    mut commands: mpsc::UnboundedReceiver<CoordinatorCommand>,
    events: mpsc::UnboundedSender<CoordinatorEvent>,
) {
    // Initialize ACP connection
    let init_info = match acp_client.initialize().await {
        Ok(info) => info,
        Err(e) => {
            let _ = events.send(CoordinatorEvent::AdapterError {
                error: authentication_error_message(&e, "ACP initialize failed"),
                kind: AdapterErrorKind::from_acp_error(&e),
            });
            return;
        }
    };

    // Create ACP session
    let acp_session_id: AcpSessionId = match acp_client.new_session(".").await {
        Ok(id) => id,
        Err(e) => {
            let _ = events.send(CoordinatorEvent::AdapterError {
                error: authentication_error_message(&e, "ACP session/new failed"),
                kind: AdapterErrorKind::from_acp_error(&e),
            });
            return;
        }
    };

    // Persist session and emit event
    let session_db_id = match store.create_session(
        "coordinator",
        ".",
        None,
        None,
        None,
        Some(&acp_session_id.to_string()),
        None,
        None,
        None,
    ) {
        Ok(id) => id,
        Err(e) => {
            let _ = events.send(CoordinatorEvent::AdapterError {
                error: format!("Failed to create session in storage: {e}"),
                kind: AdapterErrorKind::Other,
            });
            return;
        }
    };

    let _ = events.send(CoordinatorEvent::SessionCreated {
        session_db_id,
        acp_session_id: acp_session_id.to_string(),
        adapter_name: init_info
            .agent_info
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_default(),
        adapter_version: init_info
            .agent_info
            .as_ref()
            .map(|i| i.version.clone())
            .unwrap_or_default(),
        protocol_version: format!("v{}", init_info.protocol_version.as_u16()),
    });

    log::info!(
        "Coordinator started: session_db_id={session_db_id}, acp_session={acp_session_id}"
    );

    // Design Decision #3: "Session close should be used when advertised."
    // `agentCapabilities.sessionCapabilities.close` is `Some(..)` (even
    // `Some(SessionCloseCapabilities::default())`) only when the adapter
    // actually advertises `session/close` support; `None` means it was
    // omitted from the response, i.e. not advertised.
    let session_close_supported = init_info
        .agent_capabilities
        .session_capabilities
        .close
        .is_some();
    let shutdown_grace = config.shutdown_grace;

    // Wrap acp_client in Arc so spawned prompt tasks can share it.
    let acp_client = Arc::new(acp_client);

    // Main command loop — races commands, ACP events, in-flight retrieval
    // preparation, and in-flight prompt results.
    let mut active_turn_id: Option<i64> = None;
    let mut active_cancelled: bool = false;
    let mut prepare_handle: Option<PrepareHandle> = None;
    let mut prompt_handle: Option<PromptHandle> = None;
    // The turn whose ACP request stream incoming `session/update`/permission
    // events currently belong to. This is deliberately distinct from
    // `active_turn_id`: `active_turn_id` becomes Some(turn_id) as soon as a
    // turn is created (before retrieval even starts), but `dispatched_turn_id`
    // only moves forward when a NEW turn's `session/prompt` call actually
    // goes out. Without this split, a turn cancelled while a *later* turn is
    // still preparing would have its late `session/update` events attributed
    // to that later turn once it starts (both would share `active_turn_id`'s
    // value at different times) even though the later turn never issued the
    // request that produced them. Tagging events by `dispatched_turn_id`
    // instead means a late event either lands on the turn that actually
    // caused it (if no newer dispatch has begun yet) or is silently outside
    // any turn once a newer dispatch takes over — never misattributed.
    let mut dispatched_turn_id: Option<i64> = None;
    let mut pending_permissions: HashMap<PermissionRequestId, PendingPermission> = HashMap::new();
    let mut running = true;
    let mut shutdown_done: Option<oneshot::Sender<()>> = None;

    while running {
        let command = tokio::select! {
            // If a prompt is in flight, race it against commands and events.
            result = async {
                match prompt_handle.as_mut() {
                    Some(h) => (&mut *h).await,
                    None => std::future::pending().await,
                }
            } => {
                prompt_handle = None;
                let turn_id = active_turn_id.take().expect("active_turn_id was Some");
                let cancelled = active_cancelled;
                active_cancelled = false;
                handle_prompt_result(turn_id, cancelled, result, &store, &events);
                continue;
            }
            // If retrieval + enrichment is in flight, race it too, so a
            // concurrent CancelTurn is still processed while it runs.
            result = async {
                match prepare_handle.as_mut() {
                    Some(h) => (&mut *h).await,
                    None => std::future::pending().await,
                }
            } => {
                prepare_handle = None;
                let turn_id = active_turn_id.expect("active_turn_id was Some while preparing");
                match result {
                    Ok(prepared) => {
                        finish_preparation(
                            turn_id,
                            prepared,
                            active_cancelled,
                            &store,
                            &acp_client,
                            &acp_session_id,
                            &events,
                            &mut prompt_handle,
                            &mut dispatched_turn_id,
                        );
                        if prompt_handle.is_none() {
                            // Either persistence failed or the turn was
                            // cancelled while preparing; no ACP dispatch
                            // happened, so the turn is already terminal.
                            active_turn_id = None;
                            active_cancelled = false;
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("Retrieval task panicked: {e}");
                        let _ = store.set_turn_error(turn_id, &error_msg);
                        let _ = store.transition_turn(turn_id, "failed");
                        let _ = events.send(CoordinatorEvent::TurnFailed {
                            turn_id,
                            error: error_msg,
                        });
                        active_turn_id = None;
                        active_cancelled = false;
                    }
                }
                continue;
            }
            cmd = commands.recv() => match cmd {
                Some(cmd) => cmd,
                None => break,
            },
            event = acp_events.recv() => match event {
                Some(event) => {
                    handle_acp_event(&event, dispatched_turn_id, session_db_id, &store, &events, &mut pending_permissions);
                    continue;
                }
                None => continue,
            },
        };

        match command {
            CoordinatorCommand::SubmitPrompt { original } => {
                if active_turn_id.is_some() {
                    log::warn!("Rejecting SubmitPrompt: a turn is already active");
                    continue;
                }

                let turn_id = match store.create_turn(session_db_id, &original) {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = events.send(CoordinatorEvent::TurnFailed {
                            turn_id: 0,
                            error: format!("Failed to create turn: {e}"),
                        });
                        continue;
                    }
                };
                active_turn_id = Some(turn_id);
                active_cancelled = false;
                let _ = events.send(CoordinatorEvent::TurnStarted { turn_id });

                prepare_handle = Some(spawn_prepare_task(
                    indexer.clone(),
                    original,
                    config.retrieval_limit_per_source,
                    config.budget.clone(),
                ));
            }

            CoordinatorCommand::RetryTurn { turn_id } => {
                if active_turn_id.is_some() {
                    log::warn!(
                        "Rejecting RetryTurn for turn {turn_id}: a turn is already active"
                    );
                    continue;
                }

                match store.retry_turn(turn_id) {
                    Ok(Some(enriched_text)) => {
                        active_turn_id = Some(turn_id);
                        active_cancelled = false;
                        let _ = events.send(CoordinatorEvent::TurnStarted { turn_id });

                        dispatched_turn_id = Some(turn_id);
                        let client = acp_client.clone();
                        let session_id = acp_session_id.clone();
                        let handle = tokio::spawn(async move {
                            client.prompt(session_id, enriched_text).await
                        });
                        prompt_handle = Some(handle);
                    }
                    Ok(None) => {
                        log::info!(
                            "RetryTurn: turn {turn_id} is not retryable (not failed or no retained enriched prompt)"
                        );
                    }
                    Err(e) => {
                        log::error!("RetryTurn: failed to reset turn {turn_id}: {e}");
                    }
                }
            }

            CoordinatorCommand::CancelTurn => {
                // Covers both "preparing" (prepare_handle in flight) and
                // "running" (prompt_handle in flight): either way the turn
                // is finalized as Cancelled once its in-flight handle is
                // awaited by the select! branches above.
                if active_turn_id.is_some() && !active_cancelled {
                    active_cancelled = true;
                    if let Some(turn_id) = active_turn_id {
                        let _ = store.transition_turn(turn_id, "cancelled");
                    }
                    let _ = acp_client.cancel(acp_session_id.clone());
                }
            }

            CoordinatorCommand::RespondPermission {
                request_id,
                outcome,
            } => {
                let (outcome_str, chosen_option_id) = match &outcome {
                    PermissionOutcome::Selected(id) => ("selected", Some(id.as_str())),
                    PermissionOutcome::Cancelled => ("cancelled", None),
                };

                if let Some(pending) = pending_permissions.remove(&request_id) {
                    let _ = store.record_permission(
                        pending.event_id,
                        pending.turn_id,
                        &pending.tool_call_json,
                        &pending.options_json,
                        chosen_option_id,
                        outcome_str,
                    );
                } else {
                    log::warn!(
                        "RespondPermission: no pending permission request tracked for this id"
                    );
                }

                if let Err(e) = acp_client.respond_permission(request_id, outcome).await {
                    let _ = events.send(CoordinatorEvent::AdapterError {
                        error: format!("Failed to respond to permission request: {e}"),
                        kind: AdapterErrorKind::from_acp_error(&e),
                    });
                }
            }

            CoordinatorCommand::Shutdown { done } => {
                // `AcpClient::shutdown` takes `self` by value, but `acp_client`
                // is an `Arc` shared with the in-flight prompt task's spawned
                // future (see `finish_preparation`/`RetryTurn`). It must be
                // the sole owner before it can call `.shutdown()` below, so
                // every clone has to be dropped first. `prepare_handle`
                // never clones `acp_client` (retrieval only touches the
                // indexer), so only `prompt_handle` matters here: abort it
                // and await the `JoinHandle` so the aborted task -- and the
                // `Arc` clone it was holding -- is actually gone, not just
                // requested-to-stop, before `Arc::try_unwrap` is attempted.
                if let Some(handle) = prompt_handle.take() {
                    let _ = acp_client.cancel(acp_session_id.clone());
                    handle.abort();
                    let _ = handle.await;
                }
                if let Some(handle) = prepare_handle.take() {
                    handle.abort();
                    let _ = handle.await;
                }

                if session_close_supported {
                    let _ = acp_client.close_session(acp_session_id.clone()).await;
                }

                running = false;
                shutdown_done = Some(done);
            }
        }
    }

    // Every place that clones `acp_client` (the `RetryTurn` command handler
    // and `finish_preparation`) immediately stores the resulting task's
    // `JoinHandle` in `prompt_handle`, and `prepare_handle` never clones it
    // at all -- so aborting and awaiting both handles above (on the
    // `Shutdown` path) or simply never having started a turn (on the
    // `commands.recv() => None` path, e.g. `cmd_tx` dropped without an
    // explicit `Shutdown`) leaves `acp_client` uniquely owned here. Fall
    // back to a short bounded wait rather than asserting, in case that
    // invariant is ever violated by a future change.
    let acp_client = match Arc::try_unwrap(acp_client) {
        Ok(client) => Some(client),
        Err(shared) => {
            let deadline = Instant::now() + shutdown_grace;
            loop {
                if Arc::strong_count(&shared) == 1 {
                    break Arc::try_unwrap(shared).ok();
                }
                if Instant::now() >= deadline {
                    log::warn!(
                        "AcpClient still shared at shutdown after grace period; adapter process may not be reaped"
                    );
                    break None;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    };
    if let Some(client) = acp_client {
        if let Err(e) = client.shutdown(shutdown_grace).await {
            log::warn!("AcpClient::shutdown did not complete cleanly: {e}");
        }
    }
    if let Some(done) = shutdown_done {
        let _ = done.send(());
    }
}

/// Spawns the retrieval + enrichment step as its own task so the main
/// command loop can keep racing `CancelTurn` and ACP events while it runs
/// (Task 4 item 3).
fn spawn_prepare_task(
    indexer: Arc<Mutex<Indexer>>,
    original: String,
    limit: usize,
    budget: SelectionBudget,
) -> PrepareHandle {
    tokio::spawn(async move {
        let retrieval_start = Instant::now();
        let query = original.clone();
        let search_result = tokio::task::spawn_blocking(move || {
            let indexer = indexer.blocking_lock();
            indexer.search_all_hybrid(&query, limit)
        })
        .await;

        let retrieval_latency_ms = retrieval_start.elapsed().as_millis() as i64;

        let outcome = match search_result {
            Ok(Ok(result)) => {
                let snapshot = RetrievalSnapshot::from_all_source_result(&original, limit, &result);
                RetrievalOutcome::Ok(snapshot)
            }
            Ok(Err(e)) => RetrievalOutcome::Error {
                query: original.clone(),
                message: e.to_string(),
            },
            Err(e) => RetrievalOutcome::Error {
                query: original.clone(),
                message: format!("Search task panicked: {e}"),
            },
        };

        let original_prompt = OriginalPrompt::new(&original);
        let enriched = build_enriched_prompt(&original_prompt, &outcome, &budget);

        PreparedPrompt {
            original,
            limit,
            outcome,
            enriched,
            retrieval_latency_ms,
        }
    })
}

/// Handles the result of the retrieval + enrichment step once it completes:
/// persists the retrieval run and enriched prompt, then either dispatches
/// the ACP prompt (setting `prompt_handle`) or finalizes the turn without
/// dispatching (persistence failure or a cancellation that arrived while
/// preparing). Persistence failure must block ACP dispatch (Task 4 item 2).
#[allow(clippy::too_many_arguments)]
fn finish_preparation(
    turn_id: i64,
    prepared: PreparedPrompt,
    cancelled: bool,
    store: &ConversationStore,
    acp_client: &Arc<AcpClient>,
    acp_session_id: &AcpSessionId,
    events: &mpsc::UnboundedSender<CoordinatorEvent>,
    prompt_handle: &mut Option<PromptHandle>,
    dispatched_turn_id: &mut Option<i64>,
) {
    if cancelled {
        let _ = events.send(CoordinatorEvent::TurnCompleted {
            turn_id,
            stop_reason: "Cancelled".to_string(),
        });
        return;
    }

    let candidate_count = match &prepared.outcome {
        RetrievalOutcome::Ok(snapshot) => snapshot.candidates.len(),
        _ => 0,
    };
    let included_count = prepared.enriched.included.len();

    if let Err(e) = persist_retrieval(
        store,
        turn_id,
        &prepared.original,
        prepared.limit,
        &prepared.outcome,
        &prepared.enriched,
        prepared.retrieval_latency_ms,
    ) {
        let error_msg = format!("Failed to persist retrieval/enriched prompt: {e}");
        let _ = store.set_turn_error(turn_id, &error_msg);
        let _ = store.transition_turn(turn_id, "failed");
        let _ = events.send(CoordinatorEvent::TurnFailed {
            turn_id,
            error: error_msg,
        });
        return;
    }

    if let Err(e) = store.transition_turn(turn_id, "running") {
        let _ = events.send(CoordinatorEvent::TurnFailed {
            turn_id,
            error: format!("Failed to transition turn to running: {e}"),
        });
        return;
    }

    let retrieval_status = match &prepared.outcome {
        RetrievalOutcome::Ok(_) => "ok",
        RetrievalOutcome::Error { .. } => "error",
    };
    let _ = events.send(CoordinatorEvent::RetrievalCompleted {
        turn_id,
        status: retrieval_status.to_string(),
        candidate_count,
        included_count,
    });
    let _ = events.send(CoordinatorEvent::EnrichedPromptReady {
        turn_id,
        original: prepared.original.clone(),
        enriched: prepared.enriched.clone(),
    });

    // Spawn the ACP prompt as a background task so the main loop continues
    // processing commands (CancelTurn, etc.). This is also the one place a
    // new turn's ACP stream identity comes into existence, matching the
    // `dispatched_turn_id` invariant documented on it above.
    *dispatched_turn_id = Some(turn_id);
    let client = acp_client.clone();
    let session_id = acp_session_id.clone();
    let text = prepared.enriched.text.clone();
    let handle = tokio::spawn(async move { client.prompt(session_id, text).await });
    *prompt_handle = Some(handle);
}

/// Persists the retrieval run, its candidates, and the enriched prompt for a
/// turn. Returns an error if any of these writes fail, so the caller can
/// block ACP dispatch rather than silently sending an unrecorded prompt
/// (Design Decision #4, Task 4 item 2).
fn persist_retrieval(
    store: &ConversationStore,
    turn_id: i64,
    original: &str,
    limit: usize,
    outcome: &RetrievalOutcome,
    enriched: &EnrichedPrompt,
    latency_ms: i64,
) -> anyhow::Result<()> {
    let retrieval_status = match outcome {
        RetrievalOutcome::Ok(_) => "ok",
        RetrievalOutcome::Error { .. } => "error",
    };

    let run_id = store.create_retrieval_run(
        turn_id,
        original,
        limit as i64,
        retrieval_status,
        match outcome {
            RetrievalOutcome::Error { message, .. } => Some(message.as_str()),
            _ => None,
        },
        Some(latency_ms),
    )?;

    if let RetrievalOutcome::Ok(snapshot) = outcome {
        for (rank, candidate) in snapshot.candidates.iter().enumerate() {
            let included = enriched
                .included
                .iter()
                .any(|e| e.identifier == candidate.identifier && e.rank == rank + 1);
            let location_json = match &candidate.location {
                ExcerptLocation::Code {
                    file_path,
                    line_start,
                    line_end,
                } => format!(
                    r#"{{"Code":{{"file_path":"{}","line_start":{},"line_end":{}}}}}"#,
                    file_path, line_start, line_end
                ),
                ExcerptLocation::Document { file_path } => {
                    format!(r#"{{"Document":{{"file_path":"{}"}}}}"#, file_path)
                }
                ExcerptLocation::Commit { short_hash } => {
                    format!(r#"{{"Commit":{{"short_hash":"{}"}}}}"#, short_hash)
                }
            };
            store.insert_retrieval_candidate(
                run_id,
                &candidate.identifier,
                candidate.source.as_str(),
                rank as i64 + 1,
                candidate.score as f64,
                &format!("{:?}", candidate.match_type),
                &candidate.text,
                &location_json,
                included,
                None,
                None,
                None,
                None,
            )?;
        }
    }

    let retrieval_status_str = match &enriched.retrieval_status {
        RetrievalStatus::Ok => "ok",
        RetrievalStatus::Empty => "empty",
        RetrievalStatus::Error { .. } => "error",
    };

    // set_enriched_prompt returns StorageError, not anyhow::Error; StorageError
    // implements std::error::Error so `?` converts it via anyhow's blanket From.
    store.set_enriched_prompt(
        turn_id,
        &enriched.text,
        enriched.formatter_version as i64,
        None,
        retrieval_status_str,
    )?;
    Ok(())
}

fn handle_prompt_result(
    turn_id: i64,
    cancelled: bool,
    result: Result<Result<PromptOutcome, daftprompt_acp::AcpError>, tokio::task::JoinError>,
    store: &ConversationStore,
    events: &mpsc::UnboundedSender<CoordinatorEvent>,
) {
    if cancelled {
        // User already cancelled — the prompt result is informational only.
        let _ = events.send(CoordinatorEvent::TurnCompleted {
            turn_id,
            stop_reason: "Cancelled".to_string(),
        });
        return;
    }

    match result {
        Ok(Ok(outcome)) => {
            let reason = format!("{:?}", outcome.stop_reason);
            let _ = store.set_turn_stop_reason(turn_id, &reason);
            let new_state = match outcome.stop_reason {
                StopReason::Cancelled => "cancelled",
                _ => "completed",
            };
            let _ = store.transition_turn(turn_id, new_state);
            let _ = events.send(CoordinatorEvent::TurnCompleted {
                turn_id,
                stop_reason: reason,
            });
        }
        Ok(Err(e)) => {
            let error_msg = e.to_string();
            let _ = store.set_turn_error(turn_id, &error_msg);
            let _ = store.transition_turn(turn_id, "failed");
            let _ = events.send(CoordinatorEvent::TurnFailed {
                turn_id,
                error: error_msg,
            });
        }
        Err(e) => {
            let error_msg = format!("Prompt task panicked: {e}");
            let _ = store.set_turn_error(turn_id, &error_msg);
            let _ = store.transition_turn(turn_id, "failed");
            let _ = events.send(CoordinatorEvent::TurnFailed {
                turn_id,
                error: error_msg,
            });
        }
    }
}

fn handle_acp_event(
    event: &AcpEvent,
    dispatched_turn_id: Option<i64>,
    session_db_id: i64,
    store: &ConversationStore,
    events: &mpsc::UnboundedSender<CoordinatorEvent>,
    pending_permissions: &mut HashMap<PermissionRequestId, PendingPermission>,
) {
    match event {
        AcpEvent::SessionUpdate {
            update: SessionUpdateKind::Known(update),
            ..
        } => {
            let update_json = serde_json::to_string(update).unwrap_or_default();
            if let Some(turn_id) = dispatched_turn_id {
                let _ = events.send(CoordinatorEvent::AcpSessionUpdate {
                    turn_id,
                    update_json,
                });
            }
        }
        AcpEvent::SessionUpdate {
            update: SessionUpdateKind::Unknown(diag),
            ..
        } => {
            if let Some(turn_id) = dispatched_turn_id {
                let _ = events.send(CoordinatorEvent::AcpSessionUpdate {
                    turn_id,
                    update_json: diag.raw.clone(),
                });
            }
        }
        AcpEvent::PermissionRequested(request) => {
            let tool_call_json =
                serde_json::to_string(&request.tool_call).unwrap_or_default();
            let options_json =
                serde_json::to_string(&request.options).unwrap_or_default();

            if let Some(turn_id) = dispatched_turn_id {
                if let Ok(event_id) = store.append_event(
                    session_db_id,
                    Some(turn_id),
                    "inbound",
                    "permission_request",
                    Some("session/request_permission"),
                    None,
                    &tool_call_json,
                ) {
                    pending_permissions.insert(
                        request.id.clone(),
                        PendingPermission {
                            turn_id,
                            event_id,
                            tool_call_json: tool_call_json.clone(),
                            options_json: options_json.clone(),
                        },
                    );
                }
            }

            let _ = events.send(CoordinatorEvent::PermissionRequired {
                request_id: request.id.clone(),
                tool_call_json,
                options_json,
            });
        }
        AcpEvent::Diagnostic(diag) => {
            log::debug!("ACP diagnostic: {}: {}", diag.label, diag.raw);
        }
    }
}
