//! Verified reuse of stored Chronicler, Surveyor, and Operator responses
//! (Epic 007, Tasks C1 and C2): stub models run through the briefing service
//! against temporary SQLite databases and real temporary Git repositories and
//! worktrees. No credentials. Tests skip quietly when `git` is not installed.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use daftprompt_core::chronicler::{ChroniclerInput, ChroniclerModel};
use daftprompt_core::git_tools::SubprocessGit;
use daftprompt_core::git_tools::{GitError, GitToolConfig};
use daftprompt_core::operator::{OperatorInput, OperatorModel};
use daftprompt_core::store::{ResponseOutcome, ResponseQuery, ReuseInputsV1, StoredAgent};
use daftprompt_core::surveyor::{SurveyorInput, SurveyorModel};
use daftprompt_core::{
    Briefer, BriefingAgent, BriefingService, Chronicler, ChroniclerConfig, CommandsBriefer,
    ModelError, Operator, OperatorConfig, ProjectScope, ScopeOrigin, Store, Surveyor,
    SurveyorConfig,
};
use daftprompt_types::{
    AgentTaskRequest, BriefingCitation, BriefingExtraction, BriefingResult, CacheDelivery,
    CacheKind, CommandKind, CommandsExtraction, Delivered, ExtractedCommand, Project, ProjectPath,
};
use serde_json::{Value, json};

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

/// Run `git -C cwd args` with a fixed identity and date, so equal trees and
/// parents give equal commit hashes in different repositories.
fn git(cwd: &Path, date: &str, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_AUTHOR_NAME", "Ada Lovelace")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_NAME", "Ada Lovelace")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

const DAY1: &str = "2026-10-01T10:00:00+00:00";
const DAY2: &str = "2026-10-02T10:00:00+00:00";
const DAY3: &str = "2026-10-03T10:00:00+00:00";

/// A repository with two commits, both touching `README.md` and `sub/lib.txt`.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, DAY1, &["init", "-q"]);
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("README.md"), "# Demo\n").unwrap();
    std::fs::write(root.join("sub/lib.txt"), "one\n").unwrap();
    git(root, DAY1, &["add", "."]);
    git(root, DAY1, &["commit", "-q", "-m", "Start the demo"]);
    std::fs::write(root.join("README.md"), "# Demo\n\nMore.\n").unwrap();
    std::fs::write(root.join("sub/lib.txt"), "one\ntwo\n").unwrap();
    git(root, DAY2, &["add", "."]);
    git(root, DAY2, &["commit", "-q", "-m", "Grow the demo"]);
    dir
}

type Hook = Arc<dyn Fn() + Send + Sync>;

/// Counts model calls; `hook` runs inside each one.
struct Stub {
    calls: Arc<AtomicUsize>,
    hook: Option<Hook>,
}

impl ChroniclerModel for Stub {
    fn summarize(
        &self,
        _prompt: String,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<String, ModelError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = &self.hook {
            hook();
        }
        Box::pin(async { Ok("Recent work: **demo**.".to_owned()) })
    }

    fn brief(
        &self,
        _input: ChroniclerInput,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(async { Err(ModelError::Provider("unused".to_owned())) })
    }
}

fn chronicler(calls: &Arc<AtomicUsize>, hook: Option<Hook>) -> Chronicler<Stub> {
    Chronicler::new(
        ChroniclerConfig::default(),
        Stub {
            calls: calls.clone(),
            hook,
        },
    )
}

struct Fx {
    store: Arc<Store>,
    service: BriefingService,
    project: Project,
    calls: Arc<AtomicUsize>,
    chronicler: Chronicler<Stub>,
    _db: tempfile::TempDir,
}

impl Fx {
    /// A project whose default path is `root`.
    fn new(root: &Path) -> Self {
        let db = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(db.path().join("daftprompt.db")).unwrap());
        let (project, _) = store.add_project(root, Some("demo")).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        Self {
            service: BriefingService::new(Some(store.clone())),
            chronicler: chronicler(&calls, None),
            store,
            project,
            calls,
            _db: db,
        }
    }

    fn default_path(&self) -> ProjectPath {
        self.project.paths[0].clone()
    }

    fn add_path(&self, root: &Path) -> ProjectPath {
        self.store.add_path(&self.project.id, root).unwrap().0
    }

    fn model_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    async fn run(
        &self,
        path: &ProjectPath,
        request: AgentTaskRequest,
    ) -> Delivered<BriefingResult> {
        self.run_with(&self.chronicler, path, request).await
    }

    async fn run_with(
        &self,
        chronicler: &Chronicler<Stub>,
        path: &ProjectPath,
        request: AgentTaskRequest,
    ) -> Delivered<BriefingResult> {
        self.service
            .brief(
                chronicler,
                BriefingAgent::Chronicler,
                request,
                &scope(&self.project, path),
                ScopeOrigin::Registered,
            )
            .await
            .unwrap()
    }

    /// Default-task rows stored for `path`, newest first.
    fn rows(
        &self,
        path: &ProjectPath,
        agent: StoredAgent,
        config_key: &str,
    ) -> Vec<ResponseOutcome> {
        self.custom_rows(path, agent, config_key, &AgentTaskRequest::default())
    }

    fn custom_rows(
        &self,
        path: &ProjectPath,
        agent: StoredAgent,
        config_key: &str,
        request: &AgentTaskRequest,
    ) -> Vec<ResponseOutcome> {
        let request_key = daftprompt_core::project_agent::request_key(request);
        self.store
            .latest_agent_responses(
                ResponseQuery {
                    project_id: &self.project.id,
                    project_path_id: &path.id,
                    agent,
                    request_key: &request_key,
                    config_key,
                },
                20,
            )
            .unwrap()
    }

    fn chronicler_rows(&self, path: &ProjectPath) -> Vec<ResponseOutcome> {
        self.rows(
            path,
            StoredAgent::Chronicler,
            &Briefer::cache_identity(&self.chronicler),
        )
    }
}

