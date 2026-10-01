use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use super::history::HistoryEntry;
use super::runtime::RuntimeState;
use super::session::{self, SessionState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    Verified,
    PlanOnly,
}

impl ExecutionMode {
    pub fn label(self) -> &'static str {
        match self {
            ExecutionMode::Verified => "verified",
            ExecutionMode::PlanOnly => "plan-only",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    RunTask,
    RecentRuns,
    RuntimeDetails,
    Tests,
    Diagnostics,
    Config,
    WorkflowOps,
    Tools,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    LeftNav,
    Input,
    Search,
    Confirm,
    Output,
    #[allow(dead_code)]
    Palette,
}

pub enum StreamEvent {
    Started {
        #[allow(dead_code)]
        child_id: u32,
    },
    Output(String),
    Error(String),
    Finished {
        success: bool,
    },
}

#[derive(Clone)]
pub struct PaletteItem {
    pub label: String,
    pub action: PaletteAction,
}

#[derive(Clone, PartialEq)]
pub enum PaletteAction {
    Navigate(Screen),
    RunTests(String),
    RunDiagnostics(String),
    #[allow(dead_code)]
    RunTask,
    RefreshRuntime,
    RestartServer,
}

pub struct App {
    pub should_quit: bool,
    pub current_screen: Screen,
    pub active_panel: ActivePanel,
    pub nav_index: usize,
    pub runtime: RuntimeState,
    pub history: Vec<HistoryEntry>,
    pub filtered_history: Vec<usize>,
    pub history_index: usize,
    pub task_input: String,
    pub task_input_cursor: usize,
    pub search_query: String,
    #[allow(dead_code)]
    pub search_cursor: usize,
    pub output_lines: Vec<String>,
    pub output_scroll: usize,
    pub follow_output: bool,
    pub status_message: Option<String>,
    pub status_is_error: bool,
    pub test_running: bool,
    #[allow(dead_code)]
    pub test_output: Vec<String>,
    pub confirm_action: Option<String>,
    pub workflow_task_id: String,
    // Streaming execution
    pub task_running: bool,
    pub event_rx: Option<mpsc::Receiver<StreamEvent>>,
    pub child_pid: Option<u32>,
    pub start_time: Option<Instant>,
    pub last_output_line: Option<String>,
    // Command palette
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_index: usize,
    pub palette_items: Vec<PaletteItem>,
    // Runtime tail
    pub tail_mode: bool,
    pub runtime_log_lines: Vec<String>,
    pub runtime_log_scroll: usize,
    pub runtime_log_follow: bool,
    // Workflow input
    pub workflow_input_mode: bool,
    pub workflow_input: String,
    pub workflow_input_cursor: usize,
    // Execution mode
    pub execution_mode: ExecutionMode,
    // Last run result
    pub last_final_answer: String,
    pub last_critique_status: String,
    // Tools
    pub tool_registry: deterministic_ai_kernel::tools::ToolRegistry,
    pub tool_invocation_log: Vec<deterministic_ai_kernel::tools::ToolInvocation>,
    pub tool_selected: usize,
    pub tool_confirm_pending: Option<deterministic_ai_kernel::tools::ConfirmationRequest>,
    // Probe stats
    pub probe_stats: super::runtime::ProbeStats,
    // History detail view
    pub history_detail: Option<super::history::HistoryEntry>,
    // Live tool activity (for Run Task screen)
    pub tool_activity: Vec<ToolActivityEntry>,
    // History filters
    pub history_filter_type: HistoryFilterType,
    pub history_filter_status: HistoryFilterStatus,
    pub history_filter_tool: String,
    // Toast / status layer
    pub toast_message: Option<String>,
    pub toast_is_error: bool,
    pub toast_created_at: Option<Instant>,
    // Tool usage stats (for Tools screen)
    pub tool_usage_stats: std::collections::HashMap<String, ToolUsageStats>,
    // Recovery state
    pub recovery_in_progress: bool,
    pub recovery_started_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryFilterType {
    All,
    TaskRun,
    ToolInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryFilterStatus {
    All,
    Success,
    Error,
    Cancelled,
    Denied,
}

#[derive(Debug, Clone)]
pub struct ToolActivityEntry {
    pub tool_name: String,
    pub status: String,
    pub duration_ms: u64,
    pub args_preview: String,
    pub result_preview: String,
}

#[derive(Debug, Clone, Default)]
pub struct ToolUsageStats {
    pub count: u32,
    pub last_used: Option<String>,
    pub last_status: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let runtime = RuntimeState::initial();
        let history = super::history::load_history();
        let filtered_history: Vec<usize> = (0..history.len()).collect();

        let mut app = Self {
            should_quit: false,
            current_screen: Screen::Dashboard,
            active_panel: ActivePanel::LeftNav,
            nav_index: 0,
            runtime,
            history,
            filtered_history,
            history_index: 0,
            task_input: String::new(),
            task_input_cursor: 0,
            search_query: String::new(),
            search_cursor: 0,
            output_lines: Vec::new(),
            output_scroll: 0,
            follow_output: true,
            status_message: None,
            status_is_error: false,
            test_running: false,
            test_output: Vec::new(),
            confirm_action: None,
            workflow_task_id: "task1".to_string(),
            task_running: false,
            event_rx: None,
            child_pid: None,
            start_time: None,
            last_output_line: None,
            palette_open: false,
            palette_query: String::new(),
            palette_index: 0,
            palette_items: Vec::new(),
            tail_mode: false,
            runtime_log_lines: Vec::new(),
            runtime_log_scroll: 0,
            runtime_log_follow: true,
            workflow_input_mode: false,
            workflow_input: String::new(),
            workflow_input_cursor: 0,
            execution_mode: ExecutionMode::Verified,
            last_final_answer: String::new(),
            last_critique_status: String::new(),
            tool_registry: deterministic_ai_kernel::tools::ToolRegistry::new(),
            tool_invocation_log: Vec::new(),
            tool_selected: 0,
            tool_confirm_pending: None,
            history_detail: None,
            tool_activity: Vec::new(),
            history_filter_type: HistoryFilterType::All,
            history_filter_status: HistoryFilterStatus::All,
            history_filter_tool: String::new(),
            toast_message: None,
            toast_is_error: false,
            toast_created_at: None,
            tool_usage_stats: std::collections::HashMap::new(),
            probe_stats: super::runtime::ProbeStats::default(),
            recovery_in_progress: false,
            recovery_started_at: None,
        };

        app.build_palette_items();

        let session = session::load_session();
        app.restore_session(&session);

        app
    }

    fn build_palette_items(&mut self) {
        self.palette_items = vec![
            PaletteItem {
                label: "Run Task".to_string(),
                action: PaletteAction::Navigate(Screen::RunTask),
            },
            PaletteItem {
                label: "Recent Runs".to_string(),
                action: PaletteAction::Navigate(Screen::RecentRuns),
            },
            PaletteItem {
                label: "Runtime Details".to_string(),
                action: PaletteAction::Navigate(Screen::RuntimeDetails),
            },
            PaletteItem {
                label: "Quick Tests (lib)".to_string(),
                action: PaletteAction::RunTests("cargo test --lib".to_string()),
            },
            PaletteItem {
                label: "Full Tests".to_string(),
                action: PaletteAction::RunTests("cargo test".to_string()),
            },
            PaletteItem {
                label: "Doctor".to_string(),
                action: PaletteAction::RunDiagnostics("doctor".to_string()),
            },
            PaletteItem {
                label: "Doctor JSON".to_string(),
                action: PaletteAction::RunDiagnostics("doctor-json".to_string()),
            },
            PaletteItem {
                label: "Diagnostics".to_string(),
                action: PaletteAction::Navigate(Screen::Diagnostics),
            },
            PaletteItem {
                label: "Tests".to_string(),
                action: PaletteAction::Navigate(Screen::Tests),
            },
            PaletteItem {
                label: "Workflow Ops".to_string(),
                action: PaletteAction::Navigate(Screen::WorkflowOps),
            },
            PaletteItem {
                label: "Config / Paths".to_string(),
                action: PaletteAction::Navigate(Screen::Config),
            },
            PaletteItem {
                label: "Refresh Runtime".to_string(),
                action: PaletteAction::RefreshRuntime,
            },
            PaletteItem {
                label: "Restart Server".to_string(),
                action: PaletteAction::RestartServer,
            },
        ];
    }

    fn filter_palette(&mut self) {
        if self.palette_query.is_empty() {
            self.palette_index = 0;
            return;
        }
        if let Some(idx) = self.palette_items.iter().position(|item| {
            item.label
                .to_lowercase()
                .contains(&self.palette_query.to_lowercase())
        }) {
            self.palette_index = idx;
        }
    }

    pub fn get_filtered_palette_items(&self) -> Vec<(usize, &PaletteItem)> {
        if self.palette_query.is_empty() {
            self.palette_items.iter().enumerate().collect()
        } else {
            let query = self.palette_query.to_lowercase();
            self.palette_items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.label.to_lowercase().contains(&query))
                .collect()
        }
    }

