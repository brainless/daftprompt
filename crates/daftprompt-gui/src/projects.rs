//! Project selection state for the sidebar picker.
//!
//! Pure logic, like `view.rs` and `clarify.rs`: no akar, no threads. The
//! registry itself lives behind the API (`GET/POST /projects`); this module
//! only tracks which project the GUI has selected, the list it last fetched,
//! and what to say when a project action fails. `main.rs` runs the HTTP calls
//! on worker threads and feeds their outcomes back through [`ProjectMessage`].
//!
//! The selected project is never stored by the GUI. On launch the most
//! recently opened project from the API is preselected, which is how the last
//! choice survives a restart.

use std::path::{Path, PathBuf};

use daftprompt_types::Project;

use crate::api::ApiError;
use crate::view::truncate_for_display;

/// Projects shown in the picker; the API returns them most recently opened first.
pub const MENU_PROJECT_LIMIT: usize = 8;

/// A result a worker thread reports back to the render loop.
pub enum ProjectMessage {
    /// `GET /projects`.
    Listed(Result<Vec<Project>, ApiError>),
    /// `POST /projects/{id}/open`, or `POST /projects` followed by open. The
    /// token identifies the selection request that started it.
    Selected {
        token: u64,
        outcome: Result<Project, ApiError>,
    },
    /// The native folder dialog closed; `None` when it was cancelled.
    Folder(Option<PathBuf>),
}

/// How serious a sidebar message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Error,
    Info,
}

/// A short, non-blocking message about a project action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

/// What a picker entry does when chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Clear,
    Select(String),
    ChooseFolder,
}

/// One row of the picker list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub action: MenuAction,
    pub title: String,
    /// The project folder, shortened from the left.
    pub subtitle: Option<String>,
    pub selected: bool,
}

/// What dropping items on the window should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAction {
    /// Register this folder and select it.
    Register(PathBuf),
    /// Only files were dropped; say so and do nothing else.
    Rejected(String),
    /// Nothing was dropped.
    Ignore,
}

