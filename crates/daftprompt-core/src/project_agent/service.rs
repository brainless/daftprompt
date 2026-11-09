//! The briefing service: reuses or runs a briefing agent and stores its
//! successful result ([Epic 007](../../../../epics/007-project-paths-and-agent-storage.md),
//! Tasks B and C1).
//!
//! The service knows nothing about HTTP. The caller has already resolved and
//! authorized the [`ProjectScope`]. For a default-task Chronicler request that
//! does not ask to `refresh` and whose scope covers the whole registered path,
//! the service first looks for a stored response at the same commit and
//! subpath (the requested path's rows first, then other paths of the project)
//! and verifies it against the requested path; a verified hit calls no model,
//! creates no run, and inserts no row. Everything else runs the agent exactly
//! as before and, when the scope names a registered path, appends the accepted
//! result as a response snapshot with its repository identity and reuse
//! eligibility. Failed runs store nothing.
//!
//! Surveyor and Operator (Task C2) use the same lookup with a stricter
//! verification and no clean-checkout requirement. The agent's own verifier
//! repeats the profile, the preload, the Operator command candidates, and the
//! model's file-tool calls against the requested path (see
//! [`reuse`](super::reuse)); that replay, not `git status`, establishes
//! equivalence.

use std::sync::Arc;

use std::path::Path;

use daftprompt_types::{
    AgentTaskRequest, BriefingResult, CacheDelivery, CacheKind, CommandsResult, Delivered,
    ParseError,
};

use super::{Briefer, CommandsBriefer, is_default_task, request_key};
use crate::access::ProjectScope;
use crate::git_tools::{GitToolConfig, RepoIdentity, SubprocessGit};
use crate::store::{
    CandidateQuery, NewResponse, ResponseMeta, ResponseOutcome, ReuseInputsV1, Store, StoreError,
    StoredAgent, StoredAgentResponseV1, StoredResult,
};

/// Stored candidates examined per request before giving up.
const MAX_CANDIDATES: usize = 3;

/// Where a [`ProjectScope`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeOrigin {
    /// Resolved from a registered project path: results are stored.
    Registered,
    /// The legacy unregistered `DAFTPROMPT_PROJECT_ROOT` fallback: there is no
    /// path row to own a response, so nothing is stored.
    Fallback,
}

/// Which agent a [`Briefer`] is, for the stored `agent` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BriefingAgent {
    Chronicler,
    Surveyor,
}

impl BriefingAgent {
    fn stored(self) -> StoredAgent {
        match self {
            Self::Chronicler => StoredAgent::Chronicler,
            Self::Surveyor => StoredAgent::Surveyor,
        }
    }
}

/// Why a briefing call failed.
#[derive(Debug, thiserror::Error)]
pub enum BriefingError {
    /// The agent run failed; nothing was stored.
    #[error("{0}")]
    Agent(ParseError),
    /// The agent run completed (`run_id`) but its result could not be stored.
    /// The result is not returned: durable success is never reported.
    #[error("run {run_id} completed but its response could not be stored: {error}")]
    Storage { run_id: String, error: StoreError },
}

/// Runs briefing agents, reusing verified stored responses where allowed, and
/// persists their successful results.
#[derive(Debug, Clone)]
pub struct BriefingService {
    store: Option<Arc<Store>>,
    git: GitToolConfig,
}

impl BriefingService {
    /// A service that stores into `store`; with `None` it only runs agents.
    pub fn new(store: Option<Arc<Store>>) -> Self {
        Self {
            store,
            git: GitToolConfig::default(),
        }
    }

    /// Bound the Git identity probes (timeout, byte cap) by `git` instead of
    /// the defaults.
    pub fn with_git_config(mut self, git: GitToolConfig) -> Self {
        self.git = git;
        self
    }

