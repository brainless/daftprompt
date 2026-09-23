//! Structured language-model adapter used by provenance graph extraction and search.
//!
//! API credentials are held only in [`OpenAiCompatibleAdapter`] memory and are
//! sent as an authorization header. Request/response DTOs contain no credential
//! fields and are never persisted by this module.

use std::env;

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::SourcePassage;

pub const ENV_BASE_URL: &str = "DAFTPROMPT_GRAPH_BASE_URL";
pub const ENV_MODEL: &str = "DAFTPROMPT_GRAPH_MODEL";
pub const ENV_API_KEY: &str = "DAFTPROMPT_GRAPH_API_KEY";

// json_object is supported by more OpenAI-compatible endpoints than
// json_schema. Keep the complete wire contract in the prompt so the model
// does not need to know Rust type names or serde's enum representation.
const EXTRACTION_CONTRACT: &str = r#"Extract atomic provenance claims from the JSON user message.
Input shape: {"source_path":string,"passages":[{"id":string,"source_version_id":integer,"heading_path":string,"start_line":integer,"end_line":integer,"text":string}]}.
Return exactly one JSON object with this shape:
{"claims":[{"subject":{"kind":string,"name":string,"aliases":[string]},"predicate":string,"object":{"type":"entity","kind":string,"name":string,"aliases":[string]} OR {"type":"literal","value":string,"value_kind":string},"polarity":"positive"|"negative","status":"current"|"planned"|"deferred"|"completed"|"rejected"|"historical","scope":"project"|"epic"|"task"|"feature","evidence":[{"passage_id":string,"start_line":integer,"end_line":integer,"text":string}]}],"enumerations":[{"subject":{"kind":string,"name":string,"aliases":[string]},"predicate":string,"scope":"project"|"epic"|"task"|"feature","exhaustive":boolean,"evidence":{"passage_id":string,"start_line":integer,"end_line":integer,"text":string},"member_claim_indexes":[integer]}]}.
Use empty arrays when there are no claims or enumerations. Every claim needs at least one evidence item. Evidence passage_id must equal a supplied passage id; lines must be within that passage and text must exactly match those source lines. Enumeration indexes are zero-based positions in claims. Set exhaustive true only if the cited passage explicitly says the list is complete. Use entity kinds feature, index, language, dialect, extension, parser, work_item, path, or status as appropriate. Use controlled predicates indexes_language, indexes_dialect, indexes_extension, searches_index, uses_parser, excludes_path, or work_item_status. Preserve current, planned, deferred, completed, rejected, or historical status and the narrowest supported scope. Treat future work or follow-up as deferred in this source revision; never invent a date or scheduled commitment. An epic-scoped non-goal is not a project-wide prohibition. Return JSON only."#;

const INTERPRETATION_CONTRACT: &str = r#"Interpret the question in the JSON user message using only supplied graph vocabulary.
Input shape: {"question":string,"candidate_entities":[{"kind":string,"name":string,"aliases":[string]}],"candidate_predicates":[string]}.
Return exactly one JSON object: {"intent":"direct_relation"|"code_search_language"|"list_code_search_languages","entities":[{"kind":string,"name":string,"aliases":[string]}],"predicates":[string]}.
All output predicates must occur in candidate_predicates. For known entities, copy kind and name from candidate_entities and use an empty aliases array if no aliases are needed. A question may name a language absent from candidate_entities (for example Lua); return it as a language entity so a validated exhaustive set can answer it. Use code_search_language for whether code search finds one named language, with the code-search feature and target language and the searches_index and indexes_language predicates. Use list_code_search_languages for which languages code search finds; include the code-search feature but no invented target language, and include searches_index, indexes_language, and indexes_dialect when available. Use direct_relation only when a single stored relationship answers the question. Return JSON only."#;

/// Passage input for a single authoritative source extraction operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionRequest {
    pub source_path: String,
    pub passages: Vec<SourcePassage>,
}