    fn save_current_session(&self) {
        let state = SessionState {
            screen: format!("{:?}", self.current_screen),
            nav_index: self.nav_index,
            history_index: self.history_index,
            output_scroll: self.output_scroll,
            search_query: self.search_query.clone(),
        };
        session::save_session(&state);
    }

    fn restore_session(&mut self, state: &SessionState) {
        self.current_screen = match state.screen.as_str() {
            "Dashboard" => Screen::Dashboard,
            "RunTask" => Screen::RunTask,
            "RecentRuns" => Screen::RecentRuns,
            "RuntimeDetails" => Screen::RuntimeDetails,
            "Tests" => Screen::Tests,
            "Diagnostics" => Screen::Diagnostics,
            "Config" => Screen::Config,
            "WorkflowOps" => Screen::WorkflowOps,
            _ => Screen::Dashboard,
        };
        self.nav_index = state
            .nav_index
            .min(self.nav_items().len().saturating_sub(1));
        self.history_index = state.history_index;
        self.output_scroll = state.output_scroll;
        self.search_query = state.search_query.clone();
        self.filter_history();

        if !self.search_query.is_empty() {
            self.set_status("Session restored", false);
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) {
        if self.palette_open {
            self.handle_palette_key(key);
            return;
        }

        if self.task_running {
            if key == KeyCode::Esc {
                self.cancel_task();
            }
            return;
        }

        // Scroll keys when output active
        if !self.output_lines.is_empty()
            && self.is_output_scrollable()
            && self.active_panel == ActivePanel::Output
        {
            match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scroll_up(1);
                    return;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll_down(1);
                    return;
                }
                KeyCode::PageUp => {
                    self.scroll_up(10);
                    return;
                }
                KeyCode::PageDown => {
                    self.scroll_down(10);
                    return;
                }
                KeyCode::Char('g') => {
                    self.scroll_top();
                    return;
                }
                KeyCode::Char('G') => {
                    self.scroll_bottom();
                    return;
                }
                _ => {}
            }
        }

