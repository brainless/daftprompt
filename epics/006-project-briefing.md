# Epic 006: Project briefing

## Outcome

When the user opens a project, daftprompt briefs them on it. Three read-only agents look at the project from different angles and report back:

| agent | angle | default task |
|-------|-------|--------------|
| **Chronicler** | git history | summarize recent work: themes, notable changes, areas in progress |
| **Surveyor** | files and documentation | summarize what the project is, how it is laid out, and where to read further |
| **Operator** | how the project is run | list the commands the project supports (build, run, dev server, test, lint, setup) with a short summary |

Every agent accepts an optional user prompt in place of its default task, for example "what changed in auth recently?" for Chronicler. Summaries come first; other prompts use the same contracts.

The GUI requests all three briefings when a project becomes current. It shows the Chronicler and Surveyor summaries one below the other in the main content column, and the Operator's commands as a row of buttons in a fixed footer.

References: [PRD](../docs/PRD.md), [data model](../docs/data-model.md), [Epic 004](004-clarification-branch.md). Epic 004 supplies the patterns reused here: the read-only project tools, the tool ledger, Rust-validated citations, and the agent module layout.

## Scope and decisions

- **Placement.** Shared contracts live in `daftprompt-types`. Agents, tools, the language registry, validation and record construction live in `daftprompt-core`. `daftprompt-api` provides thin endpoints, and `daftprompt-gui` only renders. A future CLI calls core directly.
- **Read-only.** Agents can list, search and read files, and read git history, under the resolved project root and nothing else. They have no write, execute, shell or network tools. **Commands are discovered, never run.** Running them is a later epic with its own safety design.
- **Access goes through the root policy.** Each endpoint resolves the project through `access::resolve_project` with its own `AgentKind` (`Chronicler`, `Surveyor`, `Operator`). Per-user, team and per-agent rules therefore apply to these agents with no contract change.
- **Rust owns the records.** The model submits text and citations through a schema-defined output tool. Rust validates every citation against what the tools actually returned in that run. Rust also computes hashes and ranges and creates every ID, payload, node, context and run record.
- **Language knowledge is modular.** Deterministic, language-specific knowledge sits behind a `LanguageSupport` trait in a registry: marker files, key files and command candidates. The first implementations are Rust, TypeScript/JavaScript and Python. Adding a language adds a module, and agents and contracts stay unchanged.
- **Treat all content as data.** File contents, commit messages and docs never override an agent's instructions.
- **Stateless in this epic.** Results stay in memory and are recomputed on request. Persisting or caching briefings is a later task (see Non-goals). Briefings are not parsed by Scribe and add no labels.

## Internal model: agents and LLMs

### LLM layer (`daftprompt_core::llm`)

The provider factory moves from `daftprompt-api/src/main.rs` into core, so the API and a future CLI build models the same way.

- `ModelSettings { provider: String, model: String }`.
- `build_model(&ModelSettings) -> Result<DynModel<Completion>, LlmError>` reads each provider's credentials and base URLs from the environment, exactly as today (for example `NEURALWATT_API_KEY`). Credentials never enter config structs or results.
- Each agent's settings come from `DAFTPROMPT_<AGENT>_PROVIDER` and `DAFTPROMPT_<AGENT>_MODEL` over the agent's built-in default. Scribe and Explainer keep their current variables.

### Agent anatomy

Each agent is a module `daftprompt_core::<agent>/` with the same files as Explainer:

| file | contents |
|------|----------|
| `config.rs` | `<Agent>Config`: model settings, `max_turns`, `max_attempts`, `timeout`, tool limits; `from_env()` |
| `prompt.rs` | `SYSTEM_PROMPT`, `PROMPT_VERSION` (e.g. `chronicler-v1`) |
| `model.rs` | the model seam trait (one call = one attempt) and its Rig implementation: a multi-turn agent with the agent's tools and an output tool |
| `validate.rs` | checks the submitted extraction against the run's tool ledger |
| `graph.rs` | builds payloads, nodes, contexts and run records |
| `mod.rs` | the agent type: attempts, timeout, retry on invalid output |

