use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::workflow::contract::{
    step_specs_to_steps, task_class_to_flow, terminal_outcome, Step, StepKind, StepOutcome,
    TaskClass,
};

fn step_slug(step: &Step) -> String {
    match step.kind {
        StepKind::TightenPlannerPrompt => "tighten_planner_prompt".into(),
        StepKind::NormalizePlannerOutput => "normalize_planner_output".into(),
        StepKind::AddLlmFallbackHandling => "add_llm_fallback_handling".into(),
        StepKind::AddPlannerTestCoverage => "add_planner_test_coverage".into(),
        StepKind::ValidatePlannerOutput => "validate_planner_output".into(),
        StepKind::AnalyzeTask => "analyze_task".into(),
        StepKind::PlanExecution => "plan_execution".into(),
        StepKind::ExecuteChanges => "execute_changes".into(),
        StepKind::ReadRepository => "read_repository".into(),
        StepKind::LocateBug => "locate_bug".into(),
        StepKind::PatchCode => "patch_code".into(),
        StepKind::RunTests => "run_tests".into(),
        StepKind::ValidatePatch => "validate_patch".into(),
    }
}

fn parse_task_class(raw: &str) -> Result<TaskClass> {
    match raw {
        "Generic" => Ok(TaskClass::Generic),
        "PlannerHardening" => Ok(TaskClass::PlannerHardening),
        "CodeFix" => Ok(TaskClass::CodeFix),
        other => anyhow::bail!("unknown task_class '{}' in persistence layer", other),
    }
}

fn load_task_class(conn: &Connection, task_id: &str) -> Result<TaskClass> {
    let task_class: Option<String> = conn
        .query_row(
            "SELECT task_class FROM tasks WHERE task_id = ?1",
            [task_id],
            |r| r.get(0),
        )
        .optional()?;

    let raw = task_class
        .ok_or_else(|| anyhow::anyhow!("missing task_class for task_id '{}'", task_id))?;

    parse_task_class(&raw)
}

fn ordered_step_ids(conn: &Connection, task_id: &str) -> Result<Vec<String>> {
    let flow = task_class_to_flow(load_task_class(conn, task_id)?);
    Ok(step_specs_to_steps(&flow, None)
        .into_iter()
        .enumerate()
        .map(|(i, step)| format!("{:02}_{}", i, step_slug(&step)))
        .collect())
}

fn status_outcome(status: &str) -> Option<StepOutcome> {
    match status {
        "committed" => Some(StepOutcome::Success),
        "rejected" => Some(StepOutcome::TerminalFailure),
        _ => None,
    }
}

fn failed_step_status_from_payload(payload: &str) -> &'static str {
    let outcome = serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("outcome")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    match outcome.as_deref() {
        Some("RetryableFailure") | Some("Blocked") => "pending",
        _ => "rejected",
    }
}

pub fn seed_dependencies(db: &str, task_id: &str) -> Result<()> {
    let conn = Connection::open(db)?;
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

pub fn reconcile(db: &str, task_id: &str) -> Result<()> {
    let conn = Connection::open(db)?;
    seed_dependencies(db, task_id)?;

    conn.execute(
        "UPDATE step_status SET status = 'pending' WHERE task_id = ?1",
        [task_id],
    )?;

    let mut stmt = conn.prepare(
        "SELECT step_id, event_type, payload
         FROM event_log
         WHERE task_id = ?1 AND step_id IS NOT NULL
         ORDER BY causal_unit_id, sequence_in_unit, id",
    )?;

    let rows = stmt.query_map([task_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        let (step_id, event_type, payload) = row;

        match event_type.as_str() {
            "STEP_COMPLETED" => {
                conn.execute(
                    "UPDATE step_status SET status = 'committed'
                     WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                )?;
                conn.execute(
                    "UPDATE leases
                     SET state = 'completed'
                     WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                    params![task_id, step_id],
                )?;
            }
            "STEP_FAILED" => {
                conn.execute(
                    "UPDATE step_status SET status = ?3
                     WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id, failed_step_status_from_payload(&payload)],
                )?;
                conn.execute(
                    "UPDATE leases
                     SET state = 'released'
                     WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                    params![task_id, step_id],
                )?;
            }
            "STEP_DISPATCHED" => {
                let current: String = conn.query_row(
                    "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                    |r| r.get(0),
                )?;
                if !status_outcome(&current)
                    .as_ref()
                    .map(terminal_outcome)
                    .unwrap_or(false)
                {
                    conn.execute(
                        "UPDATE step_status SET status = 'dispatched'
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                    )?;
                }
            }
            "LEASE_EXPIRED" => {
                let current: String = conn.query_row(
                    "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                    |r| r.get(0),
                )?;
                if !status_outcome(&current)
                    .as_ref()
                    .map(terminal_outcome)
                    .unwrap_or(false)
                {
                    conn.execute(
                        "UPDATE step_status SET status = 'pending'
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                    )?;
                    conn.execute(
                        "UPDATE leases
                         SET state = 'expired'
                         WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                        params![task_id, step_id],
                    )?;
                }
            }
            _ => {}
        }
    }

    let all_steps = ordered_step_ids(&conn, task_id)?;

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
            "SELECT COUNT(*) FROM step_dependencies
             WHERE task_id = ?1 AND step_id = ?2",
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

    let mut status: BTreeMap<String, String> = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT step_id, status
         FROM step_status
         WHERE task_id = ?1
         ORDER BY step_id",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        status.insert(row.0, row.1);
    }

    println!("RECONCILE OK");
    println!("STEP_STATUS: {:?}", status);
    Ok(())
}