fn scope(project: &Project, path: &ProjectPath) -> ProjectScope {
    ProjectScope {
        project_id: project.id.clone(),
        project_path_id: path.id.clone(),
        root: path.root_path.clone().into(),
    }
}

fn default_task() -> AgentTaskRequest {
    AgentTaskRequest::default()
}

fn head(root: &Path) -> String {
    SubprocessGit::new(root, &GitToolConfig::default())
        .head_commit()
        .unwrap()
}

#[tokio::test]
async fn a_same_path_hit_calls_no_model_and_inserts_nothing() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();

    let first = fx.run(&path, default_task()).await;
    let first_cache = first.cache.clone().unwrap();
    assert_eq!(first_cache.kind, CacheKind::Fresh);
    assert_eq!(first.project_path_id.as_deref(), Some(path.id.as_str()));
    assert_eq!(fx.model_calls(), 1);
    let rows = fx.chronicler_rows(&path);
    assert_eq!(rows.len(), 1);
    let meta = rows[0].meta();
    assert!(meta.reusable);
    assert_eq!(
        meta.commit_hash.as_deref(),
        Some(head(repo.path()).as_str())
    );
    assert_eq!(meta.repository_subpath.as_deref(), Some(""));
    assert_eq!(first_cache.response_id, meta.id);

    let second = fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 1, "a hit invokes no model");
    let cache = second.cache.unwrap();
    assert_eq!(cache.kind, CacheKind::SamePath);
    assert_eq!(cache.response_id, first_cache.response_id);
    assert_eq!(cache.requested_path_id, path.id);
    assert_eq!(cache.source_path_id, path.id);
    assert_eq!(cache.captured_at, first_cache.captured_at);
    // The original run, unchanged: no new run was created.
    assert_eq!(second.result, first.result);
    assert_eq!(fx.chronicler_rows(&path).len(), 1);
}

#[tokio::test]
async fn another_path_of_the_project_reuses_and_keeps_the_source_ownership() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let source = fx.default_path();
    let worktrees = tempfile::tempdir().unwrap();
    let worktree = worktrees.path().join("wt");
    git(
        repo.path(),
        DAY3,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    let other = fx.add_path(&worktree);

    let first = fx.run(&source, default_task()).await;
    let reused = fx.run(&other, default_task()).await;
    assert_eq!(fx.model_calls(), 1);
    let cache = reused.cache.clone().unwrap();
    assert_eq!(cache.kind, CacheKind::OtherPath);
    assert_eq!(cache.requested_path_id, other.id);
    assert_eq!(cache.source_path_id, source.id);
    assert_eq!(cache.response_id, first.cache.unwrap().response_id);
    assert_eq!(reused.project_path_id.as_deref(), Some(other.id.as_str()));
    // The source's provenance is untouched: same run, same IDs.
    assert_eq!(reused.result.run.id, first.result.run.id);
    assert_eq!(reused.result, first.result);
    // The row stays with its generating path; none was cloned to the other.
    assert_eq!(fx.chronicler_rows(&source).len(), 1);
    assert!(fx.chronicler_rows(&other).is_empty());

    // The other path's own run is then preferred by that path.
    let own = fx
        .run_with(
            &fx.chronicler,
            &other,
            AgentTaskRequest {
                refresh: true,
                ..default_task()
            },
        )
        .await;
    assert_eq!(own.cache.unwrap().kind, CacheKind::Fresh);
    assert_eq!(fx.model_calls(), 2);
    let again = fx.run(&other, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
    let cache = again.cache.unwrap();
    assert_eq!(cache.kind, CacheKind::SamePath);
    assert_eq!(cache.source_path_id, other.id);
}

#[tokio::test]
async fn another_project_or_subpath_cannot_reuse() {
    if !git_available() {
        return;
    }
    // Two repositories with identical commits (same hashes), two projects.
    let first_repo = repo();
    let second_repo = repo();
    assert_eq!(head(first_repo.path()), head(second_repo.path()));
    let fx = Fx::new(first_repo.path());
    let (second_project, _) = fx
        .store
        .add_project(second_repo.path(), Some("other"))
        .unwrap();
    fx.run(&fx.default_path(), default_task()).await;
    assert_eq!(fx.model_calls(), 1);
    let result = fx
        .service
        .brief(
            &fx.chronicler,
            BriefingAgent::Chronicler,
            default_task(),
            &scope(&second_project, &second_project.paths[0]),
            ScopeOrigin::Registered,
        )
        .await
        .unwrap();
    assert_eq!(fx.model_calls(), 2, "another project runs fresh");
    assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);

    // Same project, same commit, but the root is a subdirectory.
    let sub = fx.add_path(&first_repo.path().join("sub"));
    let result = fx.run(&sub, default_task()).await;
    assert_eq!(fx.model_calls(), 3, "another subpath runs fresh");
    assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
    let rows = fx.chronicler_rows(&sub);
    assert_eq!(rows[0].meta().repository_subpath.as_deref(), Some("sub"));
    // And the subpath's own response is then reusable on its own path.
    fx.run(&sub, default_task()).await;
    assert_eq!(fx.model_calls(), 3);
}

#[tokio::test]
async fn a_different_history_view_at_the_same_head_misses() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let source = fx.default_path();
    // A shallow clone has the same HEAD but one commit of history.
    let clones = tempfile::tempdir().unwrap();
    git(
        clones.path(),
        DAY3,
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            &format!("file://{}", repo.path().display()),
            "shallow",
        ],
    );
    let shallow = clones.path().join("shallow");
    assert_eq!(head(&shallow), head(repo.path()));
    let other = fx.add_path(&shallow);

    fx.run(&source, default_task()).await;
    let result = fx.run(&other, default_task()).await;
    assert_eq!(fx.model_calls(), 2, "the history hash differs");
    assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
    // The miss stored its own, reusable, row for the shallow path.
    assert_eq!(fx.chronicler_rows(&other).len(), 1);
    fx.run(&other, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
}

