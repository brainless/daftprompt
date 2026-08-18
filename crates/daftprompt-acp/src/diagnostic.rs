//! A bounded, allocation-capped type for preserving raw ACP wire payloads
//! that daftprompt-acp did not fully understand (unknown `session/update`
//! variants, unknown `_meta` extensions, unrecognized reverse-request
//! methods) or could not parse (malformed JSON lines). Epic 014 Design
//! Decision #3 requires these to be preserved and surfaced generically
//! rather than crashing the session; this type is what makes that
//! preservation an explicit, size-bounded part of the public API instead of
//! an unbounded `serde_json::Value` leaking everywhere.

use std::fmt;

/// Maximum number of bytes of the serialized JSON representation retained by
/// [`RawDiagnostic`]. Larger payloads are truncated with a marker appended so
/// memory use stays bounded even for pathological or adversarial adapter
/// output.
pub const RAW_DIAGNOSTIC_BYTE_LIMIT: usize = 16 * 1024;

const TRUNCATION_MARKER: &str = "… [daftprompt-acp: raw diagnostic payload truncated]";

/// A bounded, best-effort textual capture of a raw protocol payload (a
/// `serde_json::Value`, a malformed line, or a method name plus params) for
/// diagnostics and transcript display. Never used as the primary typed
/// representation of a known message -- only as the fallback for what this
/// crate did not model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDiagnostic {
    /// Short machine-readable label for what this diagnostic came from, e.g.
    /// `"session/update"`, `"unknown-request:some/method"`,
    /// `"malformed-stdout-line"`.
    pub label: String,
    /// The bounded raw text payload (JSON or otherwise).
    pub raw: String,
    /// True if `raw` was truncated to stay within [`RAW_DIAGNOSTIC_BYTE_LIMIT`].
    pub truncated: bool,
}

impl RawDiagnostic {
    /// Build a diagnostic from a label and a raw string, bounding its size.
    #[must_use]
    pub fn from_text(label: impl Into<String>, raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let (raw, truncated) = bound(raw);
        Self {
            label: label.into(),
            raw,
            truncated,
        }
    }

    /// Build a diagnostic from a label and a JSON value, bounding its size.
    #[must_use]
    pub fn from_value(label: impl Into<String>, value: &serde_json::Value) -> Self {
        let raw = serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string());
        Self::from_text(label, raw)
    }
}

fn bound(raw: String) -> (String, bool) {
    if raw.len() <= RAW_DIAGNOSTIC_BYTE_LIMIT {
        return (raw, false);
    }
    // Truncate on a char boundary at/under the limit.
    let mut cut = RAW_DIAGNOSTIC_BYTE_LIMIT;
    while !raw.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut truncated = raw[..cut].to_string();
    truncated.push_str(TRUNCATION_MARKER);
    (truncated, true)
}

impl fmt::Display for RawDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.label, self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_payload_is_not_truncated() {
        let diag = RawDiagnostic::from_text("label", "hello");
        assert_eq!(diag.raw, "hello");
        assert!(!diag.truncated);
    }

    #[test]
    fn oversized_payload_is_bounded_and_marked() {
        let big = "x".repeat(RAW_DIAGNOSTIC_BYTE_LIMIT * 2);
        let diag = RawDiagnostic::from_text("label", big);
        assert!(diag.truncated);
        assert!(diag.raw.len() <= RAW_DIAGNOSTIC_BYTE_LIMIT + TRUNCATION_MARKER.len() + 4);
        assert!(diag.raw.ends_with(TRUNCATION_MARKER));
    }
}
