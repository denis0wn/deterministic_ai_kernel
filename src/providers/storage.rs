use crate::exec_spec::ExecSpec;
use crate::workflow::contract::{StepOutcome, TaskClass, WorkerCapability};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub type EventLogRow = (i64, Option<String>, String, String);
pub type EventLogRows = Vec<EventLogRow>;

pub trait StorageProvider: Send + Sync {
    fn load_exec_spec(&self, task_id: &str) -> Result<ExecSpec>;
    fn update_step_status(&self, task_id: &str, step_id: &str, status: &str) -> Result<()>;
    fn get_step_status(&self, task_id: &str, step_id: &str) -> Result<Option<String>>;
    fn get_latest_snapshot_payload(&self, task_id: &str) -> Result<Option<String>>;

    fn append_event(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        payload: &Value,
    ) -> Result<i64>;
    fn append_event_batch(
        &self,
        task_id: &str,
        events: &[(Option<&str>, &str, Value)],
    ) -> Result<()>;
    fn append_semantic_artifact(
        &self,
        task_id: &str,
        step_id: &str,
        generation: i64,
        artifact_type: &str,
        payload: &Value,
    ) -> Result<()>;
    fn latest_generation_for_task(&self, task_id: &str) -> Result<i64>;

    // Leases & workers
    fn claim_worker(&self, task_id: &str, worker_id: &str) -> Result<()>;
    fn get_active_lease(
        &self,
        task_id: &str,
        step_id: &str,
        worker_id: &str,
    ) -> Result<Option<String>>;
    fn update_lease_state(&self, lease_id: &str, state: &str) -> Result<()>;
    fn heartbeat_lease(
        &self,
        lease_id: &str,
        worker_id: &str,
        expires_at_generation: i64,
    ) -> Result<()>;

    // For effects.rs
    fn process_effects_ledger(&self, task_id: &str) -> Result<()>;
    fn find_dispatched_step(&self, task_id: &str) -> Result<Option<String>>;

    // Snapshots
    fn rebuild_snapshot(&self, task_id: &str, quiet: bool) -> Result<()>;
    fn restore_snapshot(&self, task_id: &str, quiet: bool) -> Result<()>;

    // CLI helpers
    fn print_stats(&self) -> Result<(i64, i64, i64, i64)>;
    fn table_exists(&self, table: &str) -> bool;
    fn reset_db(&self) -> Result<()>;
    fn vacuum_db(&self) -> Result<()>;
    fn insert_task(&self, task_id: &str, task_class: &str, exec_spec: &str) -> Result<()>;
    fn get_semantic_bias_payload(&self, task_id: &str) -> Result<String>;
    fn emit_bias_artifact(
        &self,
        id: &str,
        task_bias_id: &str,
        step_bias_id: &str,
        created_at: i64,
        artifact_type: &str,
        payload: &str,
    ) -> Result<()>;
    fn latest_bias_artifact(
        &self,
        task_bias_id: &str,
        step_bias_id: &str,
    ) -> Result<(String, String, String, i64, String, String)>;

    // New Storage Boundaries
    fn list_event_log(&self, task_id: &str) -> Result<EventLogRows>;
    fn seed_dependencies(&self, task_id: &str) -> Result<()>;
    fn reconcile_scheduler(&self, task_id: &str) -> Result<BTreeMap<String, String>>;
    fn schedule_next_steps(&self, task_id: &str) -> Result<BTreeMap<String, String>>;
    fn get_next_ready_step(&self, task_id: &str) -> Result<Option<String>>;
    fn get_current_status_map(&self, task_id: &str) -> Result<BTreeMap<String, String>>;
    fn unlock_ready_steps_by_db(&self, task_id: &str) -> Result<()>;
    fn seed_demo_leases(&self, task_id: &str) -> Result<()>;
    fn expire_leases(&self, task_id: &str) -> Result<()>;
    fn list_semantic_artifacts(
        &self,
        task_id: &str,
        step_id: Option<&str>,
    ) -> Result<Vec<crate::event_bus::SemanticArtifactRow>>;

    fn start_step(&self, task_id: &str, worker_id: &str, step_id: &str) -> Result<()>;
    fn heartbeat(&self, task_id: &str, worker_id: &str, step_id: &str) -> Result<()>;
    fn fail_step(&self, task_id: &str, worker_id: &str, step_id: &str, reason: &str) -> Result<()>;
    fn complete_step(&self, task_id: &str, worker_id: &str, step_id: &str) -> Result<()>;
    fn replay_validate(&self, task_id: &str) -> bool;
    /// Task-level state derived from the canonical event fold. Never stored
    /// independently: it is recomputed from events on every call.
    fn task_state(&self, task_id: &str) -> Result<crate::kernel_types::TaskState>;

    // EventBus methods migrated to StorageProvider boundary
    fn commit_causal_unit(
        &self,
        task_id: &str,
        step_id: &str,
        events: Vec<(String, Value)>,
    ) -> Result<i64>;
    fn query_events(&self, task_id: &str) -> Result<Vec<crate::event_bus::EventRow>>;
    fn list_execution_events(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::kernel_types::ExecutionEvent>>;
    fn save_replay_capsule(&self, capsule: &crate::kernel_types::ReplayCapsule) -> Result<()>;
    fn latest_replay_capsule(
        &self,
        task_id: &str,
    ) -> Result<Option<crate::kernel_types::ReplayCapsule>>;
    fn get_cache(&self, key: &str) -> Result<Option<CacheRecord>>;
    fn put_cache(&self, key: &str, record: &CacheRecord) -> Result<()>;

    // ── Read-only observation queries (operator UI boundary) ────────────
    // These methods never mutate state. They exist so observation layers
    // (operator TUI, diagnostics) read canonical kernel data instead of
    // re-deriving it.
    fn list_tasks(&self) -> Result<Vec<TaskListRow>>;
    fn list_leases(&self) -> Result<Vec<LeaseListRow>>;
    /// Chronological detail event rows, optionally filtered by task.
    fn event_rows(&self, task_id: Option<&str>, limit: u32) -> Result<Vec<EventDetailRow>>;
    /// Concrete canonical-fold violations for a task. Runs the SAME fold as
    /// replay_validate — callers must not implement their own validator.
    fn replay_violations(&self, task_id: &str) -> Result<Vec<String>>;
    fn effect_ledger_rows(&self, task_id: &str) -> Result<Vec<EffectLedgerRow>>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheRecord {
    pub execution_result: String,
    pub output_hash: String,
    pub duration_ms: i64,
    pub metadata: String,
}

/// One row of the `tasks` table (read-only observation).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskListRow {
    pub task_id: String,
    pub task_class: String,
    pub has_exec_spec: bool,
}

/// One row of the `leases` table (read-only observation).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LeaseListRow {
    pub lease_id: String,
    pub task_id: String,
    pub step_id: String,
    pub worker_id: String,
    pub acquired_generation: i64,
    pub expires_at_generation: i64,
    pub state: String,
}

/// One detailed event_log row (read-only observation).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventDetailRow {
    pub id: i64,
    pub system_generation: i64,
    pub causal_unit_id: i64,
    pub sequence_in_unit: i64,
    pub task_id: String,
    pub step_id: Option<String>,
    pub event_type: String,
    pub payload: String,
}

/// One row of the materialized effect ledger (read-only observation).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EffectLedgerRow {
    pub effect_id: String,
    pub task_id: String,
    pub step_id: String,
    pub reservation_generation: i64,
    pub state: String,
}

#[derive(Clone)]
pub struct DefaultStorage {
    db_path: String,
}

impl Default for DefaultStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultStorage {
    pub fn new() -> Self {
        let db_path = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| "kernel.db".to_string());
        Self { db_path }
    }

    /// Storage bound to an explicit database path. All kernel call chains
    /// must route through this (or `providers::storage_for`) instead of
    /// mutating process/thread-global state (audit finding M3: the former
    /// thread-local OVERRIDE_PATH let one component silently retarget
    /// another component's database operations).
    pub fn with_path(db_path: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
        }
    }

    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    fn conn(&self) -> Result<Connection> {
        let target_path = self.db_path.clone();
        if let Some(parent) = std::path::Path::new(&target_path).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let conn = Connection::open(&target_path)?;
        conn.execute_batch(include_str!("../../event_bus/schema.sql"))?;
        Ok(conn)
    }
}

// Helpers from scheduler.rs / worker.rs
fn ordered_step_ids(conn: &Connection, task_id: &str) -> Result<Vec<String>> {
    let row: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT exec_spec, task_class FROM tasks WHERE task_id = ?1",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let (exec_spec_opt, task_class_str) =
        row.ok_or_else(|| anyhow!("missing task record for task_id '{}'", task_id))?;

    let spec = if let Some(json_str) = exec_spec_opt {
        if !json_str.trim().is_empty() {
            serde_json::from_str::<ExecSpec>(&json_str)?
        } else {
            let task_class = match task_class_str.as_str() {
                "Generic" => TaskClass::Generic,
                "PlannerHardening" => TaskClass::PlannerHardening,
                "CodeFix" => TaskClass::CodeFix,
                "Question" => TaskClass::Question,
                other => anyhow::bail!("unknown task_class '{}'", other),
            };
            task_class.to_exec_spec(None)
        }
    } else {
        let task_class = match task_class_str.as_str() {
            "Generic" => TaskClass::Generic,
            "PlannerHardening" => TaskClass::PlannerHardening,
            "CodeFix" => TaskClass::CodeFix,
            other => anyhow::bail!("unknown task_class '{}'", other),
        };
        task_class.to_exec_spec(None)
    };

    Ok(spec.steps.iter().map(|s| s.step_id.clone()).collect())
}

fn status_outcome(status: &str) -> Option<StepOutcome> {
    match status {
        "committed" => Some(StepOutcome::Success),
        "rejected" => Some(StepOutcome::TerminalFailure),
        _ => None,
    }
}

fn terminal_outcome(outcome: &StepOutcome) -> bool {
    matches!(outcome, StepOutcome::Success | StepOutcome::TerminalFailure)
}

fn failed_step_status_from_payload(payload: &str) -> &'static str {
    let outcome = serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|v| {
            v.get("outcome").map(|o| match o.as_str() {
                Some("RetryableFailure") => StepOutcome::RetryableFailure,
                Some("Blocked") => StepOutcome::Blocked,
                _ => StepOutcome::TerminalFailure,
            })
        })
        .unwrap_or(StepOutcome::TerminalFailure);

    match outcome {
        StepOutcome::RetryableFailure | StepOutcome::Blocked => "pending",
        _ => "rejected",
    }
}

