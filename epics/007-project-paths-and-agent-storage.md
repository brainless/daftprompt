# Epic 007: Project paths and stored agent briefings

Status: implemented (Tasks A, B, C1, C2, D).

## Outcome

One logical project can have multiple explicitly registered root paths. Chronicler,
Surveyor, and Operator results survive restarts and belong to the project **and the
path used for the run**. A matching Git commit can locate a reusable response from
the same or another path of the same project, without pretending a new model run
occurred.

Keep the implementation small: extend the existing SQLite registry, store validated
response snapshots, and put reuse in one shared core service around the agents.
Do not implement graph persistence, worktree discovery, or remote execution.

References: [PRD](../docs/PRD.md), [data model](../docs/data-model.md),
[Epic 006](006-project-briefing.md), [development](../docs/development.md).
This epic supersedes Epic 006's no-persistence/no-cache boundary when implemented.

## What exists today

- `core/src/store/mod.rs` applies `MIGRATIONS`, an ordered list of SQL strings run
  with `execute_batch`, each in its own transaction. Migration 1 creates `project`
  with a unique canonical `root_path`. The connection enables `foreign_keys`.
  `store/projects.rs` implements registration and CRUD.
- `access::RootPolicy::grant` receives a `&Project` and returns a `ProjectScope`
  holding `project_id` and the granted `root`. The policy resolves access before an
  agent runs. Nothing identifies a registered path.
- All three agents return serializable, validated results in memory. Surveyor and
  Chronicler use `BriefingResult`; Operator adds commands in `CommandsResult`, which
  flattens the briefing. These already include run metadata, citations, graph
  fragments, tool calls, and tool output payloads.
- Surveyor and Operator preload a bounded file snapshot. Its reads and listings are
  seeded into the accepted attempt's ledger, so they appear in `tool_calls`
  alongside the model-initiated calls.
- Active Chronicler makes one recorded `git_log` call, then asks the model for
  plain text with no tools. Its older cited, tool-enabled implementation is not the
  HTTP path.
- Retries start with a fresh ledger. The retry prompt carries the previous attempt's
  validation diagnostic, which is model state rather than repository input.
- `Store` serializes one connection behind a mutex. `AgentInfo` exposes provider,
  model, and prompt version only, not limits or other settings.
- The GUI requests all three briefings when opening a project. Explicit re-run
  controls exist. No saved briefing or cache lookup exists.

## Decisions and boundaries

1. A project is a logical identity and name. A path is one local checkout/folder
   belonging to that identity. Association is explicit, never inferred from a
   remote URL, matching commit, folder name, or `git worktree list`.
2. Root paths are local to the running installation. Server-hosted paths can use
   the same identity separation later, but this epic adds no host registry,
   synchronization, SSH, authentication, or remote filesystem abstraction.
3. Store only successful validated responses. Failed attempts remain diagnostics
   and never replace a successful result. Store custom-prompt results too, but
   automatically reuse only default-task results in this first version.
4. Store summaries as ordinary SQLite `TEXT`; use versioned JSON for the remaining
   structured response, including commands and provenance. No new model output
   schema and no graph/command tables are needed.
5. Persistence and freshness are different. A saved response may be historical;
   its presence alone never makes it current. Explicit re-run bypasses reuse.
6. Saved responses are only ever delivered through verified reuse. There is no
   historical or stale display in this epic, so Task B is not user-visible until
   Task C enables reuse. A history browser is later work.
7. Implement one response table for all three agents and one shared reuse policy,
   with small agent-specific input checks. Keep HTTP and GUI details outside core.

## Project and path registry

### Migration runner

The SQL-only runner cannot generate path IDs, and Rust owns IDs. Change
`MIGRATIONS` entries to a small enum, for example
`Migration::Sql(&str)` and `Migration::Rust(fn(&Transaction) -> Result<(), StoreError>)`.
Migration 1 becomes `Sql` with identical text, so existing databases see no change.
Each entry still runs in its own transaction and sets `user_version` on commit.

`PRAGMA foreign_keys` cannot change inside a transaction. Migration 2 can rebuild
`project` with foreign keys enabled only because no table references it yet. After
this epic, `project_path` and `project_agent_response` do, so any later rebuild of
those tables must follow SQLite's documented table-rebuild procedure with foreign
key enforcement handled outside the migration transaction. Record this in the
runner's doc comment.

### Schema

Append migration 2 as a `Rust` migration; do not edit migration 1. Target schema:

```sql
CREATE TABLE project (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_opened_at TEXT NOT NULL
);

CREATE TABLE project_path (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  root_path TEXT NOT NULL UNIQUE,
  is_default INTEGER NOT NULL CHECK (is_default IN (0, 1)),
  created_at TEXT NOT NULL,
  last_opened_at TEXT NOT NULL,
  UNIQUE(project_id, id)
);
CREATE UNIQUE INDEX project_one_default_path
  ON project_path(project_id) WHERE is_default = 1;
```

`UNIQUE(project_id, id)` exists so responses can reference the pair with a
composite foreign key.

The migration renames the old table, creates the new tables, copies projects with
their IDs, names, and timestamps, and inserts one default path per old `root_path`
with a Rust-generated path ID and the project's timestamps. It must not require
folders to still exist. The unique index enforces at most one default; store
transactions enforce at least one path and exactly one default for every project.

### Behaviour

Registration keeps today's canonicalization and existing-directory checks. Adding
an already registered path to its owning project is idempotent; attaching it to a
different project is rejected. Creating a project from an already registered root
returns the owning project. Adding a second path requires that project's ID.

Default selection is explicit and persisted. Removing a default path requires a
replacement in the same transaction; removing the last path is rejected (delete
the project instead). Deleting projects/paths removes their database records and
associated responses, never folders. Removing a path also removes responses other
paths could have reused from it; that is accepted. Missing folders remain
registered and are reported as unavailable; do not silently switch to another path.

Opening a project through a path updates `last_opened_at` on both the project and
that path.

Expose `ProjectPath` in shared types. For compatibility, `Project.root_path` can
remain a computed field for the default path, alongside `paths` and
`default_path_id`; it is no longer a column or the project's identity. Do not keep
two independently writable root values.

### Scope and policy

`RootPolicy::grant` changes to receive the selected path:

```rust
fn grant(
    &self,
    principal: &Principal,
    agent: AgentKind,
    project: &Project,
    path: &ProjectPath,
) -> Result<ProjectScope, AccessError>;
```

`ProjectScope` gains `project_path_id`. The resolver loads the project, resolves
the requested path ID (or the persisted default) and checks it belongs to that
project, then calls the policy, then checks the granted root is a directory. A
policy may narrow `root` below `path.root_path`; it must never widen it. Every
policy implementation and access test is updated with the new signature.

## Stored responses

```sql
CREATE TABLE project_agent_response (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  project_path_id TEXT NOT NULL,
  agent TEXT NOT NULL CHECK (agent IN ('chronicler', 'surveyor', 'operator')),
  request_key TEXT NOT NULL,
  config_key TEXT NOT NULL,
  commit_hash TEXT,
  repository_subpath TEXT,
  reusable INTEGER NOT NULL CHECK (reusable IN (0, 1)),
  response_version INTEGER NOT NULL,
  summary_text TEXT NOT NULL,
  details_json TEXT NOT NULL CHECK (json_valid(details_json)),
  created_at TEXT NOT NULL,
  FOREIGN KEY (project_id, project_path_id)
    REFERENCES project_path(project_id, id) ON DELETE CASCADE
);
CREATE INDEX agent_response_path_lookup ON project_agent_response
  (project_id, project_path_id, agent, request_key, config_key, created_at DESC);
CREATE INDEX agent_response_commit_lookup ON project_agent_response
  (project_id, agent, request_key, config_key, commit_hash, repository_subpath);
```

`json_valid` is available because `rusqlite` is built with `bundled`, which
includes SQLite's JSON functions.

Rows are append-only successful snapshots; `id` is a response ID, not a new model
run ID. Keep the original run/node/context IDs and timestamps. `created_at` is the
snapshot storage time. Latest selection uses `(created_at DESC, id DESC)` as a
stable ordering. Retention and compaction are later work; there is no extra cache
pointer table. `reusable` records eligibility at capture, not a promise of current
freshness. A changed checkout does not mutate an old response.

Use a storage-only `StoredAgentResponseV1` DTO in core, discriminated by agent.
`summary_text` holds `summary_payload.content` unchanged, including Markdown.
`details_json` retains the rest of the accepted result: summary payload hash/time,
task and optional prompt payload, nodes/contexts, citations, profile, notices,
run metadata, run inputs, tool calls/output payloads, and Operator commands. It
also holds the reuse-check inputs described below. Do not duplicate the summary
body inside JSON. Conversion to/from the current public result type is explicit and
round-trip tested. This is a snapshot, not normalized persistence of graph records.