#[tokio::test]
async fn a_different_commit_count_is_a_different_config() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();
    fx.run(&path, default_task()).await;
    let config = ChroniclerConfig {
        commit_count: 1,
        ..Default::default()
    };
    let narrow_history = Chronicler::new(
        config,
        Stub {
            calls: fx.calls.clone(),
            hook: None,
        },
    );
    let result = fx.run_with(&narrow_history, &path, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
    assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
}

#[tokio::test]
async fn a_head_change_misses_and_a_dirty_tree_alone_does_not() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();
    fx.run(&path, default_task()).await;

    // Staged, unstaged, and untracked changes do not affect history.
    std::fs::write(repo.path().join("README.md"), "dirty\n").unwrap();
    std::fs::write(repo.path().join("scratch.txt"), "new\n").unwrap();
    let hit = fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 1);
    assert_eq!(hit.cache.unwrap().kind, CacheKind::SamePath);

    git(
        repo.path(),
        DAY3,
        &["commit", "-q", "--allow-empty", "-m", "Move HEAD"],
    );
    let fresh = fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
    assert_eq!(fresh.cache.unwrap().kind, CacheKind::Fresh);
    let hit = fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
    assert_eq!(hit.cache.unwrap().kind, CacheKind::SamePath);
    // Old rows are kept, never rewritten.
    assert_eq!(fx.chronicler_rows(&path).len(), 2);
}

#[tokio::test]
async fn folders_without_a_known_commit_persist_but_never_hit() {
    if !git_available() {
        return;
    }
    let plain = tempfile::tempdir().unwrap();
    let unborn = tempfile::tempdir().unwrap();
    git(unborn.path(), DAY1, &["init", "-q"]);
    for root in [plain.path(), unborn.path()] {
        let fx = Fx::new(root);
        let path = fx.default_path();
        for expected_calls in [1, 2] {
            let result = fx.run(&path, default_task()).await;
            assert_eq!(fx.model_calls(), expected_calls);
            assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
        }
        let rows = fx.chronicler_rows(&path);
        assert_eq!(rows.len(), 2, "both results are persisted");
        for row in &rows {
            // Two NULL commits never match, and nothing says reusable.
            assert_eq!(row.meta().commit_hash, None);
            assert_eq!(row.meta().repository_subpath, None);
            assert!(!row.meta().reusable);
        }
    }
}

#[tokio::test]
async fn custom_prompts_and_refresh_run_fresh() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();
    fx.run(&path, default_task()).await;

    let custom = AgentTaskRequest {
        prompt: Some("What changed?".to_owned()),
        ..default_task()
    };
    for expected_calls in [2, 3] {
        let result = fx.run(&path, custom.clone()).await;
        assert_eq!(fx.model_calls(), expected_calls);
        assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
    }
    let config_key = Briefer::cache_identity(&fx.chronicler);
    let rows = fx.custom_rows(&path, StoredAgent::Chronicler, &config_key, &custom);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| !row.meta().reusable));

    let refreshed = fx
        .run(
            &path,
            AgentTaskRequest {
                refresh: true,
                ..default_task()
            },
        )
        .await;
    assert_eq!(fx.model_calls(), 4);
    assert_eq!(refreshed.cache.unwrap().kind, CacheKind::Fresh);
    // Refresh still persists, and the new row is reusable.
    assert_eq!(fx.chronicler_rows(&path).len(), 2);
    fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 4);
}

#[tokio::test]
async fn a_narrowed_scope_runs_fresh_is_not_reusable_and_is_never_served() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();
    // A wide response exists; the narrowed request must not receive it.
    fx.run(&path, default_task()).await;
    let narrowed = ProjectScope {
        root: Path::new(&path.root_path).join("sub"),
        ..scope(&fx.project, &path)
    };
    for expected_calls in [2, 3] {
        let result = fx
            .service
            .brief(
                &fx.chronicler,
                BriefingAgent::Chronicler,
                default_task(),
                &narrowed,
                ScopeOrigin::Registered,
            )
            .await
            .unwrap();
        assert_eq!(fx.model_calls(), expected_calls);
        assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
    }
    let rows = fx.chronicler_rows(&path);
    assert_eq!(rows.len(), 3);
    let narrowed_rows: Vec<_> = rows.iter().filter(|row| !row.meta().reusable).collect();
    assert_eq!(narrowed_rows.len(), 2);
    assert!(
        narrowed_rows
            .iter()
            .all(|row| row.meta().commit_hash.is_none())
    );
    // A wide request is served the wide response, never a narrowed one.
    let wide = fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 3);
    let served = wide.cache.unwrap().response_id;
    assert!(
        rows.iter()
            .any(|row| row.meta().id == served && row.meta().reusable)
    );
}

#[tokio::test]
async fn an_unusable_row_is_a_miss_and_is_kept() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();
    let first = fx.run(&path, default_task()).await;
    let old_id = first.cache.unwrap().response_id;

    // An unsupported response version, written behind the store's back.
    let db = fx._db.path().join("daftprompt.db");
    rusqlite::Connection::open(db)
        .unwrap()
        .execute(
            "UPDATE project_agent_response SET response_version = 99 WHERE id = ?1",
            [&old_id],
        )
        .unwrap();

    let result = fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
    assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
    assert!(matches!(
        fx.store.get_agent_response(&old_id).unwrap(),
        ResponseOutcome::Unusable(_)
    ));
    // The fresh row is usable and served next.
    fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
}