/// A model's atomic claim proposal. Task 7 validates every field and citation
/// against the source snapshot before this value may be stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedClaim {
    pub subject: ClaimEntity,
    pub predicate: String,
    pub object: ClaimObject,
    pub polarity: Polarity,
    pub status: ClaimStatus,
    pub scope: ClaimScope,
    pub evidence: Vec<ClaimEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimEntity {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// A deterministic graph identity derived from an extracted surface entity.
/// Surface aliases carry their own kind so extensions such as `.py` do not
/// silently become language nodes when the alias index is queried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalEntity {
    pub canonical_key: String,
    pub kind: String,
    pub display_name: String,
    pub aliases: Vec<EntityAlias>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityAlias {
    pub kind: String,
    pub surface: String,
    pub normalized: String,
}

impl ClaimEntity {
    /// Normalize well-known entity spellings while retaining every supplied
    /// spelling as a separately typed lookup alias.
    pub fn canonicalized(&self) -> CanonicalEntity {
        let surface_kind = self.kind.trim().to_ascii_lowercase();
        let surface_name = self.name.trim();
        let (kind, display_name) = canonical_kind_and_name(&surface_kind, surface_name);
        let canonical_key = format!("{}:{}", kind, normalize_key_component(&display_name));
        let mut aliases = Vec::new();
        let mut add_alias = |kind: &str, value: &str| {
            let normalized = normalize_alias(value);
            if !normalized.is_empty()
                && !aliases
                    .iter()
                    .any(|alias: &EntityAlias| alias.kind == kind && alias.normalized == normalized)
            {
                aliases.push(EntityAlias {
                    kind: kind.to_owned(),
                    surface: value.to_owned(),
                    normalized,
                });
            }
        };
        add_alias(&kind, surface_name);
        for alias in &self.aliases {
            let alias_kind = if is_extension(alias) {
                "extension"
            } else {
                &kind
            };
            add_alias(alias_kind, alias);
        }
        // Common extension spellings are known aliases, not alternate entity
        // kinds. This keeps Python distinct from its `.py` file extension.
        for (entity, extension) in [
            ("Python", ".py"),
            ("Rust", ".rs"),
            ("JavaScript", ".js"),
            ("TypeScript", ".ts"),
            ("JSX", ".jsx"),
            ("TSX", ".tsx"),
        ] {
            if display_name.eq_ignore_ascii_case(entity) {
                add_alias("extension", extension);
            }
        }
        CanonicalEntity {
            canonical_key,
            kind,
            display_name,
            aliases,
        }
    }
}

fn canonical_kind_and_name(kind: &str, name: &str) -> (String, String) {
    if kind == "extension" {
        let extension = if name.trim().starts_with('.') {
            name.trim().to_ascii_lowercase()
        } else {
            format!(".{}", name.trim().to_ascii_lowercase())
        };
        return ("extension".to_owned(), extension);
    }
    let folded = name.trim().trim_start_matches('.').to_ascii_lowercase();
    let (resolved_kind, display) = match folded.as_str() {
        "python" | "py" if kind == "language" || kind == "extension" => ("language", "Python"),
        "rust" | "rs" if kind == "language" || kind == "extension" => ("language", "Rust"),
        "javascript" | "js" if kind == "language" || kind == "extension" => {
            ("language", "JavaScript")
        }
        "typescript" | "ts" if kind == "language" || kind == "extension" => {
            ("language", "TypeScript")
        }
        "jsx" if kind == "language" || kind == "dialect" || kind == "extension" => {
            ("dialect", "JSX")
        }
        "tsx" if kind == "language" || kind == "dialect" || kind == "extension" => {
            ("dialect", "TSX")
        }
        _ => (kind, name.trim()),
    };
    (resolved_kind.to_owned(), display.to_owned())
}

fn normalize_key_component(value: &str) -> String {
    value
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn normalize_alias(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_extension(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('.')
        && value.len() > 1
        && value[1..].chars().all(|ch| ch.is_ascii_alphanumeric())
}

/// Map commonly returned surface predicate phrases onto the controlled graph
/// vocabulary using the endpoint types. Distinct graph relations stay distinct.
pub fn normalize_predicate(
    surface: &str,
    subject_kind: &str,
    object_kind: &str,
) -> Option<&'static str> {
    let phrase = surface.trim().to_ascii_lowercase().replace('_', " ");
    match (phrase.as_str(), subject_kind, object_kind) {
        ("indexes language" | "indexed" | "indexes" | "supports language", "index", "language") => {
            Some("indexes_language")
        }
        ("indexes dialect" | "indexed" | "indexes" | "supports dialect", "index", "dialect") => {
            Some("indexes_dialect")
        }
        ("indexes extension" | "supports extension", "index", "extension") => {
            Some("indexes_extension")
        }
        (
            "searches index"
            | "included in code search"
            | "included in search"
            | "searches"
            | "uses index",
            "feature",
            "index",
        ) => Some("searches_index"),
        ("uses parser" | "parsed by" | "parser", "feature" | "index", "parser") => {
            Some("uses_parser")
        }
        ("excludes path" | "excludes", "feature" | "index" | "work_item", "path") => {
            Some("excludes_path")
        }
        ("work item status" | "has status", "work_item", "status") => Some("work_item_status"),
        ("searchable", "index", "language") => Some("indexes_language"),
        ("searchable", "index", "dialect") => Some("indexes_dialect"),
        ("searchable", "feature", "index") => Some("searches_index"),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaimObject {
    Entity(ClaimEntity),
    Literal { value: String, value_kind: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Positive,
    Negative,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Current,
    Planned,
    Deferred,
    Completed,
    Rejected,
    Historical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimScope {
    Project,
    Epic,
    Task,
    Feature,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimEvidence {
    pub passage_id: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionResponse {
    pub claims: Vec<ExtractedClaim>,
    /// Lists explicitly stated by the source. Members point at claim indexes so
    /// unrelated mentions cannot accidentally enter an enumeration.
    #[serde(default)]
    pub enumerations: Vec<ExtractedEnumeration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedEnumeration {
    pub subject: ClaimEntity,
    pub predicate: String,
    pub scope: ClaimScope,
    /// This is honored only when the cited wording explicitly states that the
    /// list is complete (for example, “all supported languages”).
    #[serde(default)]
    pub exhaustive: bool,
    pub evidence: ClaimEvidence,
    pub member_claim_indexes: Vec<usize>,
}

/// Typed interpretation request for a user question. Available vocabulary is
/// supplied by the caller, while the model only proposes graph concepts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterpretationRequest {
    pub question: String,
    pub candidate_entities: Vec<ClaimEntity>,
    pub candidate_predicates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterpretationResponse {
    pub intent: QuestionIntent,
    #[serde(default)]
    pub entities: Vec<ClaimEntity>,
    #[serde(default)]
    pub predicates: Vec<String>,
}

/// The relationship the caller is asking the graph to prove.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuestionIntent {
    /// A stored edge directly answers the question.
    DirectRelation,
    /// A code-search feature searches an index that indexes the target language.
    CodeSearchLanguage,
    /// Enumerate all currently supported languages and dialects for code search.
    ListCodeSearchLanguages,
}

/// Provider-neutral contract so graph code and tests do not depend on HTTP.
pub trait GraphModel: Send + Sync {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse>;
    fn interpret_question(&self, request: &InterpretationRequest)
        -> Result<InterpretationResponse>;
    fn model_name(&self) -> &str;
}

#[derive(Clone)]
pub struct OpenAiCompatibleAdapter {
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: Client,
}

impl std::fmt::Debug for OpenAiCompatibleAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiCompatibleAdapter")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish_non_exhaustive()
    }
}

impl OpenAiCompatibleAdapter {
    /// Read configuration from the environment. Base URL and model are
    /// required; the API key is optional for local OpenAI-compatible servers.
    pub fn from_env() -> Result<Self> {
        let base_url = required_env(ENV_BASE_URL)?;
        let model = required_env(ENV_MODEL)?;
        let base_url = base_url.trim_end_matches('/').to_owned();
        anyhow::ensure!(
            base_url.starts_with("http://") || base_url.starts_with("https://"),
            "{ENV_BASE_URL} must be an http(s) URL"
        );
        anyhow::ensure!(
            !base_url.contains('@'),
            "{ENV_BASE_URL} must not contain URL credentials"
        );
        let api_key = env::var(ENV_API_KEY).ok().filter(|key| !key.is_empty());
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .context("could not initialize graph model HTTP client")?;
        Ok(Self {
            base_url,
            model,
            api_key,
            client,
        })
    }

    /// Load environment configuration and run extraction, preserving the
    /// source path in configuration and provider errors alike.
    pub fn extract_from_env(request: &ExtractionRequest) -> Result<ExtractionResponse> {
        Self::from_env()
            .and_then(|adapter| adapter.extract_claims(request))
            .with_context(|| format!("extracting claims for source {}", request.source_path))
    }

    /// Load environment configuration and interpret a question, preserving
    /// the question in configuration and provider errors alike.
    pub fn interpret_from_env(request: &InterpretationRequest) -> Result<InterpretationResponse> {
        Self::from_env()
            .and_then(|adapter| adapter.interpret_question(request))
            .with_context(|| format!("interpreting graph search question {:?}", request.question))
    }

    fn request_payload(
        &self,
        operation: &str,
        input: &impl Serialize,
    ) -> Result<serde_json::Value> {
        let request = ChatCompletionRequest {
            model: &self.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: operation,
                },
                ChatMessage {
                    role: "user",
                    content: &serde_json::to_string(input)?,
                },
            ],
            response_format: JsonFormat {
                kind: "json_object",
            },
        };
        Ok(serde_json::to_value(request)?)
    }

    fn complete<T: DeserializeOwned>(&self, operation: &str, input: &impl Serialize) -> Result<T> {
        let payload = self.request_payload(operation, input)?;
        let url = format!("{}/chat/completions", self.base_url);
        let mut builder = self.client.post(url).json(&payload);
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        // Do not include the request, headers, response body, or reqwest error
        // text here: compatible providers sometimes echo sensitive material.
        let response = builder
            .send()
            .map_err(|_| anyhow!("graph model HTTP request failed"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "graph model returned HTTP status {}",
            response.status()
        );
        let envelope: ChatCompletionResponse = response
            .json()
            .map_err(|_| anyhow!("graph model returned malformed completion JSON"))?;
        parse_completion(envelope)
    }
}

fn parse_completion<T: DeserializeOwned>(envelope: ChatCompletionResponse) -> Result<T> {
    let content = envelope
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("graph model returned no completion choices"))?
        .message
        .content;
    serde_json::from_str(&content)
        .map_err(|error| anyhow!("graph model returned malformed structured JSON: {error}"))
}

impl GraphModel for OpenAiCompatibleAdapter {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        self.complete(EXTRACTION_CONTRACT, request)
            .with_context(|| format!("extracting claims for source {}", request.source_path))
    }

    fn interpret_question(
        &self,
        request: &InterpretationRequest,
    ) -> Result<InterpretationResponse> {
        self.complete(INTERPRETATION_CONTRACT, request)
            .with_context(|| format!("interpreting graph search question {:?}", request.question))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// Deterministic adapter for offline tests and explicit fixture-driven CLI runs.
#[derive(Debug, Clone)]
pub struct FixtureAdapter {
    model: String,
    extraction: ExtractionResponse,
    interpretation: InterpretationResponse,
}

impl FixtureAdapter {
    pub fn new(extraction: ExtractionResponse, interpretation: InterpretationResponse) -> Self {
        Self {
            model: "fixture".to_owned(),
            extraction,
            interpretation,
        }
    }

    pub fn named(
        model: impl Into<String>,
        extraction: ExtractionResponse,
        interpretation: InterpretationResponse,
    ) -> Self {
        Self {
            model: model.into(),
            extraction,
            interpretation,
        }
    }
}

impl GraphModel for FixtureAdapter {
    fn extract_claims(&self, _request: &ExtractionRequest) -> Result<ExtractionResponse> {
        Ok(self.extraction.clone())
    }

    fn interpret_question(
        &self,
        _request: &InterpretationRequest,
    ) -> Result<InterpretationResponse> {
        Ok(self.interpretation.clone())
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

fn required_env(name: &str) -> Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("required graph model environment variable {name} is not set"))
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    response_format: JsonFormat,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct JsonFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn adapter() -> OpenAiCompatibleAdapter {
        OpenAiCompatibleAdapter {
            base_url: "http://unused.invalid".into(),
            model: "mock-model".into(),
            api_key: None,
            client: Client::new(),
        }
    }

    #[test]
    fn extraction_payload_describes_full_wire_contract() {
        let input = ExtractionRequest {
            source_path: "README.md".into(),
            passages: vec![SourcePassage {
                id: "sv1:p0".into(),
                source_version_id: 1,
                heading_path: "".into(),
                start_line: 1,
                end_line: 1,
                text: "Python is indexed.".into(),
            }],
        };
        let body = adapter()
            .request_payload(EXTRACTION_CONTRACT, &input)
            .unwrap();
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["model"], "mock-model");
        let prompt = body["messages"][0]["content"].as_str().unwrap();
        for token in [
            "source_path",
            "source_version_id",
            "member_claim_indexes",
            "exhaustive",
            "passage_id",
            "value_kind",
            "indexes_language",
            "deferred",
            "feature",
            "type\":\"entity",
        ] {
            assert!(prompt.contains(token), "missing contract token {token}");
        }
        assert_eq!(
            serde_json::from_str::<ExtractionRequest>(
                body["messages"][1]["content"].as_str().unwrap()
            )
            .unwrap(),
            input
        );
    }

    #[test]
    fn interpretation_payload_describes_intents_and_typed_parser_rejects_bad_response() {
        let input = InterpretationRequest {
            question: "Can code search find Python?".into(),
            candidate_entities: vec![],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let body = adapter()
            .request_payload(INTERPRETATION_CONTRACT, &input)
            .unwrap();
        let prompt = body["messages"][0]["content"].as_str().unwrap();
        for token in [
            "question",
            "candidate_entities",
            "candidate_predicates",
            "direct_relation",
            "code_search_language",
            "list_code_search_languages",
            "for example Lua",
            "entities",
            "predicates",
        ] {
            assert!(prompt.contains(token), "missing contract token {token}");
        }
        assert_eq!(
            serde_json::from_str::<InterpretationRequest>(
                body["messages"][1]["content"].as_str().unwrap()
            )
            .unwrap(),
            input
        );

        let valid = serde_json::to_string(&InterpretationResponse {
            intent: QuestionIntent::CodeSearchLanguage,
            entities: vec![],
            predicates: vec!["indexes_language".into()],
        })
        .unwrap();
        let envelope = serde_json::from_value(
            serde_json::json!({"choices": [{"message": {"content": valid}}]}),
        )
        .unwrap();
        let result: InterpretationResponse = parse_completion(envelope).unwrap();
        assert_eq!(result.intent, QuestionIntent::CodeSearchLanguage);

        let targetless = serde_json::to_string(&InterpretationResponse {
            intent: QuestionIntent::ListCodeSearchLanguages,
            entities: vec![ClaimEntity {
                kind: "feature".into(), name: "CodeSearch".into(), aliases: vec![],
            }],
            predicates: vec!["searches_index".into(), "indexes_language".into(), "indexes_dialect".into()],
        }).unwrap();
        let targetless: InterpretationResponse = serde_json::from_str(&targetless).unwrap();
        assert_eq!(targetless.intent, QuestionIntent::ListCodeSearchLanguages);
        assert!(!targetless.entities.iter().any(|entity| entity.kind == "language"));

        let bad_intent = serde_json::from_value(serde_json::json!({"choices": [{"message": {"content": "{\"intent\":\"invented\",\"entities\":[],\"predicates\":[]}"}}]})).unwrap();
        let error = parse_completion::<InterpretationResponse>(bad_intent).unwrap_err();
        assert!(format!("{error:#}").contains("malformed structured JSON"));

        let bad_claim = serde_json::from_value(serde_json::json!({"choices": [{"message": {"content": "{\"claims\":[{\"predicate\":\"indexes_language\"}],\"enumerations\":[]}"}}]})).unwrap();
        let error = parse_completion::<ExtractionResponse>(bad_claim).unwrap_err();
        assert!(format!("{error:#}").contains("malformed structured JSON"));
    }

    fn sample_claim() -> ExtractedClaim {
        ExtractedClaim {
            subject: ClaimEntity {
                kind: "index".into(),
                name: "CodeIndex".into(),
                aliases: vec![],
            },
            predicate: "indexes_language".into(),
            object: ClaimObject::Entity(ClaimEntity {
                kind: "language".into(),
                name: "Python".into(),
                aliases: vec![".py".into()],
            }),
            polarity: Polarity::Positive,
            status: ClaimStatus::Current,
            scope: ClaimScope::Project,
            evidence: vec![ClaimEvidence {
                passage_id: "sv1:p0".into(),
                start_line: 1,
                end_line: 1,
                text: "Python is indexed.".into(),
            }],
        }
    }

    #[test]
    fn fixture_adapter_serves_both_typed_operations_without_network() {
        let adapter = FixtureAdapter::new(
            ExtractionResponse {
                claims: vec![sample_claim()],
                enumerations: vec![],
            },
            InterpretationResponse {
                intent: QuestionIntent::DirectRelation,
                entities: vec![],
                predicates: vec!["searches_index".into()],
            },
        );
        let extraction = ExtractionRequest {
            source_path: "README.md".into(),
            passages: vec![],
        };
        assert_eq!(
            adapter.extract_claims(&extraction).unwrap().claims,
            vec![sample_claim()]
        );
        let interpretation = InterpretationRequest {
            question: "Can we search Python?".into(),
            candidate_entities: vec![],
            candidate_predicates: vec![],
        };
        assert_eq!(
            adapter.interpret_question(&interpretation).unwrap().intent,
            QuestionIntent::DirectRelation
        );
    }

    #[test]
    fn claim_response_json_requires_typed_fields_and_variants() {
        let valid = serde_json::to_string(&ExtractionResponse {
            claims: vec![sample_claim()],
            enumerations: vec![],
        })
        .unwrap();
        assert_eq!(
            serde_json::from_str::<ExtractionResponse>(&valid)
                .unwrap()
                .claims
                .len(),
            1
        );
        assert!(
            serde_json::from_str::<ExtractionResponse>(r#"{"claims":[{"predicate":"wat"}]}"#)
                .is_err()
        );
    }

    #[test]
    fn canonical_entities_keep_dialects_and_extensions_typed() {
        let python = ClaimEntity {
            kind: "language".into(),
            name: "python".into(),
            aliases: vec![".py".into()],
        }
        .canonicalized();
        assert_eq!(python.canonical_key, "language:python");
        assert_eq!(python.kind, "language");
        assert!(python
            .aliases
            .iter()
            .any(|a| a.kind == "extension" && a.normalized == ".py"));

        let jsx = ClaimEntity {
            kind: "language".into(),
            name: "JSX".into(),
            aliases: vec![".jsx".into()],
        }
        .canonicalized();
        assert_eq!(jsx.canonical_key, "dialect:jsx");
        assert_eq!(jsx.kind, "dialect");
        assert!(jsx
            .aliases
            .iter()
            .any(|a| a.kind == "extension" && a.normalized == ".jsx"));
        assert_ne!(jsx.canonical_key, "language:jsx");
    }

    #[test]
    fn surface_predicates_normalize_by_typed_endpoints_without_merging_relations() {
        assert_eq!(
            normalize_predicate("Indexed", "index", "language"),
            Some("indexes_language")
        );
        assert_eq!(
            normalize_predicate("included in code search", "feature", "index"),
            Some("searches_index")
        );
        assert_eq!(
            normalize_predicate("searchable", "index", "language"),
            Some("indexes_language")
        );
        assert_eq!(
            normalize_predicate("uses parser", "index", "parser"),
            Some("uses_parser")
        );
    }
}