The three new agents share one generic runner (`daftprompt_core::project_agent`) for the multi-turn tool loop, attempts and timeout. It is parameterized by toolset, prompt, output type and validator. Explainer is not migrated in this epic.

### Toolsets

| agent | tools |
|-------|-------|
| Chronicler | `git_log`, `git_show` |
| Surveyor | `list_files`, `search`, `read_file` |
| Operator | `list_files`, `search`, `read_file` |

- **Git tools** (`daftprompt_core::git_tools`) sit behind a `GitHistory` trait, with a subprocess implementation and an in-memory test double.
  - `git_log`: bounded count, optional `since`, optional root-relative pathspec. It returns hash, author, date and subject, plus an optional body.
  - `git_show`: one commit's message and `--stat`, with an optional capped patch.
  - Hardening:
    - arguments are built only from validated, typed parameters (hashes are hex, nothing starts with `-`), with no shell;
    - every invocation uses `git -C <root> --no-pager`, `-c core.pager=cat`, `-c core.fsmonitor=false`, `-c log.showSignature=false`, `--no-ext-diff`, `--no-textconv` and `--no-color`;
    - the environment sets `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0` and `GIT_CONFIG_NOSYSTEM=1`;
    - each call has a timeout and output byte caps.
  - When the root is a subdirectory of a repository, history is limited to `-- .`. When the root is not a repository, or `git` is missing, the tools report it and the briefing says so; this is not an error.
- **Every tool call is recorded** in the existing `ToolRecorder` ledger, which gains the commit hashes that git tools returned. Validation accepts only citations the ledger covers.

### Language registry (`daftprompt_core::languages`)

```rust
pub trait LanguageSupport: Send + Sync {
    fn id(&self) -> LanguageId;                       // rust | typescript | python
    fn detect(&self, files: &dyn ProjectFiles) -> Option<LanguageDetection>;
    fn key_files(&self, detection: &LanguageDetection) -> Vec<String>;
    fn command_candidates(&self, files: &dyn ProjectFiles, detection: &LanguageDetection)
        -> Vec<CommandCandidate>;
}
```

- **Detection** finds marker files and records them as evidence, along with the package manager or toolchain where it can be told apart:
  - Rust: `Cargo.toml`, including workspace members, plus `rust-toolchain.toml`.
  - TypeScript/JavaScript: `package.json`, the lockfile (which selects npm, pnpm, yarn or bun), `tsconfig.json`.
  - Python: `pyproject.toml`, `uv.lock` or `poetry.lock`, `requirements*.txt`, `manage.py`.
- **Command candidates** are deterministic, parsed from manifests:
  - Rust: `cargo build`, `test`, `clippy`, `fmt`, and `cargo run -p <bin>` per binary package.
  - TypeScript/JavaScript: one candidate per `package.json` script, using the detected package manager.
  - Python: venv creation and dependency install for the detected tool, `[project.scripts]` entries, and `manage.py runserver` when present.
  - Every candidate cites the manifest lines it came from.
- **Profile:** `ProjectProfile` (the detected languages and their evidence) is computed by Rust with no model call. It is given to Surveyor and Operator as context and served on its own for the GUI.
- **Monorepos:** detection walks the root and nested manifests within the file-tool limits, and records each manifest's directory as the candidate's `working_dir`.

### Extraction contracts (what the model submits)

```rust
/// Chronicler and Surveyor.
pub struct BriefingExtraction {
    pub summary: String,                 // Markdown; [n] markers refer to citations, 1-based
    pub citations: Vec<BriefingCitation>,
}

pub enum BriefingCitation {              // tagged by "kind"
    File { path: String, start_line: u32, end_line: u32 },  // within lines read_file returned
    Commit { hash: String },             // a hash git_log/git_show returned; prefix ≥ 7 hex, resolved by Rust
}

/// Operator.
pub struct CommandsExtraction {
    pub summary: String,
    pub citations: Vec<BriefingCitation>,
    pub commands: Vec<ExtractedCommand>,
}

pub struct ExtractedCommand {
    pub label: String,                   // short button text, ≤ 24 chars
    pub command: String,                 // exactly as it would be typed
    pub working_dir: Option<String>,     // root-relative; None = root
    pub kind: CommandKind,               // setup | build | run | dev | test | lint | format | other
    pub candidate: Option<u32>,          // index into the Rust-detected candidates given in the prompt
    pub citations: Vec<u32>,             // 1-based indexes into `citations`
}
```