This deliberately preserves provenance rather than storing only the LLM's raw
extraction. In particular, Operator's command citation indexes and working
directories must survive a reload. Chronicler's summary remains plain text; its
JSON contains metadata/evidence, not a new LLM-authored structured result.

## SQL and Rust JSON evolution

SQL migration version (`PRAGMA user_version`), `response_version`, and the agent
prompt/cache-policy versions are independent.

- Decode by stored version into a frozen storage DTO, then convert to current
  application types. Do not deserialize every old blob directly into a changing
  HTTP response struct.
- Compatible additive fields may have explicit `#[serde(default)]`. Never default
  away missing evidence, required identifiers, or incompatible enum meanings.
- Breaking field/enum changes introduce another DTO version and a small Rust
  conversion. No JSON patch/delta framework or mass rewrite is needed initially.
- Unsupported versions, malformed JSON, invalid references, or inconsistent
  summary hashes make that row unusable for reuse. Keep the row, emit a diagnostic,
  and generate a fresh result. They must not prevent other projects from opening.
- JSON decoding failure is different from SQLite I/O/migration failure. Surface
  actual database failures rather than silently claiming persistence succeeded.

## Identity and reuse

### Request and configuration keys

`request_key` hashes the normalized task (existing `prompt_text()` semantics) and
prompt author, with an explicit default-task marker. Custom requests are persisted
but always run fresh in this epic; they do not overwrite a default-task result.

`config_key` hashes a stable, explicitly versioned encoding of effective agent
settings: agent kind, provider/model/endpoint identity, system prompt content and
version, generation options, validation/output limits, file/Git/preload/discovery
limits, Chronicler commit count, and a cache-policy implementation version.
Credentials are excluded. Canonicalize map ordering before hashing. A configurable
model alias changing upstream is not detectable; explicit refresh is the escape
hatch.

`AgentInfo` is insufficient. Add a `cache_identity()` method to `Briefer` and
`CommandsBriefer` that each agent builds from its own config struct and constant
system prompts. Do not read environment variables in the store or the service.

### Commit identity

Use the full resolved repository `HEAD` commit hash and the project root's
repository-relative subpath. Do not use a branch name, abbreviated hash, timestamp,
or the last commit that happened to touch the project folder. Reuse is confined to
the same logical project, agent, request/config keys, and effective access scope.

No Git, unborn HEAD, unavailable identity, probe error, or timeout means no automatic
reuse. Never match two null commit hashes. Persist those results anyway. Extend the
existing bounded, hardened Git subprocess helper with `rev-parse` and `status`
queries; never use a shell or scan worktrees.

### Access scope

Reuse requires the granted scope to cover the whole selected path:
`scope.root` equal to that path's `root_path`, both for the stored response and
for the current request. A policy that narrows the scope always runs fresh, and
its result is stored with `reusable = 0`. `LocalPolicy` never narrows, so local
behaviour is unaffected, and a later hosted policy cannot receive wider-scope
evidence. Comparing narrowed relative scopes is later work.

### Preconditions for file-agent reuse

Before Surveyor/Operator reuse is enabled, confirm in `project_tools` and
`project_agent/briefing.rs` that:

- tool errors and truncated outputs are recorded in the ledger with a comparable
  output (payload or canonical error text); and
- listing, search, read outputs and citation paths are root-relative, with no
  machine-absolute path.

Close any gap in the recorder first. A call kind whose outcome is not fully
recorded makes every response containing it a miss.

### Freshness checks

A matching hash finds **candidates**, not an automatic hit:

| Agent | Additional check before reuse on either the same or another path |
| --- | --- |
| Chronicler | Run the recorded `git_log` query again with the same count and limits, and compare its output hash with the saved one. This catches shallow-history/repository-view differences at the same HEAD. Worktree dirtiness does not invalidate this history-only agent. |
| Surveyor / Operator | No clean-checkout requirement: staged, unstaged, and untracked changes neither block capture nor block reuse. The deterministic inputs and observations below must still match on the target, and the full verifier decides. |

Git status is not consulted for file agents. A matching `HEAD` and subpath only
locate candidates; equivalence is established by replaying the recorded
observations below, which also covers dirty files, ignored files, local ignore
rules, checkout filters, sparse checkouts, and submodules wherever the model or
the preload actually looked. A dirty change the agent never observed (and that
does not change the profile, preload, or command candidates) does not affect
the response, so it does not block reuse.

