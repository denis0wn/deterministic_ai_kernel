//! Kernel adapter for the operator TUI.
//!
//! This is the ONLY component allowed to talk to the kernel. Every value it
//! returns comes from canonical kernel queries (`storage_for(db)`,
//! `task_state`, `get_current_status_map`, `load_exec_spec`,
//! `replay_violations`, …). The TUI never derives task/step/lease/effect
//! state on its own and never implements its own validator or fold.
//!
//! DB routing is explicit per query (audit finding M3): no thread-local
//! overrides, no global mutable routing.

use anyhow::Result;

use deterministic_ai_kernel::kernel_types::TaskState;
use deterministic_ai_kernel::providers;
use deterministic_ai_kernel::providers::storage::{EventDetailRow, StorageProvider};

/// Dashboard counters — all sourced from kernel queries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DashboardData {
    pub db_path: String,
    pub total_tasks: usize,
    pub pending_tasks: usize,
    pub running_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub active_leases: usize,
    pub workers: usize,
    pub total_events: i64,
    pub causal_units: i64,
    pub max_generation: i64,
    pub replay_checked_tasks: usize,
    pub replay_valid_tasks: usize,
    pub last_error: Option<String>,
}

/// One row of the TASKS screen. Task state is the kernel's TaskState.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRowVm {
    pub task_id: String,
    pub task_class: String,
    /// Canonical task state (TaskState::as_str) from the kernel fold.
    pub state: String,
    /// First non-terminal step in spec order (display-only derivation from
    /// canonical per-step statuses; not a state machine).
    pub current_step: String,
    /// Worker holding the task's active lease, if any.
    pub active_lease_worker: String,
    pub latest_generation: i64,
}

/// One step row inside the task detail view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepVm {
    pub step_id: String,
    /// Canonical step status from get_current_status_map (pending/ready/
    /// dispatched/started/committed/rejected). "-" when unknown.
    pub status: String,
}

/// Full task detail — every field sourced from kernel queries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskDetailVm {
    pub task_id: String,
    pub task_class: String,
    pub task_state: String,
    pub has_exec_spec: bool,
    pub spec_id: String,
    pub steps: Vec<StepVm>,
    pub leases: Vec<LeaseVm>,
    pub events: Vec<EventDetailRow>,
    pub effects: Vec<EffectVm>,
    pub artifacts: Vec<ArtifactVm>,
    pub replay_valid: bool,
    pub replay_violations: Vec<String>,
    pub latest_generation: i64,
    /// Display projection of canonical step statuses: first non-terminal
    /// step in spec order, or "-" when everything is terminal.
    pub current_step: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseVm {
    pub lease_id: String,
    pub step_id: String,
    pub worker_id: String,
    pub state: String,
    pub acquired_generation: i64,
    pub expires_at_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectVm {
    pub effect_id: String,
    pub step_id: String,
    pub state: String,
    pub reservation_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVm {
    pub artifact_id: String,
    pub step_id: String,
    pub artifact_type: String,
    pub source_generation: i64,
}

/// WORKERS screen row — observable lease-derived status. Worker capability
/// inference is a documented kernel limitation (L3) and is deliberately NOT
/// presented as authorization here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerVm {
    pub worker_id: String,
    pub active_leases: usize,
    pub current_task: String,
    pub current_step: String,
    pub max_expires_at_generation: i64,
    pub observed_states: String,
}

/// Replay/integrity report for one task — produced by the kernel's own
/// validator, never by UI logic.
///
/// `check_error` is semantically distinct from `violations`: it means the
/// check itself could not run (DB access failure). The UI must render
/// REPLAY CHECK FAILED for this case, never REPLAY INVALID.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplayReportVm {
    pub task_id: String,
    pub valid: bool,
    pub violations: Vec<String>,
    pub check_error: Option<String>,
    pub event_count: usize,
    pub min_generation: i64,
    pub max_generation: i64,
}

/// SYSTEM screen data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemVm {
    pub db_path: String,
    /// Whether the database answered a read probe this refresh.
    pub db_accessible: bool,
    /// Read-probe failure reason, when not accessible.
    pub db_error: Option<String>,
    pub total_events: i64,
    pub causal_units: i64,
    pub max_generation: i64,
    pub tasks_in_event_log: i64,
    pub total_tasks: usize,
    pub total_leases: usize,
    pub schema_tables: Vec<String>,
    pub kernel_version: String,
}

