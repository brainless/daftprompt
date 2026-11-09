//! Project briefing state: the three agent requests a project triggers.
//!
//! Pure logic, like `projects.rs`: no akar, no threads. When a project becomes
//! current, [`BriefingState::open`] puts the Chronicler, Surveyor and Operator
//! slots into `Loading` and returns one [`Request`] per slot; `main.rs` sends
//! them concurrently on worker threads and feeds each reply back through
//! [`BriefingState::apply`]. A reply whose project, generation or per-slot
//! sequence no longer matches is dropped, so a late answer for a previous
//! project or path (or for a section that was re-run since) never shows. The
//! GUI caches nothing itself: every `open` starts from scratch, and a reply may
//! be a stored response the API verified (its [`CacheDelivery`] is kept for
//! display).

use daftprompt_types::{BriefingResult, CacheDelivery, CommandsResult, Delivered};

use crate::api::ApiError;
use crate::projects::Stamp;

/// One section of the briefing, each backed by one agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Chronicler: recent work from git history.
    History,
    /// Surveyor: what the project is and how it is laid out.
    Overview,
    /// Operator: the commands the project supports.
    Commands,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::History, Section::Overview, Section::Commands];

    fn index(self) -> usize {
        match self {
            Section::History => 0,
            Section::Overview => 1,
            Section::Commands => 2,
        }
    }
}

/// The state of one request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Slot<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(ApiError),
}

impl<T> Slot<T> {
    pub fn is_loading(&self) -> bool {
        matches!(self, Slot::Loading)
    }
}

/// The briefing of the current project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Briefing {
    pub project_id: String,
    /// The path the briefing was requested for.
    pub path_id: Option<String>,
    /// Increases every time a project becomes current.
    pub generation: u64,
    pub history: Slot<BriefingResult>,
    pub overview: Slot<BriefingResult>,
    pub commands: Slot<CommandsResult>,
    /// The latest request number per section; older replies are ignored.
    seqs: [u64; 3],
    /// How each ready section reached the GUI; `None` while loading or when
    /// the API sent no metadata.
    caches: [Option<CacheDelivery>; 3],
}

impl Briefing {
    /// The delivery metadata of a section's current result.
    pub fn cache(&self, section: Section) -> Option<&CacheDelivery> {
        self.caches[section.index()].as_ref()
    }
}

/// One request to send, stamped so its reply can be matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub section: Section,
    pub project_id: String,
    pub path_id: Option<String>,
    pub generation: u64,
    pub seq: u64,
    /// Set only by the explicit re-run controls; an ordinary open may reuse a
    /// stored response.
    pub refresh: bool,
}

/// A successful reply, by agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Summary(Delivered<BriefingResult>),
    Commands(Delivered<CommandsResult>),
}

/// What a worker thread reports back.
pub struct BriefingMessage {
    pub request: Request,
    pub outcome: Result<Reply, ApiError>,
}

/// The main column's two tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Project,
    Parse,
}

/// What the details area shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Details {
    /// One command, by `ProjectCommand::id`.
    Command(String),
    /// Every command as a list (the footer's "+N more").
    List,
}

/// The briefing, the active tab and the open details area.
#[derive(Debug)]
pub struct BriefingState {
    briefing: Option<Briefing>,
    generation: u64,
    tab: Tab,
    details: Option<Details>,
    /// Bumped on every change a view might need to rebuild for.
    revision: u64,
}

impl Default for BriefingState {
    fn default() -> Self {
        Self {
            briefing: None,
            generation: 0,
            tab: Tab::Project,
            details: None,
            revision: 0,
        }
    }
}

impl BriefingState {
    pub fn briefing(&self) -> Option<&Briefing> {
        self.briefing.as_ref()
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The user clicked a tab. Closes the details area.
    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.details = None;
        self.revision += 1;
    }

    /// A parse was submitted: show its result.
    pub fn parse_submitted(&mut self) {
        self.set_tab(Tab::Parse);
    }

    pub fn details(&self) -> Option<&Details> {
        self.details.as_ref()
    }

    pub fn open_details(&mut self, details: Details) {
        self.details = Some(details);
        self.revision += 1;
    }

