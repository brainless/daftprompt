//! Structured language-model adapter used by provenance graph extraction and search.
//!
//! Xiaomi API credentials are held only in [`XiaomiGraphAdapter`] memory and are
//! sent as an authorization header. Request/response DTOs contain no credential
//! fields and are never persisted by this module.

use std::env;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use llm_sdk::{
    error::LlmError,
    models::xiaomi::MIMO_V2_5_PRO,
    providers::XIAOMI,
    xiaomi::{types::{XiaomiChatCompletionRequest, XiaomiThinking}, XiaomiClient},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::SourcePassage;

pub const ENV_API_KEY: &str = "XIAOMI_API_KEY";
const EXTRACTION_MAX_COMPLETION_TOKENS: u32 = 32768;
const EXTRACTION_RETRY_MAX_COMPLETION_TOKENS: u32 = 65536;
const INTERPRETATION_MAX_COMPLETION_TOKENS: u32 = 8192;
const EXTRACTION_MAX_SPLIT_DEPTH: usize = 3;

// Keep the complete JSON wire contract in the prompt so the model does not
// need to know Rust type names or serde's enum representation.
const EXTRACTION_CONTRACT: &str = r#"Extract atomic provenance claims from the JSON user message.
Input shape: {"source_path":string,"passages":[{"id":string,"source_version_id":integer,"heading_path":string,"start_line":integer,"end_line":integer,"text":string}]}.
Each passage's "text" has every source line prefixed with its own absolute one-based line number and ": ", for example "90: - Paths remain absolute during IO". Copy that leading number directly into evidence start_line/end_line; never count lines yourself. Evidence text must NOT include the "N: " line-number prefix — copy only the original line content that follows the first ": " on each numbered line, keeping its exact wording, whitespace, and punctuation.
Return exactly one JSON object with this shape:
{"claims":[{"subject":{"kind":string,"name":string,"aliases":[string]},"predicate":string,"object":{"type":"entity","kind":string,"name":string,"aliases":[string]} OR {"type":"literal","value":string,"value_kind":string},"polarity":"positive"|"negative","status":"current"|"planned"|"deferred"|"completed"|"rejected"|"historical","scope":"project"|"epic"|"task"|"feature","evidence":[{"passage_id":string,"start_line":integer,"end_line":integer,"text":string}]}],"enumerations":[{"subject":{"kind":string,"name":string,"aliases":[string]},"predicate":string,"scope":"project"|"epic"|"task"|"feature","exhaustive":boolean,"evidence":{"passage_id":string,"start_line":integer,"end_line":integer,"text":string},"member_claim_indexes":[integer]}]}.
Use empty arrays when there are no claims or enumerations. Every claim needs at least one evidence item. Evidence passage_id must equal a supplied passage id; lines must be within that passage. Evidence text must be the COMPLETE verbatim source line or contiguous lines for start_line through end_line, copied from the supplied passage text. Keep Markdown syntax (for example "- [x]" or "**Status:**"), leading spaces, punctuation, and line breaks; do not paraphrase, shorten, normalize, or omit part of a cited line. A one-line range must contain that whole line, even if only a few words support the claim. This whole-line rule applies to EVERY line in a multi-line range, not only a one-line range: copy the first line, the last line, and every line between as complete physical lines; when only the first few words of the last line support the claim and the rest of that same physical line begins an unrelated sentence from Markdown line-wrapping, still copy that entire last physical line rather than truncating it. Enumeration indexes are zero-based positions in claims. An enumeration may use ONLY indexes_language, indexes_dialect, or indexes_extension as its predicate. Its subject must be an index entity, and every referenced member claim must have the same subject, predicate, and scope. Use indexes_language only for language members such as Rust, indexes_dialect only for dialect members such as TSX or JSX, and indexes_extension only for extension members such as .rs or .tsx. Never use excludes_path, work_item_status, or any other predicate for an enumeration. Omit an enumeration if its members cannot be represented by one of these three predicates; keep any individually supported claims. Set exhaustive true only if the cited passage explicitly says the list is complete. Extract ONLY these subject -> predicate -> object relations: index -> indexes_language -> language; index -> indexes_dialect -> dialect; index -> indexes_extension -> extension; feature -> searches_index -> index; feature or index -> uses_parser -> parser; feature, index, or work_item -> excludes_path -> path; work_item -> work_item_status -> status. For indexes_language, object MUST be an entity, for example {"type":"entity","kind":"language","name":"Rust","aliases":[]}. For indexes_dialect, object MUST be an entity, for example {"type":"entity","kind":"dialect","name":"TSX","aliases":[]}. For indexes_extension, object MUST be an entity, for example {"type":"entity","kind":"extension","name":".rs","aliases":[]}; an extension literal is invalid. searches_index and uses_parser also require entity objects of the stated kinds. For excludes_path, use a path literal such as {"type":"literal","value":"vendor/","value_kind":"path"}. For work_item_status, subject.kind must be "work_item" and object MUST be a literal with exactly {"type":"literal","value":"completed","value_kind":"status"} for completed work (or substitute "current", "planned", "deferred", "rejected", or "historical" for value when the source says so). Do not use {"type":"literal","value_kind":"string"} or a status entity. Entity kind must be exactly one of these seven values, and no other string: "feature", "index", "language", "dialect", "extension", "parser", "work_item". "project", "epic", "task", and "feature" used as a scope are scope values (the separate "scope" field), never an entity kind; in particular "task" and "project" are ALWAYS invalid as an entity kind, and a work item (something with a status, such as a todo or checklist row) is entity kind "work_item", never "task". Prose often blends an indexing pipeline and a search feature in the same sentence: a component described as indexing languages, dialects, or extensions (for example "a code indexer that indexes Rust, TypeScript, or .rs files") is entity kind "index", the only subject kind for indexes_language, indexes_dialect, and indexes_extension. A search feature, search API, search UI, or search mode that a person uses to search that index (for example "code search", "the search box", or "unified search") is entity kind "feature" and takes ONLY searches_index, uses_parser, or excludes_path; a feature subject NEVER takes indexes_language, indexes_dialect, or indexes_extension, even when the same sentence names both the index and the feature. Do not emit a project entity, generic project description, or any claim outside those relations. Omit unsupported statements instead of inventing a predicate or converting the project into a feature. If this passage has no supported claim, return empty arrays. Preserve current, planned, deferred, completed, rejected, or historical status and the narrowest supported scope. Treat future work or follow-up as deferred in this source revision; never invent a date or scheduled commitment. An epic-scoped non-goal is not a project-wide prohibition. Return JSON only."#;

const INTERPRETATION_CONTRACT: &str = r#"Interpret the question in the JSON user message using only supplied graph vocabulary.
Input shape: {"question":string,"candidate_entities":[{"kind":string,"name":string,"aliases":[string]}],"candidate_predicates":[string]}.
Return exactly one JSON object: {"intent":"direct_relation"|"code_search_language"|"list_code_search_languages","entities":[{"kind":string,"name":string,"aliases":[string]}],"predicates":[string]}.
All output predicates must occur in candidate_predicates. For known entities, copy kind and name from candidate_entities and use an empty aliases array if no aliases are needed. Keep entity kinds matched to the direct predicate: indexes_language needs index and language, indexes_dialect needs index and dialect, indexes_extension needs index and extension, and searches_index needs feature and index. A file extension such as .rs is an extension entity, not the Rust language; for a question naming Rust, choose the language entity Rust even if it has .rs as an alias. A question may name a language absent from candidate_entities (for example Lua); return it as a language entity so a validated exhaustive set can answer it. Use code_search_language only when the question asks whether code SEARCH finds one named language; include the code-search feature and the named language as a language entity, plus searches_index and indexes_language when available. Never omit the named target language. If the question asks whether an INDEX indexes a language (for example "Does the code index index Rust?"), use direct_relation with the named index, named language, and indexes_language; do not use code_search_language. Use list_code_search_languages for which languages code search finds; include the code-search feature but no invented target language, and include searches_index, indexes_language, and indexes_dialect when available. Use direct_relation when a single stored relationship answers the question. Return JSON only."#;

fn interpretation_correction_contract(validation_error: &str) -> String {
    format!(
        "{INTERPRETATION_CONTRACT}\nYour previous interpretation was rejected: {validation_error}. Correct it using ONLY predicates present in candidate_predicates. Copy known entity kinds and names from candidate_entities. Match each entity kind to the direct predicate's subject or object kind; for indexes_language use the language entity, not its extension alias. You may add an absent language entity only when the question names that language. Omit unavailable predicates instead of inventing one. Return JSON only."
    )
}

fn extraction_correction_contract(validation_error: &str) -> String {
    format!(
        "{EXTRACTION_CONTRACT}\nYour previous extraction was rejected: {validation_error}. Correct the JSON. Enumerations may use only indexes_language, indexes_dialect, or indexes_extension; match the predicate to the member object kind and omit unsupported enumerations while keeping valid claims. For indexes_language, indexes_dialect, and indexes_extension, use entity objects with kind language, dialect, and extension respectively; never use an extension literal. For excludes_path and work_item_status, use literals with value_kind \"path\" and \"status\" respectively. Entity kind must be exactly one of feature, index, language, dialect, extension, parser, or work_item; \"project\", \"epic\", and \"task\" are scopes, never entity kinds, and a work item is entity kind \"work_item\", never \"task\". If the reported error says a predicate does not allow a feature subject (for example \"predicate indexes_language does not allow feature -> language\"), the fix is to change the SUBJECT's kind to \"index\", not to swap the predicate while keeping subject kind \"feature\"; indexes_language, indexes_dialect, and indexes_extension always require an index subject and never a feature subject. Recopy evidence as complete source lines with Markdown and whitespace intact. If the reported error says your evidence text does not match the snapshot for the cited end_line, and your evidence stops partway through that end_line while the snapshot's full physical line continues with different wording (a Markdown line-wrap into an unrelated sentence), the fix is to copy that entire end_line, not to stop at the words that support the claim; a multi-line range's last line follows the same whole-line rule as its first line. Reread each numbered passage line and copy its leading number into start_line/end_line instead of counting; never include the \"N: \" prefix in evidence text. Remove any claim outside the listed relations or without exact source evidence. Your immediately preceding assistant message is the exact JSON you previously submitted. Change ONLY what is needed to fix the reported error; copy every other field byte-for-byte from that previous submission, especially each claim's evidence.text, start_line, and end_line when that claim was not the one implicated by the reported error. Do not re-derive, reword, or re-quote evidence that was not part of the reported problem."
    )
}

/// Passage input for a single authoritative source extraction operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionRequest {
    pub source_path: String,
    pub passages: Vec<SourcePassage>,
}