    /// Reuse or run a Chronicler or Surveyor briefing and store a fresh result.
    pub async fn brief(
        &self,
        briefer: &dyn Briefer,
        agent: BriefingAgent,
        request: AgentTaskRequest,
        scope: &ProjectScope,
        origin: ScopeOrigin,
    ) -> Result<Delivered<BriefingResult>, BriefingError> {
        let stored_agent = agent.stored();
        let plan = self
            .plan(
                stored_agent,
                &request,
                briefer.cache_identity(),
                scope,
                origin,
            )
            .await;
        if !request.refresh
            && let Some((StoredResult::Briefing(result), cache)) = self
                .lookup(stored_agent, Verifier::Briefing(briefer), &plan, scope)
                .await
        {
            return Ok(Delivered {
                result,
                project_path_id: Some(scope.project_path_id.clone()),
                cache: Some(cache),
            });
        }
        let (result, inputs) = briefer
            .brief_captured(request, scope)
            .await
            .map_err(BriefingError::Agent)?;
        let response = match agent {
            BriefingAgent::Chronicler => StoredAgentResponseV1::chronicler(&result),
            BriefingAgent::Surveyor => {
                StoredAgentResponseV1::surveyor(&result).with_reuse_inputs(inputs)
            }
        };
        let reusable = response.details.reuse_inputs.is_some();
        let meta = self
            .persist(scope, origin, plan, response, reusable, &result.run.id)
            .await?;
        Ok(deliver(result, scope, origin, meta))
    }

    /// Reuse or run an Operator briefing and store a fresh result with its
    /// commands. A hit returns the stored commands unchanged; nothing is run.
    pub async fn commands(
        &self,
        briefer: &dyn CommandsBriefer,
        request: AgentTaskRequest,
        scope: &ProjectScope,
        origin: ScopeOrigin,
    ) -> Result<Delivered<CommandsResult>, BriefingError> {
        let plan = self
            .plan(
                StoredAgent::Operator,
                &request,
                briefer.cache_identity(),
                scope,
                origin,
            )
            .await;
        if !request.refresh
            && let Some((StoredResult::Commands(result), cache)) = self
                .lookup(
                    StoredAgent::Operator,
                    Verifier::Commands(briefer),
                    &plan,
                    scope,
                )
                .await
        {
            return Ok(Delivered {
                result,
                project_path_id: Some(scope.project_path_id.clone()),
                cache: Some(cache),
            });
        }
        let (result, inputs) = briefer
            .brief_captured(request, scope)
            .await
            .map_err(BriefingError::Agent)?;
        let response = StoredAgentResponseV1::operator(&result).with_reuse_inputs(inputs);
        let reusable = response.details.reuse_inputs.is_some();
        let meta = self
            .persist(
                scope,
                origin,
                plan,
                response,
                reusable,
                &result.briefing.run.id,
            )
            .await?;
        Ok(deliver(result, scope, origin, meta))
    }

