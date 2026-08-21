// Secret redaction for ACP event payloads.
// Strips known secret patterns before persisting payload_json.

use std::borrow::Cow;

/// Redact secret-like values from a JSON string.
///
/// This performs regex-like pattern matching on the serialized JSON to strip:
/// - Values of keys containing SECRET, KEY, TOKEN, PASSWORD (case-insensitive)
/// - Bearer auth headers
/// - Authorization header values
/// - API key patterns (sk-..., api_key=..., etc.)
pub fn redact_secrets(json: &str) -> String {
    // Strategy: parse to Value, walk all keys, redact matching values.
    // Also redact string values that look like bearer tokens or API keys
    // even if the key name doesn't match.
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(mut value) => {
            redact_value(&mut value);
            serde_json::to_string(&value).unwrap_or_else(|_| json.to_string())
        }
        Err(_) => {
            // If not valid JSON, do a best-effort text-level redaction.
            redact_text_fallback(json)
        }
    }
}

fn redact_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let key_lower = key.to_lowercase();
                if key_lower.contains("secret")
                    || key_lower.contains("key")
                    || key_lower.contains("token")
                    || key_lower.contains("password")
                {
                    *val = serde_json::Value::String("[REDACTED]".to_string());
                } else if key_lower == "authorization" || key_lower == "auth" {
                    if let serde_json::Value::String(s) = val {
                        if s.to_lowercase().starts_with("bearer ") || s.to_lowercase().starts_with("basic ") {
                            *val = serde_json::Value::String("[REDACTED]".to_string());
                        }
                    }
                } else if key_lower == "sessionid" {
                    // ACP session IDs are correlation identifiers, not
                    // credentials. Preserve them even when their UUID shape
                    // trips the generic long-opaque-string heuristic below.
                } else {
                    redact_value(val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                redact_value(val);
            }
        }
        serde_json::Value::String(s) => {
            if looks_like_secret(s) {
                *s = "[REDACTED]".to_string();
            }
        }
        _ => {}
    }
}

fn looks_like_secret(s: &str) -> bool {
    let lower = s.to_lowercase();
    // Bearer token
    if lower.starts_with("bearer ") || lower.starts_with("basic ") {
        return true;
    }
    // Common API key prefixes
    if s.starts_with("sk-") || s.starts_with("api_key=") || s.starts_with("apikey=") {
        return true;
    }
    // Environment variable style values that look like they contain secrets
    // (long hex strings or base64-like strings with SECRET/KEY/TOKEN/PASSWORD in the context)
    // We check the raw value for patterns that indicate it's a credential.
    if s.len() > 20
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/')
        && s.len() > 32
    {
        return true;
    }
    false
}

fn redact_text_fallback(text: &str) -> String {
    // Best-effort: replace patterns like "Bearer <token>" or "Authorization: <value>"
    let mut result = Cow::Borrowed(text);
    // Simple replacements for common patterns in non-JSON text
    for pattern in &["Bearer ", "bearer ", "Basic ", "basic "] {
        if let Some(pos) = result.find(pattern) {
            let start = pos + pattern.len();
            // Find end of the token (next whitespace or end of string)
            let end = result[start..]
                .find(|c: char| c.is_whitespace())
                .map(|i| start + i)
                .unwrap_or(result.len());
            let replacement = format!("{}[REDACTED]", &result[..pos + pattern.len()]);
            result = Cow::Owned(format!("{}{}", replacement, &result[end..]));
        }
    }
    result.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_api_key_in_value() {
        let json = r#"{"api_key": "sk-1234567890abcdef1234567890abcdef"}"#;
        let redacted = redact_secrets(json);
        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("sk-1234567890abcdef"));
    }

    #[test]
    fn test_redact_bearer_token() {
        let json = r#"{"authorization": "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0"}"#;
        let redacted = redact_secrets(json);
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_password_field() {
        let json = r#"{"username": "admin", "password": "supersecret123"}"#;
        let redacted = redact_secrets(json);
        assert!(redacted.contains("admin"));
        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("supersecret123"));
    }

    #[test]
    fn test_redact_nested_secret() {
        let json = r#"{"config": {"secret_token": "abc123"}, "name": "test"}"#;
        let redacted = redact_secrets(json);
        assert!(redacted.contains("[REDACTED]"));
        assert!(redacted.contains("test"));
    }

    #[test]
    fn test_preserve_normal_values() {
        let json = r#"{"prompt": "hello world", "count": 42}"#;
        let redacted = redact_secrets(json);
        assert!(redacted.contains("hello world"));
        assert!(redacted.contains("count"));
        assert!(!redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_preserve_uuid_session_identifier() {
        let json = r#"{"sessionId":"01a01050-a8b2-7a40-ab44-9e6f9400741e"}"#;
        let redacted = redact_secrets(json);
        assert!(redacted.contains("01a01050-a8b2-7a40-ab44-9e6f9400741e"));
        assert!(!redacted.contains("[REDACTED]"));
    }
}
