//! TUI presentation model — pure UI state only.
//!
//! This module contains NO kernel state: task/step/lease/effect/replay state
//! always comes from the adapter snapshot (kernel queries). Everything here
//! is presentation: selection, filters, scroll, input, toasts, screen.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::adapter::{RefreshParams, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Tasks,
    TaskDetail,
    Events,
    Workers,
    Replay,
    System,
    Help,
}

impl Screen {
    pub fn title(self) -> &'static str {
        match self {
            Screen::Dashboard => "DASHBOARD",
            Screen::Tasks => "TASKS",
            Screen::TaskDetail => "TASK DETAIL",
            Screen::Events => "EVENTS",
            Screen::Workers => "WORKERS",
            Screen::Replay => "REPLAY / INTEGRITY",
            Screen::System => "SYSTEM",
            Screen::Help => "HELP",
        }
    }
}

/// Text-input sub-modes. All input is presentation-level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    None,
    /// Typing a task id for submit-task.
    PromptTaskId,
    /// Typing a filter string.
    Filter,
    /// Typing an event-type filter (Events screen).
    EventTypeFilter,
    /// Typing a text query over event fields (Events screen).
    EventTextQuery,
}

/// Actions the UI asks the outside world (main loop/worker) to perform.
/// The UI never touches the kernel directly.
#[derive(Debug, Clone, PartialEq)]
pub enum OutAction {
    None,
    Quit,
    Refresh(RefreshParams),
    SubmitTask(String),
    ScheduleTask(String),
    RebuildSnapshot(String),
}

/// A side-effecting action awaiting explicit operator confirmation.
/// Presentation-only modal state — no canonical state is held here.
/// Submit-task is not listed because it already requires typing a task id
/// and pressing Enter, which is itself a deliberate confirmation.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    ScheduleTask(String),
    RebuildSnapshot(String),
}