#[tokio::test]
async fn an_identity_change_during_the_run_makes_the_row_not_reusable() {
    if !git_available() {
        return;
    }
    let repo = repo();
    let root = repo.path().to_path_buf();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();
    let moving = chronicler(
        &fx.calls,
        Some(Arc::new(move || {
            git(
                &root,
                DAY3,
                &["commit", "-q", "--allow-empty", "-m", "Mid-run"],
            );
        })),
    );
    let result = fx.run_with(&moving, &path, default_task()).await;
    assert_eq!(result.cache.unwrap().kind, CacheKind::Fresh);
    let rows = fx.chronicler_rows(&path);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].meta().reusable);

    // Not served: the next request runs fresh, and that row is reusable.
    fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
    fx.run(&path, default_task()).await;
    assert_eq!(fx.model_calls(), 2);
}

#[tokio::test]
async fn delivered_json_keeps_every_result_field_at_the_top_level() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let fx = Fx::new(repo.path());
    let path = fx.default_path();
    let reader = Reader {
        calls: Arc::new(AtomicUsize::new(0)),
        hook: None,
    };
    let operator = Operator::new(OperatorConfig::default(), reader);
    let delivered = fx
        .service
        .commands(
            &operator,
            default_task(),
            &scope(&fx.project, &path),
            ScopeOrigin::Registered,
        )
        .await
        .unwrap();
    let value = serde_json::to_value(&delivered).unwrap();
    let object = value.as_object().unwrap();
    let result = serde_json::to_value(&delivered.result).unwrap();
    for key in result.as_object().unwrap().keys() {
        assert!(object.contains_key(key), "missing top-level field {key}");
    }
    // Operator's flattened briefing and its commands, plus the wrapper's two.
    for key in [
        "commands",
        "summary_payload",
        "run",
        "project_path_id",
        "cache",
    ] {
        assert!(object.contains_key(key), "{key}");
    }
    assert_eq!(value["cache"]["kind"], "fresh");
    let back: Delivered<daftprompt_types::CommandsResult> = serde_json::from_value(value).unwrap();
    assert_eq!(back, delivered);

    // A service without a database delivers without cache metadata.
    let bare = BriefingService::new(None)
        .commands(
            &operator,
            default_task(),
            &scope(&fx.project, &path),
            ScopeOrigin::Registered,
        )
        .await
        .unwrap();
    assert_eq!(bare.project_path_id.as_deref(), Some(path.id.as_str()));
    assert!(bare.cache.is_none());
    let fallback = fx
        .service
        .commands(
            &operator,
            default_task(),
            &scope(&fx.project, &path),
            ScopeOrigin::Fallback,
        )
        .await
        .unwrap();
    assert!(fallback.project_path_id.is_none() && fallback.cache.is_none());
}

#[test]
fn git_identity_queries_report_head_and_subpath() {
    if !git_available() {
        return;
    }
    let config = GitToolConfig::default();
    let repo = repo();
    let top = SubprocessGit::new(repo.path(), &config);
    let hash = top.head_commit().unwrap();
    assert!(hash.len() == 40 || hash.len() == 64);
    assert_eq!(top.repository_subpath().unwrap(), "");
    let sub = SubprocessGit::new(repo.path().join("sub"), &config);
    assert_eq!(sub.repository_subpath().unwrap(), "sub");
    let identity = sub.identity().unwrap();
    assert_eq!(
        (identity.commit_hash.as_str(), identity.subpath.as_str()),
        (hash.as_str(), "sub")
    );

    // No repository, no commits, no git, and a bad directory: unknown.
    let plain = tempfile::tempdir().unwrap();
    let plain_git = SubprocessGit::new(plain.path(), &config);
    assert_eq!(plain_git.head_commit(), Err(GitError::NotARepository));
    assert!(plain_git.identity().is_none());
    let unborn = tempfile::tempdir().unwrap();
    git(unborn.path(), DAY1, &["init", "-q"]);
    let unborn_git = SubprocessGit::new(unborn.path(), &config);
    assert!(unborn_git.head_commit().is_err());
    assert!(unborn_git.identity().is_none());
    let missing = SubprocessGit::new(repo.path(), &config).with_program("daftprompt-no-such-git");
    assert!(matches!(
        missing.head_commit(),
        Err(GitError::Unavailable(_))
    ));
    assert!(missing.identity().is_none());
}

// Surveyor and Operator (Task C2).

#[derive(Clone, Copy, PartialEq, Debug)]
enum A {
    Surveyor,
    Operator,
}

const AGENTS: [A; 2] = [A::Surveyor, A::Operator];

/// A model that makes the same file-tool calls every time, then cites the
/// README. `hook` runs after the calls, inside the model call.
#[derive(Clone)]
struct Reader {
    calls: Arc<AtomicUsize>,
    hook: Option<Hook>,
}

impl Reader {
    fn act(&self, toolbox: &daftprompt_core::project_tools::ProjectToolbox) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        for (tool, args) in [
            ("read_file", json!({"path": "README.md"})),
            ("list_files", json!({"directory": "sub"})),
            ("search", json!({"query": "demo"})),
            // Readable although Git ignores it (see `file_repo`).
            ("read_file", json!({"path": "deep/local.txt"})),
            // An error until the file appears.
            ("read_file", json!({"path": "deep/missing.txt"})),
            // Exactly as long as the read limit: untruncated until it grows.
            ("read_file", json!({"path": "deep/long.txt"})),
        ] {
            toolbox.call(tool, args);
        }
        if let Some(hook) = &self.hook {
            hook();
        }
    }
}

fn readme_citation() -> Vec<BriefingCitation> {
    vec![BriefingCitation::File {
        path: "README.md".to_owned(),
        start_line: 1,
        end_line: 1,
    }]
}

impl SurveyorModel for Reader {
    fn brief(
        &self,
        input: SurveyorInput,
    ) -> daftprompt_core::surveyor::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        self.act(input.toolset.files());
        Box::pin(async {
            Ok(BriefingExtraction {
                summary: "A demo [1].".to_owned(),
                citations: readme_citation(),
            })
        })
    }
}