Validation rules:
- Summaries are non-empty and bounded. Every `[n]` marker resolves to a citation.
- File citations lie within lines read in the same run. Commit citations resolve to exactly one commit the ledger saw.
- Every command is backed by a candidate or by at least one file citation, such as a README "how to run" section. Its `working_dir` exists inside the root.
- Labels are short and unique. Duplicate `(working_dir, command)` pairs are rejected. The number of commands is capped (default 12).
- Invalid output is retried within the attempt budget, as Explainer does.

### Records (graph alignment)

A briefing is agent-authored text, recorded the same way as an Explainer explanation:

| record | details |
|--------|---------|
| Prompt payload and node `P` | Only when the request has a `prompt`. Root node, `author` = request author or `unknown`, one context: the whole prompt payload |
| Summary payload | The summary text |
| Summary node `S` | `parent_id` = `P`, or none (root) for a default task. `author` = `agent`, `run_id` = the run. Context `seq 0` is the summary payload; `seq 1…n` are the validated citations |
| Run metadata | `run` (agent `chronicler` / `surveyor` / `operator`, `project_id` set), `run_input` for `P`, one `tool_call` per invocation with output payloads |

A new context target records commit citations:

- `ContextTarget::Commit { hash }`, a full 40- or 64-hex commit id resolved by Rust.
- Data model: `node_context` gains a nullable `commit_hash` column, and the CHECK becomes "exactly one of `payload_hash`, `path`, `commit_hash`".

Commands are structured results, not graph nodes. Each `ProjectCommand` points at the summary node's citation contexts that justify it. A later `project_command` table may persist them; it is not part of this epic.

No labels are created: briefings are informational, non-blocking, and never change the record.

## API

All bodies are JSON, and errors use the existing `{kind, detail}` shape. A project route answers `not_found` (404), `forbidden` (403) or `project_unavailable` (422) before any model call.

| method and path | body | response |
|-----------------|------|----------|
| `GET /agents` | none | `AgentInfo[]`: `{kind, provider, model, prompt_version}` for every configured agent |
| `GET /projects/{id}/profile` | none | `ProjectProfile` (deterministic, no model call) |
| `POST /projects/{id}/history` | `AgentTaskRequest` | `BriefingResult` (Chronicler) |
| `POST /projects/{id}/overview` | `AgentTaskRequest` | `BriefingResult` (Surveyor) |
| `POST /projects/{id}/commands` | `AgentTaskRequest` | `CommandsResult` (Operator) |

```rust
pub struct AgentTaskRequest {
    pub prompt: Option<String>,          // None = the agent's default task
    pub author: Option<String>,          // human identity for the prompt node
}

pub struct BriefingResult {
    pub project_id: String,
    pub task: AgentTask,                 // { kind: default | prompt }
    pub prompt_payload: Option<Payload>,
    pub summary_payload: Payload,
    pub summary_node_id: String,
    pub nodes: Vec<Node>,
    pub node_contexts: Vec<NodeContext>,
    pub citations: Vec<CitationInfo>,    // CitationLocation gains Commit { hash, subject }
    pub notices: Vec<BriefingNotice>,    // e.g. not_a_git_repository, git_unavailable, truncated
    pub profile: Option<ProjectProfile>, // Surveyor and Operator
    pub run: Run,
    pub run_inputs: Vec<RunInput>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_payloads: Vec<Payload>,
}

pub struct CommandsResult {
    #[serde(flatten)]
    pub briefing: BriefingResult,
    pub commands: Vec<ProjectCommand>,
}

pub struct ProjectCommand {
    pub id: String,                      // Rust-generated, stable within the result
    pub label: String,
    pub command: String,
    pub working_dir: String,             // root-relative, "" = root
    pub kind: CommandKind,
    pub language: Option<LanguageId>,
    pub origin: CommandOrigin,           // detected (manifest candidate) | documented (cited docs)
    pub citation_indexes: Vec<u32>,      // into BriefingResult.citations
}
```

