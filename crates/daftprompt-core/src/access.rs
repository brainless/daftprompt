//! Which project folders a caller may give an agent.
//!
//! Requests name a registered project by id, never a filesystem path. The
//! server looks the project up in the [`Store`], asks a [`RootPolicy`]
//! whether this [`Principal`] may use it with this [`AgentKind`], and gets
//! back a [`ProjectScope`]: the only thing an agent is allowed to read. The
//! module knows nothing about HTTP; the API layer supplies the principal and
//! maps [`AccessError`] to status codes.
//!
//! Only the single-user local behaviour exists today: [`Principal::Local`]
//! and [`LocalPolicy`], which allows every registered project for every
//! agent. A hosted server replaces the policy (and the principal extractor in
//! the API) without touching the agents or the resolver.

use std::path::PathBuf;

use daftprompt_types::Project;

use crate::store::{Store, StoreError};

/// Who is making the call.
///
/// Non-exhaustive so adding an authenticated variant later (for example
/// `User { id, teams }`) is not a breaking change for code that matches on it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Principal {
    /// The single local user: no authentication, no ownership.
    Local,
}

/// Which agent wants the project, so a policy can restrict folders per agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Scribe,
    Explainer,
    /// The git-history briefing agent.
    Chronicler,
    /// The files-and-documentation briefing agent.
    Surveyor,
    /// The how-the-project-is-run briefing agent.
    Operator,
}

impl AgentKind {
    /// The agent name as recorded in `Run.agent`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scribe => "scribe",
            Self::Explainer => "explainer",
            Self::Chronicler => "chronicler",
            Self::Surveyor => "surveyor",
            Self::Operator => "operator",
        }
    }
}

/// What a policy grants: the project and the folder an agent may read.
///
/// The fields stay minimal on purpose. A future policy can add an allowed
/// subdirectory under `root` or a tool-limit override here; agents only ever
/// see the scope, never the stored project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScope {
    pub project_id: String,
    /// The folder agents may read, as stored at registration.
    pub root: PathBuf,
}

/// Why a project could not be granted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccessError {
    /// No registered project has this id. A hosted policy may also return
    /// this for a project the principal must not even know exists.
    #[error("{0}")]
    NotFound(String),
    /// The project exists but this principal may not use it with this agent.
    #[error("{0}")]
    Denied(String),
    /// The project is granted but its folder is gone, moved, or not a
    /// directory.
    #[error("{0}")]
    UnusableRoot(String),
    /// The registry could not be read.
    #[error("{0}")]
    Storage(String),
}

/// Decides which registered projects a principal may use with an agent.
///
/// This is the plug-in point for hosted deployments: a per-user rule (the
/// principal owns or was shared the project), a team rule (the principal's
/// team may use these roots), and a per-agent rule (for example Explainer
/// may read `docs/` only) all live in an implementation of this trait. It may
/// narrow the returned `root`; it must never widen it beyond the project's.
pub trait RootPolicy: Send + Sync {
    /// The scope granted for `project`, or the reason it is not.
    fn grant(
        &self,
        principal: &Principal,
        agent: AgentKind,
        project: &Project,
    ) -> Result<ProjectScope, AccessError>;
}

/// The single-user policy: any registered project, for any agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalPolicy;

impl RootPolicy for LocalPolicy {
    fn grant(
        &self,
        _principal: &Principal,
        _agent: AgentKind,
        project: &Project,
    ) -> Result<ProjectScope, AccessError> {
        Ok(ProjectScope {
            project_id: project.id.clone(),
            root: PathBuf::from(&project.root_path),
        })
    }
}

