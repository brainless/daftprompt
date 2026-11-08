//! Deterministic request and configuration keys for stored agent responses.
//!
//! Both keys are lowercase hex SHA-256 digests of a canonical JSON encoding
//! (object keys sorted, no whitespace), so equal inputs hash equally across
//! processes and releases. They only select *candidates*: a matching key never
//! proves a stored response is current (see Epic 007).

use daftprompt_types::AgentTaskRequest;
use serde_json::{Value, json};

use crate::scribe::support::content_hash;

/// Version of the cache-key encoding and reuse policy. Bump it to invalidate
/// every stored `config_key` when the way agents consume their inputs changes
/// in a way the other fields cannot show.
pub const CACHE_POLICY_VERSION: u32 = 1;

/// Version of the `request_key` encoding.
const REQUEST_KEY_VERSION: u32 = 1;

/// Whether `request` asks for the agent's built-in task (no usable prompt).
///
/// Only default-task responses are ever reused automatically.
pub fn is_default_task(request: &AgentTaskRequest) -> bool {
    request.prompt_text().is_none()
}

/// The key of the task a request asks for.
///
/// It hashes the normalized prompt (`prompt_text()` semantics: trimmed, blank
/// is absent) with an explicit default-task marker. The author is part of the
/// key only for a custom prompt, where it is recorded on the prompt node; a
/// default-task result does not depend on who asked.
pub fn request_key(request: &AgentTaskRequest) -> String {
    let task = match request.prompt_text() {
        None => json!({"kind": "default"}),
        Some(prompt) => json!({
            "kind": "prompt",
            "text": prompt,
            "author": request.prompt_author(),
        }),
    };
    hash_canonical(&json!({"version": REQUEST_KEY_VERSION, "task": task}))
}

/// The key of an agent's effective configuration.
///
/// `prompts` is `(version, content)` of every system prompt the agent can send;
/// the content is hashed so editing a prompt without bumping its version still
/// changes the key. `settings` carries the agent's own limits and options.
/// Credentials never belong in any argument.
pub(crate) fn config_key(
    agent: &str,
    provider: &str,
    model: &str,
    prompts: &[(&str, &str)],
    settings: Value,
) -> String {
    let prompts: Vec<Value> = prompts
        .iter()
        .map(|(version, content)| json!({"version": version, "sha256": content_hash(content)}))
        .collect();
    hash_canonical(&json!({
        "cache_policy": CACHE_POLICY_VERSION,
        "agent": agent,
        "provider": provider,
        "model": model,
        "endpoint": crate::llm::endpoint_identity(provider),
        "prompts": prompts,
        "settings": settings,
    }))
}

pub(crate) fn hash_canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    content_hash(&out)
}

/// Compact JSON with object keys in sorted order at every depth.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(prompt: Option<&str>, author: Option<&str>) -> AgentTaskRequest {
        AgentTaskRequest {
            prompt: prompt.map(str::to_owned),
            author: author.map(str::to_owned),
            project_path_id: None,
            refresh: false,
        }
    }

    #[test]
    fn canonical_encoding_sorts_keys_at_every_depth() {
        let a = json!({"b": 1, "a": {"y": [2, {"q": 1, "p": 2}], "x": null}});
        let b = json!({"a": {"x": null, "y": [2, {"p": 2, "q": 1}]}, "b": 1});
        assert_eq!(hash_canonical(&a), hash_canonical(&b));
        let mut text = String::new();
        write_canonical(&a, &mut text);
        assert_eq!(text, r#"{"a":{"x":null,"y":[2,{"p":2,"q":1}]},"b":1}"#);
        assert_ne!(hash_canonical(&a), hash_canonical(&json!({"a": 1})));
    }

    #[test]
    fn blank_prompts_and_other_paths_are_the_default_task() {
        let default = request(None, None);
        assert!(is_default_task(&default));
        assert!(is_default_task(&request(Some("  \n"), Some("ada"))));
        assert_eq!(
            request_key(&default),
            request_key(&request(Some("  "), Some("ada")))
        );
        let mut other_path = default.clone();
        other_path.project_path_id = Some("path-1".to_owned());
        assert_eq!(request_key(&default), request_key(&other_path));
    }

    #[test]
    fn custom_prompts_have_distinct_keys() {
        let default = request_key(&request(None, None));
        let custom = request_key(&request(Some("What changed?"), None));
        assert!(!is_default_task(&request(Some("What changed?"), None)));
        assert_ne!(default, custom);
        // The prompt is normalized by trimming; the author matters.
        assert_eq!(
            custom,
            request_key(&request(Some("  What changed? "), None))
        );
        assert_ne!(
            custom,
            request_key(&request(Some("What changed?"), Some("ada")))
        );
        assert_ne!(custom, request_key(&request(Some("What changed"), None)));
    }

    #[test]
    fn config_keys_follow_every_input() {
        let base = || config_key("surveyor", "p", "m", &[("v1", "prompt")], json!({"n": 1}));
        assert_eq!(base(), base());
        assert_ne!(
            base(),
            config_key("operator", "p", "m", &[("v1", "prompt")], json!({"n": 1}))
        );
        assert_ne!(
            base(),
            config_key("surveyor", "q", "m", &[("v1", "prompt")], json!({"n": 1}))
        );
        assert_ne!(
            base(),
            config_key("surveyor", "p", "n", &[("v1", "prompt")], json!({"n": 1}))
        );
        assert_ne!(
            base(),
            config_key("surveyor", "p", "m", &[("v1", "edited")], json!({"n": 1}))
        );
        assert_ne!(
            base(),
            config_key("surveyor", "p", "m", &[("v2", "prompt")], json!({"n": 1}))
        );
        assert_ne!(
            base(),
            config_key("surveyor", "p", "m", &[("v1", "prompt")], json!({"n": 2}))
        );
    }
}