/// Pick what to do with dropped paths: the first directory wins; with only
/// files there is nothing to register. `is_dir` is injected so tests need no
/// file system.
pub fn classify_drop(paths: &[PathBuf], is_dir: impl Fn(&Path) -> bool) -> DropAction {
    let Some(first) = paths.first() else {
        return DropAction::Ignore;
    };
    if let Some(dir) = paths.iter().find(|path| is_dir(path)) {
        return DropAction::Register(dir.clone());
    }
    let name = first.file_name().map_or_else(
        || first.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    DropAction::Rejected(format!(
        "{name} is a file. Drop a folder to use it as the project."
    ))
}

/// Selection state and the last fetched project list.
#[derive(Debug, Default)]
pub struct Projects {
    list: Vec<Project>,
    current: Option<Project>,
    /// The user picked something, so the startup preselection must not run.
    chosen: bool,
    /// The latest selection request; older replies are ignored.
    token: u64,
    /// Selection requests whose reply has not arrived.
    selecting: u32,
    /// Worker threads (including the folder dialog) still running.
    jobs: u32,
    notice: Option<Notice>,
}

impl Projects {
    pub fn current(&self) -> Option<&Project> {
        self.current.as_ref()
    }

    /// The id every new parse or clarify request carries.
    pub fn current_id(&self) -> Option<&str> {
        self.current.as_ref().map(|project| project.id.as_str())
    }

    #[cfg(test)]
    pub fn list(&self) -> &[Project] {
        &self.list
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    pub fn set_info(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            kind: NoticeKind::Info,
            text: text.into(),
        });
    }

    pub fn set_error(&mut self, error: &ApiError) {
        self.notice = Some(Notice {
            kind: NoticeKind::Error,
            text: error.to_string(),
        });
    }

    /// Whether any project work is outstanding (the loop then polls).
    pub fn is_busy(&self) -> bool {
        self.jobs > 0
    }

    pub fn job_started(&mut self) {
        self.jobs += 1;
    }

    pub fn job_finished(&mut self) {
        self.jobs = self.jobs.saturating_sub(1);
    }

    /// The picker's button text.
    pub fn label(&self) -> &str {
        self.current
            .as_ref()
            .map_or("No project", |project| project.name.as_str())
    }

    /// Start a selection request and return its token.
    pub fn begin_select(&mut self) -> u64 {
        self.token = self.token.wrapping_add(1);
        self.selecting += 1;
        self.notice = None;
        self.token
    }

    /// Apply a selection reply. Returns whether it became the current project;
    /// a reply from a request that was superseded (or cleared) is ignored, and
    /// a failure keeps the previous selection and records the error.
    pub fn finish_select(&mut self, token: u64, outcome: Result<Project, ApiError>) -> bool {
        self.selecting = self.selecting.saturating_sub(1);
        if token != self.token {
            return false;
        }
        match outcome {
            Ok(project) => {
                self.list.retain(|known| known.id != project.id);
                self.list.insert(0, project.clone());
                self.current = Some(project);
                self.chosen = true;
                true
            }
            Err(error) => {
                self.set_error(&error);
                false
            }
        }
    }

    /// Choose "No project". Supersedes any selection still in flight.
    pub fn clear(&mut self) {
        self.token = self.token.wrapping_add(1);
        self.current = None;
        self.chosen = true;
        self.notice = None;
    }

    /// Apply `GET /projects`. When nothing has been chosen yet the most
    /// recently opened project (the first) is preselected. A project that
    /// vanished from the registry is deselected, unless a selection is still
    /// in flight and the list may predate it.
    pub fn apply_list(&mut self, outcome: Result<Vec<Project>, ApiError>) {
        let list = match outcome {
            Ok(list) => list,
            Err(error) => {
                self.set_error(&error);
                return;
            }
        };
        if !self.chosen && self.current.is_none() {
            self.current = list.first().cloned();
        } else if self.selecting == 0
            && let Some(current) = &self.current
            && let Some(fresh) = list.iter().find(|project| project.id == current.id)
        {
            self.current = Some(fresh.clone());
        } else if self.selecting == 0 && self.current.is_some() {
            self.current = None;
            self.set_info("The selected project is no longer registered.");
        }
        self.list = list;
    }

    /// Whether a failed parse or clarify says the project list is out of date.
    pub fn error_means_stale(error: &ApiError) -> bool {
        matches!(error.kind.as_str(), "not_found" | "project_unavailable")
    }

    /// The picker rows: "No project", the most recent projects, then the
    /// folder chooser.
    pub fn entries(&self) -> Vec<MenuEntry> {
        let current_id = self.current_id();
        let mut entries = vec![MenuEntry {
            action: MenuAction::Clear,
            title: "No project".to_owned(),
            subtitle: None,
            selected: current_id.is_none(),
        }];
        entries.extend(
            self.list
                .iter()
                .take(MENU_PROJECT_LIMIT)
                .map(|project| MenuEntry {
                    action: MenuAction::Select(project.id.clone()),
                    title: truncate_for_display(&project.name, 34),
                    subtitle: Some(shorten_path(&project.root_path, 40)),
                    selected: current_id == Some(project.id.as_str()),
                }),
        );
        entries.push(MenuEntry {
            action: MenuAction::ChooseFolder,
            title: "Choose folder…".to_owned(),
            subtitle: None,
            selected: false,
        });
        entries
    }
}