impl OperatorModel for Reader {
    fn brief(
        &self,
        input: OperatorInput,
    ) -> daftprompt_core::operator::BoxFuture<'_, Result<CommandsExtraction, ModelError>> {
        self.act(input.toolset.files());
        Box::pin(async {
            Ok(CommandsExtraction {
                summary: "A demo [1].".to_owned(),
                citations: readme_citation(),
                commands: vec![
                    ExtractedCommand {
                        label: "build".to_owned(),
                        command: "cargo build".to_owned(),
                        working_dir: None,
                        kind: CommandKind::Build,
                        candidate: Some(1),
                        citations: vec![],
                    },
                    ExtractedCommand {
                        label: "site".to_owned(),
                        command: "make site".to_owned(),
                        working_dir: Some("sub".to_owned()),
                        kind: CommandKind::Run,
                        candidate: None,
                        citations: vec![1],
                    },
                ],
            })
        })
    }
}

/// [`repo`] plus a manifest and, untracked but excluded through
/// `.git/info/exclude`, files the tools can read although Git ignores them:
/// inputs that only replaying the model's observations can see.
fn file_repo() -> tempfile::TempDir {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    std::fs::create_dir_all(root.join("sub/sub")).unwrap();
    std::fs::write(root.join("sub/sub/note.txt"), "note\n").unwrap();
    std::fs::write(root.join("sub/README.md"), "# Sub\n").unwrap();
    std::fs::write(root.join("sub/Cargo.toml"), "[package]\nname = \"sub\"\n").unwrap();
    git(root, DAY3, &["add", "."]);
    git(root, DAY3, &["commit", "-q", "-m", "Add a manifest"]);
    std::fs::write(
        root.join(".git/info/exclude"),
        "deep/\npyproject.toml\ndocs/\njustfile\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("deep")).unwrap();
    std::fs::write(root.join("deep/local.txt"), "local note\n").unwrap();
    std::fs::write(root.join("deep/long.txt"), "line\n".repeat(300)).unwrap();
    dir
}

struct FileFx {
    fx: Fx,
    surveyor: Surveyor<Reader>,
    operator: Operator<Reader>,
}

struct Run {
    /// The delivered JSON without the delivery fields.
    result: Value,
    cache: CacheDelivery,
}

impl FileFx {
    fn new(root: &Path) -> Self {
        Self::with_hook(root, None)
    }

    fn with_hook(root: &Path, hook: Option<Hook>) -> Self {
        let fx = Fx::new(root);
        let reader = Reader {
            calls: fx.calls.clone(),
            hook,
        };
        Self {
            surveyor: Surveyor::new(SurveyorConfig::default(), reader.clone()),
            operator: Operator::new(OperatorConfig::default(), reader),
            fx,
        }
    }

    fn model_calls(&self) -> usize {
        self.fx.model_calls()
    }

    async fn run(&self, agent: A, path: &ProjectPath, request: AgentTaskRequest) -> Run {
        self.run_scoped(agent, &scope(&self.fx.project, path), request)
            .await
    }

    async fn run_scoped(&self, agent: A, scope: &ProjectScope, request: AgentTaskRequest) -> Run {
        let service = &self.fx.service;
        let origin = ScopeOrigin::Registered;
        let mut value = match agent {
            A::Surveyor => serde_json::to_value(
                service
                    .brief(
                        &self.surveyor,
                        BriefingAgent::Surveyor,
                        request,
                        scope,
                        origin,
                    )
                    .await
                    .unwrap(),
            ),
            A::Operator => serde_json::to_value(
                service
                    .commands(&self.operator, request, scope, origin)
                    .await
                    .unwrap(),
            ),
        }
        .unwrap();
        let cache = serde_json::from_value(value["cache"].clone()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("cache");
        object.remove("project_path_id");
        Run {
            result: value,
            cache,
        }
    }

    fn rows(&self, agent: A, path: &ProjectPath) -> Vec<ResponseOutcome> {
        match agent {
            A::Surveyor => self.fx.rows(
                path,
                StoredAgent::Surveyor,
                &Briefer::cache_identity(&self.surveyor),
            ),
            A::Operator => self.fx.rows(
                path,
                StoredAgent::Operator,
                &CommandsBriefer::cache_identity(&self.operator),
            ),
        }
    }

    /// The newest stored row's check inputs, verified against `target`.
    async fn verify(
        &self,
        agent: A,
        source: &ProjectPath,
        target: &ProjectPath,
    ) -> Result<(), String> {
        let ResponseOutcome::Usable(row) = self.rows(agent, source).remove(0) else {
            panic!("the newest row is unusable");
        };
        let inputs: ReuseInputsV1 = row.response.details.reuse_inputs.clone().unwrap();
        let scope = scope(&self.fx.project, target);
        match agent {
            A::Surveyor => Briefer::verify_reuse(&self.surveyor, &inputs, &scope).await,
            A::Operator => CommandsBriefer::verify_reuse(&self.operator, &inputs, &scope).await,
        }
    }
}

fn worktree(repo: &Path, parent: &Path) -> std::path::PathBuf {
    let worktree = parent.join("wt");
    git(
        repo,
        DAY3,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    worktree
}

#[tokio::test]
async fn file_agents_hit_on_the_same_path_without_a_model_call_or_a_command() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let ffx = FileFx::new(repo.path());
    let path = ffx.fx.default_path();
    let mut expected_calls = 0;
    for agent in AGENTS {
        let first = ffx.run(agent, &path, default_task()).await;
        expected_calls += 1;
        assert_eq!(first.cache.kind, CacheKind::Fresh);
        assert_eq!(ffx.model_calls(), expected_calls);
        let rows = ffx.rows(agent, &path);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].meta().reusable, "{agent:?}");

        let second = ffx.run(agent, &path, default_task()).await;
        assert_eq!(
            ffx.model_calls(),
            expected_calls,
            "{agent:?}: no model call"
        );
        assert_eq!(second.cache.kind, CacheKind::SamePath);
        assert_eq!(second.cache.response_id, first.cache.response_id);
        assert_eq!(second.cache.captured_at, first.cache.captured_at);
        // The original run and provenance, including Operator's commands.
        assert_eq!(second.result, first.result);
        assert_eq!(ffx.rows(agent, &path).len(), 1, "a hit inserts nothing");
    }
    // The stored Operator commands survive with directories and citations.
    let operator = ffx.run(A::Operator, &path, default_task()).await;
    let commands = operator.result["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[1]["working_dir"], "sub");
    assert_eq!(commands[1]["citation_indexes"], json!([1]));
    assert_eq!(commands[0]["origin"], "detected");
}

#[tokio::test]
async fn file_agents_hit_on_another_path_and_keep_the_source_provenance() {
    if !git_available() {
        return;
    }
    // Without the excluded files, which a worktree would not have.
    let repo = repo();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    git(repo.path(), DAY3, &["add", "."]);
    git(repo.path(), DAY3, &["commit", "-q", "-m", "Add a manifest"]);
    let parent = tempfile::tempdir().unwrap();
    let worktree = worktree(repo.path(), parent.path());

    // `deep/` is absent in both paths, so the model's reads fail identically.
    let ffx = FileFx::new(repo.path());
    let source = ffx.fx.default_path();
    let other = ffx.fx.add_path(&worktree);
    for agent in AGENTS {
        let first = ffx.run(agent, &source, default_task()).await;
        let calls = ffx.model_calls();
        let reused = ffx.run(agent, &other, default_task()).await;
        assert_eq!(ffx.model_calls(), calls, "{agent:?}: no model call");
        assert_eq!(reused.cache.kind, CacheKind::OtherPath);
        assert_eq!(reused.cache.requested_path_id, other.id);
        assert_eq!(reused.cache.source_path_id, source.id);
        assert_eq!(reused.cache.response_id, first.cache.response_id);
        assert_eq!(reused.result, first.result, "{agent:?}: same provenance");
        assert!(ffx.rows(agent, &other).is_empty(), "no row was cloned");
        assert_eq!(ffx.rows(agent, &source).len(), 1);
    }
    let reused = ffx.run(A::Operator, &other, default_task()).await;
    let commands = reused.result["commands"].as_array().unwrap();
    assert_eq!(commands[1]["working_dir"], "sub");
    assert_eq!(commands[1]["citation_indexes"], json!([1]));
}

/// One way the target can differ from the checkout that produced a response.
struct Miss {
    name: &'static str,
    mutate: fn(&Path),
    /// A part of the verifier's reason; `None` when the service rejects the
    /// target before verification (a moved checkout) or the verifier may name
    /// either the preload or a model call.
    reason: Option<&'static str>,
}

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const MISSES: &[Miss] = &[
    Miss {
        name: "a modified tracked file the model read",
        mutate: |root| write(root, "README.md", "# Demo\n\nEdited.\n"),
        reason: None,
    },
    Miss {
        name: "an untracked file in a directory listing",
        mutate: |root| write(root, "sub/scratch.txt", "new\n"),
        reason: None,
    },
    Miss {
        name: "a moved HEAD",
        mutate: |root| git(root, DAY3, &["commit", "-q", "--allow-empty", "-m", "Move"]),
        reason: None,
    },
    Miss {
        name: "a changed profile",
        mutate: |root| write(root, "pyproject.toml", "[project]\nname = \"demo\"\n"),
        reason: Some("profile"),
    },
    Miss {
        name: "a new ignored document",
        mutate: |root| write(root, "docs/extra.md", "# Extra\n"),
        reason: Some("profile"),
    },
    Miss {
        name: "a new ignored task file in the preload",
        mutate: |root| write(root, "justfile", "build:\n  true\n"),
        reason: Some("preloaded"),
    },
    Miss {
        name: "changed bytes of a file the model read but did not cite",
        mutate: |root| write(root, "deep/local.txt", "another note\n"),
        reason: Some("model call 4 "),
    },
    Miss {
        name: "an ignored-but-readable input that disappeared",
        mutate: |root| std::fs::remove_file(root.join("deep/local.txt")).unwrap(),
        reason: Some("model call 4 "),
    },
    Miss {
        name: "a tool error that became a read",
        mutate: |root| write(root, "deep/missing.txt", "now here\n"),
        reason: Some("model call 5 "),
    },
    Miss {
        name: "a read that became truncated",
        mutate: |root| write(root, "deep/long.txt", &"line\n".repeat(301)),
        reason: Some("model call 6 "),
    },
];

#[tokio::test]
async fn file_agents_miss_when_the_checkout_or_an_observation_differs() {
    if !git_available() {
        return;
    }
    for agent in AGENTS {
        for miss in MISSES {
            let repo = file_repo();
            let ffx = FileFx::new(repo.path());
            let path = ffx.fx.default_path();
            let first = ffx.run(agent, &path, default_task()).await;
            assert_eq!(ffx.model_calls(), 1);
            // The citation hash alone does not decide: the cited README is
            // untouched in all but the first case.
            (miss.mutate)(repo.path());
            if let Some(reason) = miss.reason {
                let error = ffx
                    .verify(agent, &path, &path)
                    .await
                    .expect_err(&format!("{agent:?}: {}", miss.name));
                assert!(
                    error.contains(reason),
                    "{agent:?}: {}: unexpected reason {error:?}",
                    miss.name
                );
            }
            let again = ffx.run(agent, &path, default_task()).await;
            assert_eq!(ffx.model_calls(), 2, "{agent:?}: {}", miss.name);
            assert_eq!(again.cache.kind, CacheKind::Fresh, "{}", miss.name);
            assert_ne!(again.cache.response_id, first.cache.response_id);
            // The old row is kept.
            let rows = ffx.rows(agent, &path);
            assert_eq!(rows.len(), 2, "{}", miss.name);
        }
    }
}

/// Changes the preload, the listings, the search, and the reads never see:
/// hidden paths are unavailable to the file tools.
fn unobserved_dirt(root: &Path) {
    write(root, ".hidden/tracked.txt", "edited\n");
    write(root, ".cargo/config.toml", "[build]\n");
}

#[tokio::test]
async fn unobserved_dirty_changes_do_not_block_reuse_on_the_same_or_another_path() {
    if !git_available() {
        return;
    }
    let repo = repo();
    write(repo.path(), "Cargo.toml", "[package]\nname = \"demo\"\n");
    write(repo.path(), ".hidden/tracked.txt", "tracked\n");
    git(repo.path(), DAY3, &["add", "."]);
    git(repo.path(), DAY3, &["commit", "-q", "-m", "Add files"]);
    let parent = tempfile::tempdir().unwrap();
    let worktree = worktree(repo.path(), parent.path());
    let ffx = FileFx::new(repo.path());
    let source = ffx.fx.default_path();
    let other = ffx.fx.add_path(&worktree);
    for agent in AGENTS {
        // The source run starts and ends on a dirty tree and is reusable.
        unobserved_dirt(repo.path());
        let first = ffx.run(agent, &source, default_task()).await;
        assert!(ffx.rows(agent, &source)[0].meta().reusable, "{agent:?}");
        let calls = ffx.model_calls();
        // Same path, still dirty.
        let same = ffx.run(agent, &source, default_task()).await;
        assert_eq!(same.cache.kind, CacheKind::SamePath, "{agent:?}");
        // Another path, clean and then differently dirty.
        let clean = ffx.run(agent, &other, default_task()).await;
        assert_eq!(clean.cache.kind, CacheKind::OtherPath, "{agent:?}");
        write(&worktree, ".cargo/other.toml", "x\n");
        let dirty = ffx.run(agent, &other, default_task()).await;
        assert_eq!(dirty.cache.kind, CacheKind::OtherPath, "{agent:?}");
        assert_eq!(dirty.cache.response_id, first.cache.response_id);
        assert_eq!(ffx.model_calls(), calls, "{agent:?}: no model call");
        // Reset for the other agent.
        std::fs::remove_file(worktree.join(".cargo/other.toml")).unwrap();
    }
}

#[tokio::test]
async fn an_ignored_readable_input_of_the_source_makes_another_path_miss() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let parent = tempfile::tempdir().unwrap();
    let worktree = worktree(repo.path(), parent.path());
    let ffx = FileFx::new(repo.path());
    let source = ffx.fx.default_path();
    let other = ffx.fx.add_path(&worktree);
    for agent in AGENTS {
        ffx.run(agent, &source, default_task()).await;
        let calls = ffx.model_calls();
        // The worktree is at the same commit, but lacks the
        // excluded files the model read.
        let error = ffx.verify(agent, &source, &other).await.unwrap_err();
        assert!(error.contains("model call") || error.contains("preloaded"));
        let fresh = ffx.run(agent, &other, default_task()).await;
        assert_eq!(ffx.model_calls(), calls + 1, "{agent:?}");
        assert_eq!(fresh.cache.kind, CacheKind::Fresh);
    }
}