/// Integrity self-check result — produced by the canonical kernel
/// `api::integrity_json_report`, which exercises the snapshot/restore
/// machinery against a disposable scratch DB and NEVER touches the user's
/// database. `ok=false` means the check itself could not complete; the UI
/// must not present that as "integrity OK".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegrityVm {
    pub ok: bool,
    pub summary: String,
}

/// Everything the UI needs for one render, collected in a single pass.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub dashboard: DashboardData,
    pub tasks: Vec<TaskRowVm>,
    pub workers: Vec<WorkerVm>,
    pub events: Vec<EventDetailRow>,
    pub system: SystemVm,
    pub detail: Option<TaskDetailVm>,
    pub replay: Option<ReplayReportVm>,
    /// On-demand integrity self-check result (present only after a refresh
    /// requested with run_integrity).
    pub integrity: Option<IntegrityVm>,
    /// Query-level error (rendered by the UI; never panics).
    pub error: Option<String>,
}

/// Parameters describing WHAT to refresh (owned by the UI model, sent to the
/// background worker).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshParams {
    pub selected_task: Option<String>,
    pub event_task_filter: Option<String>,
    pub event_limit: u32,
    pub replay_task: Option<String>,
    /// Run the canonical integrity self-check during this refresh.
    pub run_integrity: bool,
}

/// Read-only + safe control adapter bound to one explicit database path.
#[derive(Debug, Clone)]
pub struct KernelAdapter {
    db: String,
}

const OBSERVED_TABLES: &[&str] = &[
    "event_log",
    "effect_ledger",
    "leases",
    "tasks",
    "step_status",
    "step_dependencies",
    "state_snapshots",
    "generations",
    "replay_capsules",
    "semantic_artifacts",
    "execution_cache",
    "bias_artifacts",
    "semantic_bias_artifacts",
    "external_effects",
];

impl KernelAdapter {
    pub fn new(db: impl Into<String>) -> Self {
        Self { db: db.into() }
    }

    pub fn db_path(&self) -> &str {
        &self.db
    }

    fn storage(&self) -> providers::storage::DefaultStorage {
        providers::storage_for(&self.db)
    }

    // ── Observation ────────────────────────────────────────────────────

    fn task_rows(&self) -> Result<Vec<TaskRowVm>> {
        let storage = self.storage();
        let tasks = storage.list_tasks()?;
        let leases = storage.list_leases().unwrap_or_default();
        let mut rows = Vec::with_capacity(tasks.len());
        for task in &tasks {
            let state = storage
                .task_state(&task.task_id)
                .map(task_state_str)
                .map(str::to_string)
                .unwrap_or_else(|_| "unknown".to_string());
            let status_map = storage
                .get_current_status_map(&task.task_id)
                .unwrap_or_default();
            let current_step = current_step_display(&storage, &task.task_id, &status_map);
            let active_lease_worker = leases
                .iter()
                .find(|l| l.task_id == task.task_id && l.state == "active")
                .map(|l| l.worker_id.clone())
                .unwrap_or_else(|| "-".to_string());
            let latest_generation = storage
                .latest_generation_for_task(&task.task_id)
                .unwrap_or(0);
            rows.push(TaskRowVm {
                task_id: task.task_id.clone(),
                task_class: task.task_class.clone(),
                state,
                current_step,
                active_lease_worker,
                latest_generation,
            });
        }
        Ok(rows)
    }