/// Render one passage's text with every physical line prefixed by its
/// absolute one-based source line number (`"N: "`), so a model can copy the
/// correct `start_line`/`end_line` instead of counting lines in a raw text
/// blob. This is a display transform only: it never changes
/// [`SourcePassage::text`] itself, which stays the exact, unprefixed source
/// text used for snapshot validation and re-anchoring.
fn render_numbered_passage_text(start_line: u32, text: &str) -> String {
    let mut rendered = String::new();
    for (offset, line) in text.split_inclusive('\n').enumerate() {
        if offset > 0 {
            rendered.push('\n');
        }
        let line_number = start_line + offset as u32;
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        rendered.push_str(&line_number.to_string());
        rendered.push_str(": ");
        rendered.push_str(content);
    }
    rendered
}

/// Wire-only rendering of an [`ExtractionRequest`] used solely to build the
/// outbound extraction/correction request body. Every passage's `text` is
/// replaced with [`render_numbered_passage_text`]'s numbered-line rendering;
/// every other field, and the stored [`ExtractionRequest`]/[`SourcePassage`]
/// values themselves, are untouched. Never deserialized — this type only
/// flows outward into the request JSON.
#[derive(Serialize)]
struct NumberedExtractionRequest<'a> {
    source_path: &'a str,
    passages: Vec<NumberedSourcePassage<'a>>,
}