#[tokio::test]
async fn model_prompt_and_settings_changes_miss() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let ffx = FileFx::new(repo.path());
    let path = ffx.fx.default_path();
    let reader = Reader {
        calls: ffx.fx.calls.clone(),
        hook: None,
    };
    ffx.run(A::Surveyor, &path, default_task()).await;
    ffx.run(A::Operator, &path, default_task()).await;
    assert_eq!(ffx.model_calls(), 2);

    let model = Surveyor::new(
        SurveyorConfig::default().with_model("other"),
        reader.clone(),
    );
    let mut tools = SurveyorConfig::default();
    tools.tools.max_list_files = 7;
    let limits = Surveyor::new(tools, reader.clone());
    for surveyor in [&model, &limits] {
        let run = ffx
            .fx
            .service
            .brief(
                surveyor,
                BriefingAgent::Surveyor,
                default_task(),
                &scope(&ffx.fx.project, &path),
                ScopeOrigin::Registered,
            )
            .await
            .unwrap();
        assert_eq!(run.cache.unwrap().kind, CacheKind::Fresh);
    }
    let operator = Operator::new(OperatorConfig::default().with_model("other"), reader);
    let run = ffx
        .fx
        .service
        .commands(
            &operator,
            default_task(),
            &scope(&ffx.fx.project, &path),
            ScopeOrigin::Registered,
        )
        .await
        .unwrap();
    assert_eq!(run.cache.unwrap().kind, CacheKind::Fresh);
    assert_eq!(ffx.model_calls(), 5);
}