File-agent verification, in order:

1. Recompute the project profile and compare it with the saved profile.
2. Run `preload_common_files` again against the target and compare the hash of its
   complete ledger (call list, arguments, outputs) with the saved preload
   fingerprint. Recomputing, rather than replaying the saved preload calls, catches
   files that would now be selected, such as a new ignored document.
3. For Operator, recompute the complete Rust-detected command candidate list.
4. Replay the saved model-initiated `list_files`, `search`, and `read_file` calls
   against the target with the original effective limits, and compare canonical
   output hashes. This includes non-cited reads, listings, truncation flags, and
   errors, because all of them can influence the model.

Store the preload fingerprint and the exact model-visible observations in
`details_json`. Exclude generated IDs, timestamps, and machine-absolute roots from
comparisons. Do not replay `submit`, execute discovered commands, or count checks
as model calls. Earlier rejected attempts and the retry diagnostic are model state,
not repository input; they are not replayed. Unsupported or incomplete observation
records mean a miss. This reuses existing bounded tool machinery rather than
introducing a filesystem watcher or a full-tree index. Add one shared verifier,
not three independent cache implementations.

Check HEAD/subpath/eligibility before and after generation and before/after reuse
verification. Save a completed response even if `HEAD` changed, but mark it
ineligible for reuse. Reuse relies on replaying recorded observations, not on an
atomic filesystem snapshot: files that change and revert during a run remain a
documented limitation.

### Lookup

Lookup order: the authorized requested path first, then eligible responses from
other paths of that project at the matching commit and subpath. Check a bounded
number of newest candidates (initially 3); a miss runs the agent normally.
Cross-path lookup must not read a source path's filesystem; all verification runs
against the requested, authorized path.

Query candidate rows while holding the store lock, release it, verify, then
re-acquire it only to load the chosen row. Never hold the lock during Git probes,
filesystem replay, or model calls.

A reused response stays owned by its generating path. Do not clone the row or
rewrite its run IDs. Relative citations and command directories are interpreted
against the requested, verified path.

## Shared service and API changes

Keep agent extraction, validation, and graph construction unchanged. Add a core
briefing service around the existing `Briefer` / `CommandsBriefer` traits:

1. Resolve and authorize `(project_id, project_path_id)` through `RootPolicy`.
2. Compute identity and, unless refresh/custom prompt/narrowed scope, attempt
   verified reuse.
3. Otherwise call the existing agent, then persist its accepted result and check
   inputs atomically. Do not persist partial or invalid results.
4. Return the result plus delivery metadata. Run blocking work off the async
   runtime and the GUI thread.

A storage write failure returns a storage error and logs the completed run ID;
it must not report a durable success. No in-flight deduplication or background
refresh is required. Concurrent successful runs may each append a row.

### Delivery metadata

Do not add delivery fields to `BriefingResult` or `CommandsResult`; those stay the
exact stored agent results. Wrap them in a shared type:

```rust
pub struct Delivered<T> {
    #[serde(flatten)]
    pub result: T,
    pub project_path_id: Option<String>,
    pub cache: Option<CacheDelivery>,
}
```

`CacheDelivery` records the requested path, source path, response ID, capture
time, and `fresh | same_path | other_path`. The JSON keeps every existing field at
the top level, including Operator's flattened briefing, so older clients can
ignore the new fields.

### Endpoints

| Interface | Change |
| --- | --- |
| `POST /projects` | Keep current create-with-first-path behavior. |
| `GET /projects`, `GET /projects/{id}` | Include `paths` and `default_path_id`; `root_path` is the default path. |
| `GET /projects/{id}/paths`, `POST /projects/{id}/paths` | List paths; attach an explicitly supplied local `{path}`. |
| `PATCH /projects/{id}` | Allow setting `default_path_id` as well as renaming. |
| `DELETE /projects/{id}/paths/{path_id}` | Remove registration; require replacement default when necessary. |
| Existing history/overview/commands POST bodies | Add optional `project_path_id` and `refresh` (default false). Omitted path selects the persisted default. Responses use `Delivered<T>`. |
| Existing profile GET | Optional `project_path_id` query parameter. |
| Parse/clarify request bodies | Optional `project_path_id` so explicit selection resolves consistently; their results are not persisted here. |

Arbitrary root paths remain confined to registry operations, never accepted in
agent requests. Calls using only the legacy unregistered `DAFTPROMPT_PROJECT_ROOT`
fallback are not persisted.

### GUI

