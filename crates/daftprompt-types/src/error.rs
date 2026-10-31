//! Explicit parse failures.
//!
//! The error type stays independent of HTTP: the API maps each variant to a
//! status code in its own layer. A parse either returns a complete validated
//! graph or one of these errors; a partly invalid graph is never returned.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a parse produced no graph.
///
/// The serialized `kind` discriminant is the same value [`ParseError::kind`]
/// returns, so logs and API error bodies stay consistent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ParseError {
    /// The submitted request itself was unusable (for example empty text).
    #[serde(rename = "invalid_request")]
    InvalidRequest { detail: String },
    /// Input exceeded the configured byte or segment limit. Large input is
    /// rejected before any model call and never truncated.
    #[serde(rename = "oversized_input")]
    OversizedInput { detail: String },
    /// The model output stayed invalid after the attempt budget ran out.
    #[serde(rename = "invalid_output")]
    InvalidOutput { detail: String, attempts: u32 },
    /// The model provider failed.
    #[serde(rename = "provider_error")]
    Provider { detail: String },
    /// The model call timed out.
    #[serde(rename = "timeout")]
    Timeout { detail: String },
    /// Configuration or implementation failure unrelated to model output.
    #[serde(rename = "internal_error")]
    Internal { detail: String },
}

impl ParseError {
    /// A short machine-readable discriminant, for logs and API error bodies.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidRequest { .. } => "invalid_request",
            Self::OversizedInput { .. } => "oversized_input",
            Self::InvalidOutput { .. } => "invalid_output",
            Self::Provider { .. } => "provider_error",
            Self::Timeout { .. } => "timeout",
            Self::Internal { .. } => "internal_error",
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { detail } => write!(f, "invalid request: {detail}"),
            Self::OversizedInput { detail } => write!(f, "oversized input: {detail}"),
            Self::InvalidOutput { detail, attempts } => {
                write!(
                    f,
                    "invalid model output after {attempts} attempt(s): {detail}"
                )
            }
            Self::Provider { detail } => write!(f, "provider failure: {detail}"),
            Self::Timeout { detail } => write!(f, "model call timed out: {detail}"),
            Self::Internal { detail } => write!(f, "internal error: {detail}"),
        }
    }
}

impl std::error::Error for ParseError {}