#[tokio::test]
async fn custom_prompts_and_refresh_run_fresh_for_file_agents() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let ffx = FileFx::new(repo.path());
    let path = ffx.fx.default_path();
    for agent in AGENTS {
        let base = ffx.model_calls();
        ffx.run(agent, &path, default_task()).await;
        let custom = AgentTaskRequest {
            prompt: Some("What does this do?".to_owned()),
            ..default_task()
        };
        for expected in [base + 2, base + 3] {
            let run = ffx.run(agent, &path, custom.clone()).await;
            assert_eq!(run.cache.kind, CacheKind::Fresh);
            assert_eq!(ffx.model_calls(), expected);
        }
        let refreshed = ffx
            .run(
                agent,
                &path,
                AgentTaskRequest {
                    refresh: true,
                    ..default_task()
                },
            )
            .await;
        assert_eq!(refreshed.cache.kind, CacheKind::Fresh);
        assert_eq!(ffx.model_calls(), base + 4);
        // Custom rows are stored but not reusable; the refreshed one is.
        let hit = ffx.run(agent, &path, default_task()).await;
        assert_eq!(hit.cache.kind, CacheKind::SamePath);
        assert_eq!(hit.cache.response_id, refreshed.cache.response_id);
        assert_eq!(ffx.model_calls(), base + 4);
    }
}