    /// The keys and, when the scope covers its whole registered path, the
    /// repository identity before the run.
    async fn plan(
        &self,
        agent: StoredAgent,
        request: &AgentTaskRequest,
        config_key: String,
        scope: &ProjectScope,
        origin: ScopeOrigin,
    ) -> Plan {
        let mut plan = Plan {
            keys: Keys {
                request: request_key(request),
                config: config_key,
            },
            agent,
            default_task: is_default_task(request),
            identity: None,
        };
        let (ScopeOrigin::Registered, Some(store)) = (origin, &self.store) else {
            return plan;
        };
        // A policy that narrows the scope below the path root always runs
        // fresh and is never reusable: no identity is taken for it.
        let lookup = {
            let store = store.clone();
            let (project_id, path_id) = (scope.project_id.clone(), scope.project_path_id.clone());
            tokio::task::spawn_blocking(move || store.get_path(&project_id, &path_id)).await
        };
        match lookup {
            Ok(Ok(path)) if Path::new(&path.root_path) == scope.root => {
                plan.identity = self.probe(&scope.root).await;
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => eprintln!(
                "daftprompt: could not load path {}: {error}",
                scope.project_path_id
            ),
            Err(error) => eprintln!("daftprompt: the path lookup task failed: {error}"),
        }
        plan
    }

    /// The `HEAD` commit and subpath of `root`, or `None` when unknown.
    async fn probe(&self, root: &Path) -> Option<RepoIdentity> {
        let git = SubprocessGit::new(root, &self.git);
        // Git runs as a subprocess and is waited on synchronously.
        tokio::task::spawn_blocking(move || git.identity())
            .await
            .ok()
            .flatten()
    }

    /// A verified stored response for this request and where it came from,
    /// or `None` (a miss, with the reason logged without any source text).
    async fn lookup(
        &self,
        agent: StoredAgent,
        verifier: Verifier<'_>,
        plan: &Plan,
        scope: &ProjectScope,
    ) -> Option<(StoredResult, CacheDelivery)> {
        let store = self.store.as_ref()?;
        if !plan.default_task {
            return None;
        }
        // Without a known identity (not a repository, unborn HEAD, error,
        // timeout) or with a narrowed scope there is nothing to match.
        let Some(identity) = &plan.identity else {
            log_miss(
                agent,
                scope,
                "the repository identity is unknown or the scope is narrowed",
            );
            return None;
        };
        let candidates = {
            let store = store.clone();
            let (project_id, path_id) = (scope.project_id.clone(), scope.project_path_id.clone());
            let (request, config) = (plan.keys.request.clone(), plan.keys.config.clone());
            let (commit, subpath) = (identity.commit_hash.clone(), identity.subpath.clone());
            // The store lock is held only inside this query.
            tokio::task::spawn_blocking(move || {
                store.reuse_candidates(
                    CandidateQuery {
                        project_id: &project_id,
                        project_path_id: &path_id,
                        agent,
                        request_key: &request,
                        config_key: &config,
                        commit_hash: &commit,
                        repository_subpath: &subpath,
                    },
                    MAX_CANDIDATES,
                )
            })
            .await
        };
        let candidates = match candidates {
            Ok(Ok(candidates)) => candidates,
            Ok(Err(error)) => {
                log_miss(
                    agent,
                    scope,
                    &format!("the candidate query failed: {error}"),
                );
                return None;
            }
            Err(error) => {
                log_miss(agent, scope, &format!("the candidate task failed: {error}"));
                return None;
            }
        };
        if candidates.is_empty() {
            log_miss(agent, scope, "no stored response at this commit");
            return None;
        }
        for candidate in candidates {
            let stored = match candidate {
                ResponseOutcome::Usable(stored) => stored,
                ResponseOutcome::Unusable(unusable) => {
                    log_miss(
                        agent,
                        scope,
                        &format!(
                            "response {} is unusable: {}",
                            unusable.meta.id, unusable.reason
                        ),
                    );
                    continue;
                }
            };
            let id = &stored.meta.id;
            let Some(inputs) = &stored.response.details.reuse_inputs else {
                log_miss(
                    agent,
                    scope,
                    &format!("response {id} has no recorded check inputs"),
                );
                continue;
            };
            // All verification runs against the requested, authorized path.
            if let Err(reason) = verifier.verify(inputs, scope).await {
                log_miss(
                    agent,
                    scope,
                    &format!("response {id} failed verification: {reason}"),
                );
                continue;
            }
            if self.probe(&scope.root).await.as_ref() != Some(identity) {
                log_miss(
                    agent,
                    scope,
                    "the repository identity changed during verification",
                );
                return None;
            }
            // Reload the chosen row: it may have been removed meanwhile.
            let reloaded = {
                let store = store.clone();
                let id = id.clone();
                tokio::task::spawn_blocking(move || store.get_agent_response(&id)).await
            };
            let (meta, result) = match reloaded {
                Ok(Ok(ResponseOutcome::Usable(row))) => (row.meta, row.response.into_result()),
                _ => {
                    log_miss(
                        agent,
                        scope,
                        &format!("response {id} could not be reloaded"),
                    );
                    continue;
                }
            };
            let kind = if meta.project_path_id == scope.project_path_id {
                CacheKind::SamePath
            } else {
                CacheKind::OtherPath
            };
            eprintln!(
                "daftprompt: {} reuse hit for path {}: response {} ({kind:?}, from path {})",
                agent.as_str(),
                scope.project_path_id,
                meta.id,
                meta.project_path_id
            );
            return Some((result, cache_delivery(scope, &meta, kind)));
        }
        None
    }

    /// Store a fresh result. `reusable` is the agent's own claim (it recorded
    /// complete check inputs); the service adds the checks it owns: a default
    /// task, an unnarrowed scope, a known identity, the same identity after
    /// the run.
    async fn persist(
        &self,
        scope: &ProjectScope,
        origin: ScopeOrigin,
        plan: Plan,
        response: StoredAgentResponseV1,
        reusable: bool,
        run_id: &str,
    ) -> Result<Option<ResponseMeta>, BriefingError> {
        let (ScopeOrigin::Registered, Some(store)) = (origin, &self.store) else {
            return Ok(None);
        };
        let identity = plan.identity.clone();
        let unchanged = match &identity {
            Some(before) => self.probe(&scope.root).await.as_ref() == Some(before),
            None => false,
        };
        if identity.is_some() && !unchanged {
            eprintln!(
                "daftprompt: run {run_id} changed the repository identity; its response is not reusable"
            );
        }
        let reusable = reusable && plan.default_task && unchanged;
        let store = store.clone();
        let project_id = scope.project_id.clone();
        let project_path_id = scope.project_path_id.clone();
        // SQLite is blocking, and the store mutex must not be held across an
        // await, so the whole write runs on the blocking pool.
        let outcome = tokio::task::spawn_blocking(move || {
            store.insert_agent_response(NewResponse {
                project_id: &project_id,
                project_path_id: &project_path_id,
                request_key: &plan.keys.request,
                config_key: &plan.keys.config,
                commit_hash: identity.as_ref().map(|id| id.commit_hash.as_str()),
                repository_subpath: identity.as_ref().map(|id| id.subpath.as_str()),
                reusable,
                response: &response,
            })
        })
        .await
        .unwrap_or_else(|error| {
            Err(StoreError::Storage(format!(
                "the storage task failed: {error}"
            )))
        });
        match outcome {
            Ok(meta) => Ok(Some(meta)),
            Err(error) => {
                eprintln!(
                    "daftprompt: run {run_id} completed but its response could not be stored: {error}"
                );
                Err(BriefingError::Storage {
                    run_id: run_id.to_owned(),
                    error,
                })
            }
        }
    }
}

/// How a stored candidate is verified: by the agent that made it.
enum Verifier<'a> {
    Briefing(&'a dyn Briefer),
    Commands(&'a dyn CommandsBriefer),
}

impl Verifier<'_> {
    async fn verify(&self, inputs: &ReuseInputsV1, scope: &ProjectScope) -> Result<(), String> {
        match self {
            Self::Briefing(briefer) => briefer.verify_reuse(inputs, scope).await,
            Self::Commands(briefer) => briefer.verify_reuse(inputs, scope).await,
        }
    }
}

/// Wrap a fresh result. A stored one carries `fresh` delivery metadata; one
/// that was not stored (the unregistered fallback root, or a service without a
/// database) has no `cache`, and the fallback has no path id either.
fn deliver<T>(
    result: T,
    scope: &ProjectScope,
    origin: ScopeOrigin,
    meta: Option<ResponseMeta>,
) -> Delivered<T> {
    Delivered {
        result,
        project_path_id: (origin == ScopeOrigin::Registered).then(|| scope.project_path_id.clone()),
        cache: meta.map(|meta| cache_delivery(scope, &meta, CacheKind::Fresh)),
    }
}

fn cache_delivery(scope: &ProjectScope, meta: &ResponseMeta, kind: CacheKind) -> CacheDelivery {
    CacheDelivery {
        requested_path_id: scope.project_path_id.clone(),
        source_path_id: meta.project_path_id.clone(),
        response_id: meta.id.clone(),
        captured_at: meta.created_at.clone(),
        kind,
    }
}

fn log_miss(agent: StoredAgent, scope: &ProjectScope, reason: &str) {
    eprintln!(
        "daftprompt: {} reuse miss for path {}: {reason}",
        agent.as_str(),
        scope.project_path_id
    );
}

/// What one call knows before running: its keys, whether it is the default
/// task, and the repository identity (only for an unnarrowed registered scope).
struct Plan {
    keys: Keys,
    agent: StoredAgent,
    default_task: bool,
    identity: Option<RepoIdentity>,
}

/// The keys of one call, computed before the request is consumed.
struct Keys {
    request: String,
    config: String,
}