impl PendingAction {
    pub fn describe(&self) -> String {
        match self {
            PendingAction::ScheduleTask(id) => format!("schedule task '{id}'"),
            PendingAction::RebuildSnapshot(id) => format!("rebuild snapshot for '{id}'"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Key(KeyEvent),
    /// A fresh kernel snapshot produced by the adapter.
    Data(Snapshot),
    /// A control action completed successfully.
    ActionOk(String),
    /// A control action failed.
    ActionErr(String),
}

pub struct Model {
    pub db_path: String,
    pub screen: Screen,
    pub return_screen: Screen,

    // Selection (presentation only)
    pub tasks_sel: usize,
    pub events_sel: usize,
    pub workers_sel: usize,
    pub replay_sel: usize,
    pub detail_scroll: usize,

    // Filtering (presentation only)
    pub task_filter: String,
    pub event_task_filter: String,
    /// Canonical-state filter for the Tasks screen (exact kernel state
    /// string, e.g. "running"); None = all.
    pub task_state_filter: Option<String>,
    /// Task-class filter for the Tasks screen (exact class from kernel
    /// data); None = all.
    pub task_class_filter: Option<String>,
    /// Event-type substring filter (case-insensitive), Events screen.
    pub event_type_filter: String,
    /// Text query over event task/step/payload (case-insensitive).
    pub event_text_query: String,

    // Refresh state (presentation only)
    /// True when the last refresh failed and the displayed snapshot is the
    /// previous successful one.
    pub stale: bool,
    /// Count of successful refreshes (monotonic presentation indicator —
    /// not kernel state, no wall clock).
    pub refresh_seq: u64,

    // Input
    pub input_mode: InputMode,
    pub input_buffer: String,
    /// Side-effecting action awaiting explicit y/n confirmation.
    pub pending_confirm: Option<PendingAction>,

    // Latest kernel snapshot (source: adapter, never derived here)
    pub data: Snapshot,

    // Transient status line
    pub status_msg: Option<String>,
    pub status_is_error: bool,

    pub quitting: bool,
}

impl Model {
    pub fn new(db_path: impl Into<String>) -> Self {
        Self {
            db_path: db_path.into(),
            screen: Screen::Dashboard,
            return_screen: Screen::Dashboard,
            tasks_sel: 0,
            events_sel: 0,
            workers_sel: 0,
            replay_sel: 0,
            detail_scroll: 0,
            task_filter: String::new(),
            event_task_filter: String::new(),
            task_state_filter: None,
            task_class_filter: None,
            event_type_filter: String::new(),
            event_text_query: String::new(),
            stale: false,
            refresh_seq: 0,
            input_mode: InputMode::None,
            input_buffer: String::new(),
            pending_confirm: None,
            data: Snapshot::default(),
            status_msg: None,
            status_is_error: false,
            quitting: false,
        }
    }

    // ── Filtered projections (display-only) ────────────────────────────

    /// Indices into data.tasks passing all active task filters.
    /// Policy: task-id filter is case-insensitive substring; state and
    /// class filters are exact matches against canonical kernel strings.
    pub fn filtered_task_indices(&self) -> Vec<usize> {
        let needle = self.task_filter.to_lowercase();
        self.data
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                (needle.is_empty() || t.task_id.to_lowercase().contains(&needle))
                    && self
                        .task_state_filter
                        .as_deref()
                        .map(|s| t.state == s)
                        .unwrap_or(true)
                    && self
                        .task_class_filter
                        .as_deref()
                        .map(|c| t.task_class == c)
                        .unwrap_or(true)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Indices into data.events passing the event filters. Filters apply to
    /// the loaded event window (bounded kernel query) — display projection
    /// only, event semantics are unchanged.
    pub fn filtered_event_indices(&self) -> Vec<usize> {
        let type_needle = self.event_type_filter.to_lowercase();
        let text_needle = self.event_text_query.to_lowercase();
        self.data
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                (type_needle.is_empty() || e.event_type.to_lowercase().contains(&type_needle))
                    && (text_needle.is_empty()
                        || e.task_id.to_lowercase().contains(&text_needle)
                        || e.step_id
                            .as_deref()
                            .map(|s| s.to_lowercase().contains(&text_needle))
                            .unwrap_or(false)
                        || e.payload.to_lowercase().contains(&text_needle))
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected_task_id(&self) -> Option<String> {
        match self.screen {
            Screen::TaskDetail => self.detail_task_id(),
            _ => {
                let idx = self.filtered_task_indices();
                idx.get(self.tasks_sel)
                    .and_then(|i| self.data.tasks.get(*i))
                    .map(|t| t.task_id.clone())
            }
        }
    }

    pub fn detail_task_id(&self) -> Option<String> {
        self.data.detail.as_ref().map(|d| d.task_id.clone())
    }

    pub fn selected_replay_task_id(&self) -> Option<String> {
        let idx = self.filtered_task_indices();
        idx.get(self.replay_sel)
            .and_then(|i| self.data.tasks.get(*i))
            .map(|t| t.task_id.clone())
    }

    /// Parameters describing what the background worker should collect.
    /// Periodic refresh never runs the integrity check; only the explicit
    /// 'i' action sets run_integrity.
    pub fn refresh_params(&self) -> RefreshParams {
        RefreshParams {
            selected_task: match self.screen {
                Screen::TaskDetail => self.detail_task_id().or_else(|| self.selected_task_id()),
                _ => self.selected_task_id(),
            },
            event_task_filter: if self.event_task_filter.is_empty() {
                None
            } else {
                Some(self.event_task_filter.clone())
            },
            event_limit: 200,
            replay_task: self.selected_replay_task_id(),
            run_integrity: false,
        }
    }

    fn clamp_selections(&mut self) {
        let n = self.filtered_task_indices().len();
        if n == 0 {
            self.tasks_sel = 0;
            self.replay_sel = 0;
        } else {
            self.tasks_sel = self.tasks_sel.min(n - 1);
            self.replay_sel = self.replay_sel.min(n - 1);
        }
        let ev = self.data.events.len();
        self.events_sel = if ev == 0 {
            0
        } else {
            self.events_sel.min(ev - 1)
        };
        let w = self.data.workers.len();
        self.workers_sel = if w == 0 {
            0
        } else {
            self.workers_sel.min(w - 1)
        };
    }

    fn move_selection(&mut self, delta: i64) {
        let (len, sel) = match self.screen {
            Screen::Tasks => (self.filtered_task_indices().len(), &mut self.tasks_sel),
            Screen::Events => (self.filtered_event_indices().len(), &mut self.events_sel),
            Screen::Workers => (self.data.workers.len(), &mut self.workers_sel),
            Screen::Replay => (self.filtered_task_indices().len(), &mut self.replay_sel),
            Screen::TaskDetail => {
                // scroll the detail pane instead
                let max = self
                    .data
                    .detail
                    .as_ref()
                    .map(|d| d.steps.len() + d.leases.len() + d.events.len())
                    .unwrap_or(0);
                if delta > 0 {
                    self.detail_scroll = (self.detail_scroll + 1).min(max.saturating_sub(1));
                } else {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                }
                return;
            }
            _ => return,
        };
        if len == 0 {
            *sel = 0;
            return;
        }
        let cur = *sel as i64;
        *sel = (cur + delta).clamp(0, len as i64 - 1) as usize;
    }

    fn switch_screen(&mut self, next: Screen) {
        if next == self.screen {
            return;
        }
        if self.screen == Screen::TaskDetail || self.screen == Screen::Help {
            // leaving a sub-screen: remember nothing special
        }
        self.return_screen = self.screen;
        self.screen = next;
    }
}

/// Pure update function. Returns the action the runtime should perform.
pub fn update(model: &mut Model, msg: Msg) -> OutAction {
    match msg {
        Msg::Data(mut snap) => {
            // Refresh-failure retention: a failed refresh must not wipe the
            // operator's view. The previous successful snapshot stays
            // displayable under an explicit error banner (stale marker).
            if snap.error.is_some() {
                // "Stale" only makes sense when a previous successful
                // snapshot exists; a failed FIRST refresh just surfaces the
                // error over an empty view.
                model.stale = model.refresh_seq > 0;
                model.status_is_error = true;
                model.status_msg = snap.error.clone();
            } else {
                // The on-demand integrity result must survive subsequent
                // refreshes that did not request it (run_integrity=false).
                if snap.integrity.is_none() {
                    snap.integrity = model.data.integrity.clone();
                }
                model.data = snap;
                model.stale = false;
                model.refresh_seq += 1;
                if model.status_is_error {
                    model.status_msg = None;
                    model.status_is_error = false;
                }
                model.clamp_selections();
            }
            OutAction::None
        }
        Msg::ActionOk(text) => {
            model.status_msg = Some(text);
            model.status_is_error = false;
            OutAction::Refresh(model.refresh_params())
        }
        Msg::ActionErr(text) => {
            model.status_msg = Some(text);
            model.status_is_error = true;
            OutAction::Refresh(model.refresh_params())
        }
        Msg::Key(key) => handle_key(model, key),
    }
}

fn handle_key(model: &mut Model, key: KeyEvent) -> OutAction {
    // Ctrl-C / Ctrl-Q always quit.
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'))
    {
        model.quitting = true;
        return OutAction::Quit;
    }

    // A pending side-effecting action captures the next key: y confirms,
    // anything else (n/Esc/q/other) cancels without acting. Authorization
    // itself is still enforced below the UI by the kernel; this is only
    // accidental-activation protection.
    if let Some(pending) = model.pending_confirm.take() {
        return match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                model.status_msg = Some(format!("{}…", pending.describe()));
                model.status_is_error = false;
                match pending {
                    PendingAction::ScheduleTask(id) => OutAction::ScheduleTask(id),
                    PendingAction::RebuildSnapshot(id) => OutAction::RebuildSnapshot(id),
                }
            }
            _ => {
                model.status_msg = Some("cancelled".to_string());
                model.status_is_error = false;
                OutAction::None
            }
        };
    }

    // Input modes capture all keys.
    if model.input_mode != InputMode::None {
        return handle_input_key(model, key);
    }

    match key.code {
        KeyCode::Char('q') => {
            model.quitting = true;
            OutAction::Quit
        }
        KeyCode::Char('?') => {
            if model.screen == Screen::Help {
                model.screen = model.return_screen;
            } else {
                model.return_screen = model.screen;
                model.screen = Screen::Help;
            }
            OutAction::None
        }
        KeyCode::Char('1') => goto(model, Screen::Dashboard),
        KeyCode::Char('2') => goto(model, Screen::Tasks),
        KeyCode::Char('3') => goto(model, Screen::Events),
        KeyCode::Char('4') => goto(model, Screen::Workers),
        KeyCode::Char('5') => goto(model, Screen::Replay),
        KeyCode::Char('6') => goto(model, Screen::System),
        KeyCode::Char('j') | KeyCode::Down => {
            model.move_selection(1);
            OutAction::None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            model.move_selection(-1);
            OutAction::None
        }
        KeyCode::Char('r') => OutAction::Refresh(model.refresh_params()),
        KeyCode::Char('/') => {
            model.input_mode = match model.screen {
                Screen::Events => InputMode::Filter,
                _ => InputMode::Filter,
            };
            model.input_buffer.clear();
            OutAction::None
        }
        KeyCode::Esc => {
            match model.screen {
                Screen::TaskDetail => {
                    model.screen = Screen::Tasks;
                }
                Screen::Help => {
                    model.screen = model.return_screen;
                }
                _ => {}
            }
            OutAction::None
        }
        KeyCode::Enter => match model.screen {
            Screen::Tasks => {
                if let Some(task) = model.selected_task_id() {
                    model.return_screen = Screen::Tasks;
                    model.screen = Screen::TaskDetail;
                    model.detail_scroll = 0;
                    // Refresh with the newly selected task detail.
                    let params = RefreshParams {
                        selected_task: Some(task),
                        ..model.refresh_params()
                    };
                    OutAction::Refresh(params)
                } else {
                    OutAction::None
                }
            }
            Screen::Replay => {
                // Re-validate the selected task via the kernel validator.
                OutAction::Refresh(model.refresh_params())
            }
            _ => OutAction::None,
        },
        KeyCode::Char('f') if model.screen == Screen::Tasks => {
            // Cycle canonical-state filter: all -> pending -> running ->
            // completed -> failed -> all. States are exact kernel strings.
            model.task_state_filter = match model.task_state_filter.as_deref() {
                None => Some("pending".to_string()),
                Some("pending") => Some("running".to_string()),
                Some("running") => Some("completed".to_string()),
                Some("completed") => Some("failed".to_string()),
                _ => None,
            };
            model.tasks_sel = 0;
            OutAction::None
        }
        KeyCode::Char('c') if model.screen == Screen::Tasks => {
            // Cycle class filter over the distinct classes present in the
            // current kernel data (sorted for determinism).
            let mut classes: Vec<String> = model
                .data
                .tasks
                .iter()
                .map(|t| t.task_class.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            classes.sort();
            if classes.is_empty() {
                model.task_class_filter = None;
            } else {
                model.task_class_filter = match model
                    .task_class_filter
                    .as_deref()
                    .and_then(|cur| classes.iter().position(|c| c == cur))
                {
                    None => Some(classes[0].clone()),
                    Some(i) if i + 1 < classes.len() => Some(classes[i + 1].clone()),
                    Some(_) => None,
                };
            }
            model.tasks_sel = 0;
            OutAction::None
        }
        KeyCode::Char('f') if model.screen == Screen::Events => {
            model.input_mode = InputMode::EventTypeFilter;
            model.input_buffer = model.event_type_filter.clone();
            OutAction::None
        }
        KeyCode::Char('t') if model.screen == Screen::Events => {
            model.input_mode = InputMode::EventTextQuery;
            model.input_buffer = model.event_text_query.clone();
            OutAction::None
        }
        KeyCode::Char('n') if model.screen == Screen::Tasks => {
            model.input_mode = InputMode::PromptTaskId;
            model.input_buffer.clear();
            OutAction::None
        }
        KeyCode::Char('s') if model.screen == Screen::Tasks => {
            // Side-effecting: require explicit confirmation before acting.
            if let Some(task) = model.selected_task_id() {
                model.pending_confirm = Some(PendingAction::ScheduleTask(task));
                OutAction::None
            } else {
                OutAction::None
            }
        }
        KeyCode::Char('b') if model.screen == Screen::TaskDetail => {
            // Side-effecting: require explicit confirmation before acting.
            if let Some(task) = model.detail_task_id() {
                model.pending_confirm = Some(PendingAction::RebuildSnapshot(task));
                OutAction::None
            } else {
                OutAction::None
            }
        }
        KeyCode::Char('i') if model.screen == Screen::Replay => {
            // On-demand canonical integrity self-check (non-destructive,
            // runs on the background worker via the refresh mechanism).
            let mut params = model.refresh_params();
            params.run_integrity = true;
            OutAction::Refresh(params)
        }
        _ => OutAction::None,
    }
}

fn goto(model: &mut Model, screen: Screen) -> OutAction {
    model.switch_screen(screen);
    OutAction::Refresh(model.refresh_params())
}

fn handle_input_key(model: &mut Model, key: KeyEvent) -> OutAction {
    match key.code {
        KeyCode::Esc => {
            model.input_mode = InputMode::None;
            model.input_buffer.clear();
            OutAction::None
        }
        KeyCode::Enter => {
            let value = model.input_buffer.trim().to_string();
            let mode = model.input_mode;
            model.input_mode = InputMode::None;
            model.input_buffer.clear();
            match mode {
                InputMode::PromptTaskId => {
                    if value.is_empty() {
                        return OutAction::None;
                    }
                    model.status_msg = Some(format!("submitting task {value}…"));
                    model.status_is_error = false;
                    OutAction::SubmitTask(value)
                }
                InputMode::Filter => match model.screen {
                    Screen::Events => {
                        model.event_task_filter = value;
                        model.events_sel = 0;
                        OutAction::Refresh(model.refresh_params())
                    }
                    _ => {
                        model.task_filter = value;
                        model.tasks_sel = 0;
                        OutAction::Refresh(model.refresh_params())
                    }
                },
                InputMode::EventTypeFilter => {
                    model.event_type_filter = value;
                    model.events_sel = 0;
                    OutAction::None
                }
                InputMode::EventTextQuery => {
                    model.event_text_query = value;
                    model.events_sel = 0;
                    OutAction::None
                }
                InputMode::None => OutAction::None,
            }
        }
        KeyCode::Backspace => {
            model.input_buffer.pop();
            OutAction::None
        }
        KeyCode::Char(c) => {
            model.input_buffer.push(c);
            OutAction::None
        }
        _ => OutAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{TaskDetailVm, TaskRowVm};

    fn key(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn model_with_tasks(ids: &[&str]) -> Model {
        let mut m = Model::new("/tmp/test.db");
        m.data.tasks = ids
            .iter()
            .map(|id| TaskRowVm {
                task_id: id.to_string(),
                task_class: "Generic".into(),
                state: "running".into(),
                current_step: "-".into(),
                active_lease_worker: "-".into(),
                latest_generation: 0,
            })
            .collect();
        m
    }

    #[test]
    fn screen_switch_via_number_keys() {
        let mut m = Model::new("/tmp/x.db");
        assert!(matches!(update(&mut m, key('2')), OutAction::Refresh(_)));
        assert_eq!(m.screen, Screen::Tasks);
        assert!(matches!(update(&mut m, key('3')), OutAction::Refresh(_)));
        assert_eq!(m.screen, Screen::Events);
        assert_eq!(m.screen.title(), "EVENTS");
    }

    #[test]
    fn quit_sets_flag_and_returns_quit() {
        let mut m = Model::new("/tmp/x.db");
        assert_eq!(update(&mut m, key('q')), OutAction::Quit);
        assert!(m.quitting);
    }

    #[test]
    fn selection_moves_down_up_and_clamps() {
        let mut m = model_with_tasks(&["a", "b", "c"]);
        m.screen = Screen::Tasks;
        update(&mut m, key('j'));
        assert_eq!(m.tasks_sel, 1);
        update(&mut m, key('j'));
        update(&mut m, key('j')); // clamps at 2
        assert_eq!(m.tasks_sel, 2);
        update(&mut m, key('k'));
        assert_eq!(m.tasks_sel, 1);
    }

    #[test]
    fn enter_on_tasks_opens_detail_and_refreshes() {
        let mut m = model_with_tasks(&["task-1"]);
        m.screen = Screen::Tasks;
        let out = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert_eq!(m.screen, Screen::TaskDetail);
        match out {
            OutAction::Refresh(p) => assert_eq!(p.selected_task.as_deref(), Some("task-1")),
            other => panic!("expected refresh, got {other:?}"),
        }
    }

    #[test]
    fn esc_returns_from_detail_to_tasks() {
        let mut m = model_with_tasks(&["task-1"]);
        m.screen = Screen::TaskDetail;
        update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert_eq!(m.screen, Screen::Tasks);
    }

    #[test]
    fn filter_narrows_task_list() {
        let mut m = model_with_tasks(&["alpha", "beta", "gamma"]);
        m.screen = Screen::Tasks;
        // open filter, type "gam", confirm
        update(&mut m, key('/'));
        assert_eq!(m.input_mode, InputMode::Filter);
        update(&mut m, key('g'));
        update(&mut m, key('a'));
        update(&mut m, key('m'));
        let out = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert_eq!(m.task_filter, "gam");
        assert_eq!(m.filtered_task_indices(), vec![2]);
        assert!(matches!(out, OutAction::Refresh(_)));
    }

    #[test]
    fn filter_input_esc_cancels_without_applying() {
        let mut m = model_with_tasks(&["alpha", "beta"]);
        m.screen = Screen::Tasks;
        update(&mut m, key('/'));
        update(&mut m, key('z'));
        update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert_eq!(m.task_filter, "");
        assert_eq!(m.input_mode, InputMode::None);
    }

    #[test]
    fn submit_task_prompt_emits_action() {
        let mut m = model_with_tasks(&[]);
        m.screen = Screen::Tasks;
        update(&mut m, key('n'));
        assert_eq!(m.input_mode, InputMode::PromptTaskId);
        for c in "new-task".chars() {
            update(&mut m, key(c));
        }
        let out = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert_eq!(out, OutAction::SubmitTask("new-task".into()));
        assert_eq!(m.input_mode, InputMode::None);
    }

    #[test]
    fn schedule_requires_confirmation_then_dispatches_on_y() {
        let mut m = model_with_tasks(&["task-9"]);
        m.screen = Screen::Tasks;
        // 's' must NOT act immediately; it stages a confirmation.
        let out = update(&mut m, key('s'));
        assert_eq!(out, OutAction::None);
        assert_eq!(
            m.pending_confirm,
            Some(PendingAction::ScheduleTask("task-9".into()))
        );
        // 'y' confirms and dispatches the canonical schedule action.
        let out = update(&mut m, key('y'));
        assert_eq!(out, OutAction::ScheduleTask("task-9".into()));
        assert_eq!(m.pending_confirm, None);
    }

    #[test]
    fn schedule_confirmation_cancels_on_n() {
        let mut m = model_with_tasks(&["task-9"]);
        m.screen = Screen::Tasks;
        update(&mut m, key('s'));
        assert!(m.pending_confirm.is_some());
        let out = update(&mut m, key('n'));
        assert_eq!(out, OutAction::None);
        assert_eq!(m.pending_confirm, None);
    }

    #[test]
    fn rebuild_snapshot_requires_confirmation() {
        let mut m = model_with_tasks(&["task-d"]);
        m.screen = Screen::TaskDetail;
        m.data.detail = Some(TaskDetailVm {
            task_id: "task-d".into(),
            ..Default::default()
        });
        let out = update(&mut m, key('b'));
        assert_eq!(out, OutAction::None);
        assert_eq!(
            m.pending_confirm,
            Some(PendingAction::RebuildSnapshot("task-d".into()))
        );
        let out = update(&mut m, key('y'));
        assert_eq!(out, OutAction::RebuildSnapshot("task-d".into()));
    }

    #[test]
    fn integrity_key_on_replay_requests_integrity_refresh() {
        let mut m = model_with_tasks(&["task-r"]);
        m.screen = Screen::Replay;
        let out = update(&mut m, key('i'));
        match out {
            OutAction::Refresh(p) => {
                assert!(p.run_integrity, "'i' must request the integrity check");
                assert_eq!(p.replay_task.as_deref(), Some("task-r"));
            }
            other => panic!("expected refresh with run_integrity, got {other:?}"),
        }
    }

    #[test]
    fn integrity_result_persists_across_non_integrity_refresh() {
        use crate::adapter::IntegrityVm;
        let mut m = model_with_tasks(&["task-r"]);
        // A refresh carrying an integrity result stores it.
        let mut snap = Snapshot::default();
        snap.integrity = Some(IntegrityVm {
            ok: true,
            summary: "ok".into(),
        });
        update(&mut m, Msg::Data(snap));
        assert!(m.data.integrity.is_some());
        // A later refresh WITHOUT run_integrity must keep the last result.
        let snap2 = Snapshot::default();
        update(&mut m, Msg::Data(snap2));
        assert!(
            m.data.integrity.is_some(),
            "integrity result must survive a non-integrity refresh"
        );
    }

    #[test]
    fn action_error_sets_status_and_requests_refresh() {
        let mut m = Model::new("/tmp/x.db");
        let out = update(&mut m, Msg::ActionErr("boom".into()));
        assert_eq!(m.status_msg.as_deref(), Some("boom"));
        assert!(m.status_is_error);
        assert!(matches!(out, OutAction::Refresh(_)));
    }

    #[test]
    fn data_message_replaces_snapshot_and_clamps_selection() {
        let mut m = model_with_tasks(&["a", "b", "c"]);
        m.screen = Screen::Tasks;
        m.tasks_sel = 2;
        let mut snap = Snapshot::default();
        snap.tasks = vec![TaskRowVm {
            task_id: "only".into(),
            task_class: "Generic".into(),
            state: "pending".into(),
            current_step: "-".into(),
            active_lease_worker: "-".into(),
            latest_generation: 0,
        }];
        update(&mut m, Msg::Data(snap));
        assert_eq!(m.tasks_sel, 0, "selection must clamp to the new data");
        assert_eq!(m.data.tasks.len(), 1);
    }

    #[test]
    fn events_filter_goes_into_refresh_params() {
        let mut m = Model::new("/tmp/x.db");
        m.screen = Screen::Events;
        update(&mut m, key('/'));
        for c in "task-7".chars() {
            update(&mut m, key(c));
        }
        update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        let p = m.refresh_params();
        assert_eq!(p.event_task_filter.as_deref(), Some("task-7"));
    }

    #[test]
    fn replay_screen_revalidate_is_refresh() {
        let mut m = model_with_tasks(&["task-r"]);
        m.screen = Screen::Replay;
        let out = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        match out {
            OutAction::Refresh(p) => assert_eq!(p.replay_task.as_deref(), Some("task-r")),
            other => panic!("expected refresh, got {other:?}"),
        }
    }

    #[test]
    fn help_toggle() {
        let mut m = Model::new("/tmp/x.db");
        m.screen = Screen::Tasks;
        update(&mut m, key('?'));
        assert_eq!(m.screen, Screen::Help);
        update(&mut m, key('?'));
        assert_eq!(m.screen, Screen::Tasks);
    }

    #[test]
    fn state_filter_cycles_canonical_states() {
        let mut m = model_with_tasks(&["a", "b"]);
        m.data.tasks[0].state = "running".into();
        m.data.tasks[1].state = "failed".into();
        m.screen = Screen::Tasks;

        assert_eq!(m.task_state_filter, None);
        update(&mut m, key('f'));
        assert_eq!(m.task_state_filter.as_deref(), Some("pending"));
        update(&mut m, key('f'));
        assert_eq!(m.task_state_filter.as_deref(), Some("running"));
        // only the running task passes
        assert_eq!(m.filtered_task_indices(), vec![0]);
        update(&mut m, key('f'));
        assert_eq!(m.task_state_filter.as_deref(), Some("completed"));
        update(&mut m, key('f'));
        assert_eq!(m.task_state_filter.as_deref(), Some("failed"));
        assert_eq!(m.filtered_task_indices(), vec![1]);
        update(&mut m, key('f'));
        assert_eq!(m.task_state_filter, None);
        assert_eq!(m.filtered_task_indices().len(), 2);
    }

    #[test]
    fn class_filter_cycles_over_present_classes_sorted() {
        let mut m = model_with_tasks(&["a", "b"]);
        m.data.tasks[0].task_class = "Generic".into();
        m.data.tasks[1].task_class = "CodeFix".into();
        m.screen = Screen::Tasks;

        update(&mut m, key('c'));
        assert_eq!(m.task_class_filter.as_deref(), Some("CodeFix")); // sorted first
        assert_eq!(m.filtered_task_indices(), vec![1]);
        update(&mut m, key('c'));
        assert_eq!(m.task_class_filter.as_deref(), Some("Generic"));
        assert_eq!(m.filtered_task_indices(), vec![0]);
        update(&mut m, key('c'));
        assert_eq!(m.task_class_filter, None);
        assert_eq!(m.filtered_task_indices().len(), 2);
    }

    #[test]
    fn event_type_and_text_filters_apply_to_loaded_window() {
        use deterministic_ai_kernel::providers::storage::EventDetailRow;
        let mut m = Model::new("/tmp/x.db");
        m.screen = Screen::Events;
        m.data.events = vec![
            EventDetailRow {
                id: 1,
                system_generation: 1,
                causal_unit_id: 1,
                sequence_in_unit: 0,
                task_id: "task-1".into(),
                step_id: Some("00_a".into()),
                event_type: "STEP_STARTED".into(),
                payload: r#"{"worker_id":"w-1"}"#.into(),
            },
            EventDetailRow {
                id: 2,
                system_generation: 2,
                causal_unit_id: 2,
                sequence_in_unit: 0,
                task_id: "task-2".into(),
                step_id: None,
                event_type: "LEASE_ACQUIRED".into(),
                payload: r#"{"lease_id":"l"}"#.into(),
            },
        ];

        m.event_type_filter = "STEP".into();
        assert_eq!(m.filtered_event_indices(), vec![0]);
        m.event_type_filter = "step_started".into(); // case-insensitive
        assert_eq!(m.filtered_event_indices(), vec![0]);
        m.event_type_filter = "".into();

        m.event_text_query = "w-1".into();
        assert_eq!(m.filtered_event_indices(), vec![0]);
        m.event_text_query = "task-2".into();
        assert_eq!(m.filtered_event_indices(), vec![1]);
        m.event_text_query = "missing".into();
        assert!(m.filtered_event_indices().is_empty());
    }

    #[test]
    fn refresh_failure_retains_previous_snapshot_and_marks_stale() {
        let mut m = model_with_tasks(&["keep-me"]);
        // successful refresh first
        let good = {
            let mut s = Snapshot::default();
            s.tasks = m.data.tasks.clone();
            s
        };
        update(&mut m, Msg::Data(good.clone()));
        assert_eq!(m.refresh_seq, 1);
        assert!(!m.stale);

        // failed refresh: previous data must survive
        let mut bad = Snapshot::default();
        bad.error = Some("tasks query failed".into());
        update(&mut m, Msg::Data(bad));
        assert!(m.stale);
        assert!(m.status_is_error);
        assert_eq!(m.status_msg.as_deref(), Some("tasks query failed"));
        assert_eq!(m.data.tasks.len(), 1, "stale data retained");
        assert_eq!(m.data.tasks[0].task_id, "keep-me");

        // next successful refresh clears the stale state
        update(&mut m, Msg::Data(good));
        assert!(!m.stale);
        assert!(!m.status_is_error);
        assert_eq!(m.refresh_seq, 2);
    }
}
