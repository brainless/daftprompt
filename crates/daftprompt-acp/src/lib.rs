//! `daftprompt-acp`: the Agent Client Protocol (ACP) runtime boundary.
//!
//! This is Epic 014 ("ACP Prompt Enrichment Client") Task 1. It is the
//! **only** crate in the daftprompt workspace that depends on the ACP SDK
//! (`agent-client-protocol`) or any ACP wire schema. Everything else in the
//! workspace that needs to talk to an ACP adapter (currently `codex-acp`)
//! does so through the typed API re-exported here -- [`AcpClient`],
//! [`AcpEvent`], [`AcpError`] -- never through raw process handles or
//! `serde_json::Value` protocol payloads.
//!
//! See `epics/014-acp-prompt-enrichment-client.md` Task 1 for the
//! acceptance criteria this crate is built to satisfy, and
//! `epics/research/014-acp-adapter-compatibility.md` for the Task 0 spike
//! findings this design is built on (in particular: the maintained
//! `agent-client-protocol` crate v2.0.0 is wire-compatible with the local
//! codex-acp adapter, so no hand-rolled JSON-RPC layer is needed).
//!
//! # Module layout
//!
//! - [`launch`] -- [`AdapterLaunchProfile`]: how to start an adapter process
//!   (executable, args, env), never through a shell.
//! - [`client`] -- [`AcpClient`]: process supervision, request
//!   correlation, timeouts, cancellation, permission responses, session
//!   close, and graceful shutdown, driven on a background task so ACP IO
//!   never touches the caller's thread (Design Decision #8).
//! - [`events`] -- typed events ([`AcpEvent`]) delivered from the adapter:
//!   session updates, reverse permission requests, and generic diagnostics
//!   for anything unrecognized.
//! - [`error`] -- [`AcpError`]: distinguishable timeout, malformed-message,
//!   broken-pipe, early-exit, and protocol error variants.
//! - [`diagnostic`] -- [`RawDiagnostic`]: the bounded, size-capped type used
//!   to preserve raw protocol payloads this crate did not fully model,
//!   instead of leaking unbounded `serde_json::Value`s through the public
//!   API or dropping the data silently.

mod client;
mod diagnostic;
mod error;
mod events;
mod launch;

pub use client::{AcpClient, AcpClientConfig, AcpEvents, InitializeInfo, PromptOutcome};
pub use diagnostic::{RawDiagnostic, RAW_DIAGNOSTIC_BYTE_LIMIT};
pub use error::AcpError;
pub use events::{
    AcpEvent, AcpSessionId, PermissionOutcome, PermissionRequest, PermissionRequestId,
    SessionUpdateKind,
};
pub use launch::AdapterLaunchProfile;

// Re-exported so callers can match on known `session/update` variants,
// tool-call/permission-option fields, and stop reasons without needing
// their own direct dependency on the ACP SDK crate; see the module docs
// above for why this crate stays the only *Cargo.toml* dependent on it.
pub use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthMethod, PermissionOption, PermissionOptionId, PermissionOptionKind,
    SessionUpdate, StopReason, ToolCall, ToolCallUpdate,
};