pub fn schedule(db: &str, task_id: &str) -> Result<()> {
    let mut conn = Connection::open(db)?;
    seed_dependencies(db, task_id)?;
    let tx = conn.transaction()?;

    tx.execute(
        "UPDATE step_status SET status = 'pending' WHERE task_id = ?1 AND status NOT IN ('committed','rejected')",
        [task_id],
    )?;

    {
        let mut stmt = tx.prepare(
            "SELECT step_id, event_type, payload
             FROM event_log
             WHERE task_id = ?1 AND step_id IS NOT NULL
             ORDER BY causal_unit_id, sequence_in_unit, id",
        )?;
        let rows = stmt.query_map([task_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;

        for row in rows.filter_map(|r| r.ok()) {
            let (step_id, event_type, payload) = row;

            match event_type.as_str() {
                "STEP_COMPLETED" => {
                    tx.execute(
                        "UPDATE step_status SET status = 'committed'
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                    )?;
                    tx.execute(
                        "UPDATE leases
                         SET state = 'completed'
                         WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                        params![task_id, step_id],
                    )?;
                }
                "STEP_FAILED" => {
                    tx.execute(
                        "UPDATE step_status SET status = ?3
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id, failed_step_status_from_payload(&payload)],
                    )?;
                    tx.execute(
                        "UPDATE leases
                         SET state = 'released'
                         WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                        params![task_id, step_id],
                    )?;
                }
                "STEP_DISPATCHED" => {
                    let current: String = tx.query_row(
                        "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                        |r| r.get(0),
                    )?;
                    if !status_outcome(&current)
                        .as_ref()
                        .map(terminal_outcome)
                        .unwrap_or(false)
                    {
                        tx.execute(
                            "UPDATE step_status SET status = 'dispatched'
                             WHERE task_id = ?1 AND step_id = ?2",
                            params![task_id, step_id],
                        )?;
                    }
                }
                "LEASE_EXPIRED" => {
                    let current: String = tx.query_row(
                        "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                        |r| r.get(0),
                    )?;
                    if !status_outcome(&current)
                        .as_ref()
                        .map(terminal_outcome)
                        .unwrap_or(false)
                    {
                        tx.execute(
                            "UPDATE step_status SET status = 'pending'
                             WHERE task_id = ?1 AND step_id = ?2",
                            params![task_id, step_id],
                        )?;
                        tx.execute(
                            "UPDATE leases
                             SET state = 'expired'
                             WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                            params![task_id, step_id],
                        )?;
                    }
                }
                _ => {}
            }
        }
    }

    let all_steps = ordered_step_ids(&tx, task_id)?;

    for step_id in &all_steps {
        let current: String = tx.query_row(
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

        let dep_total: i64 = tx.query_row(
            "SELECT COUNT(*) FROM step_dependencies
             WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if dep_total == 0 {
            tx.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
            continue;
        }

        let dep_satisfied: i64 = tx.query_row(
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
            tx.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
        }
    }

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
                    "SELECT 1
                 FROM leases
                 WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'
                 LIMIT 1",
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
        let current_generation: i64 = tx.query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
            [],
            |r| r.get(0),
        )?;

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

        let causal_unit_id = current_generation + 1;

        let lease_payload = json!({
            "lease_id": lease_id,
            "worker_id": "worker-scheduler",
            "acquired_generation": current_generation,
            "expires_at_generation": current_generation + 2
        });

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, 'LEASE_ACQUIRED', ?5, ?6)",
            params![
                causal_unit_id,
                causal_unit_id,
                task_id,
                step_id,
                serde_json::to_string(&lease_payload)?,
                causal_unit_id
            ],
        )?;

        let dispatch_payload = json!({
            "lease_id": lease_id,
            "worker_id": "worker-scheduler"
        });

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 1, ?3, ?4, 'STEP_DISPATCHED', ?5, ?6)",
            params![
                causal_unit_id,
                causal_unit_id,
                task_id,
                step_id,
                serde_json::to_string(&dispatch_payload)?,
                causal_unit_id
            ],
        )?;

        println!("DISPATCHED: {}", step_id);
    }

    tx.commit()?;

    let conn = Connection::open(db)?;
    let mut status: BTreeMap<String, String> = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT step_id, status
         FROM step_status
         WHERE task_id = ?1
         ORDER BY step_id",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        status.insert(row.0, row.1);
    }

    println!("STEP_STATUS: {:?}", status);
    Ok(())
}

pub fn current_status_map(db: &str, task_id: &str) -> Result<BTreeMap<String, String>> {
    let conn = Connection::open(db)?;
    let mut out = BTreeMap::new();

    let mut stmt = conn.prepare(
        "SELECT step_id, status
         FROM step_status
         WHERE task_id = ?1
         ORDER BY step_id",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        out.insert(row.0, row.1);
    }

    Ok(out)
}

pub fn next_ready_step(db: &str, task_id: &str) -> Result<Option<String>> {
    let conn = Connection::open(db)?;

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
                "SELECT 1 FROM leases
                 WHERE task_id = ?1
                   AND step_id = ?2
                   AND state = 'active'
                 LIMIT 1",
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