## GUI

- **Briefing state:** one `Briefing { project_id, generation, history, overview, commands }`, where each slot is `Idle | Loading | Ready(result) | Failed(error)`. The three requests are sent concurrently, off the render thread, whenever a project becomes current. A reply whose `project_id` or generation no longer matches is discarded.
- **Tabs:** the main column has two tabs, **Project** and **Parse**. Opening a project switches to Project and submitting a parse switches to Parse, and the user can switch freely between them.
- **Project tab:** the Chronicler and Surveyor summaries are stacked as sections, each with a title, loading or error state, rendered summary, citations and a re-run control. Briefings are not cached; they re-run whenever a project becomes current, including on launch.
- **Footer:** a fixed footer holds one row of command buttons from `CommandsResult.commands`. Clicking a button opens a details popover with the label, command, working directory, kind, origin and cited evidence. It never runs or copies the command.
- **Errors:** they stay inside the section they belong to and never block parsing or clarifying.

## Tasks

- **A. Foundation and Chronicler (implemented):**
  - the `llm` module (move the provider factory);
  - `AgentKind` additions;
  - the shared types above;
  - `ContextTarget::Commit`;
  - `git_tools`;
  - the `project_agent` runner;
  - Chronicler;
  - `GET /agents` and `POST /projects/{id}/history`.

  Implementation notes and deviations from the text above:
  - `BriefingResult.profile` is not defined yet; Task B adds `profile: Option<ProjectProfile>` with `ProjectProfile`.
  - `BriefingNotice` is `{ kind, detail }` with `kind` one of `not_a_git_repository`, `git_unavailable`, `truncated`.
  - Because the provider factory moved into core, `daftprompt-core` now enables Rig's `reqwest` and `rustls` features (the OpenAI-compatible clients need them); the API no longer declares them. Core also enables Tokio's `rt` feature for `spawn_blocking` around git subprocess calls.
  - `GET /agents` is fed by a default `info()` method on the `Parser` and `Clarifier` traits (implemented by Scribe and Explainer) and by the new object-safe `project_agent::Briefer` trait, which the API holds for Chronicler (and later Surveyor). An unconfigured agent is not listed.
  - Hardening beyond the list: `--literal-pathspecs` (no pathspec magic), `LC_ALL=C` (stable error text), scrubbed `GIT_DIR`, `GIT_WORK_TREE`, `GIT_EXTERNAL_DIFF`, `GIT_CONFIG_COUNT` and similar variables, `^{commit}` on `git_show` hashes, and NUL-delimited output parsing that drops forged records.
  - `git_show` of a commit that changes nothing under the root (the same commits `git_log` leaves out) answers `not found`, rather than showing its message.
  - `POST /projects/{id}/history` accepts an empty body as the default task.
- **B. Languages and Surveyor:** the `languages` registry with Rust, TypeScript and Python, `GET /projects/{id}/profile`, Surveyor and `POST /projects/{id}/overview`.
- **C. Operator:** command candidates, `CommandsExtraction` validation, Operator and `POST /projects/{id}/commands`.
- **D. GUI:** briefing state, stacked summaries and the command footer.

Each task updates `docs/api.md`, `docs/data-model.md` and `docs/development.md` (new dependencies, such as a TOML parser for manifests) as needed.

## Evaluation

- **Automated, no credentials:** ordinary tests use stub models and an in-memory project or git double. Git tests create a temporary repository with fixed author and date.
- **Opt-in live evaluation** over sample projects (Rust workspace, TypeScript app, Python service, non-git folder, monorepo). It measures:
  - citation validity;
  - invalid outputs and retries;
  - latency per agent;
  - Chronicler: whether summaries reference real recent commits;
  - Surveyor: whether key docs are read and cited;
  - Operator: precision and recall of commands against a hand-written expected list per sample, plus correct working directories and package managers.

## Non-goals

- Running commands, or any write or execute access.
- Persisting or caching briefings. A later task can cache per project and agent, keyed by `HEAD` and the cited files' hashes, using the existing staleness model.
- Languages beyond Rust, TypeScript/JavaScript and Python.
- Parsing briefings with Scribe, or labelling them.