        // Tail log scroll
        if self.tail_mode
            && self.active_panel == ActivePanel::Output
            && self.current_screen == Screen::RuntimeDetails
        {
            match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.runtime_log_scroll > 0 {
                        self.runtime_log_scroll -= 1;
                        self.runtime_log_follow = false;
                    }
                    return;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.runtime_log_scroll =
                        (self.runtime_log_scroll + 1).min(self.runtime_log_lines.len());
                    if self.runtime_log_scroll >= self.runtime_log_lines.len() {
                        self.runtime_log_follow = true;
                    }
                    return;
                }
                KeyCode::PageUp => {
                    self.runtime_log_scroll = self.runtime_log_scroll.saturating_sub(10);
                    self.runtime_log_follow = false;
                    return;
                }
                KeyCode::PageDown => {
                    self.runtime_log_scroll =
                        (self.runtime_log_scroll + 10).min(self.runtime_log_lines.len());
                    if self.runtime_log_scroll >= self.runtime_log_lines.len() {
                        self.runtime_log_follow = true;
                    }
                    return;
                }
                KeyCode::Char('g') => {
                    self.runtime_log_scroll = 0;
                    self.runtime_log_follow = false;
                    return;
                }
                KeyCode::Char('G') => {
                    self.runtime_log_scroll = self.runtime_log_lines.len();
                    self.runtime_log_follow = true;
                    return;
                }
                _ => {}
            }
        }

        // Open palette
        if key == KeyCode::Char(':') && self.current_screen != Screen::RunTask {
            self.open_palette();
            return;
        }

        match self.current_screen {
            Screen::Dashboard => self.handle_dashboard_key(key),
            Screen::RunTask => self.handle_run_task_key(key),
            Screen::RecentRuns => self.handle_recent_runs_key(key),
            Screen::RuntimeDetails => self.handle_runtime_key(key),
            Screen::Tests => self.handle_tests_key(key),
            Screen::Diagnostics => self.handle_diagnostics_key(key),
            Screen::Config => self.handle_config_key(key),
            Screen::WorkflowOps => self.handle_workflow_key(key),
            Screen::Tools => self.handle_tools_key(key),
        }
    }

    fn handle_palette_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => {
                self.palette_open = false;
            }
            KeyCode::Enter => {
                let items = self.get_filtered_palette_items();
                if let Some((_, item)) = items.get(self.palette_index) {
                    let action = item.action.clone();
                    self.palette_open = false;
                    self.execute_palette_action(action);
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.palette_index > 0 => {
                self.palette_index -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let items = self.get_filtered_palette_items();
                if self.palette_index + 1 < items.len() {
                    self.palette_index += 1;
                }
            }
            KeyCode::Char(c) => {
                self.palette_query.push(c);
                self.filter_palette();
            }
            KeyCode::Backspace => {
                self.palette_query.pop();
                self.filter_palette();
            }
            _ => {}
        }
    }

    fn execute_palette_action(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::Navigate(screen) => {
                self.current_screen = screen;
                self.nav_index = 0;
                self.set_status(&format!("Opened {:?}", screen), false);
            }
            PaletteAction::RunTests(cmd) => {
                self.current_screen = Screen::Tests;
                self.run_tests_streaming(&cmd);
            }
            PaletteAction::RunDiagnostics(cmd) => {
                self.current_screen = Screen::Diagnostics;
                self.run_command_streaming(&[&cmd]);
            }
            PaletteAction::RunTask => {
                self.current_screen = Screen::RunTask;
                self.active_panel = ActivePanel::Input;
            }
            PaletteAction::RefreshRuntime => {
                self.runtime.probe_force(&mut self.probe_stats);
                self.set_status("Runtime online", self.runtime.is_running);
            }
            PaletteAction::RestartServer => {
                self.initiate_recovery();
            }
        }
    }

    fn open_palette(&mut self) {
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_index = 0;
    }

    fn handle_dashboard_key(&mut self, key: KeyCode) {
        let nav_count = self.nav_items().len();
        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.save_current_session();
                self.should_quit = true;
            }
            KeyCode::Up | KeyCode::Char('k') if self.nav_index > 0 => {
                self.nav_index -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') if self.nav_index + 1 < nav_count => {
                self.nav_index += 1;
            }
            KeyCode::Enter => {
                if self.nav_index == nav_count - 1 {
                    self.save_current_session();
                    self.should_quit = true;
                    return;
                }
                self.current_screen = match self.nav_index {
                    0 => Screen::RunTask,
                    1 => Screen::RuntimeDetails,
                    2 => Screen::Diagnostics,
                    3 => Screen::RecentRuns,
                    4 => Screen::Tests,
                    5 => Screen::WorkflowOps,
                    6 => Screen::Tools,
                    7 => Screen::Config,
                    _ => Screen::Dashboard,
                };
                self.nav_index = 0;
            }
            _ => {}
        }
    }

    fn handle_run_task_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => {
                self.current_screen = Screen::Dashboard;
                self.task_input.clear();
                self.output_lines.clear();
                self.status_message = None;
                self.active_panel = ActivePanel::Input;
            }
            KeyCode::Tab => {
                self.active_panel = match self.active_panel {
                    ActivePanel::Input => ActivePanel::Output,
                    ActivePanel::Output => ActivePanel::Input,
                    _ => ActivePanel::Input,
                };
            }
            KeyCode::Char('m') if self.active_panel == ActivePanel::Input && !self.task_running => {
                self.execution_mode = match self.execution_mode {
                    ExecutionMode::Verified => ExecutionMode::PlanOnly,
                    ExecutionMode::PlanOnly => ExecutionMode::Verified,
                };
                let label = self.execution_mode.label();
                self.set_status(&format!("Mode: {}", label), false);
            }
            KeyCode::Char('R') if !self.task_running => {
                self.initiate_recovery();
            }
            KeyCode::Char(c) if self.active_panel == ActivePanel::Input => {
                self.task_input.insert(self.task_input_cursor, c);
                self.task_input_cursor += 1;
            }
            KeyCode::Backspace
                if self.active_panel == ActivePanel::Input && self.task_input_cursor > 0 =>
            {
                self.task_input_cursor -= 1;
                self.task_input.remove(self.task_input_cursor);
            }
            KeyCode::Left
                if self.active_panel == ActivePanel::Input && self.task_input_cursor > 0 =>
            {
                self.task_input_cursor -= 1;
            }
            KeyCode::Right
                if self.active_panel == ActivePanel::Input
                    && self.task_input_cursor < self.task_input.len() =>
            {
                self.task_input_cursor += 1;
            }
            KeyCode::Enter
                if self.active_panel == ActivePanel::Input
                    && !self.task_input.is_empty()
                    && !self.task_running =>
            {
                // Runtime check for verified mode
                if self.execution_mode == ExecutionMode::Verified && !self.runtime.is_running {
                    let recovery_hint = match self.runtime.server_state {
                        super::runtime::ServerState::Crashed => {
                            "MLX server crashed. Press R to restart, or m to switch to plan-only."
                        }
                        super::runtime::ServerState::Recovering => {
                            "Recovery in progress... waiting for server."
                        }
                        _ => "MLX server offline. Press R to restart, or m to switch to plan-only.",
                    };
                    self.set_status(recovery_hint, true);
                    return;
                }
                self.start_task_streaming();
            }
            _ => {}
        }
    }

    fn handle_recent_runs_key(&mut self, key: KeyCode) {
        // If detail view is open, only Esc closes it
        if self.history_detail.is_some() {
            if key == KeyCode::Esc {
                self.history_detail = None;
            }
            return;
        }

        match self.active_panel {
            ActivePanel::Search => match key {
                KeyCode::Esc => {
                    self.active_panel = ActivePanel::LeftNav;
                    self.search_query.clear();
                    self.filter_history();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.filter_history();
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.filter_history();
                }
                _ => {}
            },
            _ => match key {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.current_screen = Screen::Dashboard;
                    self.search_query.clear();
                    self.filter_history();
                }
                KeyCode::Char('/') => {
                    self.active_panel = ActivePanel::Search;
                }
                KeyCode::Char('1') => {
                    self.history_filter_type = match self.history_filter_type {
                        HistoryFilterType::All => HistoryFilterType::TaskRun,
                        HistoryFilterType::TaskRun => HistoryFilterType::ToolInvocation,
                        HistoryFilterType::ToolInvocation => HistoryFilterType::All,
                    };
                    self.filter_history();
                    self.show_toast(&format!("Type: {:?}", self.history_filter_type), false);
                }
                KeyCode::Char('2') => {
                    self.history_filter_status = match self.history_filter_status {
                        HistoryFilterStatus::All => HistoryFilterStatus::Success,
                        HistoryFilterStatus::Success => HistoryFilterStatus::Error,
                        HistoryFilterStatus::Error => HistoryFilterStatus::Cancelled,
                        HistoryFilterStatus::Cancelled => HistoryFilterStatus::Denied,
                        HistoryFilterStatus::Denied => HistoryFilterStatus::All,
                    };
                    self.filter_history();
                    self.show_toast(&format!("Status: {:?}", self.history_filter_status), false);
                }
                KeyCode::Char('0') => {
                    self.history_filter_type = HistoryFilterType::All;
                    self.history_filter_status = HistoryFilterStatus::All;
                    self.history_filter_tool.clear();
                    self.filter_history();
                    self.show_toast("Filters reset", false);
                }
                KeyCode::Up | KeyCode::Char('k') if self.history_index > 0 => {
                    self.history_index -= 1;
                }
                KeyCode::Down | KeyCode::Char('j')
                    if self.history_index + 1 < self.filtered_history.len() =>
                {
                    self.history_index += 1;
                }
                KeyCode::Enter => {
                    if let Some(&idx) = self.filtered_history.get(self.history_index) {
                        if let Some(entry) = self.history.get(idx) {
                            self.history_detail = Some(entry.clone());
                        }
                    }
                }
                KeyCode::Delete | KeyCode::Char('d') => {
                    if let Some(&idx) = self.filtered_history.get(self.history_index) {
                        let entry = &self.history[idx];
                        let _ = super::history::delete_entry(&entry.file_path);
                        self.history.remove(idx);
                        self.filter_history();
                        if self.history_index >= self.filtered_history.len()
                            && self.history_index > 0
                        {
                            self.history_index -= 1;
                        }
                        self.set_status("Entry deleted", false);
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(&idx) = self.filtered_history.get(self.history_index) {
                        if let Some(entry) = self.history.get(idx) {
                            self.task_input = entry.task.clone();
                            self.current_screen = Screen::RunTask;
                            self.active_panel = ActivePanel::Input;
                        }
                    }
                }
                _ => {}
            },
        }
    }

    fn handle_runtime_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.current_screen = Screen::Dashboard;
                self.tail_mode = false;
            }
            KeyCode::Char('t') => {
                self.tail_mode = !self.tail_mode;
                if self.tail_mode {
                    self.refresh_runtime_log();
                    self.set_status("Tail started", false);
                } else {
                    self.set_status("Tail stopped", false);
                }
            }
            KeyCode::Char('r') => {
                self.runtime.probe_force(&mut self.probe_stats);
                if self.tail_mode {
                    self.refresh_runtime_log();
                }
                let msg = if self.runtime.is_running {
                    "Runtime online"
                } else {
                    "Runtime offline"
                };
                self.set_status(msg, !self.runtime.is_running);
            }
            KeyCode::Char('R') => {
                self.initiate_recovery();
            }
            _ => {}
        }
    }

    fn refresh_runtime_log(&mut self) {
        let log_path = "/tmp/replay_os_mlx.log";
        if let Ok(content) = std::fs::read_to_string(log_path) {
            self.runtime_log_lines = content.lines().map(|s| s.to_string()).collect();
            if self.runtime_log_follow {
                self.runtime_log_scroll = self.runtime_log_lines.len();
            }
        } else {
            self.runtime_log_lines.clear();
            self.set_status("Runtime log not found", true);
        }
    }

    fn handle_tests_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.current_screen = Screen::Dashboard;
                self.output_lines.clear();
                self.active_panel = ActivePanel::LeftNav;
            }
            KeyCode::Char('1') => self.run_tests_streaming("cargo test --lib"),
            KeyCode::Char('2') => self.run_tests_streaming("cargo test"),
            _ => {}
        }
    }

    fn handle_diagnostics_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.current_screen = Screen::Dashboard;
                self.output_lines.clear();
                self.active_panel = ActivePanel::LeftNav;
            }
            KeyCode::Char('1') => self.run_command_streaming(&["doctor"]),
            KeyCode::Char('2') => self.run_command_streaming(&["doctor-json"]),
            _ => {}
        }
    }

    fn handle_tools_key(&mut self, key: KeyCode) {
        // Handle confirmation dialog if pending
        if self.tool_confirm_pending.is_some() {
            match key {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Tab => {
                    let req = self.tool_confirm_pending.take().unwrap();
                    self.execute_tool_with_confirmation(&req.tool, &req.arguments, true);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    let req = self.tool_confirm_pending.take().unwrap();
                    let invocation = deterministic_ai_kernel::tools::ToolInvocation {
                        tool_name: req.tool,
                        arguments: req.arguments,
                        status: deterministic_ai_kernel::tools::ToolStatus::Cancelled,
                        output: None,
                        error: Some("User denied confirmation".to_string()),
                        duration_ms: 0,
                        confirmed: false,
                        timestamp: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                    };
                    self.tool_invocation_log.push(invocation);
                    self.set_status("Tool cancelled by user", true);
                }
                _ => {}
            }
            return;
        }

        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.current_screen = Screen::Dashboard;
            }
            KeyCode::Up | KeyCode::Char('k') if self.tool_selected > 0 => {
                self.tool_selected -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let count = self.tool_registry.all().len();
                if self.tool_selected + 1 < count {
                    self.tool_selected += 1;
                }
            }
            KeyCode::Char('t') => {
                if let Some(tool) = self.tool_registry.all().get(self.tool_selected) {
                    let test_args = match tool.name {
                        "write_file" => {
                            serde_json::json!({"path": "/tmp/replay_os_test.txt", "content": "test content from Replay OS"})
                        }
                        "edit_file" => {
                            // Create test file first
                            let _ = std::fs::write("/tmp/replay_os_edit_test.txt", "hello world");
                            serde_json::json!({"path": "/tmp/replay_os_edit_test.txt", "old_string": "world", "new_string": "ReplayOS"})
                        }
                        // NOTE: shell_execute was removed from the registry
                        // (security debt closure) — no test args for it.
                        "open_url" => serde_json::json!({"url": "https://example.com"}),
                        _ => serde_json::json!({"path": "."}),
                    };

                    if tool.confirmation_required {
                        self.tool_confirm_pending =
                            Some(deterministic_ai_kernel::tools::ConfirmationRequest::new(
                                tool, &test_args,
                            ));
                    } else {
                        self.execute_tool_with_confirmation(tool.name, &test_args, true);
                    }
                }
            }
            _ => {}
        }
    }

    fn execute_tool_with_confirmation(
        &mut self,
        tool_name: &str,
        args: &serde_json::Value,
        confirmed: bool,
    ) {
        let start = std::time::Instant::now();
        self.set_status(&format!("Running {}...", tool_name), false);

        // Add activity entry — running
        let args_preview = format_tool_args_short(tool_name, args);
        self.tool_activity.push(ToolActivityEntry {
            tool_name: tool_name.to_string(),
            status: "running".to_string(),
            duration_ms: 0,
            args_preview: args_preview.clone(),
            result_preview: String::new(),
        });

        // Run tool synchronously (blocking for now). The confirmation state
        // is passed into the enforcement boundary itself (tools::registry),
        // so the gate holds even if a future caller skips the UI dialog.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let workspace = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let result = rt.block_on(deterministic_ai_kernel::tools::execute_tool(
            tool_name, args, &workspace, confirmed,
        ));
        let duration_ms = start.elapsed().as_millis() as u64;

        // Update activity entry — done
        if let Some(entry) = self.tool_activity.last_mut() {
            entry.status = if result.success {
                "success".to_string()
            } else {
                "error".to_string()
            };
            entry.duration_ms = duration_ms;
            entry.result_preview = format_tool_result_short(tool_name, &result.output);
        }

        let invocation = deterministic_ai_kernel::tools::ToolInvocation {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            status: if result.success {
                deterministic_ai_kernel::tools::ToolStatus::Success
            } else {
                deterministic_ai_kernel::tools::ToolStatus::Error(
                    result.error.clone().unwrap_or_default(),
                )
            },
            output: Some(result.output.to_string()),
            error: result.error.clone(),
            duration_ms,
            confirmed,
            timestamp: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        };

        // Structured output rendering
        let output_line = format_tool_output_line(tool_name, args, &result);
        self.output_lines.push(output_line);

        self.tool_invocation_log.push(invocation.clone());
        super::history::save_tool_invocation(&invocation);

        // Update usage stats
        let stats = self
            .tool_usage_stats
            .entry(tool_name.to_string())
            .or_default();
        stats.count += 1;
        stats.last_used = Some(invocation.timestamp.clone());
        stats.last_status = Some(if result.success {
            "success".to_string()
        } else {
            "error".to_string()
        });

        // Toast
        if result.success {
            self.show_toast(
                &format!("{} completed ({}ms)", tool_name, duration_ms),
                false,
            );
        } else {
            self.show_toast(
                &format!("{} failed: {}", tool_name, result.error.unwrap_or_default()),
                true,
            );
        }
    }

    pub fn show_toast(&mut self, msg: &str, is_error: bool) {
        self.toast_message = Some(msg.to_string());
        self.toast_is_error = is_error;
        self.toast_created_at = Some(Instant::now());
    }

    pub fn clear_old_toast(&mut self) {
        if let Some(created) = self.toast_created_at {
            if created.elapsed() > Duration::from_secs(3) {
                self.toast_message = None;
                self.toast_created_at = None;
            }
        }
    }

    pub fn apply_history_filters(&mut self) {
        self.filtered_history = self
            .history
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                // Type filter
                match &self.history_filter_type {
                    HistoryFilterType::TaskRun if entry.entry_type != "task_run" => return None,
                    HistoryFilterType::ToolInvocation if entry.entry_type != "tool_invocation" => {
                        return None
                    }
                    _ => {}
                }
                // Status filter
                match &self.history_filter_status {
                    HistoryFilterStatus::Success if !entry.ok => return None,
                    HistoryFilterStatus::Error if entry.ok => return None,
                    HistoryFilterStatus::Cancelled
                        if entry.entry_type != "tool_invocation" || entry.ok =>
                    {
                        return None
                    }
                    HistoryFilterStatus::Denied
                        if entry.confirmed || entry.entry_type != "tool_invocation" =>
                    {
                        return None
                    }
                    _ => {}
                }
                // Tool name filter
                if !self.history_filter_tool.is_empty() {
                    let q = self.history_filter_tool.to_lowercase();
                    if !entry.tool_name.to_lowercase().contains(&q)
                        && !entry.task.to_lowercase().contains(&q)
                    {
                        return None;
                    }
                }
                // Text search
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    if !entry.task.to_lowercase().contains(&q) && !entry.timestamp.contains(&q) {
                        return None;
                    }
                }
                Some(i)
            })
            .collect();
        self.history_index = 0;
    }

    fn handle_config_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.current_screen = Screen::Dashboard;
            }
            _ => {}
        }
    }

    fn handle_workflow_key(&mut self, key: KeyCode) {
        // Handle confirm dialog
        if self.confirm_action.is_some() && self.active_panel == ActivePanel::Confirm {
            match key {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.execute_workflow_action();
                    self.active_panel = ActivePanel::LeftNav;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.confirm_action = None;
                    self.active_panel = ActivePanel::LeftNav;
                    self.set_status("Workflow cancelled", false);
                }
                _ => {}
            }
            return;
        }

        // Handle task_id input mode
        if self.workflow_input_mode {
            match key {
                KeyCode::Esc => {
                    self.workflow_input_mode = false;
                    self.active_panel = ActivePanel::LeftNav;
                }
                KeyCode::Enter if !self.workflow_input.is_empty() => {
                    self.workflow_task_id = self.workflow_input.clone();
                    self.workflow_input_mode = false;
                    self.active_panel = ActivePanel::Confirm;
                    self.confirm_action = Some(self.workflow_input.clone());
                    self.workflow_input.clear();
                }
                KeyCode::Char(c) => {
                    self.workflow_input.insert(self.workflow_input_cursor, c);
                    self.workflow_input_cursor += 1;
                }
                KeyCode::Backspace if self.workflow_input_cursor > 0 => {
                    self.workflow_input_cursor -= 1;
                    self.workflow_input.remove(self.workflow_input_cursor);
                }
                _ => {}
            }
            return;
        }

        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.current_screen = Screen::Dashboard;
                self.output_lines.clear();
            }
            KeyCode::Char('1') => self.start_workflow_action("schedule"),
            KeyCode::Char('2') => self.start_workflow_action("reconcile"),
            KeyCode::Char('3') => self.start_workflow_action("execute-effects"),
            KeyCode::Char('4') => self.start_workflow_action("expire-leases"),
            _ => {}
        }
    }

    fn start_workflow_action(&mut self, action: &str) {
        self.workflow_input_mode = true;
        self.workflow_input = self.workflow_task_id.clone();
        self.workflow_input_cursor = self.workflow_input.len();
        self.active_panel = ActivePanel::Input;
        self.confirm_action = Some(action.to_string());
    }

    fn execute_workflow_action(&mut self) {
        if let Some(action) = self.confirm_action.take() {
            let task_id = self.workflow_task_id.clone();
            self.run_command_streaming(&[action.as_str(), task_id.as_str()]);
        }
    }

    // ── Scroll ───────────────────────────────────────────────────────────────

    fn is_output_scrollable(&self) -> bool {
        matches!(
            self.current_screen,
            Screen::RunTask | Screen::Tests | Screen::Diagnostics | Screen::WorkflowOps
        )
    }

    fn scroll_up(&mut self, amount: usize) {
        if self.output_scroll > 0 {
            self.output_scroll = self.output_scroll.saturating_sub(amount);
            self.follow_output = false;
        }
    }

    fn scroll_down(&mut self, amount: usize) {
        let max = self.output_lines.len();
        self.output_scroll = (self.output_scroll + amount).min(max);
        if self.output_scroll >= max {
            self.follow_output = true;
        }
    }

    fn scroll_top(&mut self) {
        self.output_scroll = 0;
        self.follow_output = false;
    }
    fn scroll_bottom(&mut self) {
        self.output_scroll = self.output_lines.len();
        self.follow_output = true;
    }

    // ── Streaming ────────────────────────────────────────────────────────────

    fn start_task_streaming(&mut self) {
        let payload = self.task_input.clone();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let (tx, rx) = mpsc::channel();
        self.task_running = true;
        self.output_lines.clear();
        self.output_scroll = 0;
        self.follow_output = true;
        self.event_rx = Some(rx);
        self.start_time = Some(Instant::now());
        self.last_output_line = None;
        self.last_final_answer.clear();
        self.last_critique_status.clear();

        let mode_label = self.execution_mode.label();
        self.output_lines.push(format!(
            "[mode: {}] payload=\"{}\" seed={}",
            mode_label,
            &payload[..payload.len().min(50)],
            seed
        ));

        let spawn_fn = match self.execution_mode {
            ExecutionMode::Verified => super::runtime::spawn_verified_execution,
            ExecutionMode::PlanOnly => super::runtime::spawn_plan_only,
        };
        if let Some(pid) = spawn_fn(&payload, seed, tx) {
            self.child_pid = Some(pid);
        } else {
            self.set_status("Failed to spawn process", true);
            self.task_running = false;
        }
    }

    fn run_tests_streaming(&mut self, cmd: &str) {
        let (tx, rx) = mpsc::channel();
        self.test_running = true;
        self.task_running = true;
        self.output_lines.clear();
        self.output_scroll = 0;
        self.follow_output = true;
        self.event_rx = Some(rx);
        self.start_time = Some(Instant::now());
        self.last_output_line = None;
        self.output_lines.push(format!("$ {}", cmd));
        // No shell: fixed palette strings ("cargo test [--lib]") split into
        // argv directly. Removes the sh -c remnant of shell_execute.
        let argv: Vec<&str> = cmd.split_whitespace().collect();
        let pid = argv
            .split_first()
            .and_then(|(prog, args)| super::runtime::spawn_streaming(prog, args, tx));
        if let Some(pid) = pid {
            self.child_pid = Some(pid);
        } else {
            self.set_status("Failed to spawn process", true);
            self.task_running = false;
            self.test_running = false;
        }
    }

    fn run_command_streaming(&mut self, args: &[&str]) {
        let (tx, rx) = mpsc::channel();
        self.test_running = true;
        self.task_running = true;
        self.output_lines.clear();
        self.output_scroll = 0;
        self.follow_output = true;
        self.event_rx = Some(rx);
        self.start_time = Some(Instant::now());
        self.last_output_line = None;
        self.output_lines
            .push(format!("$ deterministic_ai_kernel {}", args.join(" ")));
        // No shell: argv is built structurally; the user-typed task id is a
        // single argv element, never re-parsed by a shell.
        let mut argv: Vec<&str> = vec!["run", "--bin", "deterministic_ai_kernel", "--"];
        argv.extend_from_slice(args);
        let pid = super::runtime::spawn_streaming("cargo", &argv, tx);
        if let Some(pid) = pid {
            self.child_pid = Some(pid);
        } else {
            self.set_status("Failed to spawn process", true);
            self.task_running = false;
            self.test_running = false;
        }
    }

    fn cancel_task(&mut self) {
        if let Some(pid) = self.child_pid {
            super::runtime::kill_process(pid);
        }
        self.child_pid = None;
        self.event_rx = None;
        self.task_running = false;
        self.test_running = false;
        self.set_status("Cancelled", true);
    }

    fn process_stream_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                StreamEvent::Started { child_id: _ } => {
                    self.set_status("Running...", false);
                }
                StreamEvent::Output(line) => {
                    self.last_output_line = Some(line.clone());
                    if self.output_lines.len() > 500 {
                        self.output_lines.remove(0);
                    }
                    self.output_lines.push(line.clone());
                    if self.follow_output {
                        self.output_scroll = self.output_lines.len();
                    }

                    // Track tool activity from streaming output
                    if line.starts_with("[tool]") {
                        let parts: Vec<&str> = line.split(" · ").collect();
                        if parts.len() >= 2 {
                            let tool_name = parts.get(1).unwrap_or(&"").to_string();
                            let status = parts.get(2).unwrap_or(&"running").to_string();
                            let args_preview = parts.get(3).unwrap_or(&"").to_string();
                            let result_preview = parts.get(4).unwrap_or(&"").to_string();
                            self.tool_activity.push(ToolActivityEntry {
                                tool_name,
                                status,
                                duration_ms: 0,
                                args_preview,
                                result_preview,
                            });
                        }
                    }

                    // Track observability milestones
                    if line.starts_with("observability:") && line.contains("execution_critique") {
                        let desc = line.split("description=").nth(1).unwrap_or("ok");
                        self.show_toast(&format!("Critique: {desc}"), false);
                    }
                }
                StreamEvent::Error(line) => {
                    self.last_output_line = Some(line.clone());
                    if self.output_lines.len() > 500 {
                        self.output_lines.remove(0);
                    }
                    self.output_lines.push(line);
                    if self.follow_output {
                        self.output_scroll = self.output_lines.len();
                    }
                }
                StreamEvent::Finished { success } => {
                    self.child_pid = None;
                    self.task_running = false;
                    self.test_running = false;
                    self.event_rx = None;
                    let elapsed = self
                        .start_time
                        .map(|t| t.elapsed().as_millis())
                        .unwrap_or(0);
                    self.start_time = None;

                    // Collect multi-line JSON: find first line starting with '{' and last '}'
                    let json_str = {
                        let mut start_idx = None;
                        let mut end_idx = None;
                        for (i, line) in self.output_lines.iter().enumerate() {
                            if start_idx.is_none() && line.trim_start().starts_with('{') {
                                start_idx = Some(i);
                            }
                            if line.trim_end().ends_with('}') {
                                end_idx = Some(i);
                            }
                        }
                        match (start_idx, end_idx) {
                            (Some(start), Some(end)) => {
                                let mut buf = String::new();
                                for line in &self.output_lines[start..=end] {
                                    buf.push_str(line);
                                    buf.push('\n');
                                }
                                Some(buf)
                            }
                            _ => None,
                        }
                    };

                    if let Some(raw) = json_str {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                            self.last_final_answer = json
                                .get("final_answer")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            self.last_critique_status = json
                                .get("critique_status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            if let Some(seed_str) = self
                                .output_lines
                                .iter()
                                .find(|l| l.contains("seed="))
                                .and_then(|l| l.split("seed=").nth(1))
                                .map(|s| s.split_whitespace().next().unwrap_or("0"))
                            {
                                if let Ok(seed) = seed_str.parse::<u64>() {
                                    super::history::save_entry(&self.task_input, seed, &json);
                                    self.history = super::history::load_history();
                                    self.filter_history();
                                }
                            }
                        }
                    }
                    if success {
                        self.set_status(&format!("Completed in {}ms", elapsed), false);
                    } else {
                        self.set_status(&format!("Failed ({}ms)", elapsed), true);
                    }
                    self.output_scroll = self.output_lines.len();
                    self.follow_output = true;
                }
            }
        }
    }

    fn filter_history(&mut self) {
        self.apply_history_filters();
    }

    fn set_status(&mut self, msg: &str, is_error: bool) {
        self.status_message = Some(msg.to_string());
        self.status_is_error = is_error;
    }

    pub fn tick(&mut self) {
        self.process_stream_events();
        self.clear_old_toast();
        if self.tail_mode && self.current_screen == Screen::RuntimeDetails {
            self.refresh_runtime_log();
        }

        // Handle recovery: if we started a recovery, poll for server
        if self.recovery_in_progress {
            if let Some(started) = self.recovery_started_at {
                if started.elapsed() > Duration::from_secs(super::runtime::RECOVERY_WAIT_SECS + 2) {
                    // Recovery timeout — check if server came back
                    self.runtime.probe_force(&mut self.probe_stats);
                    self.recovery_in_progress = false;
                    self.recovery_started_at = None;
                    if self.runtime.is_running {
                        self.set_status("Recovery successful — server online", false);
                    } else {
                        self.set_status(
                            "Recovery failed — server still offline. Try switching to plan-only.",
                            true,
                        );
                    }
                }
            }
        }

        if self.current_screen == Screen::Dashboard || self.current_screen == Screen::RuntimeDetails
        {
            self.runtime.probe_cached(&mut self.probe_stats);
        }
    }

    pub fn nav_items(&self) -> Vec<&str> {
        vec![
            "Run Task",
            "Runtime Details",
            "Diagnostics",
            "Recent Runs",
            "Tests",
            "Workflow Ops",
            "Tools / Capabilities",
            "Config / Paths",
            "Exit",
        ]
    }

    pub fn config_items(&self) -> Vec<(&str, String, bool)> {
        let project_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let env_path = format!("{}/.env", project_dir);
        let manifest_path = format!("{}/config/model_manifest.json", project_dir);
        let history_dir = format!("{}/.replay_os/history", project_dir);
        let env_exists = std::path::Path::new(&env_path).exists();
        let manifest_exists = std::path::Path::new(&manifest_path).exists();
        vec![
            ("Project Dir", project_dir, true),
            ("Env File", env_path, env_exists),
            ("Manifest", manifest_path, manifest_exists),
            (
                "Base URL",
                self.runtime.base_url.clone(),
                !self.runtime.base_url.is_empty(),
            ),
            (
                "Model",
                self.runtime.model_id.clone(),
                self.runtime.model_id != "unknown",
            ),
            ("History Dir", history_dir, true),
            (
                "Server State",
                self.runtime.server_state.label().to_string(),
                self.runtime.is_running,
            ),
            (
                "Server Ownership",
                self.runtime.server_ownership.label().to_string(),
                true,
            ),
            (
                "Recovery Attempts",
                self.runtime.recovery_attempts.to_string(),
                self.runtime.recovery_attempts < 3,
            ),
        ]
    }

    /// Initiate server recovery flow.
    pub fn initiate_recovery(&mut self) {
        if self.recovery_in_progress {
            self.set_status("Recovery already in progress", true);
            return;
        }

        self.set_status("Recovery started...", false);
        let action = self.runtime.start_recovery();

        match action {
            super::runtime::RecoveryAction::Launched { message } => {
                self.recovery_in_progress = true;
                self.recovery_started_at = Some(Instant::now());
                self.set_status(&message, false);
            }
            super::runtime::RecoveryAction::LaunchFailed { message } => {
                self.set_status(&message, true);
            }
            super::runtime::RecoveryAction::ShowInstructions { message } => {
                self.set_status("Server is externally managed. Run manually:", true);
                // Show instructions in output
                for line in message.lines() {
                    self.output_lines.push(format!("  {}", line));
                }
                self.current_screen = Screen::RunTask;
                self.active_panel = ActivePanel::Output;
            }
            super::runtime::RecoveryAction::RateLimited { message } => {
                self.set_status(&message, true);
            }
        }
    }
}

