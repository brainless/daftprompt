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
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

// ── Types ──

pub struct CoordinatorConfig {
    pub retrieval_limit_per_source: usize,
    pub budget: SelectionBudget,
    pub request_timeout: Duration,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            retrieval_limit_per_source: 10,
            budget: SelectionBudget::default(),
            request_timeout: Duration::from_secs(60),
        }
    }
}

pub enum CoordinatorCommand {
    SubmitPrompt { original: String },
    CancelTurn,
    RespondPermission {
        request_id: PermissionRequestId,
        outcome: PermissionOutcome,
    },
    Shutdown,
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
        enriched: String,
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
    },
}

type PromptHandle = JoinHandle<Result<PromptOutcome, daftprompt_acp::AcpError>>;

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
                error: format!("ACP initialize failed: {e}"),
            });
            return;
        }
    };

    // Create ACP session
    let acp_session_id: AcpSessionId = match acp_client.new_session(".").await {
        Ok(id) => id,
        Err(e) => {
            let _ = events.send(CoordinatorEvent::AdapterError {
                error: format!("ACP session/new failed: {e}"),
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

    // Wrap acp_client in Arc so spawned prompt tasks can share it.
    let acp_client = Arc::new(acp_client);

    // Main command loop — races commands, ACP events, and in-flight prompt results.
    let mut active_turn_id: Option<i64> = None;
    let mut active_cancelled: bool = false;
    let mut prompt_handle: Option<PromptHandle> = None;
    let mut running = true;

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
            cmd = commands.recv() => match cmd {
                Some(cmd) => cmd,
                None => break,
            },
            event = acp_events.recv() => match event {
                Some(event) => {
                    handle_acp_event(&event, active_turn_id, &store, &events);
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
                let _ = events.send(CoordinatorEvent::TurnStarted { turn_id });

                // Retrieval (blocking, off render thread)
                let query = original.clone();
                let limit = config.retrieval_limit_per_source;
                let indexer_clone = indexer.clone();
                let retrieval_start = Instant::now();
                let search_result =
                    tokio::task::spawn_blocking(move || {
                        let indexer = indexer_clone.blocking_lock();
                        indexer.search_all_hybrid(&query, limit)
                    })
                    .await;

                let retrieval_latency_ms = retrieval_start.elapsed().as_millis() as i64;

                let outcome = match search_result {
                    Ok(Ok(result)) => {
                        let snapshot =
                            RetrievalSnapshot::from_all_source_result(&original, limit, &result);
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

                let candidate_count = match &outcome {
                    RetrievalOutcome::Ok(snapshot) => snapshot.candidates.len(),
                    _ => 0,
                };

                // Enrichment
                let original_prompt = OriginalPrompt::new(&original);
                let enriched = build_enriched_prompt(&original_prompt, &outcome, &config.budget);
                let included_count = enriched.included.len();

                // Persist retrieval and enriched prompt
                persist_retrieval(
                    &store,
                    turn_id,
                    &original,
                    limit,
                    &outcome,
                    &enriched,
                    retrieval_latency_ms,
                );

                // Transition to running
                if let Err(e) = store.transition_turn(turn_id, "running") {
                    let _ = events.send(CoordinatorEvent::TurnFailed {
                        turn_id,
                        error: format!("Failed to transition turn to running: {e}"),
                    });
                    active_turn_id = None;
                    continue;
                }

                let retrieval_status = match &outcome {
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
                    original: original.clone(),
                    enriched: enriched.text.clone(),
                });

                // Spawn the ACP prompt as a background task so the main loop
                // continues processing commands (CancelTurn, etc.).
                let client = acp_client.clone();
                let session_id = acp_session_id.clone();
                let text = enriched.text.clone();
                let handle = tokio::spawn(async move {
                    client.prompt(session_id, text).await
                });

                prompt_handle = Some(handle);
            }

            CoordinatorCommand::CancelTurn => {
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
                let _ = acp_client
                    .respond_permission(request_id, outcome)
                    .await;
            }

            CoordinatorCommand::Shutdown => {
                // If a prompt is in flight, cancel it first.
                if prompt_handle.is_some() {
                    let _ = acp_client.cancel(acp_session_id.clone());
                }
                let _ = acp_client.close_session(acp_session_id.clone()).await;
                running = false;
            }
        }
    }
}

fn persist_retrieval(
    store: &ConversationStore,
    turn_id: i64,
    original: &str,
    limit: usize,
    outcome: &RetrievalOutcome,
    enriched: &EnrichedPrompt,
    latency_ms: i64,
) {
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
    );

    if let Ok(run_id) = run_id {
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
                let _ = store.insert_retrieval_candidate(
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
                );
            }
        }
    }

    let retrieval_status_str = match &enriched.retrieval_status {
        RetrievalStatus::Ok => "ok",
        RetrievalStatus::Empty => "empty",
        RetrievalStatus::Error { .. } => "error",
    };

    let _ = store.set_enriched_prompt(
        turn_id,
        &enriched.text,
        enriched.formatter_version as i64,
        None,
        retrieval_status_str,
    );
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
    active_turn_id: Option<i64>,
    store: &ConversationStore,
    events: &mpsc::UnboundedSender<CoordinatorEvent>,
) {
    match event {
        AcpEvent::SessionUpdate {
            update: SessionUpdateKind::Known(update),
            ..
        } => {
            let update_json = serde_json::to_string(update).unwrap_or_default();
            if let Some(turn_id) = active_turn_id {
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
            if let Some(turn_id) = active_turn_id {
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

            if let Some(turn_id) = active_turn_id {
                let _ = store.append_event(
                    turn_id,
                    Some(turn_id),
                    "incoming",
                    "permission_request",
                    Some("session/request_permission"),
                    None,
                    &tool_call_json,
                );
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