/// Keep the end of a long path, which is the part that tells projects apart.
pub fn shorten_path(path: &str, max_chars: usize) -> String {
    let count = path.chars().count();
    if count <= max_chars || max_chars < 2 {
        return path.to_owned();
    }
    let tail: String = path.chars().skip(count - (max_chars - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, name: &str) -> Project {
        Project {
            id: id.to_owned(),
            name: name.to_owned(),
            root_path: format!("/work/{name}"),
            created_at: "t0".to_owned(),
            last_opened_at: "t1".to_owned(),
        }
    }

    fn api_error(kind: &str) -> ApiError {
        ApiError::new(kind, "details")
    }

    #[test]
    fn starts_with_no_project_selected() {
        let projects = Projects::default();
        assert_eq!(projects.current_id(), None);
        assert_eq!(projects.label(), "No project");
    }

    #[test]
    fn the_most_recent_project_is_preselected_on_load() {
        let mut projects = Projects::default();
        projects.apply_list(Ok(vec![project("b", "beta"), project("a", "alpha")]));
        assert_eq!(projects.current_id(), Some("b"));
        assert_eq!(projects.label(), "beta");
    }

    #[test]
    fn an_empty_registry_leaves_no_project() {
        let mut projects = Projects::default();
        projects.apply_list(Ok(Vec::new()));
        assert_eq!(projects.current_id(), None);
    }

    #[test]
    fn a_user_choice_made_before_the_list_arrives_is_kept() {
        let mut projects = Projects::default();
        projects.clear();
        projects.apply_list(Ok(vec![project("b", "beta")]));
        assert_eq!(projects.current_id(), None);

        let mut projects = Projects::default();
        let token = projects.begin_select();
        assert!(projects.finish_select(token, Ok(project("a", "alpha"))));
        projects.apply_list(Ok(vec![project("b", "beta"), project("a", "alpha")]));
        assert_eq!(projects.current_id(), Some("a"));
    }

    #[test]
    fn selecting_makes_the_project_current_and_most_recent() {
        let mut projects = Projects::default();
        projects.apply_list(Ok(vec![project("b", "beta"), project("a", "alpha")]));
        let token = projects.begin_select();
        assert!(projects.finish_select(token, Ok(project("a", "alpha"))));
        assert_eq!(projects.current_id(), Some("a"));
        let ids: Vec<_> = projects.list().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn a_new_project_is_added_to_the_list() {
        let mut projects = Projects::default();
        let token = projects.begin_select();
        projects.finish_select(token, Ok(project("n", "new")));
        assert_eq!(projects.list().len(), 1);
        assert_eq!(projects.label(), "new");
    }

    #[test]
    fn clearing_deselects_and_cancels_a_pending_selection() {
        let mut projects = Projects::default();
        let token = projects.begin_select();
        projects.clear();
        assert!(!projects.finish_select(token, Ok(project("a", "alpha"))));
        assert_eq!(projects.current_id(), None);
    }

    #[test]
    fn only_the_latest_selection_wins() {
        let mut projects = Projects::default();
        let first = projects.begin_select();
        let second = projects.begin_select();
        assert!(projects.finish_select(second, Ok(project("b", "beta"))));
        assert!(!projects.finish_select(first, Ok(project("a", "alpha"))));
        assert_eq!(projects.current_id(), Some("b"));
    }

    #[test]
    fn a_failed_selection_keeps_the_previous_project_and_reports() {
        let mut projects = Projects::default();
        let token = projects.begin_select();
        projects.finish_select(token, Ok(project("a", "alpha")));
        let token = projects.begin_select();
        assert!(!projects.finish_select(token, Err(api_error("not_found"))));
        assert_eq!(projects.current_id(), Some("a"));
        let notice = projects.notice().expect("an error notice");
        assert_eq!(notice.kind, NoticeKind::Error);
        assert!(notice.text.contains("not registered"));
        // The next action starts clean.
        projects.begin_select();
        assert!(projects.notice().is_none());
    }

    #[test]
    fn a_vanished_project_is_deselected_on_refresh() {
        let mut projects = Projects::default();
        let token = projects.begin_select();
        projects.finish_select(token, Ok(project("a", "alpha")));
        projects.apply_list(Ok(vec![project("b", "beta")]));
        assert_eq!(projects.current_id(), None);
        assert!(projects.notice().is_some());
    }

    #[test]
    fn a_list_fetched_before_a_pending_selection_does_not_undo_it() {
        let mut projects = Projects::default();
        let token = projects.begin_select();
        projects.finish_select(token, Ok(project("a", "alpha")));
        let _pending = projects.begin_select();
        projects.apply_list(Ok(vec![project("b", "beta")]));
        assert_eq!(projects.current_id(), Some("a"));
    }

    #[test]
    fn a_failed_list_reports_and_changes_nothing() {
        let mut projects = Projects::default();
        projects.apply_list(Err(ApiError::connection("refused")));
        assert_eq!(projects.current_id(), None);
        assert_eq!(projects.notice().map(|n| n.kind), Some(NoticeKind::Error));
    }

    #[test]
    fn menu_lists_no_project_projects_then_the_folder_chooser() {
        let mut projects = Projects::default();
        projects.apply_list(Ok(vec![project("b", "beta"), project("a", "alpha")]));
        let entries = projects.entries();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].action, MenuAction::Clear);
        assert!(!entries[0].selected);
        assert_eq!(entries[1].action, MenuAction::Select("b".to_owned()));
        assert!(entries[1].selected);
        assert_eq!(entries[1].subtitle.as_deref(), Some("/work/beta"));
        assert_eq!(entries[2].action, MenuAction::Select("a".to_owned()));
        assert_eq!(entries[3].action, MenuAction::ChooseFolder);
        assert_eq!(entries[3].title, "Choose folder…");

        projects.clear();
        assert!(projects.entries()[0].selected);
        assert!(!projects.entries()[1].selected);
    }

    #[test]
    fn menu_shows_only_the_most_recent_projects() {
        let mut projects = Projects::default();
        let many = (0..20).map(|i| project(&format!("p{i}"), &format!("n{i}")));
        projects.apply_list(Ok(many.collect()));
        assert_eq!(projects.entries().len(), MENU_PROJECT_LIMIT + 2);
    }

    #[test]
    fn long_paths_keep_their_tail() {
        assert_eq!(shorten_path("/a/b", 10), "/a/b");
        let shortened = shorten_path("/Users/me/code/some/deep/project", 12);
        assert_eq!(shortened.chars().count(), 12);
        assert!(shortened.starts_with('…'));
        assert!(shortened.ends_with("p/project"));
    }

    #[test]
    fn a_dropped_directory_is_registered() {
        let paths = [PathBuf::from("/work/app")];
        let action = classify_drop(&paths, |_| true);
        assert_eq!(action, DropAction::Register(PathBuf::from("/work/app")));
    }

    #[test]
    fn a_dropped_file_is_rejected_with_a_message() {
        let paths = [PathBuf::from("/work/notes.txt")];
        match classify_drop(&paths, |_| false) {
            DropAction::Rejected(message) => assert!(message.contains("notes.txt")),
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn several_items_use_the_first_directory() {
        let paths = [
            PathBuf::from("/work/a.txt"),
            PathBuf::from("/work/one"),
            PathBuf::from("/work/two"),
        ];
        let action = classify_drop(&paths, |path| !path.to_string_lossy().ends_with(".txt"));
        assert_eq!(action, DropAction::Register(PathBuf::from("/work/one")));
    }

    #[test]
    fn an_empty_drop_is_ignored() {
        assert_eq!(classify_drop(&[], |_| true), DropAction::Ignore);
    }

    #[test]
    fn stale_project_errors_ask_for_a_refresh() {
        assert!(Projects::error_means_stale(&api_error("not_found")));
        assert!(Projects::error_means_stale(&api_error(
            "project_unavailable"
        )));
        assert!(!Projects::error_means_stale(&api_error("forbidden")));
        assert!(!Projects::error_means_stale(&api_error("connection")));
    }
}