Explicit GUI re-run sends `refresh: true`; ordinary opens may reuse. Stamp GUI
requests with both project and path IDs so a late reply for an old path cannot
replace the current selection. A small path selector lists registered paths;
registration can initially remain API-only. Parse/clarify/profile requests follow
that selection.

## Implementation tasks

- **A. Registry and scope:** convert the migration runner to SQL/Rust entries;
  append the transactional migration that backfills paths; add path CRUD, default
  selection, and composite ownership checks; change `RootPolicy::grant` and
  `ProjectScope` to carry the selected path, retaining single-path behavior for
  requests without a path ID.
- **B. Response snapshots:** add the response table, versioned storage DTOs,
  validated-result conversions, insert/load methods, `cache_identity()`, and
  shared service wiring that persists fresh responses. Not user-visible until C1.
- **C1. Reuse service and Chronicler:** add Git `rev-parse`/`status` queries,
  deterministic keys, same-path then cross-path lookup, scope check, refresh
  bypass, `Delivered<T>` metadata, and Chronicler's history-hash verification.
  Log cache hit/miss reason without source bodies. Unknown inputs are misses.
- **C2. File-agent reuse:** close any recorder gaps from the preconditions, store
  preload fingerprints and observations, then add the shared verifier (profile,
  recomputed preload, Operator candidates, model-call replay) for Surveyor and
  Operator.
- **D. Clients and documentation:** wire path selection and refresh through the
  GUI; update API/development/data-model docs with the implemented schema/version
  and current behavior. No changes to model prompts are required for storage.

## Acceptance tests

Use SQLite temp databases, temporary Git repositories/worktrees created by tests,
and stub models; ordinary tests need no credentials. Creating test worktrees does
not introduce production worktree discovery.

- Upgrade a v1 database with available and missing roots; IDs/timestamps survive,
  one default exists per project, reopening does not repeat migration.
- Register two paths explicitly; reject cross-project ownership and wrong path IDs;
  default selection, rename, removal, cascade cleanup, and denied/missing roots work.
- A policy receives the selected path; a narrowing policy runs fresh, stores
  `reusable = 0`, and never receives a stored response.
- Round-trip each agent's response, summaries, commands, citations, IDs, and tool
  provenance after restart. Failures do not insert successful response rows.
- Read an older supported JSON fixture; reject an unsupported version or corrupt
  reference as a cache miss while preserving the row and logging the reason.
- Same project/commit/subpath/config/verified inputs at another path avoids a model
  call and preserves source provenance; another project or subpath cannot reuse it.
- Chronicler: changed history output at the same HEAD causes a miss; a dirty
  working tree alone does not.
- Surveyor/Operator: a dirty tracked file or untracked file the agent never
  observed still hits (also from another path and when the source run started
  dirty); a modified tracked file the model read, or an untracked file that
  appears in a listing, search, or the preload, misses. A changed profile, a changed preload
  selection (including a new ignored document), changed read bytes, changed tool
  errors or truncation, ignored-readable inputs, model/prompt/settings, or HEAD
  cause misses. Citation hashes alone are insufficient.
- Non-Git/unborn repositories still persist results but do not get automatic hits.
  Mid-run or mid-verification identity changes prevent reuse eligibility.
- Custom prompts and `refresh: true` run fresh; old API bodies use the default path.
  Cache hits invoke no model, create no fake run, and do not execute commands.
- Delivered JSON keeps every existing result field at the top level, including
  Operator's flattened briefing.
- Access policy runs before every lookup. A late response from another path cannot
  update the GUI.

Run formatting, relevant core/API/GUI tests, and workspace checks when implemented.

## Known deviations

- Attaching a path that is registered to a different project returns `400`
  (`invalid_request`).
- `DELETE /projects/{id}/paths/{path_id}` takes the replacement default as the
  `replacement_default_path_id` query parameter, not a body.
- `POST /projects/{id}/open` opens only the default path; the GUI's path selector
  changes the selection without marking another path opened.
- Git identity probes use `GitToolConfig::default()` limits because the API does
  not call `BriefingService::with_git_config`.
- Operator's candidate fingerprint also hashes the manifests its candidates cite.
- Surveyor/Operator reuse has no clean-checkout requirement; the shared service only checks `HEAD` and subpath identity around generation and verification, and the agent verifier decides equivalence.
- The GUI does not call `GET /projects/{id}/profile`, so the path selection is not
  applied to a profile request there; the profile reaches it only inside briefings.