    pub fn close_details(&mut self) {
        if self.details.take().is_some() {
            self.revision += 1;
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether any request is still in flight (the loop then polls).
    pub fn is_loading(&self) -> bool {
        self.briefing.as_ref().is_some_and(|b| {
            b.history.is_loading() || b.overview.is_loading() || b.commands.is_loading()
        })
    }

    /// A project or path became current: switch to the Project tab, discard
    /// the old briefing, and return the three requests to send concurrently.
    pub fn open(&mut self, stamp: &Stamp) -> Vec<Request> {
        self.generation += 1;
        self.briefing = Some(Briefing {
            project_id: stamp.project_id.clone(),
            path_id: stamp.path_id.clone(),
            generation: self.generation,
            history: Slot::Loading,
            overview: Slot::Loading,
            commands: Slot::Loading,
            seqs: [1; 3],
            caches: [None, None, None],
        });
        self.tab = Tab::Project;
        self.details = None;
        self.revision += 1;
        Section::ALL
            .into_iter()
            .map(|section| Request {
                section,
                project_id: stamp.project_id.clone(),
                path_id: stamp.path_id.clone(),
                generation: self.generation,
                seq: 1,
                refresh: false,
            })
            .collect()
    }

    /// "No project": drop the briefing; replies still in flight become stale.
    pub fn clear(&mut self) {
        self.generation += 1;
        self.briefing = None;
        self.details = None;
        self.revision += 1;
    }

    /// Follow the selection: open a briefing when a different project or
    /// path is current, clear it when none is. Nothing happens when the
    /// briefing already belongs to `current`.
    pub fn sync(&mut self, current: Option<&Stamp>) -> Vec<Request> {
        let have = self.briefing.as_ref().map(|b| (&b.project_id, &b.path_id));
        match (current, have) {
            (Some(stamp), have) if have != Some((&stamp.project_id, &stamp.path_id)) => {
                self.open(stamp)
            }
            (None, Some(_)) => {
                self.clear();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Re-send one section's request, leaving the others alone. `None` when
    /// there is no briefing or that section is already loading.
    pub fn rerun(&mut self, section: Section) -> Option<Request> {
        let briefing = self.briefing.as_mut()?;
        let loading = match section {
            Section::History => briefing.history.is_loading(),
            Section::Overview => briefing.overview.is_loading(),
            Section::Commands => briefing.commands.is_loading(),
        };
        if loading {
            return None;
        }
        match section {
            Section::History => briefing.history = Slot::Loading,
            Section::Overview => briefing.overview = Slot::Loading,
            Section::Commands => {
                briefing.commands = Slot::Loading;
                self.details = None;
            }
        }
        briefing.seqs[section.index()] += 1;
        briefing.caches[section.index()] = None;
        self.revision += 1;
        Some(Request {
            section,
            project_id: briefing.project_id.clone(),
            path_id: briefing.path_id.clone(),
            generation: briefing.generation,
            seq: briefing.seqs[section.index()],
            refresh: true,
        })
    }

    /// Apply a reply. Returns whether it was current; a reply for another
    /// project or path, an older generation, or a section re-run since is
    /// dropped.
    pub fn apply(&mut self, message: BriefingMessage) -> bool {
        let BriefingMessage { request, outcome } = message;
        let Some(briefing) = self.briefing.as_mut() else {
            return false;
        };
        if briefing.project_id != request.project_id
            || briefing.path_id != request.path_id
            || briefing.generation != request.generation
            || briefing.seqs[request.section.index()] != request.seq
        {
            return false;
        }
        let mismatch = || ApiError::decode("the API answered with a different kind of result");
        match (request.section, outcome) {
            (Section::History, Ok(Reply::Summary(delivered))) => {
                briefing.caches[request.section.index()] = delivered.cache;
                briefing.history = Slot::Ready(delivered.result);
            }
            (Section::Overview, Ok(Reply::Summary(delivered))) => {
                briefing.caches[request.section.index()] = delivered.cache;
                briefing.overview = Slot::Ready(delivered.result);
            }
            (Section::Commands, Ok(Reply::Commands(delivered))) => {
                briefing.caches[request.section.index()] = delivered.cache;
                briefing.commands = Slot::Ready(delivered.result);
            }
            (Section::History, Err(error)) => briefing.history = Slot::Failed(error),
            (Section::Overview, Err(error)) => briefing.overview = Slot::Failed(error),
            (Section::Commands, Err(error)) => briefing.commands = Slot::Failed(error),
            (Section::History, Ok(_)) => briefing.history = Slot::Failed(mismatch()),
            (Section::Overview, Ok(_)) => briefing.overview = Slot::Failed(mismatch()),
            (Section::Commands, Ok(_)) => briefing.commands = Slot::Failed(mismatch()),
        }
        self.revision += 1;
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use daftprompt_types::{AgentTask, Payload, Run, RunStatus};

    pub(crate) fn run() -> Run {
        Run {
            id: "run".into(),
            agent: "chronicler".into(),
            provider: Some("p".into()),
            model: Some("m".into()),
            prompt_version: Some("v".into()),
            started_at: "t".into(),
            ended_at: Some("t".into()),
            status: RunStatus::Done,
            attempts: 1,
            consistency_followup: false,
            consistency_used: false,
            project_id: None,
        }
    }

    pub(crate) fn briefing_result(project: &str, summary: &str) -> BriefingResult {
        BriefingResult {
            project_id: project.into(),
            task: AgentTask::Default,
            prompt_payload: None,
            summary_payload: Payload {
                hash: "h".into(),
                content: summary.into(),
                created_at: "t".into(),
            },
            summary_node_id: "s".into(),
            nodes: Vec::new(),
            node_contexts: Vec::new(),
            citations: Vec::new(),
            notices: Vec::new(),
            profile: None,
            run: run(),
            run_inputs: Vec::new(),
            tool_calls: Vec::new(),
            tool_payloads: Vec::new(),
        }
    }

    pub(crate) fn stamp(project: &str) -> Stamp {
        Stamp {
            project_id: project.into(),
            path_id: None,
        }
    }

    fn path_stamp(project: &str, path: &str) -> Stamp {
        Stamp {
            project_id: project.into(),
            path_id: Some(path.into()),
        }
    }

    pub(crate) fn delivered<T>(result: T) -> Delivered<T> {
        Delivered {
            result,
            project_path_id: None,
            cache: None,
        }
    }

    fn reply(request: &Request, outcome: Result<Reply, ApiError>) -> BriefingMessage {
        BriefingMessage {
            request: request.clone(),
            outcome,
        }
    }

    fn summary(project: &str) -> Result<Reply, ApiError> {
        Ok(Reply::Summary(delivered(briefing_result(project, "text"))))
    }

    fn commands_reply(project: &str) -> Result<Reply, ApiError> {
        Ok(Reply::Commands(delivered(CommandsResult {
            briefing: briefing_result(project, "text"),
            commands: Vec::new(),
        })))
    }

    #[test]
    fn opening_a_project_starts_all_three_slots_loading() {
        let mut state = BriefingState::default();
        assert!(state.briefing().is_none());
        let requests = state.open(&stamp("p1"));
        assert_eq!(requests.len(), 3);
        let sections: Vec<_> = requests.iter().map(|r| r.section).collect();
        assert_eq!(sections, Section::ALL);
        let b = state.briefing().expect("a briefing");
        assert_eq!(b.project_id, "p1");
        assert!(b.history.is_loading() && b.overview.is_loading() && b.commands.is_loading());
        assert!(state.is_loading());
    }

    #[test]
    fn replies_fill_their_own_slots() {
        let mut state = BriefingState::default();
        let requests = state.open(&stamp("p1"));
        assert!(state.apply(reply(&requests[0], summary("p1"))));
        assert!(state.apply(reply(&requests[2], commands_reply("p1"))));
        assert!(state.apply(reply(&requests[1], Err(ApiError::new("timeout", "slow")))));
        let b = state.briefing().unwrap();
        assert!(matches!(b.history, Slot::Ready(_)));
        assert!(matches!(b.overview, Slot::Failed(_)));
        assert!(matches!(b.commands, Slot::Ready(_)));
        assert!(!state.is_loading());
    }

    #[test]
    fn stale_replies_are_dropped() {
        let mut state = BriefingState::default();
        let old = state.open(&stamp("p1"));
        let new = state.open(&stamp("p2"));
        // Another project and an older generation.
        assert!(!state.apply(reply(&old[0], summary("p1"))));
        assert!(state.briefing().unwrap().history.is_loading());
        // The same project opened again is a new generation too.
        let again = state.open(&stamp("p2"));
        assert!(!state.apply(reply(&new[0], summary("p2"))));
        assert!(state.apply(reply(&again[0], summary("p2"))));
    }

    #[test]
    fn a_reply_after_clearing_is_dropped() {
        let mut state = BriefingState::default();
        let requests = state.open(&stamp("p1"));
        state.clear();
        assert!(!state.apply(reply(&requests[0], summary("p1"))));
        assert!(state.briefing().is_none());
        assert!(!state.is_loading());
    }

    #[test]
    fn rerun_affects_only_its_own_slot() {
        let mut state = BriefingState::default();
        let requests = state.open(&stamp("p1"));
        for request in &requests {
            let outcome = match request.section {
                Section::Commands => commands_reply("p1"),
                _ => summary("p1"),
            };
            state.apply(reply(request, outcome));
        }
        let again = state.rerun(Section::Overview).expect("a request");
        assert_eq!(again.section, Section::Overview);
        assert_eq!(again.generation, requests[0].generation);
        let b = state.briefing().unwrap();
        assert!(b.overview.is_loading());
        assert!(matches!(b.history, Slot::Ready(_)));
        assert!(matches!(b.commands, Slot::Ready(_)));
        // Already loading: refused.
        assert!(state.rerun(Section::Overview).is_none());
        // The earlier overview reply (same generation, older seq) is stale.
        assert!(!state.apply(reply(&requests[1], summary("p1"))));
        assert!(state.apply(reply(&again, summary("p1"))));
    }

    #[test]
    fn rerun_without_a_briefing_does_nothing() {
        let mut state = BriefingState::default();
        assert!(state.rerun(Section::History).is_none());
    }

    #[test]
    fn clearing_resets_everything() {
        let mut state = BriefingState::default();
        state.open(&stamp("p1"));
        state.open_details(Details::List);
        state.clear();
        assert!(state.briefing().is_none());
        assert!(state.details().is_none());
        assert!(!state.is_loading());
    }

    #[test]
    fn the_tab_follows_project_open_and_parse_submit() {
        let mut state = BriefingState::default();
        state.set_tab(Tab::Parse);
        state.open(&stamp("p1"));
        assert_eq!(state.tab(), Tab::Project);
        state.parse_submitted();
        assert_eq!(state.tab(), Tab::Parse);
        state.set_tab(Tab::Project);
        assert_eq!(state.tab(), Tab::Project);
    }

    #[test]
    fn opening_or_switching_closes_the_details() {
        let mut state = BriefingState::default();
        state.open(&stamp("p1"));
        state.open_details(Details::Command("c1".into()));
        state.set_tab(Tab::Parse);
        assert!(state.details().is_none());
        state.open_details(Details::List);
        state.open(&stamp("p2"));
        assert!(state.details().is_none());
    }

    #[test]
    fn sync_follows_the_current_project() {
        let mut state = BriefingState::default();
        assert!(state.sync(None).is_empty());
        assert_eq!(state.sync(Some(&stamp("p1"))).len(), 3);
        // Unchanged: nothing is re-sent.
        assert!(state.sync(Some(&stamp("p1"))).is_empty());
        assert_eq!(state.sync(Some(&stamp("p2"))).len(), 3);
        assert!(state.sync(None).is_empty());
        assert!(state.briefing().is_none());
    }

    #[test]
    fn a_wrong_kind_of_reply_fails_the_slot() {
        let mut state = BriefingState::default();
        let requests = state.open(&stamp("p1"));
        assert!(state.apply(reply(&requests[2], summary("p1"))));
        assert!(matches!(
            state.briefing().unwrap().commands,
            Slot::Failed(_)
        ));
    }

    #[test]
    fn requests_carry_the_path_and_only_reruns_refresh() {
        let mut state = BriefingState::default();
        let requests = state.open(&path_stamp("p1", "w1"));
        assert!(
            requests
                .iter()
                .all(|r| r.path_id.as_deref() == Some("w1") && !r.refresh)
        );
        state.apply(reply(&requests[0], summary("p1")));
        let again = state.rerun(Section::History).expect("a request");
        assert_eq!(again.path_id.as_deref(), Some("w1"));
        assert!(again.refresh);
    }

    #[test]
    fn a_path_change_reopens_and_drops_late_replies_from_the_old_path() {
        let mut state = BriefingState::default();
        let old = state.open(&path_stamp("p1", "d"));
        // Same project, other path: a new briefing.
        assert!(state.sync(Some(&path_stamp("p1", "d"))).is_empty());
        let new = state.sync(Some(&path_stamp("p1", "w")));
        assert_eq!(new.len(), 3);
        assert!(state.briefing().unwrap().history.is_loading());
        assert!(!state.apply(reply(&old[0], summary("p1"))));
        assert!(state.briefing().unwrap().history.is_loading());
        assert!(state.apply(reply(&new[0], summary("p1"))));
    }

    #[test]
    fn a_reply_with_the_same_generation_but_another_path_is_dropped() {
        let mut state = BriefingState::default();
        let requests = state.open(&path_stamp("p1", "d"));
        let mut forged = requests[0].clone();
        forged.path_id = Some("w".into());
        assert!(!state.apply(reply(&forged, summary("p1"))));
        assert!(state.apply(reply(&requests[0], summary("p1"))));
    }

    #[test]
    fn cache_metadata_is_kept_per_section_and_cleared_on_rerun() {
        use daftprompt_types::CacheKind;
        let mut state = BriefingState::default();
        let requests = state.open(&path_stamp("p1", "w"));
        let mut reused = delivered(briefing_result("p1", "text"));
        reused.cache = Some(CacheDelivery {
            requested_path_id: "w".into(),
            source_path_id: "d".into(),
            response_id: "r1".into(),
            captured_at: "2026-11-08T10:30:00Z".into(),
            kind: CacheKind::OtherPath,
        });
        state.apply(reply(&requests[0], Ok(Reply::Summary(reused))));
        state.apply(reply(&requests[1], summary("p1")));
        let b = state.briefing().unwrap();
        assert_eq!(
            b.cache(Section::History).map(|c| c.kind),
            Some(CacheKind::OtherPath)
        );
        assert!(b.cache(Section::Overview).is_none());
        state.rerun(Section::History);
        assert!(state.briefing().unwrap().cache(Section::History).is_none());
    }
}