/// Resolve an optional `project_id` to the scope an agent may use.
///
/// `None` means the request names no project, and the result is `None`: the
/// caller falls back to its server-wide default, if any. Otherwise the project
/// must exist in `store`, `policy` must grant it, and the granted root must
/// still be a directory (a folder deleted or moved since registration is
/// [`AccessError::UnusableRoot`]).
pub fn resolve_project(
    store: &Store,
    policy: &dyn RootPolicy,
    principal: &Principal,
    agent: AgentKind,
    project_id: Option<&str>,
) -> Result<Option<ProjectScope>, AccessError> {
    let Some(id) = project_id else {
        return Ok(None);
    };
    let project = store.get_project(id).map_err(|error| match error {
        StoreError::NotFound(detail) => AccessError::NotFound(detail),
        other => AccessError::Storage(other.to_string()),
    })?;
    let scope = policy.grant(principal, agent, &project)?;
    if !scope.root.is_dir() {
        return Err(AccessError::UnusableRoot(format!(
            "the folder of project {id:?} ({}) no longer exists or is not a directory",
            scope.root.display()
        )));
    }
    Ok(Some(scope))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DenyAll;

    impl RootPolicy for DenyAll {
        fn grant(
            &self,
            _principal: &Principal,
            agent: AgentKind,
            project: &Project,
        ) -> Result<ProjectScope, AccessError> {
            Err(AccessError::Denied(format!(
                "{} may not use {}",
                agent.as_str(),
                project.id
            )))
        }
    }

    /// Denies only the Scribe, to show the agent reaches the policy.
    struct ExplainerOnly;

    impl RootPolicy for ExplainerOnly {
        fn grant(
            &self,
            principal: &Principal,
            agent: AgentKind,
            project: &Project,
        ) -> Result<ProjectScope, AccessError> {
            match agent {
                AgentKind::Explainer => LocalPolicy.grant(principal, agent, project),
                _ => Err(AccessError::Denied(format!(
                    "{} is not allowed",
                    agent.as_str()
                ))),
            }
        }
    }

    fn store_with_project() -> (Store, Project, tempfile::TempDir) {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let (project, _) = store.add_project(folder.path(), None).unwrap();
        (store, project, folder)
    }

    fn resolve(
        store: &Store,
        policy: &dyn RootPolicy,
        agent: AgentKind,
        id: Option<&str>,
    ) -> Result<Option<ProjectScope>, AccessError> {
        resolve_project(store, policy, &Principal::Local, agent, id)
    }

    #[test]
    fn no_project_id_resolves_to_no_scope() {
        let (store, _, _folder) = store_with_project();
        let scope = resolve(&store, &DenyAll, AgentKind::Scribe, None).unwrap();
        assert_eq!(scope, None);
    }

    #[test]
    fn a_registered_project_resolves_to_its_root() {
        let (store, project, _folder) = store_with_project();
        let scope = resolve(
            &store,
            &LocalPolicy,
            AgentKind::Explainer,
            Some(&project.id),
        )
        .unwrap()
        .expect("a scope");
        assert_eq!(scope.project_id, project.id);
        assert_eq!(scope.root, PathBuf::from(&project.root_path));
    }

    #[test]
    fn an_unknown_project_is_not_found() {
        let (store, _, _folder) = store_with_project();
        let error = resolve(
            &store,
            &LocalPolicy,
            AgentKind::Scribe,
            Some("project-nope"),
        );
        assert!(matches!(error, Err(AccessError::NotFound(_))), "{error:?}");
    }

    #[test]
    fn a_denying_policy_is_denied_even_for_a_known_project() {
        let (store, project, _folder) = store_with_project();
        let error = resolve(&store, &DenyAll, AgentKind::Explainer, Some(&project.id));
        assert!(matches!(error, Err(AccessError::Denied(_))), "{error:?}");
    }

    #[test]
    fn the_policy_sees_which_agent_is_asking() {
        let (store, project, _folder) = store_with_project();
        let id = Some(project.id.as_str());
        assert!(
            resolve(&store, &ExplainerOnly, AgentKind::Explainer, id)
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            resolve(&store, &ExplainerOnly, AgentKind::Scribe, id),
            Err(AccessError::Denied(_))
        ));
    }

    #[test]
    fn a_folder_deleted_after_registration_is_unusable() {
        let (store, project, folder) = store_with_project();
        drop(folder);
        let error = resolve(&store, &LocalPolicy, AgentKind::Scribe, Some(&project.id));
        assert!(
            matches!(error, Err(AccessError::UnusableRoot(_))),
            "{error:?}"
        );
    }
}