    fn worker_rows(&self) -> Result<Vec<WorkerVm>> {
        let storage = self.storage();
        let leases = storage.list_leases()?;
        let mut workers: Vec<WorkerVm> = Vec::new();
        for lease in &leases {
            let entry = workers.iter_mut().find(|w| w.worker_id == lease.worker_id);
            match entry {
                Some(w) => {
                    if lease.state == "active" {
                        w.active_leases += 1;
                        w.current_task = lease.task_id.clone();
                        w.current_step = lease.step_id.clone();
                    }
                    w.max_expires_at_generation =
                        w.max_expires_at_generation.max(lease.expires_at_generation);
                    if !w.observed_states.contains(&lease.state) {
                        if !w.observed_states.is_empty() {
                            w.observed_states.push(',');
                        }
                        w.observed_states.push_str(&lease.state);
                    }
                }
                None => workers.push(WorkerVm {
                    worker_id: lease.worker_id.clone(),
                    active_leases: if lease.state == "active" { 1 } else { 0 },
                    current_task: if lease.state == "active" {
                        lease.task_id.clone()
                    } else {
                        "-".to_string()
                    },
                    current_step: if lease.state == "active" {
                        lease.step_id.clone()
                    } else {
                        "-".to_string()
                    },
                    max_expires_at_generation: lease.expires_at_generation,
                    observed_states: lease.state.clone(),
                }),
            }
        }
        workers.sort_by(|a, b| a.worker_id.cmp(&b.worker_id));
        Ok(workers)
    }