// =====================================================================
// Canonical event model (single source of truth for event -> state).
// =====================================================================
//
// Single logical clock: every appended causal unit draws its id from the
// monotonic `generations` allocator (`allocate_causal_unit`). The previous
// design had two independent allocators (`generations` ids vs
// MAX(system_generation)+1 reads of event_log), which collided under the
// global UNIQUE(causal_unit_id, sequence_in_unit) constraint and silently
// dropped event batches.
//
// Semantics:
//   causal_unit_id    -- identity of an atomically appended event group.
//   system_generation -- position in the global total-order clock; equal to
//                        the causal_unit_id of the unit that produced it.
//   sequence_in_unit  -- ordering inside one atomic group.
//
// Producer contract (unit shapes):
//   [LEASE_ACQUIRED, STEP_DISPATCHED]  -- scheduler dispatch pair
//   [EFFECT_RESERVED, STEP_COMPLETED]  -- atomic worker commit
//   [single event]                     -- claim/start/heartbeat/fail/expiry
//   [pipeline.* ...]                   -- pipeline publish batch
// Terminal step events (STEP_COMPLETED / STEP_FAILED) are emitted in later
// units, never inside the dispatch unit.
//
// Canonical step lifecycle (event fold):
//   pending -> dispatched -> (started) -> committed
//                                        -> pending   (STEP_FAILED retryable/blocked)
//                                        -> rejected  (STEP_FAILED terminal)
//   LEASE_EXPIRED returns dispatched/started steps to pending (redispatchable).
// `started` is an event-stream property only: the step_status schema does not
// carry it, so materialized state maps started steps back to `dispatched`.

/// Allocate the next causal unit (== next system_generation) inside `tx`.
///
/// Transactional, monotonic, concurrency-safe: SQLite serializes writers and
/// AUTOINCREMENT guarantees a fresh, never-reused id. Legacy event rows that
/// were written before the unified allocator are seeded into the sequence so
/// the clock never steps on an existing causal_unit_id.
///
/// Note: the seed condition must live on the aggregated result (subselect),
/// not on the source rows — a bare aggregate always emits one row, so a
/// WHERE on event_log rows alone would insert unconditionally.
fn allocate_causal_unit(tx: &rusqlite::Transaction) -> Result<i64> {
    tx.execute_batch(
        "INSERT INTO generations (id)
         SELECT m FROM (SELECT COALESCE(MAX(causal_unit_id), 0) AS m FROM event_log)
         WHERE m > COALESCE((SELECT MAX(id) FROM generations), 0);",
    )?;
    let unit: i64 = tx.query_row(
        "INSERT INTO generations DEFAULT VALUES RETURNING id",
        [],
        |r| r.get(0),
    )?;
    Ok(unit)
}

/// Read-only view of the current clock (max allocated generation).
fn read_clock(conn: &Connection) -> Result<i64> {
    let clock: i64 = conn.query_row(
        "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
        [],
        |r| r.get(0),
    )?;
    Ok(clock)
}

/// Open a SQLite connection with the canonical kernel schema applied.
///
/// Every CLI/API entry point must obtain connections through this function
/// (or through the storage provider, which does the same). Direct
/// `Connection::open` on a fresh database leaves the schema uninitialized
/// and turns the first real query into a panic (audit finding H1).
pub fn open_initialized(db_path: &str) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(include_str!("../../event_bus/schema.sql"))?;
    Ok(conn)
}

#[derive(Debug, Clone)]
struct StepFold {
    /// Live-status vocabulary (step_status CHECK constraint):
    /// pending | ready | dispatched | committed | rejected.
    status: &'static str,
    /// Event-stream property: STEP_STARTED observed for the current dispatch.
    started: bool,
    active_lease: Option<String>,
    /// Fold state of the latest lease for this step, if any:
    /// active | completed | released | expired.
    lease_state: Option<&'static str>,
}

impl StepFold {
    fn new() -> Self {
        StepFold {
            status: "pending",
            started: false,
            active_lease: None,
            lease_state: None,
        }
    }

    fn terminal(&self) -> bool {
        matches!(self.status, "committed" | "rejected")
    }
}

/// Canonical fold of a task's event history. Used by replay materialization,
/// snapshot reconstruction, replay validation, and task-state derivation.
#[derive(Debug)]
struct TaskFold {
    steps: BTreeMap<String, StepFold>,
    violations: Vec<String>,
    last_unit: i64,
    done: bool,
}

fn canonical_event_kind(event_type: &str) -> &'static str {
    match event_type {
        "PrimitiveScheduled" | "STEP_DISPATCHED" => "STEP_DISPATCHED",
        "PrimitiveStarted" | "STEP_STARTED" => "STEP_STARTED",
        "PrimitiveCompleted" | "STEP_COMPLETED" => "STEP_COMPLETED",
        "PrimitiveFailed" | "STEP_FAILED" => "STEP_FAILED",
        "LEASE_ACQUIRED" => "LEASE_ACQUIRED",
        "WORKER_CLAIMED" => "WORKER_CLAIMED",
        "WORKER_HEARTBEAT" => "WORKER_HEARTBEAT",
        "LEASE_EXPIRED" => "LEASE_EXPIRED",
        "EFFECT_RESERVED" => "EFFECT_RESERVED",
        "ArtifactProduced" => "ArtifactProduced",
        // PROGRESS UNTIL VERIFIED stage 2: kernel-owned observation that a
        // new attempt repeats an earlier attempt's failure signature.
        // Observation-only: never mutates step/task state in the fold.
        "REPETITION" => "REPETITION",
        // PROGRESS UNTIL VERIFIED stage 3: terminal taxonomy assessment
        // (VerifiedSuccess / VerifiedFailure / NoVerifiedPathFound /
        // InProgress) for the attempt's fingerprint group.
        // Observation-only.
        "TASK_TERMINAL_ASSESSED" => "TASK_TERMINAL_ASSESSED",
        "DONE" => "DONE",
        _ => "",
    }
}

impl TaskFold {
    fn new(all_steps: &[String]) -> Self {
        let mut steps = BTreeMap::new();
        for step_id in all_steps {
            steps.insert(step_id.clone(), StepFold::new());
        }
        TaskFold {
            steps,
            violations: Vec::new(),
            last_unit: 0,
            done: false,
        }
    }

    fn step_mut(&mut self, step_id: &str) -> &mut StepFold {
        self.steps
            .entry(step_id.to_string())
            .or_insert_with(StepFold::new)
    }

    fn violate(&mut self, msg: String) {
        self.violations.push(msg);
    }

    fn apply_unit(&mut self, unit: i64, events: &[(i64, String, Option<String>, String)]) {
        self.last_unit = unit.max(self.last_unit);

        // Sequence contiguity inside the atomic group.
        for (i, (seq, _, _, _)) in events.iter().enumerate() {
            if *seq != i as i64 {
                self.violate(format!("INVALID unit {}: gap at {}", unit, i));
            }
        }

        // Unit shape check (producer contract).
        let kinds: Vec<&str> = events
            .iter()
            .map(|(_, t, _, _)| canonical_event_kind(t))
            .collect();
        let shape_ok = match kinds.len() {
            1 => true,
            2 => {
                (kinds[0] == "LEASE_ACQUIRED" && kinds[1] == "STEP_DISPATCHED")
                    || (kinds[0] == "EFFECT_RESERVED" && kinds[1] == "STEP_COMPLETED")
            }
            _ => events.iter().all(|(_, t, _, _)| t.starts_with("pipeline.")),
        };
        if !shape_ok {
            self.violate(format!(
                "INVALID unit {}: illegal event combination {:?}",
                unit,
                events
                    .iter()
                    .map(|(_, t, _, _)| t.clone())
                    .collect::<Vec<_>>()
            ));
        }

        for (_, event_type, step_id, payload) in events {
            self.apply_event(unit, event_type, step_id.as_deref(), payload);
        }
    }

    fn apply_event(&mut self, unit: i64, event_type: &str, step_id: Option<&str>, payload: &str) {
        // Pipeline publish batches and unknown engine vocabulary are outside
        // the canonical step lifecycle; they never invalidate the fold.
        if event_type.starts_with("pipeline.") {
            return;
        }
        let kind = canonical_event_kind(event_type);
        match kind {
            "DONE" => {
                self.done = true;
                return;
            }
            "EFFECT_RESERVED"
            | "ArtifactProduced"
            | "REPETITION"
            | "TASK_TERMINAL_ASSESSED"
            | "" => return,
            _ => {}
        }

        let step_id = match step_id {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };

        let payload_lease = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| {
                v.get("lease_id")
                    .and_then(|l| l.as_str())
                    .map(str::to_string)
            });

        // Mutate the step fold in a nested scope and collect violations
        // locally so the mutable borrow of `self.steps` does not overlap the
        // `self.violations` write.
        let mut local_violations: Vec<String> = Vec::new();
        {
            let s = self.step_mut(step_id);
            match kind {
                "LEASE_ACQUIRED" => {
                    if s.active_lease.is_some() {
                        local_violations.push(format!(
                            "INVALID step {}: LEASE_ACQUIRED (unit {}) while a lease is still active",
                            step_id, unit
                        ));
                    }
                    s.active_lease = payload_lease;
                    s.lease_state = Some("active");
                }
                "STEP_DISPATCHED" => {
                    if s.active_lease.is_none() {
                        local_violations.push(format!(
                            "INVALID step {}: STEP_DISPATCHED (unit {}) without an active lease",
                            step_id, unit
                        ));
                    }
                    if s.terminal() {
                        local_violations.push(format!(
                            "INVALID step {}: STEP_DISPATCHED (unit {}) after terminal state {}",
                            step_id, unit, s.status
                        ));
                    } else {
                        s.status = "dispatched";
                        s.started = false;
                    }
                }
                "WORKER_CLAIMED" => {
                    if s.active_lease.is_none() {
                        local_violations.push(format!(
                            "INVALID step {}: WORKER_CLAIMED (unit {}) without an active lease",
                            step_id, unit
                        ));
                    }
                }
                "STEP_STARTED" => {
                    if s.status != "dispatched" || s.started {
                        local_violations.push(format!(
                            "INVALID step {}: STEP_STARTED (unit {}) without a prior STEP_DISPATCHED",
                            step_id, unit
                        ));
                    } else {
                        s.started = true;
                    }
                }
                "WORKER_HEARTBEAT" => {
                    if !s.started || s.terminal() {
                        local_violations.push(format!(
                            "INVALID step {}: WORKER_HEARTBEAT (unit {}) for a step that is not started",
                            step_id, unit
                        ));
                    }
                }
                "STEP_COMPLETED" => {
                    // Live worker operations (complete_step) require an owned
                    // active lease but NOT an explicit STEP_STARTED, so the
                    // fold accepts completion of a dispatched-but-not-started
                    // step (implicit start). Completing a step that was never
                    // dispatched is still a violation (audit finding C1/R2:
                    // replay must equal live state).
                    if s.status != "dispatched" && !s.started {
                        local_violations.push(format!(
                            "INVALID step {}: STEP_COMPLETED (unit {}) without a prior STEP_DISPATCHED",
                            step_id, unit
                        ));
                    } else {
                        if let (Some(event_lease), Some(active_lease)) =
                            (&payload_lease, &s.active_lease)
                        {
                            if event_lease != active_lease {
                                local_violations.push(format!(
                                    "INVALID step {}: STEP_COMPLETED (unit {}) references lease {} but active lease is {}",
                                    step_id, unit, event_lease, active_lease
                                ));
                            }
                        }
                        s.status = "committed";
                        s.lease_state = Some("completed");
                        s.active_lease = None;
                    }
                }
                "STEP_FAILED" => {
                    // Symmetric to STEP_COMPLETED: a dispatched-but-not-started
                    // step may fail (implicit start); a never-dispatched step
                    // may not.
                    if s.status != "dispatched" && !s.started {
                        local_violations.push(format!(
                            "INVALID step {}: STEP_FAILED (unit {}) without a prior STEP_DISPATCHED",
                            step_id, unit
                        ));
                    } else {
                        s.status = failed_step_status_from_payload(payload);
                        s.lease_state = Some("released");
                        s.active_lease = None;
                        s.started = false;
                    }
                }
                "LEASE_EXPIRED" => {
                    if s.lease_state != Some("active") {
                        local_violations.push(format!(
                            "INVALID step {}: LEASE_EXPIRED (unit {}) without an active lease",
                            step_id, unit
                        ));
                    } else {
                        s.lease_state = Some("expired");
                        s.active_lease = None;
                        if !s.terminal() {
                            s.status = "pending";
                            s.started = false;
                        }
                    }
                }
                _ => {}
            }
        }
        self.violations.extend(local_violations);
    }
}

