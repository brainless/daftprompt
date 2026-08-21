//! Typed events delivered to callers of [`crate::AcpClient`].
//!
//! Task 1 requires the public API to "expose typed events and errors rather
//! than raw process objects." Known `session/update` variants and permission
//! requests are exposed using the already-strongly-typed ACP schema types
//! (re-exported below); anything this crate could not parse becomes a
//! [`RawDiagnostic`] wrapped in [`SessionUpdateKind::Unknown`] or
//! [`AcpEvent::Diagnostic`] instead of crashing the session (Design
//! Decision #3).

use agent_client_protocol::schema::v1::{PermissionOption, SessionId, SessionUpdate, ToolCallUpdate};

use crate::diagnostic::RawDiagnostic;

/// The ACP session identifier. Re-exported directly since it is already an
/// opaque, strongly-typed wire value (an `Arc<str>` newtype) with no
/// transport-specific baggage.
pub type AcpSessionId = SessionId;

/// Opaque handle correlating a [`AcpEvent::PermissionRequested`] event with a
/// later [`crate::AcpClient::respond_permission`] call. Not the same as the
/// underlying JSON-RPC request id (which is an SDK-internal implementation
/// detail); this crate mints its own handle so the correlation stays part of
/// the typed public API.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionRequestId(pub(crate) String);

impl PermissionRequestId {
    /// Stable application-level correlation value for durable traces.
    /// This is minted by daftprompt-acp and is not the SDK's JSON-RPC id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A reverse request from the adapter asking the user to approve or reject a
/// tool call.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    /// Handle to use with [`crate::AcpClient::respond_permission`].
    pub id: PermissionRequestId,
    /// The session this request belongs to.
    pub session_id: AcpSessionId,
    /// The tool call description supplied by the adapter, as-is.
    pub tool_call: ToolCallUpdate,
    /// The exact options offered by the adapter. A response must choose one
    /// of these `option_id`s verbatim, or cancel; see
    /// [`crate::AcpError::InvalidPermissionOption`].
    pub options: Vec<PermissionOption>,
}

/// A caller's decision for a pending [`PermissionRequest`], passed to
/// [`crate::AcpClient::respond_permission`]. `Selected` must reference one of
/// the `option_id`s the adapter actually offered (see
/// [`crate::AcpError::InvalidPermissionOption`]); the runtime never invents
/// or substitutes an option on the caller's behalf (Design Decision #7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOutcome {
    /// The user chose one of the offered options, by its exact `optionId`.
    Selected(String),
    /// The prompt turn was cancelled before the user responded.
    Cancelled,
}

/// A single `session/update` notification's payload: either a known,
/// fully-typed variant, or a bounded diagnostic when this crate could not
/// parse it.
#[derive(Debug, Clone)]
pub enum SessionUpdateKind {
    /// A `session/update` payload that parsed against the pinned schema
    /// version.
    Known(SessionUpdate),
    /// A `session/update` payload with a `sessionUpdate` discriminator (or
    /// shape) this crate's pinned schema does not recognize. Preserved, not
    /// dropped, per Design Decision #3.
    Unknown(RawDiagnostic),
}

/// Everything daftprompt-acp can deliver to a caller after
/// [`crate::AcpClient::launch`]. Delivered in wire order on one channel so
/// relative ordering between session updates, permission requests, and
/// diagnostics is preserved.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// A `session/update` notification from the agent.
    SessionUpdate {
        /// The session this update pertains to.
        session_id: AcpSessionId,
        /// The update payload.
        update: SessionUpdateKind,
    },
    /// A `session/request_permission` reverse request from the agent.
    /// Interleaves with `SessionUpdate` events for the same or other
    /// sessions; answering it does not block delivery of other events (see
    /// Task 1's "Concurrent reverse requests and streaming notifications do
    /// not block prompt-response correlation").
    PermissionRequested(PermissionRequest),
    /// A message or reverse request this crate did not recognize at all
    /// (unknown method name). Surfaced generically rather than dropped or
    /// treated as fatal.
    Diagnostic(RawDiagnostic),
}