    fn system_info(&self) -> SystemVm {
        let storage = self.storage();
        // The reachability probe is a real read (print_stats). If the
        // database cannot answer, report that explicitly — never silently
        // present zeros as if the DB were healthy.
        let (total_events, causal_units, max_generation, tasks_in_log) = match storage.print_stats()
        {
            Ok(s) => s,
            Err(e) => {
                return SystemVm {
                    db_path: self.db.clone(),
                    db_accessible: false,
                    db_error: Some(e.to_string()),
                    kernel_version: env!("CARGO_PKG_VERSION").to_string(),
                    ..Default::default()
                };
            }
        };
        let schema_tables = OBSERVED_TABLES
            .iter()
            .filter(|t| storage.table_exists(t))
            .map(|t| t.to_string())
            .collect();
        let total_leases = storage.list_leases().map(|l| l.len()).unwrap_or(0);
        SystemVm {
            db_path: self.db.clone(),
            db_accessible: true,
            db_error: None,
            total_events,
            causal_units,
            max_generation,
            tasks_in_event_log: tasks_in_log,
            total_tasks: storage.list_tasks().map(|t| t.len()).unwrap_or(0),
            total_leases,
            schema_tables,
            kernel_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    fn dashboard(&self, tasks: &[TaskRowVm], workers: &[WorkerVm]) -> DashboardData {
        let storage = self.storage();
        let (total_events, causal_units, max_generation, _) =
            storage.print_stats().unwrap_or((0, 0, 0, 0));
        let mut d = DashboardData {
            db_path: self.db.clone(),
            total_tasks: tasks.len(),
            total_events,
            causal_units,
            max_generation,
            workers: workers.len(),
            ..Default::default()
        };
        for t in tasks {
            match t.state.as_str() {
                "pending" => d.pending_tasks += 1,
                "running" => d.running_tasks += 1,
                "completed" => d.completed_tasks += 1,
                "failed" => d.failed_tasks += 1,
                _ => {}
            }
        }
        for w in workers {
            d.active_leases += w.active_leases;
        }
        d.replay_checked_tasks = tasks.len();
        d.replay_valid_tasks = tasks
            .iter()
            .filter(|t| storage.replay_validate(&t.task_id))
            .count();
        d
    }

    /// Task detail — canonical state only.
    fn task_detail(&self, task_id: &str) -> Result<TaskDetailVm> {
        let storage = self.storage();
        let tasks = storage.list_tasks()?;
        let task = tasks.iter().find(|t| t.task_id == task_id);

        let task_state = storage
            .task_state(task_id)
            .map(|s| s.as_str().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        // Steps in canonical spec order with canonical statuses.
        let (spec_id, mut steps) = match storage.load_exec_spec(task_id) {
            Ok(spec) => {
                let status_map = storage.get_current_status_map(task_id).unwrap_or_default();
                let steps = spec
                    .steps
                    .iter()
                    .map(|s| StepVm {
                        step_id: s.step_id.clone(),
                        status: status_map
                            .get(&s.step_id)
                            .cloned()
                            .unwrap_or_else(|| "pending".to_string()),
                    })
                    .collect();
                (spec.spec_id.clone(), steps)
            }
            Err(_) => {
                // No spec: fall back to whatever statuses exist (still kernel
                // data, not UI inference).
                let status_map = storage.get_current_status_map(task_id).unwrap_or_default();
                let steps: Vec<StepVm> = status_map
                    .iter()
                    .map(|(k, v)| StepVm {
                        step_id: k.clone(),
                        status: v.clone(),
                    })
                    .collect();
                (String::new(), steps)
            }
        };
        steps.sort_by(|a, b| a.step_id.cmp(&b.step_id));

        let leases = storage
            .list_leases()?
            .into_iter()
            .filter(|l| l.task_id == task_id)
            .map(|l| LeaseVm {
                lease_id: l.lease_id,
                step_id: l.step_id,
                worker_id: l.worker_id,
                state: l.state,
                acquired_generation: l.acquired_generation,
                expires_at_generation: l.expires_at_generation,
            })
            .collect();

        let events = storage.event_rows(Some(task_id), 200).unwrap_or_default();

        let effects = storage
            .effect_ledger_rows(task_id)
            .unwrap_or_default()
            .into_iter()
            .map(|e| EffectVm {
                effect_id: e.effect_id,
                step_id: e.step_id,
                state: e.state,
                reservation_generation: e.reservation_generation,
            })
            .collect();

        let artifacts = storage
            .list_semantic_artifacts(task_id, None)
            .unwrap_or_default()
            .into_iter()
            .map(|a| ArtifactVm {
                artifact_id: a.artifact_id.to_string(),
                step_id: a.step_id.clone(),
                artifact_type: a.artifact_type.clone(),
                source_generation: a.source_generation,
            })
            .collect();

        let violations = storage.replay_violations(task_id).unwrap_or_default();
        let replay_valid = violations.is_empty() && storage.replay_validate(task_id);

        // Display projection from canonical statuses (same helper the Tasks
        // screen uses) — never derived from event text.
        let status_map: std::collections::BTreeMap<String, String> = steps
            .iter()
            .map(|s| (s.step_id.clone(), s.status.clone()))
            .collect();
        let current_step = current_step_display(&storage, task_id, &status_map);

        Ok(TaskDetailVm {
            task_id: task_id.to_string(),
            task_class: task.map(|t| t.task_class.clone()).unwrap_or_default(),
            task_state,
            has_exec_spec: task.map(|t| t.has_exec_spec).unwrap_or(false),
            spec_id,
            steps,
            leases,
            events,
            effects,
            artifacts,
            replay_valid,
            replay_violations: violations,
            latest_generation: storage.latest_generation_for_task(task_id).unwrap_or(0),
            current_step,
        })
    }

    fn replay_report(&self, task_id: &str) -> ReplayReportVm {
        let storage = self.storage();
        // A query failure means the check could not run at all. That is
        // REPLAY CHECK FAILED, not REPLAY INVALID.
        let violations = match storage.replay_violations(task_id) {
            Ok(v) => v,
            Err(e) => {
                return ReplayReportVm {
                    task_id: task_id.to_string(),
                    valid: false,
                    violations: Vec::new(),
                    check_error: Some(e.to_string()),
                    event_count: 0,
                    min_generation: 0,
                    max_generation: 0,
                };
            }
        };
        let valid = violations.is_empty() && storage.replay_validate(task_id);
        // Operator metadata about the validated task (bounded read, display
        // only — the verdict itself comes from the kernel fold).
        let rows = storage
            .event_rows(Some(task_id), 10_000)
            .unwrap_or_default();
        let event_count = rows.len();
        let min_generation = rows.iter().map(|r| r.system_generation).min().unwrap_or(0);
        let max_generation = rows.iter().map(|r| r.system_generation).max().unwrap_or(0);
        ReplayReportVm {
            task_id: task_id.to_string(),
            valid,
            violations,
            check_error: None,
            event_count,
            min_generation,
            max_generation,
        }
    }

    /// Collect one full snapshot. Errors are captured into Snapshot.error —
    /// the UI must remain functional against a broken/empty DB.
    pub fn collect(&self, params: &RefreshParams) -> Snapshot {
        let mut snap = Snapshot::default();

        let tasks = match self.task_rows() {
            Ok(t) => t,
            Err(e) => {
                snap.error = Some(format!("tasks query failed: {e}"));
                Vec::new()
            }
        };
        let workers = match self.worker_rows() {
            Ok(w) => w,
            Err(e) => {
                snap.error = Some(format!("workers query failed: {e}"));
                Vec::new()
            }
        };

        snap.dashboard = self.dashboard(&tasks, &workers);
        snap.tasks = tasks;
        snap.workers = workers;

        let event_limit = if params.event_limit == 0 {
            200
        } else {
            params.event_limit
        };
        snap.events = self
            .storage()
            .event_rows(params.event_task_filter.as_deref(), event_limit)
            .unwrap_or_default();

        snap.system = self.system_info();

        if let Some(task) = &params.selected_task {
            match self.task_detail(task) {
                Ok(d) => snap.detail = Some(d),
                Err(e) => snap.error = Some(format!("task detail failed: {e}")),
            }
        }
        if let Some(task) = &params.replay_task {
            snap.replay = Some(self.replay_report(task));
        }
        if params.run_integrity {
            snap.integrity = Some(self.integrity_report());
        }
        snap
    }

    // ── Safe control operations (canonical kernel paths only) ──────────

    /// Insert a Generic task through the canonical storage API, then schedule
    /// it through the kernel scheduler facade. No state is invented here.
    pub fn submit_task(&self, task_id: &str) -> Result<()> {
        let storage = self.storage();
        storage.insert_task(task_id, "Generic", "")?;
        deterministic_ai_kernel::scheduler::schedule(&self.db, task_id)
    }

    /// Run the canonical scheduler pass for one task.
    pub fn schedule_task(&self, task_id: &str) -> Result<()> {
        deterministic_ai_kernel::scheduler::schedule(&self.db, task_id)
    }

    /// Rebuild the snapshot for one task via the canonical kernel function.
    pub fn rebuild_snapshot(&self, task_id: &str) -> Result<()> {
        self.storage().rebuild_snapshot(task_id, true)
    }

    /// Run the canonical kernel integrity self-check. This exercises the
    /// snapshot/restore machinery against a disposable scratch DB via
    /// `api::integrity_json_report` and NEVER touches the user's database.
    /// A failure here means the check could not complete (ok=false) — the UI
    /// must present that distinctly from a passing check, never as "OK".
    pub fn integrity_report(&self) -> IntegrityVm {
        match deterministic_ai_kernel::api::integrity_json_report(&self.db) {
            Ok(v) => {
                let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                let summary = if ok {
                    format!(
                        "snapshot_version={} schema_version={} state_hash={} state={}",
                        v.get("snapshot_version")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0),
                        v.get("schema_version")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0),
                        if v.get("state_hash_present")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false)
                        {
                            "present"
                        } else {
                            "MISSING"
                        },
                        if v.get("state_present")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false)
                        {
                            "present"
                        } else {
                            "MISSING"
                        },
                    )
                } else {
                    "integrity report returned ok=false".to_string()
                };
                IntegrityVm { ok, summary }
            }
            Err(e) => IntegrityVm {
                ok: false,
                summary: format!("integrity check failed: {e}"),
            },
        }
    }
}