#[derive(Serialize)]
struct NumberedSourcePassage<'a> {
    id: &'a str,
    source_version_id: i64,
    heading_path: &'a str,
    start_line: u32,
    end_line: u32,
    text: String,
}

impl<'a> From<&'a ExtractionRequest> for NumberedExtractionRequest<'a> {
    fn from(request: &'a ExtractionRequest) -> Self {
        NumberedExtractionRequest {
            source_path: &request.source_path,
            passages: request
                .passages
                .iter()
                .map(|passage| NumberedSourcePassage {
                    id: &passage.id,
                    source_version_id: passage.source_version_id,
                    heading_path: &passage.heading_path,
                    start_line: passage.start_line,
                    end_line: passage.end_line,
                    text: render_numbered_passage_text(passage.start_line, &passage.text),
                })
                .collect(),
        }
    }
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
    /// A provider may cap source text sent in one extraction request.
    fn extraction_batch_max_chars(&self) -> Option<usize> {
        None
    }
    /// A provider may cap the number of passages in one extraction request.
    fn extraction_batch_max_passages(&self) -> Option<usize> {
        None
    }
    /// Optionally ask a model to correct one rejected extraction. Fixture and
    /// offline adapters retain the original validation error by default.
    /// `previous_response` is the exact (already-parsed) response that was
    /// rejected, so an implementation can show it back to the model as prior
    /// conversation turn and ask for a minimal, targeted fix instead of a
    /// from-scratch re-derivation of every field.
    fn retry_extraction(
        &self,
        _request: &ExtractionRequest,
        _previous_response: &ExtractionResponse,
        _validation_error: &str,
    ) -> Option<Result<ExtractionResponse>> {
        None
    }
    fn interpret_question(&self, request: &InterpretationRequest)
        -> Result<InterpretationResponse>;
    /// A provider may correct one rejected interpretation using the same
    /// question and active graph vocabulary. Offline adapters retain the error.
    fn retry_interpretation(
        &self,
        _request: &InterpretationRequest,
        _validation_error: &str,
    ) -> Option<Result<InterpretationResponse>> {
        None
    }
    fn model_name(&self) -> &str;
}

pub struct XiaomiGraphAdapter {
    api_key: String,
}

impl std::fmt::Debug for XiaomiGraphAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("XiaomiGraphAdapter")
            .field("provider", &XIAOMI)
            .field("model", &MIMO_V2_5_PRO)
            .field("api_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl XiaomiGraphAdapter {
    /// Read the Xiaomi API key from the environment.
    pub fn from_env() -> Result<Self> {
        Ok(Self { api_key: required_env(ENV_API_KEY)? })
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

    fn request_value(
        &self,
        messages: Vec<ChatMessage>,
        max_completion_tokens: u32,
    ) -> Result<serde_json::Value> {
        let request = ChatCompletionRequest {
            model: MIMO_V2_5_PRO,
            max_completion_tokens,
            thinking: XiaomiThinking::disabled(),
            messages,
            response_format: JsonFormat {
                kind: "json_object",
            },
        };
        Ok(serde_json::to_value(request)?)
    }

    fn request_payload(
        &self,
        operation: &str,
        input: &impl Serialize,
        max_completion_tokens: u32,
    ) -> Result<serde_json::Value> {
        self.request_value(
            vec![
                ChatMessage {
                    role: "system",
                    content: operation.to_owned(),
                },
                ChatMessage {
                    role: "user",
                    content: serde_json::to_string(input)?,
                },
            ],
            max_completion_tokens,
        )
    }

    /// Build a correction request that carries the rejected prior turn as
    /// genuine conversation history (a preceding assistant message), so the
    /// model can be told to copy unaffected fields from it byte-for-byte
    /// instead of re-deriving the whole claim set from the correction prompt
    /// alone.
    fn correction_request_payload(
        &self,
        correction_prompt: &str,
        input: &impl Serialize,
        previous_response_json: &str,
        max_completion_tokens: u32,
    ) -> Result<serde_json::Value> {
        self.request_value(
            vec![
                ChatMessage {
                    role: "system",
                    content: correction_prompt.to_owned(),
                },
                ChatMessage {
                    role: "user",
                    content: serde_json::to_string(input)?,
                },
                ChatMessage {
                    role: "assistant",
                    content: previous_response_json.to_owned(),
                },
                ChatMessage {
                    role: "user",
                    content: "Your previous message above is the exact JSON you submitted; it was rejected for the reason already given in the system prompt. Apply only the minimal fix needed and leave every other field, especially evidence.text, start_line, and end_line for any claim not implicated in the reported error, byte-for-byte unchanged.".to_owned(),
                },
            ],
            max_completion_tokens,
        )
    }

    fn complete<T: DeserializeOwned>(
        &self,
        phase: &str,
        operation: &str,
        input: &impl Serialize,
        max_completion_tokens: u32,
    ) -> Result<T> {
        let payload = self.request_payload(operation, input, max_completion_tokens)?;
        self.run_completion(phase, payload, max_completion_tokens)
    }

    fn complete_correction<T: DeserializeOwned>(
        &self,
        phase: &str,
        correction_prompt: &str,
        input: &impl Serialize,
        previous_response_json: &str,
        max_completion_tokens: u32,
    ) -> Result<T> {
        let payload = self.correction_request_payload(
            correction_prompt,
            input,
            previous_response_json,
            max_completion_tokens,
        )?;
        self.run_completion(phase, payload, max_completion_tokens)
    }

    fn run_completion<T: DeserializeOwned>(
        &self,
        phase: &str,
        payload: serde_json::Value,
        max_completion_tokens: u32,
    ) -> Result<T> {
        let request: XiaomiChatCompletionRequest = serde_json::from_value(payload)?;
        let client = XiaomiClient::new(&self.api_key)
            .map_err(sanitized_sdk_error)?
            .with_model(MIMO_V2_5_PRO);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("could not initialize graph model runtime")?;
        eprintln!(
            "graph model: {phase} request started (max completion tokens: {max_completion_tokens})"
        );
        let started = Instant::now();
        // SDK errors can contain provider response text. Do not surface that
        // text in graph errors or query logs.
        let response = match runtime.block_on(client.create_chat_completion(request)) {
            Ok(response) => response,
            Err(error) => {
                eprintln!(
                    "graph model: {phase} request failed after {:.1}s",
                    started.elapsed().as_secs_f64()
                );
                return Err(sanitized_sdk_error(error));
            }
        };
        let completion_tokens = response.usage.completion_tokens;
        let choice = response.choices.into_iter().next().ok_or_else(|| {
            eprintln!(
                "graph model: {phase} request finished after {:.1}s (completion tokens: {completion_tokens}, finish reason: missing choice)",
                started.elapsed().as_secs_f64()
            );
            anyhow::Error::new(CompletionContentError::Missing)
        })?;
        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => "stop",
            Some("length") => "length",
            Some("content_filter") => "content_filter",
            Some(_) => "other",
            None => "missing",
        };
        eprintln!(
            "graph model: {phase} request finished after {:.1}s (completion tokens: {completion_tokens}, finish reason: {finish_reason})",
            started.elapsed().as_secs_f64()
        );
        if choice.finish_reason.as_deref() == Some("length") {
            return Err(CompletionContentError::TokenLimit(max_completion_tokens).into());
        }
        let content = choice
            .message
            .content
            .ok_or_else(|| anyhow::Error::new(CompletionContentError::Missing))?;
        parse_content(&content)
            .with_context(|| format!("graph model completion used {completion_tokens} tokens"))
    }
}