#[tokio::test]
async fn a_narrowed_scope_is_never_served_or_reusable_for_file_agents() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let ffx = FileFx::new(repo.path());
    let path = ffx.fx.default_path();
    let narrowed = ProjectScope {
        root: Path::new(&path.root_path).join("sub"),
        ..scope(&ffx.fx.project, &path)
    };
    for agent in AGENTS {
        let base = ffx.model_calls();
        ffx.run(agent, &path, default_task()).await;
        for expected in [base + 2, base + 3] {
            let run = ffx.run_scoped(agent, &narrowed, default_task()).await;
            assert_eq!(run.cache.kind, CacheKind::Fresh);
            assert_eq!(ffx.model_calls(), expected);
        }
        let rows = ffx.rows(agent, &path);
        assert!(
            rows.iter().any(|row| !row.meta().reusable),
            "{agent:?}: narrowed rows are not reusable"
        );
        let wide = ffx.run(agent, &path, default_task()).await;
        let served = rows
            .iter()
            .find(|row| row.meta().id == wide.cache.response_id)
            .unwrap();
        assert!(served.meta().reusable, "{agent:?}");
    }
}

#[tokio::test]
async fn a_head_change_during_the_run_makes_the_row_not_reusable() {
    if !git_available() {
        return;
    }
    for agent in AGENTS {
        let repo = file_repo();
        let root = repo.path().to_path_buf();
        let hook: Hook = Arc::new(move || {
            git(
                &root,
                DAY3,
                &["commit", "-q", "--allow-empty", "-m", "Mid-run"],
            )
        });
        let ffx = FileFx::with_hook(repo.path(), Some(hook));
        let path = ffx.fx.default_path();
        let run = ffx.run(agent, &path, default_task()).await;
        assert_eq!(run.cache.kind, CacheKind::Fresh);
        let rows = ffx.rows(agent, &path);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].meta().reusable, "{agent:?}");
        let again = ffx.run(agent, &path, default_task()).await;
        assert_eq!(again.cache.kind, CacheKind::Fresh, "{agent:?}");
        assert_eq!(ffx.model_calls(), 2);
    }
}

#[tokio::test]
async fn unusable_and_input_less_file_rows_are_misses_and_are_kept() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let ffx = FileFx::new(repo.path());
    let path = ffx.fx.default_path();
    let db = ffx.fx._db.path().join("daftprompt.db");
    for agent in AGENTS {
        let base = ffx.model_calls();
        let first = ffx.run(agent, &path, default_task()).await;
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute(
                "UPDATE project_agent_response SET response_version = 99 WHERE id = ?1",
                [&first.cache.response_id],
            )
            .unwrap();
        let second = ffx.run(agent, &path, default_task()).await;
        assert_eq!(second.cache.kind, CacheKind::Fresh);
        assert_eq!(ffx.model_calls(), base + 2);
        assert!(matches!(
            ffx.fx
                .store
                .get_agent_response(&first.cache.response_id)
                .unwrap(),
            ResponseOutcome::Unusable(_)
        ));

        // A row whose inputs were removed (as an older build wrote them)
        // is a candidate that cannot be verified.
        let details: String = connection
            .query_row(
                "SELECT details_json FROM project_agent_response WHERE id = ?1",
                [&second.cache.response_id],
                |row| row.get(0),
            )
            .unwrap();
        let mut details: Value = serde_json::from_str(&details).unwrap();
        details.as_object_mut().unwrap().remove("reuse_inputs");
        connection
            .execute(
                "UPDATE project_agent_response SET details_json = ?1 WHERE id = ?2",
                rusqlite::params![details.to_string(), second.cache.response_id],
            )
            .unwrap();
        let third = ffx.run(agent, &path, default_task()).await;
        assert_eq!(third.cache.kind, CacheKind::Fresh);
        assert_eq!(ffx.model_calls(), base + 3);
        assert_eq!(ffx.rows(agent, &path).len(), 3);
        // The new row is served.
        let hit = ffx.run(agent, &path, default_task()).await;
        assert_eq!(hit.cache.response_id, third.cache.response_id);
    }
}

#[test]
fn file_agent_inputs_round_trip_and_record_the_calls_and_limits() {
    if !git_available() {
        return;
    }
    let repo = file_repo();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let ffx = FileFx::new(repo.path());
        let path = ffx.fx.default_path();
        for agent in AGENTS {
            ffx.run(agent, &path, default_task()).await;
            let ResponseOutcome::Usable(row) = ffx.rows(agent, &path).remove(0) else {
                panic!("unusable");
            };
            let inputs = row.response.details.reuse_inputs.unwrap();
            let json = serde_json::to_string(&inputs).unwrap();
            assert_eq!(
                serde_json::from_str::<ReuseInputsV1>(&json).unwrap(),
                inputs
            );
            assert!(
                !json.contains(repo.path().to_str().unwrap()),
                "no absolute root"
            );
            let files = match &inputs {
                ReuseInputsV1::Surveyor(files) => files,
                ReuseInputsV1::Operator {
                    files,
                    working_dirs,
                    ..
                } => {
                    assert_eq!(working_dirs, &["sub".to_owned()]);
                    files
                }
                other => panic!("unexpected {other:?}"),
            };
            // Only the model's own six calls: the preload is a fingerprint.
            let tools: Vec<_> = files.model_calls.iter().map(|c| c.tool.as_str()).collect();
            assert_eq!(
                tools,
                [
                    "read_file",
                    "list_files",
                    "search",
                    "read_file",
                    "read_file",
                    "read_file"
                ]
            );
            assert_eq!(files.tool_limits["max_read_lines"], 300);
        }
    });
}