/// Display-only helper: first non-terminal step in spec order, else "-".
/// This is a projection of canonical statuses for display — not state.
fn current_step_display(
    storage: &providers::storage::DefaultStorage,
    task_id: &str,
    status_map: &std::collections::BTreeMap<String, String>,
) -> String {
    let ordered: Vec<String> = match storage.load_exec_spec(task_id) {
        Ok(spec) => spec.steps.iter().map(|s| s.step_id.clone()).collect(),
        Err(_) => status_map.keys().cloned().collect(),
    };
    for step in ordered {
        match status_map.get(&step).map(|s| s.as_str()) {
            Some("committed") | Some("rejected") => continue,
            Some(_) | None => return step,
        }
    }
    "-".to_string()
}

/// Map a kernel TaskState to the UI string (kept explicit so tests can pin
/// the mapping; the string itself comes from the kernel type).
pub fn task_state_str(state: TaskState) -> &'static str {
    state.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_db(name: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("dak_tui_test_{name}_{nanos}.db"))
            .to_string_lossy()
            .into_owned()
    }

    fn cleanup(db: &str) {
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_file(format!("{db}-wal"));
        let _ = std::fs::remove_file(format!("{db}-shm"));
    }

    /// Seed a real canonical lifecycle through the production kernel APIs
    /// (storage + scheduler/worker facades), exactly like the CLI does.
    fn seed_lifecycle(db: &str, task: &str, mode: &str) {
        let storage = providers::storage_for(db);
        storage
            .insert_task(task, "Generic", "")
            .expect("insert task");
        deterministic_ai_kernel::scheduler::schedule(db, task).expect("schedule");

        let worker = "worker-planner";
        storage.claim_worker(task, worker).expect("claim");
        storage
            .start_step(task, worker, "00_analyze_task")
            .expect("start");
        match mode {
            "commit" => {
                storage
                    .complete_step(task, worker, "00_analyze_task")
                    .expect("complete");
                // commit the remaining steps so the task reaches the
                // canonical terminal "completed" state
                deterministic_ai_kernel::scheduler::schedule(db, task).expect("sched2");
                storage.claim_worker(task, worker).expect("claim2");
                storage
                    .start_step(task, worker, "01_plan_execution")
                    .expect("start2");
                storage
                    .complete_step(task, worker, "01_plan_execution")
                    .expect("complete2");
                deterministic_ai_kernel::scheduler::schedule(db, task).expect("sched3");
                storage
                    .claim_worker(task, "worker-executor")
                    .expect("claim3");
                storage
                    .start_step(task, "worker-executor", "02_execute_changes")
                    .expect("start3");
                storage
                    .complete_step(task, "worker-executor", "02_execute_changes")
                    .expect("complete3");
            }
            "terminal" => {
                storage
                    .fail_step(task, worker, "00_analyze_task", "fatal: boom")
                    .expect("fail");
            }
            "retryable" => {
                storage
                    .fail_step(task, worker, "00_analyze_task", "retry: transient")
                    .expect("fail");
            }
            "running" => {} // leave the step started
            _ => unreachable!(),
        }
    }

    #[test]
    fn task_state_mapping_uses_kernel_strings() {
        assert_eq!(task_state_str(TaskState::Pending), "pending");
        assert_eq!(task_state_str(TaskState::Running), "running");
        assert_eq!(task_state_str(TaskState::Completed), "completed");
        assert_eq!(task_state_str(TaskState::Failed), "failed");
    }

    #[test]
    fn adapter_is_bound_to_explicit_db() {
        let a = KernelAdapter::new("/tmp/some_db.sqlite");
        assert_eq!(a.db_path(), "/tmp/some_db.sqlite");
        let b = KernelAdapter::new("/tmp/other_db.sqlite");
        assert_ne!(a.db_path(), b.db_path());
    }

    #[test]
    fn adapter_task_states_match_kernel_fold() {
        let db = unique_db("states");
        seed_lifecycle(&db, "task-done", "commit");
        seed_lifecycle(&db, "task-failed", "terminal");
        seed_lifecycle(&db, "task-retry", "retryable");
        seed_lifecycle(&db, "task-running", "running");

        let adapter = KernelAdapter::new(db.clone());
        let storage = providers::storage_for(&db);

        let rows = adapter.task_rows().expect("task rows");
        assert_eq!(rows.len(), 4);
        for row in &rows {
            // The UI string must equal the kernel's canonical TaskState.
            let kernel_state = storage.task_state(&row.task_id).expect("kernel state");
            assert_eq!(
                row.state,
                task_state_str(kernel_state),
                "task {}",
                row.task_id
            );
        }

        let by_id = |id: &str| rows.iter().find(|r| r.task_id == id).unwrap().clone();
        assert_eq!(by_id("task-done").state, "completed");
        assert_eq!(by_id("task-failed").state, "failed");
        // retryable failure keeps the task non-terminal
        assert_eq!(by_id("task-retry").state, "running");
        assert_eq!(by_id("task-running").state, "running");

        cleanup(&db);
    }

    #[test]
    fn adapter_detail_steps_match_kernel_status_map() {
        let db = unique_db("detail");
        seed_lifecycle(&db, "task-detail", "commit");

        let adapter = KernelAdapter::new(&db);
        let storage = providers::storage_for(&db);

        let detail = adapter.task_detail("task-detail").expect("detail");
        assert_eq!(detail.task_id, "task-detail");
        assert_eq!(detail.task_state, "completed"); // all steps committed

        let kernel_map = storage.get_current_status_map("task-detail").expect("map");
        // Every rendered step status must equal the canonical kernel map.
        assert_eq!(detail.steps.len(), 3);
        for step in &detail.steps {
            let kernel_status = kernel_map
                .get(&step.step_id)
                .map(String::as_str)
                .unwrap_or("pending");
            assert_eq!(
                step.status, kernel_status,
                "UI step status must equal the kernel status map ({})",
                step.step_id
            );
            assert_eq!(step.status, "committed");
        }
        assert!(!detail.events.is_empty(), "real events must be visible");
        assert!(detail.replay_valid, "valid lifecycle must replay VALID");

        cleanup(&db);
    }

    #[test]
    fn adapter_replay_report_surfaces_kernel_violations() {
        let db = unique_db("replay");
        seed_lifecycle(&db, "task-bad", "running");

        // Inject a malformed unit: STEP_COMPLETED sequenced before
        // STEP_STARTED (canonical-fold violation).
        {
            let conn =
                deterministic_ai_kernel::providers::storage::open_initialized(&db).expect("conn");
            conn.execute(
                "INSERT INTO event_log
                    (task_id, causal_unit_id, sequence_in_unit, event_type, payload,
                     system_generation, logical_generation, step_id)
                 VALUES
                    ('task-bad', 9800, 0, 'STEP_COMPLETED',
                     '{\"step_id\":\"00_analyze_task\",\"outcome\":\"Success\"}',
                     9800, 9800, '00_analyze_task'),
                    ('task-bad', 9800, 1, 'STEP_STARTED',
                     '{\"step_id\":\"00_analyze_task\"}',
                     9800, 9800, '00_analyze_task')",
                [],
            )
            .expect("inject");
        }

        let adapter = KernelAdapter::new(&db);
        let report = adapter.replay_report("task-bad");
        assert!(!report.valid, "malformed lifecycle must be INVALID");
        assert!(
            !report.violations.is_empty(),
            "violations must come from the kernel fold"
        );

        cleanup(&db);
    }

    #[test]
    fn adapter_db_routing_isolates_databases() {
        let db_a = unique_db("route_a");
        let db_b = unique_db("route_b");
        seed_lifecycle(&db_a, "only-in-a", "commit");

        let adapter_a = KernelAdapter::new(db_a.clone());
        let adapter_b = KernelAdapter::new(db_b.clone());

        let tasks_a = adapter_a.task_rows().expect("a");
        let tasks_b = adapter_b.task_rows().expect("b");
        assert_eq!(tasks_a.len(), 1);
        assert!(
            tasks_b.is_empty(),
            "adapter B must not see adapter A's data"
        );

        let snap_b = adapter_b.collect(&RefreshParams::default());
        assert!(
            snap_b.error.is_none(),
            "fresh empty DB must not be an error"
        );
        assert_eq!(snap_b.dashboard.total_tasks, 0);

        cleanup(&db_a);
        cleanup(&db_b);
    }

    #[test]
    fn adapter_collect_never_panics_on_unreadable_db() {
        // A path whose parent cannot be created must surface as an error in
        // the snapshot, never as a panic.
        let adapter = KernelAdapter::new("/proc/definitely-not-writable/db.sqlite");
        let snap = adapter.collect(&RefreshParams::default());
        assert!(snap.error.is_some());
    }

    #[test]
    fn adapter_workers_reflect_leases() {
        let db = unique_db("workers");
        seed_lifecycle(&db, "task-w", "running");

        let adapter = KernelAdapter::new(&db);
        let workers = adapter.worker_rows().expect("workers");
        let w = workers.iter().find(|w| w.worker_id == "worker-planner");
        assert!(w.is_some(), "worker with an active lease must be visible");
        let w = w.unwrap();
        assert_eq!(w.active_leases, 1);
        assert_eq!(w.current_task, "task-w");
        assert_eq!(w.current_step, "00_analyze_task");

        cleanup(&db);
    }

    #[test]
    fn adapter_replay_report_check_failed_on_unreadable_db() {
        let adapter = KernelAdapter::new("/proc/not-a-db/x.sqlite");
        let rep = adapter.replay_report("any-task");
        assert!(!rep.valid);
        assert!(
            rep.check_error.is_some(),
            "query failure must be CHECK FAILED"
        );
        assert!(rep.violations.is_empty(), "no fabricated violations");
    }

    #[test]
    fn adapter_system_info_reports_unreadable_db() {
        let adapter = KernelAdapter::new("/proc/not-a-db/x.sqlite");
        let info = adapter.system_info();
        assert!(!info.db_accessible);
        assert!(info.db_error.is_some());
    }

    #[test]
    fn adapter_replay_report_metadata_from_seeded_db() {
        let db = unique_db("replay_meta");
        seed_lifecycle(&db, "task-meta", "commit");
        let adapter = KernelAdapter::new(db.clone());
        let rep = adapter.replay_report("task-meta");
        assert!(rep.check_error.is_none());
        assert!(rep.valid, "valid lifecycle must be VALID");
        assert!(rep.event_count > 0);
        assert!(rep.min_generation <= rep.max_generation);
        cleanup(&db);
    }

    #[test]
    fn adapter_task_detail_current_step_follows_canonical_statuses() {
        let db = unique_db("curstep");
        // commit only step 00; current step must be 01
        let storage = providers::storage_for(&db);
        storage.insert_task("t-cur", "Generic", "").expect("insert");
        deterministic_ai_kernel::scheduler::schedule(&db, "t-cur").expect("schedule");
        storage
            .claim_worker("t-cur", "worker-planner")
            .expect("claim");
        storage
            .start_step("t-cur", "worker-planner", "00_analyze_task")
            .expect("start");
        storage
            .complete_step("t-cur", "worker-planner", "00_analyze_task")
            .expect("complete");

        let adapter = KernelAdapter::new(db.clone());
        let detail = adapter.task_detail("t-cur").expect("detail");
        assert_eq!(detail.current_step, "01_plan_execution");
        cleanup(&db);
    }

    #[test]
    fn adapter_integrity_report_uses_canonical_api() {
        // integrity_json_report exercises snapshot/restore on a scratch DB
        // and never touches the user's DB. On any initialized environment it
        // must return a definitive result (ok true/false), never panic.
        let db = unique_db("integrity");
        seed_lifecycle(&db, "task-int", "commit");
        let adapter = KernelAdapter::new(db.clone());
        let rep = adapter.integrity_report();
        // The scratch-DB self-check is deterministic and should pass.
        assert!(rep.ok, "integrity self-check should pass: {}", rep.summary);
        assert!(!rep.summary.is_empty());
        cleanup(&db);
    }

    #[test]
    fn adapter_collect_runs_integrity_only_when_requested() {
        let db = unique_db("intreq");
        seed_lifecycle(&db, "task-ir", "commit");
        let adapter = KernelAdapter::new(db.clone());
        // Not requested -> no integrity result.
        let snap = adapter.collect(&RefreshParams::default());
        assert!(snap.integrity.is_none());
        // Requested -> integrity result present.
        let params = RefreshParams {
            run_integrity: true,
            ..Default::default()
        };
        let snap = adapter.collect(&params);
        assert!(snap.integrity.is_some());
        cleanup(&db);
    }

    #[test]
    fn adapter_control_ops_use_canonical_paths() {
        let db = unique_db("control");
        let adapter = KernelAdapter::new(db.clone());
        // submit_task goes through insert_task + scheduler::schedule.
        adapter.submit_task("task-ctl").expect("submit");
        let rows = adapter.task_rows().expect("rows");
        assert!(rows.iter().any(|r| r.task_id == "task-ctl"));
        // schedule_task re-runs the canonical scheduler pass idempotently.
        adapter.schedule_task("task-ctl").expect("schedule");
        // rebuild_snapshot uses the canonical kernel function.
        adapter.rebuild_snapshot("task-ctl").expect("snapshot");
        cleanup(&db);
    }
}
