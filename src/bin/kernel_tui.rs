//! kernel-tui — observation + control TUI over the deterministic kernel.
//!
//! Architecture boundary:
//!
//!   USER → TUI (this binary) → KernelAdapter → kernel storage/API → event store
//!
//! The TUI holds presentation state only (see model.rs). All canonical state
//! (tasks, steps, leases, effects, replay validity) comes from kernel
//! queries through adapter.rs. The UI never re-derives kernel state, never
//! implements its own validator/fold, and never exposes shell execution.
//!
//! DB routing is explicit (`--db` / KERNEL_DB_PATH / ./kernel.db) and bound
//! per adapter instance — no thread-local overrides, no global mutable
//! routing (audit finding M3).
//!
//! Responsiveness: kernel queries run on a background worker thread; the
//! input/render loop only applies pure `model::update` transitions and
//! renders. Control actions (submit/schedule/rebuild-snapshot) execute on
//! the worker thread through the same canonical kernel APIs the CLI uses.

#[path = "kernel_tui/adapter.rs"]
mod adapter;
#[path = "kernel_tui/model.rs"]
mod model;
#[path = "kernel_tui/view.rs"]
mod view;

use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use adapter::{KernelAdapter, RefreshParams, Snapshot};
use model::{Model, OutAction};

/// Messages UI → worker.
enum WorkerMsg {
    Refresh(RefreshParams),
    Action(OutAction),
}

/// Messages worker → UI.
enum UiMsg {
    Snap(Snapshot),
    ActionOk(String),
    ActionErr(String),
}

fn resolve_db(args: &[String]) -> String {
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--db" {
            if let Some(v) = args.get(i + 1) {
                return v.clone();
            }
        } else if let Some(rest) = args[i].strip_prefix("--db=") {
            return rest.to_string();
        }
        i += 1;
    }
    std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| "kernel.db".to_string())
}

fn dispatch(wtx: &mpsc::Sender<WorkerMsg>, out: OutAction) {
    match out {
        OutAction::None | OutAction::Quit => {}
        OutAction::Refresh(p) => {
            let _ = wtx.send(WorkerMsg::Refresh(p));
        }
        action => {
            let _ = wtx.send(WorkerMsg::Action(action));
        }
    }
}

fn run_action(adapter: &KernelAdapter, action: OutAction) -> std::result::Result<String, String> {
    match action {
        OutAction::SubmitTask(id) => adapter
            .submit_task(&id)
            .map(|_| format!("task {id} submitted + scheduled"))
            .map_err(|e| format!("submit-task failed: {e}")),
        OutAction::ScheduleTask(id) => adapter
            .schedule_task(&id)
            .map(|_| format!("scheduled {id}"))
            .map_err(|e| format!("schedule failed: {e}")),
        OutAction::RebuildSnapshot(id) => adapter
            .rebuild_snapshot(&id)
            .map(|_| format!("snapshot rebuilt for {id}"))
            .map_err(|e| format!("snapshot rebuild failed: {e}")),
        _ => Ok(String::new()),
    }
}

/// Background worker: owns the adapter, answers refresh/action requests,
/// auto-collects periodically so the UI tracks live kernel activity.
fn worker_loop(adapter: KernelAdapter, wrx: mpsc::Receiver<WorkerMsg>, utx: mpsc::Sender<UiMsg>) {
    let mut params = RefreshParams::default();
    let mut dirty = true; // collect once at startup
    loop {
        match wrx.recv_timeout(Duration::from_millis(250)) {
            Ok(WorkerMsg::Refresh(p)) => {
                params = p;
                dirty = true;
            }
            Ok(WorkerMsg::Action(a)) => {
                match run_action(&adapter, a) {
                    Ok(msg) if !msg.is_empty() => {
                        if utx.send(UiMsg::ActionOk(msg)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if utx.send(UiMsg::ActionErr(e)).is_err() {
                            break;
                        }
                    }
                }
                dirty = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if dirty {
            dirty = false;
            let snap = adapter.collect(&params);
            if utx.send(UiMsg::Snap(snap)).is_err() {
                break;
            }
        }
    }
}

fn run(db: String) -> Result<()> {
    let adapter = KernelAdapter::new(db);

    // Initial synchronous snapshot: read-only, fast, and guarantees the
    // first frame renders real kernel data.
    let mut model = Model::new(adapter.db_path().to_string());
    model.data = adapter.collect(&model.refresh_params());

    // Terminal setup
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (wtx, wrx) = mpsc::channel::<WorkerMsg>();
    let (utx, urx) = mpsc::channel::<UiMsg>();

    let worker_adapter = adapter.clone();
    let worker = std::thread::spawn(move || worker_loop(worker_adapter, wrx, utx));

    // Kick periodic tracking with the current selection.
    let _ = wtx.send(WorkerMsg::Refresh(model.refresh_params()));

    let mut last_auto = Instant::now();
    let result = loop {
        // Apply everything the worker produced.
        for msg in urx.try_iter() {
            let out = match msg {
                UiMsg::Snap(s) => model::update(&mut model, model::Msg::Data(s)),
                UiMsg::ActionOk(t) => model::update(&mut model, model::Msg::ActionOk(t)),
                UiMsg::ActionErr(t) => model::update(&mut model, model::Msg::ActionErr(t)),
            };
            dispatch(&wtx, out);
        }

        // Input (non-blocking poll keeps the loop responsive).
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(k) => {
                    let out = model::update(&mut model, model::Msg::Key(k));
                    dispatch(&wtx, out);
                }
                _ => {}
            }
        }

        // Periodic refresh request: the UI follows kernel activity
        // (new events, lease changes, scheduler work) without blocking.
        if last_auto.elapsed() >= Duration::from_secs(2) {
            last_auto = Instant::now();
            let _ = wtx.send(WorkerMsg::Refresh(model.refresh_params()));
        }

        terminal.draw(|f| view::draw(f, &model))?;

        if model.quitting {
            break Ok(());
        }
    };

    // Teardown
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    drop(wtx);
    let _ = worker.join();
    result
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let db = resolve_db(&args);
    if let Err(e) = run(db) {
        eprintln!("kernel-tui error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_resolution_prefers_flag_then_env_then_default() {
        let args = vec![
            "kernel_tui".to_string(),
            "--db".to_string(),
            "/tmp/x.db".to_string(),
        ];
        assert_eq!(resolve_db(&args), "/tmp/x.db");

        let args = vec!["kernel_tui".to_string(), "--db=/tmp/y.db".to_string()];
        assert_eq!(resolve_db(&args), "/tmp/y.db");
    }

    #[test]
    fn dispatch_routes_actions_and_swallow_quit() {
        let (tx, rx) = mpsc::channel();
        dispatch(&tx, OutAction::None);
        dispatch(&tx, OutAction::Quit);
        dispatch(&tx, OutAction::Refresh(RefreshParams::default()));
        dispatch(&tx, OutAction::SubmitTask("t".into()));
        assert!(matches!(rx.try_recv(), Ok(WorkerMsg::Refresh(_))));
        assert!(matches!(rx.try_recv(), Ok(WorkerMsg::Action(_))));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn run_action_reports_kernel_errors_without_panicking() {
        // A nonexistent directory forces a connection error; the adapter
        // must surface it as Err, never panic.
        let adapter = KernelAdapter::new("/nonexistent-dir-xyz/db.sqlite");
        let res = run_action(&adapter, OutAction::SubmitTask("t1".into()));
        assert!(res.is_err());
    }
}