// ── Tool output formatting helpers ──────────────────────────────────────────

fn format_tool_args_short(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "read_file" | "write_file" | "edit_file" | "get_file_info" => args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        "list_directory" | "find_files" => args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".")
            .to_string(),
        "grep_files" => {
            let pat = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("{pat} in {path}")
        }
        "shell_execute" | "shell_execute_readonly" => args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        "fetch_url" => args
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        "open_url" => args
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        _ => args.to_string(),
    }
}

fn format_tool_result_short(tool_name: &str, output: &serde_json::Value) -> String {
    match tool_name {
        "read_file" => {
            let lines = output.get("lines").and_then(|v| v.as_u64()).unwrap_or(0);
            let bytes = output.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{lines} lines, {bytes} bytes")
        }
        "write_file" => {
            let bytes = output
                .get("bytes_written")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!("{bytes} bytes written")
        }
        "edit_file" => {
            let reps = output
                .get("replacements")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!("{reps} replacement(s)")
        }
        "list_directory" => {
            let count = output.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{count} entries")
        }
        "find_files" => {
            let count = output.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{count} files found")
        }
        "grep_files" => {
            let matches = output.get("matches").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{matches} matches")
        }
        "shell_execute" | "shell_execute_readonly" => {
            let code = output
                .get("exit_code")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1);
            let stdout = output.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
            let preview = if stdout.len() > 40 {
                format!("{}...", &stdout[..40])
            } else {
                stdout.to_string()
            };
            format!("exit {code}: {preview}")
        }
        "fetch_url" => {
            let status = output.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
            let bytes = output.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("HTTP {status}, {bytes} bytes")
        }
        "open_url" => "opened".to_string(),
        _ => String::new(),
    }
}

fn format_tool_output_line(
    tool_name: &str,
    args: &serde_json::Value,
    result: &deterministic_ai_kernel::tools::ToolResult,
) -> String {
    let icon = if result.success { "✓" } else { "✗" };
    let duration = result.duration_ms;
    let args_short = format_tool_args_short(tool_name, args);
    let result_short = if result.success {
        format_tool_result_short(tool_name, &result.output)
    } else {
        result.error.clone().unwrap_or_default()
    };

    format!("[tool] {icon} {tool_name} · {args_short} · {duration}ms · {result_short}")
}