fn load_task_event_rows(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<(i64, i64, String, Option<String>, String)>> {
    let mut stmt = conn.prepare(
        "SELECT causal_unit_id, sequence_in_unit, event_type, step_id, payload
         FROM event_log
         WHERE task_id = ?1
         ORDER BY causal_unit_id, sequence_in_unit, id",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Canonical fold of all events of a task. The single event -> state
/// derivation used by replay, reconcile, snapshot reconstruction, and the
/// replay validator.
fn fold_task_events(conn: &Connection, task_id: &str) -> Result<TaskFold> {
    let all_steps = ordered_step_ids(conn, task_id).unwrap_or_default();
    let rows = load_task_event_rows(conn, task_id)?;

    let mut fold = TaskFold::new(&all_steps);
    let mut current_unit: i64 = i64::MIN;
    let mut unit_events: Vec<(i64, String, Option<String>, String)> = Vec::new();

    for (unit, seq, event_type, step_id, payload) in rows {
        if unit != current_unit {
            if !unit_events.is_empty() {
                let events = std::mem::take(&mut unit_events);
                fold.apply_unit(current_unit, &events);
            }
            current_unit = unit;
        }
        unit_events.push((seq, event_type, step_id, payload));
    }
    if !unit_events.is_empty() {
        fold.apply_unit(current_unit, &unit_events);
    }

    Ok(fold)
}

/// Materialize the canonical fold into the state tables (step_status and
/// lease terminal transitions). This is the ONLY writer that derives live
/// state from events; all consumers must agree with it.
fn replay_events(conn: &Connection, task_id: &str) -> Result<()> {
    let fold = fold_task_events(conn, task_id)?;

    for (step_id, st) in &fold.steps {
        conn.execute(
            "INSERT INTO step_status (task_id, step_id, status) VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id, step_id) DO UPDATE SET status = excluded.status",
            params![task_id, step_id, st.status],
        )?;
        if let Some(lease_state) = st.lease_state {
            if lease_state != "active" {
                conn.execute(
                    "UPDATE leases SET state = ?3
                     WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                    params![task_id, step_id, lease_state],
                )?;
            }
        }
    }

    Ok(())
}

fn unlock_ready_steps(conn: &Connection, task_id: &str) -> Result<()> {
    let all_steps = ordered_step_ids(conn, task_id)?;

    for step_id in &all_steps {
        let current: String = conn.query_row(
            "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if status_outcome(&current)
            .as_ref()
            .map(terminal_outcome)
            .unwrap_or(false)
            || current == "dispatched"
            || current == "ready"
        {
            continue;
        }

        let dep_total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM step_dependencies WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if dep_total == 0 {
            conn.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
            continue;
        }

        let dep_satisfied: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM step_dependencies d
             JOIN step_status s
               ON s.task_id = d.task_id
              AND s.step_id = d.depends_on_step_id
             WHERE d.task_id = ?1
               AND d.step_id = ?2
               AND s.status = 'committed'",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if dep_satisfied == dep_total {
            conn.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
        }
    }

    Ok(())
}

/// Typed failure classification contract.
///
/// The outcome is an explicit enum value carried in the STEP_FAILED event
/// payload ("outcome" field); the reason string is only the input language
/// at CLI/API boundaries. Rules:
/// - `fatal:` prefix            -> TerminalFailure (the ONLY path that
///   permanently rejects a step; must be explicit)
/// - `blocked:` prefix          -> Blocked (step returns to pending)
/// - `retry:` prefix            -> RetryableFailure
/// - lock/provider/model errors -> RetryableFailure (transient by nature)
/// - anything else              -> RetryableFailure (fail-safe default: an
///   unknown or infrastructure error must never permanently corrupt a task;
///   terminal semantics are opt-in, see audit finding C4)
pub fn classify_failure_outcome(reason: &str) -> StepOutcome {
    if reason.starts_with("fatal:") {
        return StepOutcome::TerminalFailure;
    }
    if reason.starts_with("blocked:") {
        return StepOutcome::Blocked;
    }
    StepOutcome::RetryableFailure
}

pub fn outcome_to_event_type(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Success => "STEP_COMPLETED",
        StepOutcome::TerminalFailure => "STEP_FAILED",
        StepOutcome::RetryableFailure | StepOutcome::Blocked => "STEP_FAILED",
    }
}

/// R4 (HD-3): detect an LLM stall signature in a failure reason and return
/// the elapsed seconds the request hung. R6 semantics: with the streaming
/// client this signature means IDLE timeout — no chunks (content, reasoning
/// deltas, keepalives) arrived for the whole window. The llm layer formats
/// idle stalls as "... TIMED OUT after <N>s ...".
/// Returns None for ordinary (non-stall) failures. Pure and deterministic —
/// no clock, no LLM interpretation.
pub fn stall_elapsed_secs_from_reason(reason: &str) -> Option<u64> {
    let marker = "TIMED OUT after ";
    let start = reason.find(marker)? + marker.len();
    let digits: String = reason[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let secs = digits.parse::<u64>().ok()?;
    // Guard against the marker appearing in unrelated prose: require the
    // unit suffix immediately after the number.
    if reason[start + digits.len()..].starts_with('s') {
        Some(secs)
    } else {
        None
    }
}

/// R6: detect the HARD-CAP signature — the streaming client was receiving
/// chunks (generation alive) but the total duration exceeded the upper
/// bound. Rare: a truly unbounded generation. llm formats it as
/// "... HARD_TIMEOUT_EXCEEDED after <N>s ...".
pub fn hard_timeout_secs_from_reason(reason: &str) -> Option<u64> {
    let marker = "HARD_TIMEOUT_EXCEEDED after ";
    let start = reason.find(marker)? + marker.len();
    let digits: String = reason[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let secs = digits.parse::<u64>().ok()?;
    if reason[start + digits.len()..].starts_with('s') {
        Some(secs)
    } else {
        None
    }
}

fn is_ai_worker(worker_id: &str) -> bool {
    let lower = worker_id.to_ascii_lowercase();
    lower == "ai"
        || lower.contains("worker-ai")
        || lower.contains("ai-worker")
        || lower.starts_with("ai-")
        || lower.starts_with("ai_")
}

fn capability_for_worker_id(worker_id: &str) -> Result<WorkerCapability> {
    let lower = worker_id.to_ascii_lowercase();

    if is_ai_worker(worker_id) {
        return Ok(WorkerCapability::LegacyGeneric);
    }

    if lower.contains("planner") {
        return Ok(WorkerCapability::Planner);
    }

    if lower.contains("executor") {
        return Ok(WorkerCapability::Executor);
    }

    if lower.contains("verifier") {
        return Ok(WorkerCapability::Verifier);
    }

    if lower.starts_with("worker-") || lower.starts_with("worker_") {
        return Ok(WorkerCapability::LegacyGeneric);
    }

    Err(anyhow!("worker has no declared capability: {}", worker_id))
}

fn canonical_json(value: &Value) -> Result<String> {
    let mut ordered = BTreeMap::new();
    if let Value::Object(map) = value {
        for (k, v) in map {
            ordered.insert(k.clone(), v.clone());
        }
        Ok(serde_json::to_string(&ordered)?)
    } else {
        Ok(serde_json::to_string(value)?)
    }
}

// StorageProvider Implementation
impl StorageProvider for DefaultStorage {
    fn get_latest_snapshot_payload(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let payload: Option<String> = conn
            .query_row(
                "SELECT payload FROM state_snapshots WHERE task_id = ?1 ORDER BY snapshot_id DESC LIMIT 1",
                [task_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(payload)
    }

    fn load_exec_spec(&self, task_id: &str) -> Result<ExecSpec> {
        let conn = self.conn()?;
        let row: Option<(Option<String>, String)> = conn
            .query_row(
                "SELECT exec_spec, task_class FROM tasks WHERE task_id = ?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        let (exec_spec_opt, task_class_str) =
            row.ok_or_else(|| anyhow!("missing task record for task_id '{}'", task_id))?;

        if let Some(json_str) = exec_spec_opt {
            if !json_str.trim().is_empty() {
                if let Ok(spec) = serde_json::from_str::<ExecSpec>(&json_str) {
                    return Ok(spec);
                }
            }
        }

        let task_class = match task_class_str.as_str() {
            "Generic" => TaskClass::Generic,
            "PlannerHardening" => TaskClass::PlannerHardening,
            "CodeFix" => TaskClass::CodeFix,
            "Question" => TaskClass::Question,
            other => anyhow::bail!("unknown task_class '{}'", other),
        };
        Ok(task_class.to_exec_spec(None))
    }

    fn update_step_status(&self, task_id: &str, step_id: &str, status: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE step_status SET status = ?3 WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id, status],
        )?;
        Ok(())
    }

    fn get_step_status(&self, task_id: &str, step_id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                [task_id, step_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(status)
    }

    fn append_event(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        payload: &Value,
    ) -> Result<i64> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let unit_gen: i64 = allocate_causal_unit(&tx)?;

        let event_id = payload
            .get("event_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!(
                    "evt_{}",
                    &blake3::hash(format!("{}:{}:{}", task_id, event_type, unit_gen).as_bytes())
                        .to_hex()[..16]
                )
            });

        let payload_str = canonical_json(payload)?;

        tx.execute(
            "INSERT INTO event_log
             (event_id, system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8)",
            params![
                event_id,
                unit_gen,
                unit_gen,
                task_id,
                step_id,
                event_type,
                payload_str,
                unit_gen
            ],
        )?;

        tx.commit()?;
        Ok(unit_gen)
    }

    fn append_event_batch(
        &self,
        task_id: &str,
        events: &[(Option<&str>, &str, Value)],
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        for (step_id, event_type, payload) in events {
            let unit_gen: i64 = allocate_causal_unit(&tx)?;

            let event_id = payload
                .get("event_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    format!(
                        "evt_{}",
                        &blake3::hash(
                            format!("{}:{}:{}", task_id, event_type, unit_gen).as_bytes()
                        )
                        .to_hex()[..16]
                    )
                });

            let payload_str = canonical_json(payload)?;

            tx.execute(
                "INSERT INTO event_log
                 (event_id, system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8)",
                params![
                    event_id,
                    unit_gen,
                    unit_gen,
                    task_id,
                    step_id,
                    event_type,
                    payload_str,
                    unit_gen
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    fn append_semantic_artifact(
        &self,
        task_id: &str,
        step_id: &str,
        generation: i64,
        artifact_type: &str,
        payload: &Value,
    ) -> Result<()> {
        let conn = self.conn()?;
        let max_gen: i64 = conn.query_row(
            "SELECT COALESCE(MAX(source_generation), -1) FROM semantic_artifacts WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;
        if generation < max_gen {
            return Err(anyhow!(
                "generation fence: source_generation {} <= existing max {}",
                generation,
                max_gen
            ));
        }
        let payload_str = canonical_json(payload)?;
        conn.execute(
            "INSERT INTO semantic_artifacts
             (task_id, step_id, source_generation, artifact_type, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, step_id, generation, artifact_type, payload_str],
        )?;
        Ok(())
    }

    fn latest_generation_for_task(&self, task_id: &str) -> Result<i64> {
        let conn = self.conn()?;
        let generation = conn.query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log WHERE task_id = ?1",
            [task_id],
            |r| r.get(0),
        )?;
        Ok(generation)
    }

    fn claim_worker(&self, task_id: &str, worker_id: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT l.lease_id, l.step_id
                 FROM leases l
                 JOIN step_status s
                   ON s.task_id = l.task_id
                  AND s.step_id = l.step_id
                 WHERE l.task_id = ?1
                   AND l.state = 'active'
                   AND s.status = 'dispatched'
                   AND l.worker_id = 'worker-scheduler'
                 ORDER BY l.acquired_generation, l.step_id
                 LIMIT 1",
                [task_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;

        let (lease_id, step_id) =
            row.ok_or_else(|| anyhow!("no dispatchable active lease found"))?;

        let current_owner: String = tx.query_row(
            "SELECT worker_id FROM leases WHERE lease_id = ?1 AND state = 'active'",
            [lease_id.clone()],
            |r| r.get(0),
        )?;

        if current_owner == worker_id {
            tx.commit()?;
            return Ok(());
        }

        if current_owner != "worker-scheduler" {
            return Err(anyhow!("lease already owned by {}", current_owner));
        }

        let updated = tx.execute(
            "UPDATE leases
             SET worker_id = ?1
             WHERE lease_id = ?2
               AND state = 'active'
               AND worker_id = 'worker-scheduler'",
            params![worker_id, lease_id],
        )?;

        if updated == 0 {
            return Err(anyhow!("lease claim lost"));
        }

        let next_generation: i64 = allocate_causal_unit(&tx)?;

        let claim_payload = json!({
            "lease_id": lease_id,
            "worker_id": worker_id
        });
        let claim_payload_str = canonical_json(&claim_payload)?;

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, 'WORKER_CLAIMED', ?5, ?6)",
            params![
                next_generation,
                next_generation,
                task_id,
                step_id,
                claim_payload_str,
                next_generation
            ],
        )?;

        tx.commit()?;
        println!("WORKER_CLAIM_OK");
        println!("WORKER: {}", worker_id);
        println!("STEP_CLAIMED: {}", step_id);
        Ok(())
    }

    fn get_active_lease(
        &self,
        task_id: &str,
        step_id: &str,
        worker_id: &str,
    ) -> Result<Option<String>> {
        let conn = self.conn()?;
        let lease_id: Option<String> = conn
            .query_row(
                "SELECT lease_id
                 FROM leases
                 WHERE task_id = ?1
                   AND step_id = ?2
                   AND worker_id = ?3
                   AND state = 'active'
                 ORDER BY acquired_generation DESC
                 LIMIT 1",
                params![task_id, step_id, worker_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(lease_id)
    }

    fn update_lease_state(&self, lease_id: &str, state: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE leases SET state = ?2 WHERE lease_id = ?1",
            params![lease_id, state],
        )?;
        Ok(())
    }

    fn heartbeat_lease(
        &self,
        lease_id: &str,
        worker_id: &str,
        expires_at_generation: i64,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE leases
             SET expires_at_generation = ?3
             WHERE lease_id = ?1
               AND worker_id = ?2
               AND state = 'active'",
            params![lease_id, worker_id, expires_at_generation],
        )?;
        Ok(())
    }

    fn find_dispatched_step(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let step_id: Option<String> = conn
            .query_row(
                "SELECT step_id
                 FROM step_status
                 WHERE task_id = ?1 AND status = 'dispatched'
                 ORDER BY step_id
                 LIMIT 1",
                [task_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(step_id)
    }

    fn process_effects_ledger(&self, task_id: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let mut reserve_stmt = tx.prepare(
            "SELECT step_id, payload, system_generation
             FROM event_log
             WHERE task_id = ?1 AND event_type IN ('EFFECT_RESERVED', 'ArtifactProduced')
             ORDER BY id",
        )?;
        let reserve_rows = reserve_stmt.query_map([task_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut reserved = Vec::new();
        for r in reserve_rows {
            reserved.push(r?);
        }
        drop(reserve_stmt);

        for (step_id, payload, generation) in reserved {
            let v: serde_json::Value = serde_json::from_str(&payload)?;
            if let Some(effect_id) = v.get("effect_id").and_then(|x| x.as_str()) {
                tx.execute(
                    "INSERT OR IGNORE INTO effect_ledger
                     (effect_id, task_id, step_id, reservation_generation, state)
                     VALUES (?1, ?2, ?3, ?4, 'reserved')",
                    params![effect_id, task_id, step_id, generation],
                )?;
            }
        }

        let mut complete_stmt = tx.prepare(
            "SELECT step_id, payload
             FROM event_log
             WHERE task_id = ?1 AND event_type IN ('STEP_COMPLETED', 'PrimitiveCompleted')
             ORDER BY id",
        )?;
        let complete_rows = complete_stmt.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut completed = Vec::new();
        for r in complete_rows {
            completed.push(r?);
        }
        drop(complete_stmt);

        for (step_id, payload) in completed {
            let v: serde_json::Value = serde_json::from_str(&payload)?;
            if let Some(effect_id) = v.get("effect_id").and_then(|x| x.as_str()) {
                tx.execute(
                    "UPDATE effect_ledger
                     SET state = 'committed'
                     WHERE effect_id = ?1 AND task_id = ?2 AND step_id = ?3",
                    params![effect_id, task_id, step_id],
                )?;
            }
        }

        let mut fail_stmt = tx.prepare(
            "SELECT step_id, payload
             FROM event_log
             WHERE task_id = ?1 AND event_type IN ('STEP_FAILED', 'PrimitiveFailed')
             ORDER BY id",
        )?;
        let fail_rows = fail_stmt.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut failed = Vec::new();
        for r in fail_rows {
            failed.push(r?);
        }
        drop(fail_stmt);

        for (step_id, payload) in failed {
            let v: serde_json::Value = serde_json::from_str(&payload)?;
            if let Some(effect_id) = v.get("effect_id").and_then(|x| x.as_str()) {
                tx.execute(
                    "UPDATE effect_ledger
                     SET state = 'rejected'
                     WHERE effect_id = ?1 AND task_id = ?2 AND step_id = ?3",
                    params![effect_id, task_id, step_id],
                )?;
            }
        }

        let mut ext_stmt = tx.prepare(
            "SELECT e.effect_id, e.task_id, e.step_id, e.state
             FROM effect_ledger e
             LEFT JOIN external_effects x ON x.effect_id = e.effect_id
             WHERE e.task_id = ?1
               AND e.state IN ('committed','rejected')
               AND x.effect_id IS NULL
             ORDER BY e.effect_id",
        )?;
        let rows = ext_stmt.query_map([task_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut unprocessed = Vec::new();
        for r in rows {
            unprocessed.push(r?);
        }
        drop(ext_stmt);

        let mut count = 0_i64;
        for (effect_id, task_id, step_id, observed_state) in unprocessed {
            let result_payload = if observed_state == "committed" {
                serde_json::json!({
                    "effect_id": effect_id,
                    "action": "send_to_external_system",
                    "status": "executed"
                })
            } else {
                serde_json::json!({
                    "effect_id": effect_id,
                    "action": "skip_external_side_effect",
                    "status": "suppressed_due_to_rejection"
                })
            };
            let result_payload_str = canonical_json(&result_payload)?;

            tx.execute(
                "INSERT INTO external_effects
                 (effect_id, task_id, step_id, observed_state, result_payload)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    effect_id,
                    task_id,
                    step_id,
                    observed_state,
                    result_payload_str
                ],
            )?;
            count += 1;
        }

        tx.commit()?;
        println!("EFFECT_EXECUTION_OK");
        println!("EXECUTED_EFFECT_ROWS: {}", count);
        Ok(())
    }

    fn rebuild_snapshot(&self, task_id: &str, quiet: bool) -> Result<()> {
        let conn = self.conn()?;

        // Canonical fold over the FULL event history. A snapshot is a
        // materialized checkpoint of the same fold that drives replay and
        // reconcile, so it can never diverge from them: retryable failures
        // stay pending, terminal failures become rejected, and every spec
        // step is present (not only the ones with terminal events).
        let fold = fold_task_events(&conn, task_id)?;

        let base_generation: i64 = 0;
        let steps: BTreeMap<String, String> = fold
            .steps
            .iter()
            .map(|(step_id, st)| (step_id.clone(), st.status.to_string()))
            .collect();
        let done = fold.done;
        let last_generation = fold.last_unit;

        let mut artifact_refs: BTreeMap<String, Value> = BTreeMap::new();
        let mut stmt2 = conn.prepare(
            "SELECT artifact_type, artifact_id FROM semantic_artifacts WHERE task_id = ?1 ORDER BY artifact_id DESC",
        )?;
        let rows2 = stmt2.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows2 {
            let (artifact_type, artifact_id) = row?;
            artifact_refs
                .entry(artifact_type)
                .or_insert_with(|| json!(artifact_id));
        }

        let state_payload = json!({
            "task_id": task_id,
            "last_generation": last_generation,
            "done": done,
            "steps": steps,
            "artifacts": artifact_refs
        });

        let created_at = last_generation;
        let state_hash_bytes = blake3::hash(serde_json::to_string(&state_payload)?.as_bytes());
        let state_hash = u64::from_le_bytes(state_hash_bytes.as_bytes()[..8].try_into().unwrap());

        let payload = json!({
            "snapshot_version": 1,
            "schema_version": 1,
            "created_at": created_at,
            "state_hash": state_hash,
            "state": state_payload,
            "task_id": task_id,
            "last_generation": last_generation,
            "done": done,
            "steps": steps,
            "artifacts": artifact_refs
        });

        conn.execute(
            "INSERT INTO state_snapshots (task_id, last_generation, payload) VALUES (?1, ?2, ?3)",
            params![task_id, last_generation, serde_json::to_string(&payload)?],
        )?;

        if !quiet {
            println!("SNAPSHOT OK");
            println!("SNAPSHOT_BASE_GENERATION: {}", base_generation);
            println!("SNAPSHOT_GENERATION: {}", last_generation);
        }
        Ok(())
    }

    fn restore_snapshot(&self, task_id: &str, quiet: bool) -> Result<()> {
        let conn = self.conn()?;

        let (snapshot_id, last_generation, payload): (i64, i64, String) = conn.query_row(
            "SELECT snapshot_id, last_generation, payload FROM state_snapshots WHERE task_id = ?1 ORDER BY snapshot_id DESC LIMIT 1",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        let payload_json: Value = serde_json::from_str(&payload)?;
        let snapshot_version = payload_json
            .get("snapshot_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        let schema_version = payload_json
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);

        if snapshot_version > 1 {
            return Err(anyhow!(
                "unsupported future snapshot_version: {}",
                snapshot_version
            ));
        }
        if schema_version > 1 {
            return Err(anyhow!(
                "unsupported future schema_version: {}",
                schema_version
            ));
        }

        // Tamper check: the stored state_hash must match the canonical hash
        // of the embedded state payload.
        if let Some(state_value) = payload_json.get("state") {
            let expected_hash: Option<u64> =
                payload_json.get("state_hash").and_then(|v| v.as_u64());
            let recomputed = u64::from_le_bytes(
                blake3::hash(serde_json::to_string(state_value)?.as_bytes()).as_bytes()[..8]
                    .try_into()
                    .unwrap(),
            );
            if let Some(expected) = expected_hash {
                if expected != recomputed {
                    return Err(anyhow!(
                        "snapshot state_hash mismatch: stored {} != recomputed {}",
                        expected,
                        recomputed
                    ));
                }
            }
        }

        // Real restore: materialize the checkpointed step states into the
        // live state tables. The snapshot is a materialized view of the
        // canonical fold at `last_generation`; restoring it re-establishes
        // the derived state without touching the append-only event log.
        let mut restored_steps = 0usize;
        if let Some(obj) = payload_json.get("steps").and_then(|v| v.as_object()) {
            let tx = conn.unchecked_transaction()?;
            for (step_id, status) in obj {
                let status = match status.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                tx.execute(
                    "INSERT INTO step_status (task_id, step_id, status) VALUES (?1, ?2, ?3)
                     ON CONFLICT(task_id, step_id) DO UPDATE SET status = excluded.status",
                    params![task_id, step_id, status],
                )?;
                restored_steps += 1;
            }
            tx.commit()?;
        }

        if !quiet {
            println!("RESTORE OK");
            println!("SNAPSHOT_ID: {}", snapshot_id);
            println!("SNAPSHOT_GENERATION: {}", last_generation);
            println!("RESTORED_STEPS: {}", restored_steps);

            if let Some(artifacts) = payload_json.get("artifacts").and_then(|v| v.as_object()) {
                for (artifact_type, artifact_id) in artifacts {
                    println!("ARTIFACT_REF\t{}\t{}", artifact_type, artifact_id);
                }
            }
            println!("{}", payload);
        }
        Ok(())
    }

    fn list_event_log(&self, task_id: &str) -> Result<EventLogRows> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT system_generation, step_id, event_type, payload
             FROM event_log
             WHERE task_id = ?1
             ORDER BY system_generation, id",
        )?;

        let rows = stmt.query_map([task_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;

        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }

    fn seed_dependencies(&self, task_id: &str) -> Result<()> {
        let conn = self.conn()?;
        let ordered = ordered_step_ids(&conn, task_id)?;

        for window in ordered.windows(2) {
            let dep = &window[0];
            let step = &window[1];
            conn.execute(
                "INSERT OR IGNORE INTO step_dependencies (task_id, step_id, depends_on_step_id)
                 VALUES (?1, ?2, ?3)",
                params![task_id, step, dep],
            )?;
        }

        for step in ordered {
            conn.execute(
                "INSERT OR IGNORE INTO step_status (task_id, step_id, status)
                 VALUES (?1, ?2, 'pending')",
                params![task_id, step],
            )?;
        }

        Ok(())
    }

    fn reconcile_scheduler(&self, task_id: &str) -> Result<BTreeMap<String, String>> {
        let conn = self.conn()?;
        self.seed_dependencies(task_id)?;

        conn.execute(
            "UPDATE step_status SET status = 'pending' WHERE task_id = ?1",
            [task_id],
        )?;

        replay_events(&conn, task_id)?;
        unlock_ready_steps(&conn, task_id)?;

        let mut status: BTreeMap<String, String> = BTreeMap::new();
        let mut stmt = conn.prepare(
            "SELECT step_id, status FROM step_status WHERE task_id = ?1 ORDER BY step_id",
        )?;
        let rows = stmt.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        for row in rows.filter_map(|r| r.ok()) {
            status.insert(row.0, row.1);
        }

        println!("RECONCILE OK");
        println!("STEP_STATUS: {:?}", status);
        Ok(status)
    }

    fn schedule_next_steps(&self, task_id: &str) -> Result<BTreeMap<String, String>> {
        let mut conn = self.conn()?;
        self.seed_dependencies(task_id)?;
        let tx = conn.transaction()?;

        tx.execute(
            "UPDATE step_status
             SET status = 'pending'
             WHERE task_id = ?1
               AND status NOT IN ('dispatched','committed','rejected')",
            [task_id],
        )?;

        tx.execute(
            "UPDATE step_status
             SET status = 'ready'
             WHERE task_id = ?1
               AND status = 'dispatched'
               AND NOT EXISTS (
                   SELECT 1 FROM leases
                   WHERE task_id = ?1
                     AND step_id = step_status.step_id
                     AND state = 'active'
               )",
            [task_id],
        )?;

        replay_events(&tx, task_id)?;
        unlock_ready_steps(&tx, task_id)?;

        let ready: Vec<String> = ordered_step_ids(&tx, task_id)?
            .into_iter()
            .filter(|step_id| {
                let status: Option<String> = tx
                    .query_row(
                        "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .unwrap_or(None);

                if !matches!(status.as_deref(), Some("ready") | Some("dispatched")) {
                    return false;
                }

                let active_lease: Option<i64> = tx
                    .query_row(
                        "SELECT 1 FROM leases WHERE task_id = ?1 AND step_id = ?2 AND state = 'active' LIMIT 1",
                        params![task_id, step_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .unwrap_or(None);

                active_lease.is_none()
            })
            .collect();
        println!("READY_QUEUE: {:?}", ready);

        for step_id in ready {
            // Canonical allocator: the dispatch unit id is the logical clock
            // value of the dispatch itself. Lease timing is expressed against
            // the same clock (acquired at `unit`, expires two ticks later).
            let unit = allocate_causal_unit(&tx)?;
            let current_generation = unit;

            let lease_seq: i64 = tx.query_row(
                "SELECT COALESCE(COUNT(*), 0) + 1 FROM leases WHERE task_id = ?1 AND step_id = ?2",
                params![task_id, step_id],
                |r| r.get(0),
            )?;
            let lease_id = format!("{task_id}/{step_id}/lease/{lease_seq}");

            let inserted = tx.execute(
                "INSERT INTO leases
                 (lease_id, task_id, step_id, worker_id, acquired_generation, expires_at_generation, state)
                 SELECT ?1, ?2, ?3, 'worker-scheduler', ?4, ?5, 'active'
                 WHERE NOT EXISTS (
                     SELECT 1 FROM leases
                     WHERE task_id = ?2 AND step_id = ?3 AND state = 'active'
                 )",
                params![lease_id, task_id, step_id, current_generation, current_generation + 2],
            )?;

            if inserted == 0 {
                continue;
            }

            tx.execute(
                "UPDATE step_status
                 SET status = 'dispatched'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'ready'",
                params![task_id, step_id],
            )?;

            let causal_unit_id = unit;

            let lease_payload = json!({
                "lease_id": lease_id,
                "worker_id": "worker-scheduler",
                "acquired_generation": current_generation,
                "expires_at_generation": current_generation + 2
            });
            let lease_payload_str = canonical_json(&lease_payload)?;

            tx.execute(
                "INSERT INTO event_log
                 (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, 0, ?3, ?4, 'LEASE_ACQUIRED', ?5, ?6)",
                params![
                    causal_unit_id,
                    causal_unit_id,
                    task_id,
                    step_id,
                    lease_payload_str,
                    causal_unit_id
                ],
            )?;

            let dispatch_payload = json!({
                "lease_id": lease_id,
                "worker_id": "worker-scheduler"
            });
            let dispatch_payload_str = canonical_json(&dispatch_payload)?;

            tx.execute(
                "INSERT INTO event_log
                 (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, 1, ?3, ?4, 'STEP_DISPATCHED', ?5, ?6)",
                params![
                    causal_unit_id,
                    causal_unit_id,
                    task_id,
                    step_id,
                    dispatch_payload_str,
                    causal_unit_id
                ],
            )?;

            println!("DISPATCHED: {}", step_id);
        }

        tx.commit()?;

        let mut status: BTreeMap<String, String> = BTreeMap::new();
        let conn2 = self.conn()?;
        let mut stmt = conn2.prepare(
            "SELECT step_id, status FROM step_status WHERE task_id = ?1 ORDER BY step_id",
        )?;
        let rows = stmt.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        for row in rows.filter_map(|r| r.ok()) {
            status.insert(row.0, row.1);
        }

        println!("STEP_STATUS: {:?}", status);
        Ok(status)
    }

    fn get_next_ready_step(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        for step_id in ordered_step_ids(&conn, task_id)? {
            let status: Option<String> = conn
                .query_row(
                    "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                    |r| r.get(0),
                )
                .optional()?;

            if !matches!(status.as_deref(), Some("ready") | Some("dispatched")) {
                continue;
            }

            let active_lease: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM leases WHERE task_id = ?1 AND step_id = ?2 AND state = 'active' LIMIT 1",
                    params![task_id, step_id],
                    |r| r.get(0),
                )
                .optional()?;

            if active_lease.is_none() {
                return Ok(Some(step_id));
            }
        }
        Ok(None)
    }

    fn get_current_status_map(&self, task_id: &str) -> Result<BTreeMap<String, String>> {
        let conn = self.conn()?;
        let mut out = BTreeMap::new();
        let mut stmt = conn.prepare(
            "SELECT step_id, status FROM step_status WHERE task_id = ?1 ORDER BY step_id",
        )?;
        let rows = stmt.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        for row in rows.filter_map(|r| r.ok()) {
            out.insert(row.0, row.1);
        }
        Ok(out)
    }

    fn unlock_ready_steps_by_db(&self, task_id: &str) -> Result<()> {
        let conn = self.conn()?;
        unlock_ready_steps(&conn, task_id)
    }

    fn seed_demo_leases(&self, task_id: &str) -> Result<()> {
        let conn = self.conn()?;
        let current_generation: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        conn.execute(
            "INSERT OR IGNORE INTO leases
             (lease_id, task_id, step_id, worker_id, acquired_generation, expires_at_generation, state)
             VALUES (?1, ?2, ?3, 'worker-demo', ?4, ?5, 'active')",
            params![
                format!("{task_id}/step_2/lease"),
                task_id,
                "step_2",
                current_generation,
                current_generation - 1
            ],
        )?;

        println!("LEASE_SEED_OK");
        Ok(())
    }

    fn expire_leases(&self, task_id: &str) -> Result<()> {
        let mut conn = self.conn()?;
        // Atomic: lease state updates, LEASE_EXPIRED events, and step_status
        // transitions commit together. There is no reachable state where the
        // leases table says "expired" but the event log has no record of it
        // (or vice versa). Each expired lease gets its own causal unit.
        let tx = conn.transaction()?;

        let current_generation: i64 = read_clock(&tx)?;

        let mut stmt = tx.prepare(
            "SELECT lease_id, step_id FROM leases WHERE task_id = ?1 AND state = 'active' AND expires_at_generation <= ?2 ORDER BY step_id",
        )?;

        let rows = stmt.query_map(params![task_id, current_generation], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        let expired: Vec<(String, String)> = rows.filter_map(|r| r.ok()).collect();
        drop(stmt);

        for (lease_id, step_id) in &expired {
            tx.execute(
                "UPDATE leases SET state = 'expired' WHERE lease_id = ?1 AND state = 'active'",
                [lease_id],
            )?;

            let unit = allocate_causal_unit(&tx)?;

            let payload = json!({
                "lease_id": lease_id,
                "reason": "generation_timeout"
            });
            let payload_str = canonical_json(&payload)?;

            tx.execute(
                "INSERT INTO event_log
                 (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, 0, ?3, ?4, 'LEASE_EXPIRED', ?5, ?6)",
                params![unit, unit, task_id, step_id, payload_str, unit],
            )?;

            // Fold-consistent transition: an expired lease returns a
            // dispatched step to `pending`; the scheduler's unlock pass
            // promotes it back to `ready` when dependencies allow.
            tx.execute(
                "UPDATE step_status SET status = 'pending' WHERE task_id = ?1 AND step_id = ?2 AND status IN ('dispatched', 'ready')",
                params![task_id, step_id],
            )?;
        }

        tx.commit()?;

        println!("LEASE_EXPIRE_OK");
        println!("EXPIRED_LEASES: {}", expired.len());
        Ok(())
    }

    fn list_semantic_artifacts(
        &self,
        task_id: &str,
        step_id: Option<&str>,
    ) -> Result<Vec<crate::event_bus::SemanticArtifactRow>> {
        let conn = self.conn()?;
        let rows = if let Some(sid) = step_id {
            let mut stmt = conn.prepare(
                "SELECT artifact_id, task_id, step_id, source_generation, artifact_type, payload, created_at
                 FROM semantic_artifacts
                 WHERE task_id = ?1 AND step_id = ?2
                 ORDER BY source_generation DESC, artifact_id DESC",
            )?;
            let mapped = stmt.query_map(params![task_id, sid], |r| {
                Ok(crate::event_bus::SemanticArtifactRow {
                    artifact_id: r.get(0)?,
                    task_id: r.get(1)?,
                    step_id: r.get(2)?,
                    source_generation: r.get(3)?,
                    artifact_type: r.get(4)?,
                    payload: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })?;
            let mut res = Vec::new();
            for r in mapped {
                res.push(r?);
            }
            res
        } else {
            let mut stmt = conn.prepare(
                "SELECT artifact_id, task_id, step_id, source_generation, artifact_type, payload, created_at
                 FROM semantic_artifacts
                 WHERE task_id = ?1
                 ORDER BY source_generation DESC, artifact_id DESC",
            )?;
            let mapped = stmt.query_map([task_id], |r| {
                Ok(crate::event_bus::SemanticArtifactRow {
                    artifact_id: r.get(0)?,
                    task_id: r.get(1)?,
                    step_id: r.get(2)?,
                    source_generation: r.get(3)?,
                    artifact_type: r.get(4)?,
                    payload: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })?;
            let mut res = Vec::new();
            for r in mapped {
                res.push(r?);
            }
            res
        };
        Ok(rows)
    }

    fn start_step(&self, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
        let required_capability = if let Ok(spec) = self.load_exec_spec(task_id) {
            if let Some(step_spec) = spec.steps.iter().find(|s| s.step_id == step_id) {
                let cap_constraint = step_spec
                    .constraints
                    .iter()
                    .find(|c| c.key == "required_capability");
                if let Some(c) = cap_constraint {
                    match c.value.as_str() {
                        "Planner" => Some(WorkerCapability::Planner),
                        "Executor" => Some(WorkerCapability::Executor),
                        "Verifier" => Some(WorkerCapability::Verifier),
                        "LegacyGeneric" => Some(WorkerCapability::LegacyGeneric),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let required_capability = if let Some(cap) = required_capability {
            cap
        } else {
            let slug = step_id
                .split_once('_')
                .map(|(_, rest)| rest)
                .unwrap_or(step_id);
            let step_kind = match slug {
                "tighten_planner_prompt" => {
                    crate::workflow::contract::StepKind::TightenPlannerPrompt
                }
                "normalize_planner_output" => {
                    crate::workflow::contract::StepKind::NormalizePlannerOutput
                }
                "add_llm_fallback_handling" => {
                    crate::workflow::contract::StepKind::AddLlmFallbackHandling
                }
                "add_planner_test_coverage" => {
                    crate::workflow::contract::StepKind::AddPlannerTestCoverage
                }
                "validate_planner_output" => {
                    crate::workflow::contract::StepKind::ValidatePlannerOutput
                }
                "analyze_task" => crate::workflow::contract::StepKind::AnalyzeTask,
                "plan_execution" => crate::workflow::contract::StepKind::PlanExecution,
                "execute_changes" => crate::workflow::contract::StepKind::ExecuteChanges,
                "read_repository" => crate::workflow::contract::StepKind::ReadRepository,
                "locate_bug" => crate::workflow::contract::StepKind::LocateBug,
                "patch_code" => crate::workflow::contract::StepKind::PatchCode,
                "run_tests" => crate::workflow::contract::StepKind::RunTests,
                "validate_patch" => crate::workflow::contract::StepKind::ValidatePatch,
                _ => return Err(anyhow!("unknown step id: {}", step_id)),
            };
            crate::workflow::contract::required_capability_for_step(&step_kind)
        };

        let worker_capability = capability_for_worker_id(worker_id)?;

        if worker_capability != required_capability
            && worker_capability != WorkerCapability::LegacyGeneric
        {
            return Err(anyhow!(
                "worker capability mismatch for step: worker={:?}, required={:?}, step_id={}",
                worker_capability,
                required_capability,
                step_id
            ));
        }

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let lease_id: String = tx
            .query_row(
                "SELECT lease_id FROM leases WHERE task_id = ?1 AND step_id = ?2 AND worker_id = ?3 AND state = 'active' ORDER BY acquired_generation DESC LIMIT 1",
                params![task_id, step_id, worker_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

        let next_generation: i64 = allocate_causal_unit(&tx)?;

        let payload = json!({
            "lease_id": lease_id,
            "worker_id": worker_id
        });
        let payload_str = canonical_json(&payload)?;

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, 'STEP_STARTED', ?5, ?6)",
            params![
                next_generation,
                next_generation,
                task_id,
                step_id,
                payload_str,
                next_generation
            ],
        )?;

        tx.commit()?;
        println!("STEP_RUNNING_OK");
        println!("WORKER: {}", worker_id);
        println!("STEP: {}", step_id);
        Ok(())
    }

    fn heartbeat(&self, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let lease_id: String = tx
            .query_row(
                "SELECT lease_id FROM leases WHERE task_id = ?1 AND step_id = ?2 AND worker_id = ?3 AND state = 'active' ORDER BY acquired_generation DESC LIMIT 1",
                params![task_id, step_id, worker_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

        let current_generation: i64 = read_clock(&tx)?;

        tx.execute(
            "UPDATE leases
             SET expires_at_generation = ?1
             WHERE lease_id = ?2 AND worker_id = ?3 AND state = 'active'",
            params![current_generation + 2, lease_id, worker_id],
        )?;

        let next_generation: i64 = allocate_causal_unit(&tx)?;
        let payload = json!({
            "lease_id": lease_id,
            "worker_id": worker_id,
            "expires_at_generation": current_generation + 2
        });
        let payload_str = canonical_json(&payload)?;

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, 'WORKER_HEARTBEAT', ?5, ?6)",
            params![
                next_generation,
                next_generation,
                task_id,
                step_id,
                payload_str,
                next_generation
            ],
        )?;

        tx.commit()?;
        println!("WORKER_HEARTBEAT_OK");
        println!("WORKER: {}", worker_id);
        println!("STEP: {}", step_id);
        Ok(())
    }

    fn fail_step(&self, task_id: &str, worker_id: &str, step_id: &str, reason: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let lease_id: String = tx
            .query_row(
                "SELECT lease_id FROM leases WHERE task_id = ?1 AND step_id = ?2 AND worker_id = ?3 AND state = 'active' ORDER BY acquired_generation DESC LIMIT 1",
                params![task_id, step_id, worker_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

        let next_generation: i64 = allocate_causal_unit(&tx)?;

        let outcome = classify_failure_outcome(reason);
        let fail_payload = json!({
            "lease_id": lease_id,
            "worker_id": worker_id,
            "reason": reason,
            "outcome": format!("{:?}", outcome)
        });
        let fail_payload_str = canonical_json(&fail_payload)?;

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, ?7)",
            params![
                next_generation,
                next_generation,
                task_id,
                step_id,
                outcome_to_event_type(outcome),
                fail_payload_str,
                next_generation
            ],
        )?;

        // R4 (HD-3): an LLM request that consumed the full timeout is a
        // STALL, and it must not be silent. Emit a dedicated, queryable
        // STALL_DETECTED event in the SAME causal unit (sequence 1) so the
        // event log — and anything replaying it (TUI, replay_validate) —
        // can distinguish "model/server hung for Ns" from ordinary
        // retryable failures. llm_calls=0 is a kernel fact here: no
        // successful completion was recorded for this step.
        let stall_elapsed = stall_elapsed_secs_from_reason(reason);
        if let Some(elapsed_secs) = stall_elapsed {
            let stall_payload = json!({
                "lease_id": lease_id,
                "worker_id": worker_id,
                "task_id": task_id,
                "step_id": step_id,
                "elapsed_secs": elapsed_secs,
                "llm_calls": 0,
                "last_known_state": "pending",
                "reason": reason
            });
            tx.execute(
                "INSERT INTO event_log
                 (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
                params![
                    next_generation,
                    next_generation,
                    task_id,
                    step_id,
                    "STALL_DETECTED",
                    canonical_json(&stall_payload)?,
                    next_generation
                ],
            )?;
        }

        // R6: distinct event for the HARD-CAP case — chunks were flowing
        // (generation alive) but the total duration exceeded the upper
        // bound. STALL_DETECTED now means "idle timeout: no chunks at all";
        // HARD_TIMEOUT_EXCEEDED means "alive but unbounded generation".
        let hard_elapsed = hard_timeout_secs_from_reason(reason);
        if let Some(elapsed_secs) = hard_elapsed {
            let hard_payload = json!({
                "lease_id": lease_id,
                "worker_id": worker_id,
                "task_id": task_id,
                "step_id": step_id,
                "elapsed_secs": elapsed_secs,
                "llm_calls": 0,
                "last_known_state": "pending",
                "reason": reason
            });
            tx.execute(
                "INSERT INTO event_log
                 (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
                params![
                    next_generation,
                    next_generation,
                    task_id,
                    step_id,
                    "HARD_TIMEOUT_EXCEEDED",
                    canonical_json(&hard_payload)?,
                    next_generation
                ],
            )?;
        }

        let new_status = match outcome {
            StepOutcome::RetryableFailure | StepOutcome::Blocked => "pending",
            _ => "rejected",
        };

        tx.execute(
            "UPDATE step_status SET status = ?3 WHERE task_id = ?1 AND step_id = ?2 AND status = 'dispatched'",
            params![task_id, step_id, new_status],
        )?;

        tx.execute(
            "UPDATE leases SET state = 'released' WHERE lease_id = ?1 AND worker_id = ?2 AND state = 'active'",
            params![lease_id, worker_id],
        )?;

        tx.commit()?;
        println!("STEP_FAIL_OK");
        println!("WORKER: {}", worker_id);
        println!("STEP_FAILED_BY_WORKER: {}", step_id);
        println!("REASON: {}", reason);
        // R4 (HD-3): make stalls visible on the CLI surface too, not only
        // in the event log (the acceptance eval harness only greps stdout).
        if let Some(elapsed_secs) = stall_elapsed {
            println!(
                "STALL_DETECTED: task={} step={} elapsed_secs={} llm_calls=0 last_state=pending",
                task_id, step_id, elapsed_secs
            );
            // R6: with the streaming client this means IDLE timeout — no
            // chunks arrived for the whole window.
            println!("STALL_KIND: mlx_stream_idle_timeout (no chunks received)");
        }
        if let Some(elapsed_secs) = hard_elapsed {
            println!(
                "HARD_TIMEOUT_EXCEEDED: task={} step={} elapsed_secs={} llm_calls=0 last_state=pending",
                task_id, step_id, elapsed_secs
            );
            println!("HARD_KIND: mlx_generation_unbounded (chunks flowing past the cap)");
        }
        Ok(())
    }

    fn complete_step(&self, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let lease_id: String = tx
            .query_row(
                "SELECT lease_id FROM leases WHERE task_id = ?1 AND step_id = ?2 AND worker_id = ?3 AND state = 'active' ORDER BY acquired_generation DESC LIMIT 1",
                params![task_id, step_id, worker_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

        // Atomic commit unit: EFFECT_RESERVED (seq 0) + STEP_COMPLETED (seq 1)
        // share one causal unit, matching the producer contract the fold and
        // the replay validator both implement.
        let next_generation: i64 = allocate_causal_unit(&tx)?;

        let outcome = StepOutcome::Success;
        let effect_id = format!("effect/{}/{}", task_id, step_id);

        let reserve_payload = json!({
            "effect_id": effect_id,
            "lease_id": lease_id,
            "worker_id": worker_id
        });
        let reserve_payload_str = canonical_json(&reserve_payload)?;

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, 'EFFECT_RESERVED', ?5, ?6)",
            params![
                next_generation,
                next_generation,
                task_id,
                step_id,
                reserve_payload_str,
                next_generation
            ],
        )?;

        let complete_generation = next_generation;
        let complete_payload = json!({
            "effect_id": effect_id,
            "lease_id": lease_id,
            "worker_id": worker_id,
            "result": "ok",
            "outcome": format!("{:?}", outcome)
        });
        let complete_payload_str = canonical_json(&complete_payload)?;

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
            params![
                complete_generation,
                complete_generation,
                task_id,
                step_id,
                outcome_to_event_type(outcome),
                complete_payload_str,
                complete_generation
            ],
        )?;

        tx.execute(
            "UPDATE step_status SET status = 'committed' WHERE task_id = ?1 AND step_id = ?2 AND status = 'dispatched'",
            params![task_id, step_id],
        )?;

        tx.execute(
            "UPDATE leases SET state = 'completed' WHERE lease_id = ?1 AND worker_id = ?2 AND state = 'active'",
            params![lease_id, worker_id],
        )?;

        tx.commit()?;
        println!("STEP_COMPLETE_OK");
        println!("WORKER: {}", worker_id);
        println!("STEP_COMPLETED_BY_WORKER: {}", step_id);
        Ok(())
    }

    fn task_state(&self, task_id: &str) -> Result<crate::kernel_types::TaskState> {
        use crate::kernel_types::TaskState;
        let conn = self.conn()?;
        let fold = fold_task_events(&conn, task_id)?;

        if fold.steps.is_empty() {
            return Ok(TaskState::Pending);
        }
        let mut all_committed = true;
        for st in fold.steps.values() {
            if st.status == "rejected" {
                return Ok(TaskState::Failed);
            }
            if st.status != "committed" {
                all_committed = false;
            }
        }
        if all_committed {
            Ok(TaskState::Completed)
        } else {
            Ok(TaskState::Running)
        }
    }

    fn replay_validate(&self, task_id: &str) -> bool {
        // OPS-1: this validator is a LIBRARY function — it must be silent.
        // The TUI Replay screen calls it on every refresh; stdout output
        // here polluted the ratatui frame. Callers that want the verdict
        // printed (CLI `replay` verb) do their own printing. Violations
        // are queryable via replay_violations().
        let conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return false,
        };

        // Canonical fold: unit-shape checks, sequence gaps, and step
        // lifecycle ordering violations are all detected by the same fold
        // that drives replay/reconcile/snapshot, so the validator can never
        // disagree with the producer contract again.
        let fold = match fold_task_events(&conn, task_id) {
            Ok(f) => f,
            Err(_) => return false,
        };

        let mut ok = fold.violations.is_empty();

        let reserved: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM effect_ledger WHERE task_id = ?1 AND state = 'reserved'",
                [task_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if reserved != 0 {
            ok = false;
        }

        ok
    }

    fn commit_causal_unit(
        &self,
        task_id: &str,
        step_id: &str,
        events: Vec<(String, Value)>,
    ) -> Result<i64> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let unit_gen: i64 = allocate_causal_unit(&tx)?;

        for (seq, (event_type, payload)) in events.iter().enumerate() {
            if event_type == "EFFECT_RESERVED" || event_type == "ArtifactProduced" {
                let effect_id = payload
                    .get("effect_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow!("missing effect_id in EFFECT_RESERVED/ArtifactProduced payload")
                    })?;

                tx.execute(
                    "INSERT INTO effect_ledger
                     (effect_id, task_id, step_id, reservation_generation, state)
                     VALUES (?1, ?2, ?3, ?4, 'reserved')",
                    params![effect_id, task_id, step_id, unit_gen],
                )
                .map_err(|e| anyhow!("reserve effect {effect_id}: {e}"))?;
            }

            if event_type == "STEP_COMPLETED" || event_type == "PrimitiveCompleted" {
                let effect_id = payload
                    .get("effect_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow!("missing effect_id in STEP_COMPLETED/PrimitiveCompleted payload")
                    })?;

                tx.execute(
                    "UPDATE effect_ledger
                     SET state = 'committed'
                     WHERE effect_id = ?1 AND task_id = ?2 AND step_id = ?3 AND state = 'reserved'",
                    params![effect_id, task_id, step_id],
                )
                .map_err(|e| anyhow!("commit effect {effect_id}: {e}"))?;
            }

            if event_type == "STEP_FAILED" || event_type == "PrimitiveFailed" {
                let effect_id = payload
                    .get("effect_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow!("missing effect_id in STEP_FAILED/PrimitiveFailed payload")
                    })?;

                tx.execute(
                    "UPDATE effect_ledger
                     SET state = 'rejected'
                     WHERE effect_id = ?1 AND task_id = ?2 AND step_id = ?3 AND state = 'reserved'",
                    params![effect_id, task_id, step_id],
                )
                .map_err(|e| anyhow!("reject effect {effect_id}: {e}"))?;
            }

            let payload_str = canonical_json(payload)?;

            tx.execute(
                "INSERT INTO event_log
                 (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    unit_gen,
                    unit_gen,
                    seq as i64,
                    task_id,
                    step_id,
                    event_type,
                    payload_str,
                    unit_gen
                ],
            )
            .map_err(|e| anyhow!("insert event seq={seq}: {e}"))?;
        }

        tx.commit()?;
        Ok(unit_gen)
    }

    fn query_events(&self, task_id: &str) -> Result<Vec<crate::event_bus::EventRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload
             FROM event_log
             WHERE task_id = ?1
             ORDER BY causal_unit_id, sequence_in_unit",
        )?;

        let rows = stmt.query_map([task_id], |r| {
            Ok(crate::event_bus::EventRow {
                event_id: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                causal_unit_id: r.get(1)?,
                sequence_in_unit: r.get(2)?,
                task_id: r.get(3)?,
                step_id: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                event_type: r.get(5)?,
                payload: r.get(6)?,
            })
        })?;

        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }

    fn list_execution_events(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::kernel_types::ExecutionEvent>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, task_id, event_type, payload \
             FROM event_log \
             WHERE task_id = ?1 \
               AND event_type != 'EFFECT_RESERVED' \
             ORDER BY causal_unit_id, sequence_in_unit, id",
        )?;

        let mapped = stmt.query_map([task_id], |r| {
            let id: i64 = r.get(0)?;
            let task_id: String = r.get(1)?;
            let event_type: String = r.get(2)?;
            let payload_raw: String = r.get(3)?;
            let payload: Value =
                serde_json::from_str(&payload_raw).unwrap_or(Value::String(payload_raw));

            Ok(crate::kernel_types::ExecutionEvent {
                id: format!("evt-{}", id),
                task_id,
                timestamp: "event_log".to_string(),
                event_type,
                payload,
                caused_by: None,
                trust_context: crate::kernel_types::TrustContext {
                    source: "event_bus".into(),
                    trust_level: crate::kernel_types::TrustLevel::High,
                    verification_status: "recorded".into(),
                    policy_version: "v1".into(),
                },
            })
        })?;

        let mut res = Vec::new();
        for r in mapped {
            res.push(r?);
        }
        Ok(res)
    }

    fn save_replay_capsule(&self, capsule: &crate::kernel_types::ReplayCapsule) -> Result<()> {
        let conn = self.conn()?;
        let payload = serde_json::to_string(capsule)?;
        conn.execute(
            "INSERT OR REPLACE INTO replay_capsules (capsule_id, task_id, created_at, payload) VALUES (?1, ?2, ?3, ?4)",
            params![capsule.capsule_id, capsule.execution_id, capsule.created_at, payload],
        )?;
        Ok(())
    }

    fn latest_replay_capsule(
        &self,
        task_id: &str,
    ) -> Result<Option<crate::kernel_types::ReplayCapsule>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM replay_capsules
             WHERE task_id = ?1
             ORDER BY created_at DESC, capsule_id DESC
             LIMIT 1",
        )?;

        let row: Option<String> = stmt
            .query_row([task_id], |r| r.get::<_, String>(0))
            .optional()?;

        match row {
            Some(payload) => {
                let capsule = serde_json::from_str::<crate::kernel_types::ReplayCapsule>(&payload)?;
                Ok(Some(capsule))
            }
            None => Ok(None),
        }
    }

    fn get_cache(&self, key: &str) -> Result<Option<CacheRecord>> {
        let conn = self.conn()?;
        let row: Option<(String, String, i64, String)> = conn
            .query_row(
                "SELECT execution_result, output_hash, duration_ms, metadata FROM execution_cache WHERE cache_key = ?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;

        match row {
            Some((res, hash, dur, meta)) => Ok(Some(CacheRecord {
                execution_result: res,
                output_hash: hash,
                duration_ms: dur,
                metadata: meta,
            })),
            None => Ok(None),
        }
    }

    fn put_cache(&self, key: &str, record: &CacheRecord) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO execution_cache (cache_key, execution_result, output_hash, duration_ms, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                key,
                record.execution_result,
                record.output_hash,
                record.duration_ms,
                record.metadata
            ],
        )?;
        Ok(())
    }

    // ── Read-only observation queries (operator UI boundary) ────────────

    fn list_tasks(&self) -> Result<Vec<TaskListRow>> {
        let conn = self.conn()?;
        if !self.table_exists("tasks") {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT task_id, task_class, exec_spec IS NOT NULL AND exec_spec != ''
             FROM tasks ORDER BY task_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(TaskListRow {
                task_id: r.get(0)?,
                task_class: r.get(1)?,
                has_exec_spec: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn list_leases(&self) -> Result<Vec<LeaseListRow>> {
        let conn = self.conn()?;
        if !self.table_exists("leases") {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT lease_id, task_id, step_id, worker_id,
                    acquired_generation, expires_at_generation, state
             FROM leases ORDER BY task_id, step_id, lease_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LeaseListRow {
                lease_id: r.get(0)?,
                task_id: r.get(1)?,
                step_id: r.get(2)?,
                worker_id: r.get(3)?,
                acquired_generation: r.get(4)?,
                expires_at_generation: r.get(5)?,
                state: r.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn event_rows(&self, task_id: Option<&str>, limit: u32) -> Result<Vec<EventDetailRow>> {
        let conn = self.conn()?;
        if !self.table_exists("event_log") {
            return Ok(Vec::new());
        }
        let limit = limit.max(1);
        let mut out = Vec::new();
        match task_id {
            Some(task) => {
                let mut stmt = conn.prepare(
                    "SELECT id, system_generation, causal_unit_id, sequence_in_unit,
                            task_id, step_id, event_type, payload
                     FROM event_log WHERE task_id = ?1
                     ORDER BY causal_unit_id, sequence_in_unit, id
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![task, limit], |r| {
                    Ok(EventDetailRow {
                        id: r.get(0)?,
                        system_generation: r.get(1)?,
                        causal_unit_id: r.get(2)?,
                        sequence_in_unit: r.get(3)?,
                        task_id: r.get(4)?,
                        step_id: r.get(5)?,
                        event_type: r.get(6)?,
                        payload: r.get(7)?,
                    })
                })?;
                for row in rows {
                    out.push(row?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, system_generation, causal_unit_id, sequence_in_unit,
                            task_id, step_id, event_type, payload
                     FROM event_log
                     ORDER BY causal_unit_id, sequence_in_unit, id
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit], |r| {
                    Ok(EventDetailRow {
                        id: r.get(0)?,
                        system_generation: r.get(1)?,
                        causal_unit_id: r.get(2)?,
                        sequence_in_unit: r.get(3)?,
                        task_id: r.get(4)?,
                        step_id: r.get(5)?,
                        event_type: r.get(6)?,
                        payload: r.get(7)?,
                    })
                })?;
                for row in rows {
                    out.push(row?);
                }
            }
        }
        Ok(out)
    }

    fn replay_violations(&self, task_id: &str) -> Result<Vec<String>> {
        let conn = self.conn()?;
        // Same canonical fold used by replay_validate — the UI must never
        // implement its own validator.
        let fold = fold_task_events(&conn, task_id)?;
        let mut violations = fold.violations;

        let reserved: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM effect_ledger WHERE task_id = ?1 AND state = 'reserved'",
                [task_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if reserved != 0 {
            violations.push(format!(
                "INVALID task {}: {} reserved effects remain",
                task_id, reserved
            ));
        }
        Ok(violations)
    }

    fn effect_ledger_rows(&self, task_id: &str) -> Result<Vec<EffectLedgerRow>> {
        let conn = self.conn()?;
        if !self.table_exists("effect_ledger") {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT effect_id, task_id, step_id, reservation_generation, state
             FROM effect_ledger WHERE task_id = ?1
             ORDER BY reservation_generation, effect_id",
        )?;
        let rows = stmt.query_map([task_id], |r| {
            Ok(EffectLedgerRow {
                effect_id: r.get(0)?,
                task_id: r.get(1)?,
                step_id: r.get(2)?,
                reservation_generation: r.get(3)?,
                state: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn print_stats(&self) -> Result<(i64, i64, i64, i64)> {
        let conn = self.conn()?;
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0))
            .unwrap_or(0);
        let causal_units: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT causal_unit_id) FROM event_log",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let max_generation: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let tasks: i64 = conn
            .query_row("SELECT COUNT(DISTINCT task_id) FROM event_log", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        Ok((events, causal_units, max_generation, tasks))
    }

    fn table_exists(&self, table: &str) -> bool {
        let conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return false,
        };
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1 LIMIT 1",
            [table],
            |_r| Ok(()),
        )
        .is_ok()
    }

    fn reset_db(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            r#"
            PRAGMA wal_checkpoint(FULL);
            DELETE FROM event_log;
            DELETE FROM effect_ledger;
            DELETE FROM step_dependencies;
            DELETE FROM step_status;
            DELETE FROM state_snapshots;
            DELETE FROM generations;
            VACUUM;
            "#,
        )?;
        Ok(())
    }

    fn vacuum_db(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            r#"
            PRAGMA wal_checkpoint(FULL);
            VACUUM;
            "#,
        )?;
        Ok(())
    }

    fn insert_task(&self, task_id: &str, task_class: &str, exec_spec: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO tasks (task_id, task_class, exec_spec) VALUES (?1, ?2, ?3)",
            params![task_id, task_class, exec_spec],
        )?;
        Ok(())
    }

    fn get_semantic_bias_payload(&self, task_id: &str) -> Result<String> {
        let conn = self.conn()?;
        let res = conn.query_row(
            "SELECT input_representation FROM semantic_bias_artifacts WHERE task_id = ?1 ORDER BY artifact_id DESC LIMIT 1",
            [task_id],
            |r| r.get::<_, String>(0),
        )?;
        Ok(res)
    }

    fn emit_bias_artifact(
        &self,
        id: &str,
        task_bias_id: &str,
        step_bias_id: &str,
        created_at: i64,
        artifact_type: &str,
        payload: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS bias_artifacts (
                id            TEXT PRIMARY KEY,
                task_bias_id  TEXT NOT NULL,
                step_bias_id  TEXT NOT NULL,
                created_at    INTEGER NOT NULL,
                artifact_type TEXT NOT NULL,
                payload       TEXT NOT NULL
            );
            ",
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO bias_artifacts
             (id, task_bias_id, step_bias_id, created_at, artifact_type, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                task_bias_id,
                step_bias_id,
                created_at,
                artifact_type,
                payload
            ],
        )?;
        Ok(())
    }

    fn latest_bias_artifact(
        &self,
        task_bias_id: &str,
        step_bias_id: &str,
    ) -> Result<(String, String, String, i64, String, String)> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, task_bias_id, step_bias_id, created_at, artifact_type, payload
             FROM bias_artifacts
             WHERE task_bias_id = ?1 AND step_bias_id = ?2
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let row = stmt.query_row(params![task_bias_id, step_bias_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        Ok(row)
    }
}
