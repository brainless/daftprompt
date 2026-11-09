//! The shared daftprompt library.
//!
//! Everything that is not HTTP or GUI lives here: text handling, validation,
//! graph construction, agents, and prompts. The public API stays independent
//! of Axum, akar, and any other transport so a future CLI can reuse it.
//!
//! - [`text`]: segmentation, exact-once quote anchoring, and byte/line-column
//!   coordinate conversion.
//! - [`scribe`]: the first parsing agent. Scribe turns submitted text into a
//!   validated in-memory graph through a typed Rig extraction.
//! - [`explainer`]: the clarification agent. Explainer answers a question about one
//!   extracted span with a bounded tool loop and validated citations.
//! - [`chronicler`]: the git-history briefing agent (Epic 006), built on the shared
//!   [`project_agent`] runner and the read-only [`git_tools`].
//! - [`languages`]: deterministic per-language project detection and the
//!   [`ProjectProfile`](daftprompt_types::ProjectProfile) (Epic 006).
//! - [`surveyor`]: the files-and-documentation briefing agent (Epic 006).
//! - [`operator`]: the commands briefing agent (Epic 006): candidates detected by
//!   [`languages`], picked and labelled by the model, validated by Rust.
//! - [`llm`]: the provider factory that turns provider and model names into a Rig model.
//! - [`project_tools`]: read-only `list_files`, `search`, and `read_file` with a
//!   per-run call ledger, reusable by any agent, plus Rig adapters.
//! - [`access`]: which registered project folders a caller may give an agent: the
//!   principal, a root policy, and the per-call project resolver.
//! - [`store`]: the SQLite database, its migrations, the project registry, and
//!   stored agent response snapshots.
//! - [`eval`]: the manually annotated fixture set and the scoring rules used by
//!   the opt-in live Scribe evaluation.

pub mod access;
pub mod agent_log;
pub mod chronicler;
pub mod eval;
pub mod explainer;
pub mod git_tools;
pub mod languages;
pub mod llm;
pub mod operator;
pub mod project_agent;
pub mod project_tools;
pub mod scribe;
pub mod store;
pub mod surveyor;
pub mod text;

pub use scribe::{
    BoxFuture, DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_SEGMENTS,
    DEFAULT_TIMEOUT, ExtractionModel, ModelError, ModelInput, PROMPT_VERSION, Parser, RigExtractor,
    SYSTEM_PROMPT, Scribe, ScribeConfig, render_segments,
};
pub use text::{
    AnchoredSpan, LineColumn, Segment, SegmentRange, SpanError, anchor_reference, anchor_span,
    byte_offset, byte_range_to_source_range, position_at, segments, source_range_to_byte_range,
};

pub use access::{
    AccessError, AgentKind, LocalPolicy, Principal, ProjectScope, RootPolicy, resolve_project,
};
pub use chronicler::{Chronicler, ChroniclerConfig, ChroniclerModel, RigChronicler};
pub use explainer::{
    Clarifier, Explainer, ExplainerConfig, ExplainerInput, ExplainerModel, RigExplainer,
};
pub use llm::{LlmError, ModelSettings, build_model};
pub use operator::{Operator, OperatorConfig, OperatorModel, RigOperator};
pub use project_agent::{
    Briefer, BriefingAgent, BriefingError, BriefingService, CommandsBriefer, ScopeOrigin,
};
pub use store::{Store, StoreError, default_db_path};
pub use surveyor::{RigSurveyor, Surveyor, SurveyorConfig, SurveyorModel};