fn sanitized_sdk_error(error: LlmError) -> anyhow::Error {
    match error {
        LlmError::Authentication { .. } => anyhow!("{XIAOMI} graph model authentication failed"),
        LlmError::RateLimit { .. } => anyhow!("{XIAOMI} graph model rate limit exceeded"),
        LlmError::InvalidRequest { .. } => anyhow!("{XIAOMI} graph model rejected the request"),
        LlmError::Api { status, .. } => anyhow!("{XIAOMI} graph model returned HTTP status {status}"),
        LlmError::Network { .. } => anyhow!("{XIAOMI} graph model network request failed"),
        LlmError::Parse { .. } | LlmError::Internal { .. } => MalformedSdkResponse.into(),
        _ => anyhow!("{XIAOMI} graph model request failed"),
    }
}

#[derive(Debug)]
struct MalformedSdkResponse;

impl std::fmt::Display for MalformedSdkResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{XIAOMI} graph model returned a malformed response"
        )
    }
}

impl std::error::Error for MalformedSdkResponse {}

#[cfg(test)]
fn parse_completion<T: DeserializeOwned>(envelope: ChatCompletionResponse) -> Result<T> {
    let content = envelope
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("graph model returned no completion choices"))?
        .message
        .content;
    parse_content(&content)
}

fn parse_content<T: DeserializeOwned>(content: &str) -> Result<T> {
    if content.trim().is_empty() {
        return Err(CompletionContentError::Missing.into());
    }
    serde_json::from_str(content).map_err(|_| CompletionContentError::Malformed.into())
}

#[derive(Debug)]
enum CompletionContentError {
    Missing,
    Malformed,
    TokenLimit(u32),
}

impl std::fmt::Display for CompletionContentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(formatter, "graph model returned no completion content"),
            Self::Malformed => write!(formatter, "graph model returned malformed structured JSON"),
            Self::TokenLimit(limit) => write!(
                formatter,
                "graph model reached its completion token limit ({limit} tokens)"
            ),
        }
    }
}

impl std::error::Error for CompletionContentError {}

#[cfg(test)]
fn retry_incomplete_extraction(
    mut complete: impl FnMut(u32) -> Result<ExtractionResponse>,
) -> Result<ExtractionResponse> {
    let first = complete(EXTRACTION_MAX_COMPLETION_TOKENS);
    retry_incomplete_extraction_after_first(first, complete)
}

fn retry_incomplete_extraction_after_first(
    first: Result<ExtractionResponse>,
    mut complete: impl FnMut(u32) -> Result<ExtractionResponse>,
) -> Result<ExtractionResponse> {
    match first {
        Ok(response) => Ok(response),
        Err(error)
            if error.is::<CompletionContentError>() || error.is::<MalformedSdkResponse>() =>
        {
            let retry_limit = if matches!(
                error.downcast_ref::<CompletionContentError>(),
                Some(CompletionContentError::TokenLimit(_))
            ) {
                EXTRACTION_RETRY_MAX_COMPLETION_TOKENS
            } else {
                EXTRACTION_MAX_COMPLETION_TOKENS
            };
            eprintln!(
                "graph model: retrying incomplete extraction (max completion tokens: {retry_limit})"
            );
            complete(retry_limit).map_err(|retry_error| {
                retry_error.context(
                    "graph model extraction retry failed after an incomplete structured response",
                )
            })
        }
        Err(error) => Err(error),
    }
}

fn extract_with_adaptive_split(
    request: &ExtractionRequest,
    complete: &mut impl FnMut(&ExtractionRequest, u32) -> Result<ExtractionResponse>,
    depth: usize,
) -> Result<ExtractionResponse> {
    let first = complete(request, EXTRACTION_MAX_COMPLETION_TOKENS);
    // Empty or malformed content from a multi-passage request is often a
    // batch-size failure. Split before spending another request at that size.
    let split_on_first_failure = request.passages.len() > 1
        && depth < EXTRACTION_MAX_SPLIT_DEPTH
        && first.as_ref().is_err_and(|error| {
            matches!(
                error.downcast_ref::<CompletionContentError>(),
                Some(CompletionContentError::Missing | CompletionContentError::Malformed)
            ) || error.is::<MalformedSdkResponse>()
        });
    let result = if split_on_first_failure {
        first
    } else {
        retry_incomplete_extraction_after_first(first, |limit| complete(request, limit))
    };
    match result {
        Ok(response) => Ok(response),
        Err(error)
            if (error.is::<CompletionContentError>() || error.is::<MalformedSdkResponse>())
                && request.passages.len() > 1
                && depth < EXTRACTION_MAX_SPLIT_DEPTH =>
        {
            let midpoint = request.passages.len() / 2;
            eprintln!(
                "graph model: splitting incomplete extraction at depth {depth} ({} passages into {midpoint} and {})",
                request.passages.len(),
                request.passages.len() - midpoint
            );
            let mut combined = ExtractionResponse::default();
            for passages in [&request.passages[..midpoint], &request.passages[midpoint..]] {
                let part = ExtractionRequest {
                    source_path: request.source_path.clone(),
                    passages: passages.to_vec(),
                };
                let mut response = extract_with_adaptive_split(&part, complete, depth + 1)
                    .context("graph model extraction failed after splitting an incomplete batch")?;
                let claim_offset = combined.claims.len();
                for enumeration in &mut response.enumerations {
                    for index in &mut enumeration.member_claim_indexes {
                        if *index >= response.claims.len() {
                            return Err(anyhow!(
                                "graph model enumeration references a missing claim in a split batch"
                            ));
                        }
                        *index = index.checked_add(claim_offset).ok_or_else(|| {
                            anyhow!("graph model enumeration index overflow while merging split batch")
                        })?;
                    }
                }
                combined.claims.append(&mut response.claims);
                combined.enumerations.append(&mut response.enumerations);
            }
            Ok(combined)
        }
        Err(error) => Err(error),
    }
}

impl GraphModel for XiaomiGraphAdapter {
    fn extraction_batch_max_chars(&self) -> Option<usize> {
        Some(6_000)
    }

    fn extraction_batch_max_passages(&self) -> Option<usize> {
        Some(4)
    }

    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        extract_with_adaptive_split(
            request,
            &mut |part, limit| {
                let numbered = NumberedExtractionRequest::from(part);
                self.complete("extraction", EXTRACTION_CONTRACT, &numbered, limit)
            },
            0,
        )
            .with_context(|| format!("extracting claims for source {}", request.source_path))
    }

    fn retry_extraction(
        &self,
        request: &ExtractionRequest,
        previous_response: &ExtractionResponse,
        validation_error: &str,
    ) -> Option<Result<ExtractionResponse>> {
        let correction = extraction_correction_contract(validation_error);
        let numbered = NumberedExtractionRequest::from(request);
        let previous_response_json = match serde_json::to_string(previous_response) {
            Ok(json) => json,
            Err(error) => return Some(Err(error.into())),
        };
        Some(
            self.complete_correction(
                "correction",
                &correction,
                &numbered,
                &previous_response_json,
                EXTRACTION_MAX_COMPLETION_TOKENS,
            )
            .with_context(|| format!("correcting claims for source {}", request.source_path)),
        )
    }

    fn interpret_question(
        &self,
        request: &InterpretationRequest,
    ) -> Result<InterpretationResponse> {
        self.complete(
            "interpretation",
            INTERPRETATION_CONTRACT,
            request,
            INTERPRETATION_MAX_COMPLETION_TOKENS,
        )
        .with_context(|| format!("interpreting graph search question {:?}", request.question))
    }

    fn retry_interpretation(
        &self,
        request: &InterpretationRequest,
        validation_error: &str,
    ) -> Option<Result<InterpretationResponse>> {
        let correction = interpretation_correction_contract(validation_error);
        Some(
            self.complete(
                "interpretation correction",
                &correction,
                request,
                INTERPRETATION_MAX_COMPLETION_TOKENS,
            )
            .with_context(|| format!("correcting graph search question {:?}", request.question)),
        )
    }

    fn model_name(&self) -> &str {
        MIMO_V2_5_PRO
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
    max_completion_tokens: u32,
    thinking: XiaomiThinking,
    messages: Vec<ChatMessage>,
    response_format: JsonFormat,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct JsonFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[cfg(test)]
#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[cfg(test)]
#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[cfg(test)]
#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn adapter() -> XiaomiGraphAdapter {
        XiaomiGraphAdapter {
            api_key: "unused-test-key".into(),
        }
    }

    #[test]
    fn numbered_passage_text_prefixes_every_line_with_its_absolute_line_number() {
        let text = "first line\nsecond line\nthird line\n";
        let rendered = render_numbered_passage_text(88, text);
        assert_eq!(
            rendered,
            "88: first line\n89: second line\n90: third line"
        );
    }

    #[test]
    fn numbered_passage_text_handles_a_final_line_without_trailing_newline() {
        let text = "only line";
        assert_eq!(render_numbered_passage_text(42, text), "42: only line");
    }

    #[test]
    fn numbered_passage_text_strips_prefix_back_to_the_original_line_content() {
        let text = "- Paths remain absolute during IO and canonical repo-relative strings in\nthe next line\n";
        let rendered = render_numbered_passage_text(90, text);
        let stripped = rendered
            .split('\n')
            .map(|numbered_line| {
                numbered_line
                    .split_once(": ")
                    .map(|(_, content)| content)
                    .unwrap_or(numbered_line)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let original_without_trailing_newline = text.trim_end_matches('\n');
        assert_eq!(stripped, original_without_trailing_newline);
    }

    #[test]
    fn numbered_extraction_request_renders_passages_without_mutating_source_passages() {
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: "Heading".into(),
            start_line: 90,
            end_line: 91,
            text: "- Paths remain absolute during IO and canonical repo-relative strings in\nthe next line\n"
                .into(),
        };
        let request = ExtractionRequest {
            source_path: "epics/009-typescript-tsx-code-indexer.md".into(),
            passages: vec![passage.clone()],
        };
        let numbered = NumberedExtractionRequest::from(&request);
        let body = serde_json::to_value(&numbered).unwrap();
        let rendered_text = body["passages"][0]["text"].as_str().unwrap();
        assert!(rendered_text.starts_with("90: - Paths remain absolute"));
        assert!(rendered_text.contains("91: the next line"));
        // The original ExtractionRequest/SourcePassage are unchanged: they
        // still hold the raw, unprefixed source text used for snapshot
        // validation and re-anchoring.
        assert_eq!(request.passages[0], passage);
        assert!(!request.passages[0].text.starts_with("90:"));
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
            .request_payload(
                EXTRACTION_CONTRACT,
                &input,
                EXTRACTION_MAX_COMPLETION_TOKENS,
            )
            .unwrap();
        let sdk_request: XiaomiChatCompletionRequest =
            serde_json::from_value(body.clone()).unwrap();
        assert_eq!(sdk_request.model, MIMO_V2_5_PRO);
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["model"], MIMO_V2_5_PRO);
        assert_eq!(body["thinking"], serde_json::json!({"type": "disabled"}));
        assert_eq!(sdk_request.thinking, Some(XiaomiThinking::disabled()));
        assert_eq!(
            body["max_completion_tokens"],
            EXTRACTION_MAX_COMPLETION_TOKENS
        );
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
            "\"type\":\"literal\",\"value\":\"completed\",\"value_kind\":\"status\"",
            "COMPLETE verbatim source line",
            "Keep Markdown syntax",
            "leading spaces, punctuation, and line breaks",
            "\"task\" and \"project\" are ALWAYS invalid as an entity kind",
            "Omit unsupported statements",
            "prefixed with its own absolute one-based line number",
            "Copy that leading number directly into evidence start_line/end_line",
            "never count lines yourself",
            "Evidence text must NOT include the \"N: \" line-number prefix",
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
    fn extraction_and_correction_contracts_distinguish_entities_from_literals() {
        for (predicate, object) in [
            (
                "indexes_language",
                r#"{"type":"entity","kind":"language","name":"Rust","aliases":[]}"#,
            ),
            (
                "indexes_dialect",
                r#"{"type":"entity","kind":"dialect","name":"TSX","aliases":[]}"#,
            ),
            (
                "indexes_extension",
                r#"{"type":"entity","kind":"extension","name":".rs","aliases":[]}"#,
            ),
            (
                "excludes_path",
                r#"{"type":"literal","value":"vendor/","value_kind":"path"}"#,
            ),
            (
                "work_item_status",
                r#"{"type":"literal","value":"completed","value_kind":"status"}"#,
            ),
        ] {
            assert!(EXTRACTION_CONTRACT.contains(predicate));
            assert!(EXTRACTION_CONTRACT.contains(object));
        }
        let correction = extraction_correction_contract("invalid extension literal");
        assert!(correction.contains("invalid extension literal"));
        assert!(correction.contains("never use an extension literal"));
        assert!(correction.contains("use literals with value_kind \"path\" and \"status\""));
    }

    /// Keeps the prompt's stated entity-kind vocabulary in sync with
    /// `validate_entity`'s actual allow-list: every kind the validator
    /// accepts must be named in the extraction contract so the model is
    /// never left to guess (see the "task" entity-kind regression this
    /// guards against).
    #[test]
    fn extraction_contract_lists_every_valid_entity_kind() {
        for kind in crate::graph::ENTITY_KINDS {
            assert!(
                EXTRACTION_CONTRACT.contains(kind),
                "extraction contract prompt is missing entity kind {kind:?} from ENTITY_KINDS"
            );
        }
        // Scope-only vocabulary must be explicitly disclaimed as invalid
        // entity kinds, since the model previously guessed "task".
        for invalid_kind in ["project", "epic", "task"] {
            assert!(
                !crate::graph::ENTITY_KINDS.contains(&invalid_kind),
                "{invalid_kind} must never become a valid entity kind"
            );
        }
        assert!(EXTRACTION_CONTRACT.contains("\"task\""));
        assert!(EXTRACTION_CONTRACT.contains("ALWAYS invalid as an entity kind"));
    }

    #[test]
    fn correction_contract_reinforces_entity_kind_vocabulary_and_minimal_edits() {
        let correction = extraction_correction_contract("invalid subject entity kind \"task\"");
        for kind in crate::graph::ENTITY_KINDS {
            assert!(correction.contains(kind));
        }
        assert!(correction.contains("never entity kinds"));
        assert!(correction.contains("Change ONLY what is needed to fix the reported error"));
        assert!(correction.contains("byte-for-byte from that previous submission"));
        assert!(correction.contains("Your immediately preceding assistant message"));
    }

    /// Guards against the model conflating an "index" entity (the thing that
    /// indexes languages/dialects/extensions) with a "feature" entity (the
    /// search capability that queries that index) when prose describes both
    /// in the same sentence, as epic docs repeatedly do. See the
    /// `epics/004-code-indexer.md` regression this fixes: the model emitted
    /// `subject.kind: "feature"` for `indexes_language`/`indexes_extension`.
    #[test]
    fn extraction_contract_disambiguates_index_from_feature_subject_kind() {
        assert!(EXTRACTION_CONTRACT.contains("entity kind \"index\", the only subject kind for indexes_language, indexes_dialect, and indexes_extension"));
        assert!(EXTRACTION_CONTRACT.contains("a feature subject NEVER takes indexes_language, indexes_dialect, or indexes_extension"));
    }

    #[test]
    fn correction_contract_reinforces_index_vs_feature_subject_kind() {
        let correction = extraction_correction_contract(
            "source epics/004-code-indexer.md: claim 4: predicate indexes_extension does not allow feature -> extension; model correction also failed after: source epics/004-code-indexer.md: claim 4: predicate indexes_language does not allow feature -> language",
        );
        assert!(correction.contains("does not allow feature -> language"));
        assert!(correction.contains("change the SUBJECT's kind to \"index\", not to swap the predicate"));
        assert!(correction.contains("always require an index subject and never a feature subject"));
    }

    /// Guards against the model truncating the LAST line of a multi-line
    /// evidence span at the words that support the claim while leaving out
    /// the rest of that physical line, which is forbidden by the same
    /// whole-line rule that already applies to a one-line range. See the
    /// `epics/009-typescript-tsx-code-indexer.md` regression this fixes: a
    /// correction extended a truncated single-line quote across the right
    /// number of physical lines but still truncated the trailing line at
    /// "filenames." instead of including the rest of that Markdown-wrapped
    /// physical line ("This epic makes those seams language-neutral...").
    #[test]
    fn extraction_contract_requires_whole_lines_for_every_line_in_a_multiline_range() {
        assert!(EXTRACTION_CONTRACT.contains(
            "This whole-line rule applies to EVERY line in a multi-line range, not only a one-line range"
        ));
        assert!(EXTRACTION_CONTRACT.contains(
            "still copy that entire last physical line rather than truncating it"
        ));
    }

    #[test]
    fn correction_contract_reinforces_whole_line_requirement_for_trailing_line() {
        let correction = extraction_correction_contract(
            "source epics/009-typescript-tsx-code-indexer.md: claim 6: evidence text does not match snapshot lines 18-20: expected \"file discovery is `.rs`-only, parsing and queries are hardcoded to Rust,\\nmetadata hardcodes `language = \\\"rust\\\"`, and test repositories hardcode Rust\\nfilenames. This epic makes those seams language-neutral and runs the same\\n\"; got \"file discovery is `.rs`-only, parsing and queries are hardcoded to Rust,\\nmetadata hardcodes `language = \\\"rust\\\"`, and test repositories hardcode Rust\\nfilenames.\"",
        );
        assert!(correction.contains("does not match the snapshot for the cited end_line"));
        assert!(correction.contains(
            "the fix is to copy that entire end_line, not to stop at the words that support the claim"
        ));
        assert!(correction.contains(
            "a multi-line range's last line follows the same whole-line rule as its first line"
        ));
    }

    #[test]
    fn correction_request_payload_includes_previous_response_as_prior_assistant_turn() {
        let input = ExtractionRequest {
            source_path: "epics/005-migrate-to-akar-ui.md".into(),
            passages: vec![SourcePassage {
                id: "sv1:p0".into(),
                source_version_id: 1,
                heading_path: "".into(),
                start_line: 583,
                end_line: 583,
                text: "**Description:** Replace the search box.".into(),
            }],
        };
        let previous_response_json =
            r#"{"claims":[{"subject":{"kind":"task","name":"X","aliases":[]}}],"enumerations":[]}"#;
        let correction = extraction_correction_contract("invalid subject entity kind \"task\"");
        let body = adapter()
            .correction_request_payload(
                &correction,
                &input,
                previous_response_json,
                EXTRACTION_MAX_COMPLETION_TOKENS,
            )
            .unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], correction);
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(
            serde_json::from_str::<ExtractionRequest>(messages[1]["content"].as_str().unwrap())
                .unwrap(),
            input
        );
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], previous_response_json);
        assert_eq!(messages[3]["role"], "user");
        let closing_instruction = messages[3]["content"].as_str().unwrap();
        assert!(closing_instruction.contains("exact JSON you submitted"));
        assert!(closing_instruction.contains("byte-for-byte unchanged"));

        // The SDK's own request type must still deserialize the four-message
        // history (Vec<XiaomiMessage>, not a fixed-size pair).
        let sdk_request: XiaomiChatCompletionRequest = serde_json::from_value(body).unwrap();
        assert_eq!(sdk_request.messages.len(), 4);
    }

    #[test]
    fn extraction_contract_limits_enumerations_to_matching_index_members() {
        for contract in [
            EXTRACTION_CONTRACT.to_owned(),
            extraction_correction_contract("unsupported enumeration predicate excludes_path"),
        ] {
            assert!(contract.contains(
                "An enumeration may use ONLY indexes_language, indexes_dialect, or indexes_extension"
            ));
            assert!(contract.contains("indexes_dialect only for dialect members such as TSX or JSX"));
            assert!(contract.contains("Never use excludes_path, work_item_status, or any other predicate for an enumeration"));
            assert!(contract.contains("keep any individually supported claims"));
        }
    }

    #[test]
    fn interpretation_payload_describes_intents_and_typed_parser_rejects_bad_response() {
        let input = InterpretationRequest {
            question: "Can code search find Python?".into(),
            candidate_entities: vec![],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let body = adapter()
            .request_payload(
                INTERPRETATION_CONTRACT,
                &input,
                INTERPRETATION_MAX_COMPLETION_TOKENS,
            )
            .unwrap();
        assert_eq!(
            body["max_completion_tokens"],
            INTERPRETATION_MAX_COMPLETION_TOKENS
        );
        assert_eq!(body["thinking"], serde_json::json!({"type": "disabled"}));
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
    fn malformed_extraction_gets_one_retry_and_returns_valid_result() {
        let mut calls = 0;
        let response = retry_incomplete_extraction(|limit| {
            assert_eq!(limit, EXTRACTION_MAX_COMPLETION_TOKENS);
            calls += 1;
            parse_content::<ExtractionResponse>(if calls == 1 {
                ""
            } else {
                r#"{"claims":[],"enumerations":[]}"#
            })
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert!(response.claims.is_empty());
    }

    #[test]
    fn token_limit_gets_one_retry_with_larger_budget() {
        let mut limits = Vec::new();
        let response = retry_incomplete_extraction(|limit| {
            limits.push(limit);
            if limits.len() == 1 {
                Err(CompletionContentError::TokenLimit(limit).into())
            } else {
                parse_content::<ExtractionResponse>(r#"{"claims":[],"enumerations":[]}"#)
            }
        })
        .unwrap();
        assert!(response.claims.is_empty());
        assert_eq!(
            limits,
            [
                EXTRACTION_MAX_COMPLETION_TOKENS,
                EXTRACTION_RETRY_MAX_COMPLETION_TOKENS
            ]
        );
    }

    #[test]
    fn incomplete_batch_splits_and_rebases_enumeration_members() {
        let request = ExtractionRequest {
            source_path: "epics/010.md".into(),
            passages: (0..4)
                .map(|index| SourcePassage {
                    id: format!("p{index}"),
                    source_version_id: 1,
                    heading_path: String::new(),
                    start_line: index + 1,
                    end_line: index + 1,
                    text: format!("line {index}"),
                })
                .collect(),
        };
        let mut batch_sizes = Vec::new();
        let response = extract_with_adaptive_split(
            &request,
            &mut |part, _| {
                batch_sizes.push(part.passages.len());
                if part.passages.len() > 2 {
                    return Err(CompletionContentError::Missing.into());
                }
                Ok(ExtractionResponse {
                    claims: vec![sample_claim()],
                    enumerations: vec![ExtractedEnumeration {
                        subject: ClaimEntity {
                            kind: "index".into(),
                            name: "CodeIndex".into(),
                            aliases: vec![],
                        },
                        predicate: "indexes_language".into(),
                        scope: ClaimScope::Project,
                        exhaustive: false,
                        evidence: ClaimEvidence {
                            passage_id: part.passages[0].id.clone(),
                            start_line: part.passages[0].start_line,
                            end_line: part.passages[0].end_line,
                            text: part.passages[0].text.clone(),
                        },
                        member_claim_indexes: vec![0],
                    }],
                })
            },
            0,
        )
        .unwrap();
        assert_eq!(batch_sizes, [4, 2, 2]);
        assert_eq!(response.claims.len(), 2);
        assert_eq!(
            response
                .enumerations
                .iter()
                .map(|enumeration| enumeration.member_claim_indexes.clone())
                .collect::<Vec<_>>(),
            [vec![0], vec![1]]
        );
    }

    #[test]
    fn incomplete_single_passage_stops_after_one_retry() {
        let request = ExtractionRequest {
            source_path: "epics/010.md".into(),
            passages: vec![SourcePassage {
                id: "p0".into(),
                source_version_id: 1,
                heading_path: String::new(),
                start_line: 1,
                end_line: 1,
                text: "line".into(),
            }],
        };
        let mut calls = 0;
        let error = extract_with_adaptive_split(
            &request,
            &mut |_, _| {
                calls += 1;
                Err(CompletionContentError::Missing.into())
            },
            0,
        )
        .unwrap_err();
        assert_eq!(calls, 2);
        assert!(format!("{error:#}").contains("no completion content"));
    }

    #[test]
    fn malformed_multi_passage_response_splits_before_retry() {
        let request = ExtractionRequest {
            source_path: "epics/010.md".into(),
            passages: (0..2)
                .map(|index| SourcePassage {
                    id: format!("p{index}"),
                    source_version_id: 1,
                    heading_path: String::new(),
                    start_line: index + 1,
                    end_line: index + 1,
                    text: format!("line {index}"),
                })
                .collect(),
        };
        let mut batch_sizes = Vec::new();
        extract_with_adaptive_split(
            &request,
            &mut |part, _| {
                batch_sizes.push(part.passages.len());
                if part.passages.len() > 1 {
                    Err(CompletionContentError::Malformed.into())
                } else {
                    Ok(ExtractionResponse::default())
                }
            },
            0,
        )
        .unwrap();
        assert_eq!(batch_sizes, [2, 1, 1]);
    }

    #[test]
    fn token_limited_multi_passage_response_retries_before_split() {
        let request = ExtractionRequest {
            source_path: "epics/010.md".into(),
            passages: (0..2)
                .map(|index| SourcePassage {
                    id: format!("p{index}"),
                    source_version_id: 1,
                    heading_path: String::new(),
                    start_line: index + 1,
                    end_line: index + 1,
                    text: format!("line {index}"),
                })
                .collect(),
        };
        let mut limits = Vec::new();
        extract_with_adaptive_split(
            &request,
            &mut |_, limit| {
                limits.push(limit);
                if limits.len() == 1 {
                    Err(CompletionContentError::TokenLimit(limit).into())
                } else {
                    Ok(ExtractionResponse::default())
                }
            },
            0,
        )
        .unwrap();
        assert_eq!(
            limits,
            [
                EXTRACTION_MAX_COMPLETION_TOKENS,
                EXTRACTION_RETRY_MAX_COMPLETION_TOKENS
            ]
        );
    }

    #[test]
    fn malformed_extraction_stops_after_second_failure_without_response_text() {
        let mut calls = 0;
        let error = retry_incomplete_extraction(|limit| {
            assert_eq!(limit, EXTRACTION_MAX_COMPLETION_TOKENS);
            calls += 1;
            parse_content::<ExtractionResponse>("secret invalid response")
        })
        .unwrap_err();
        assert_eq!(calls, 2);
        let message = format!("{error:#}");
        assert!(message.contains("retry failed"));
        assert!(message.contains("malformed structured JSON"));
        assert!(!message.contains("secret invalid response"));
    }

    #[test]
    fn non_content_error_does_not_retry_extraction() {
        let mut calls = 0;
        let error = retry_incomplete_extraction(|_| {
            calls += 1;
            Err(anyhow!("provider unavailable"))
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(error.to_string().contains("provider unavailable"));
    }

    #[test]
    fn malformed_sdk_response_gets_one_retry_without_leaking_provider_text() {
        let mut calls = 0;
        let response = retry_incomplete_extraction(|_| {
            calls += 1;
            if calls == 1 {
                Err(sanitized_sdk_error(LlmError::internal(
                    "Failed to parse response: private response body",
                )))
            } else {
                Ok(ExtractionResponse::default())
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert!(response.claims.is_empty());

        let error = sanitized_sdk_error(LlmError::internal("private response body"));
        assert_eq!(
            error.to_string(),
            "xiaomi graph model returned a malformed response"
        );
        assert!(!format!("{error:#}").contains("private response body"));
    }

    #[test]
    fn authentication_and_rate_limit_do_not_retry() {
        for is_rate_limit in [false, true] {
            let mut calls = 0;
            let result = retry_incomplete_extraction(|_| {
                calls += 1;
                let error = if is_rate_limit {
                    LlmError::rate_limit("private rate limit detail", None)
                } else {
                    LlmError::authentication("private authentication detail")
                };
                Err(sanitized_sdk_error(error))
            });
            assert_eq!(calls, 1);
            let message = format!("{:#}", result.unwrap_err());
            assert!(!message.contains("private"));
        }
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
