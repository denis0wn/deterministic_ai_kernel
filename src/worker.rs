use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::workflow::contract::{required_capability_for_step, StepKind, StepOutcome, WorkerCapability};


fn parse_step_kind_from_step_id(step_id: &str) -> Result<StepKind> {
    let slug = step_id
        .split_once('_')
        .map(|(_, rest)| rest)
        .unwrap_or(step_id);

    match slug {
        "tighten_planner_prompt" => Ok(StepKind::TightenPlannerPrompt),
        "normalize_planner_output" => Ok(StepKind::NormalizePlannerOutput),
        "add_llm_fallback_handling" => Ok(StepKind::AddLlmFallbackHandling),
        "add_planner_test_coverage" => Ok(StepKind::AddPlannerTestCoverage),
        "validate_planner_output" => Ok(StepKind::ValidatePlannerOutput),
        "analyze_task" => Ok(StepKind::AnalyzeTask),
        "plan_execution" => Ok(StepKind::PlanExecution),
        "execute_changes" => Ok(StepKind::ExecuteChanges),
        _ => Err(anyhow!("unknown step id: {}", step_id)),
    }
}

fn capability_for_worker_id(worker_id: &str) -> Result<WorkerCapability> {
    let lower = worker_id.to_ascii_lowercase();

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


fn outcome_to_event_type(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Success => "STEP_COMPLETED",
        StepOutcome::RetryableFailure
        | StepOutcome::TerminalFailure
        | StepOutcome::Blocked => "STEP_FAILED",
    }
}

fn classify_failure_outcome(reason: &str) -> StepOutcome {
    let lower = reason.trim().to_ascii_lowercase();

    if lower.starts_with("retry:")
        || lower.starts_with("transient:")
        || lower.starts_with("timeout")
    {
        return StepOutcome::RetryableFailure;
    }

    if lower.starts_with("blocked:")
        || lower.starts_with("waiting_on:")
        || lower.starts_with("dependency:")
    {
        return StepOutcome::Blocked;
    }

    StepOutcome::TerminalFailure
}

pub fn claim_worker(db: &str, task_id: &str, worker_id: &str) -> Result<()> {
    let mut conn = Connection::open(db)?;
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

    let (lease_id, step_id) = row.ok_or_else(|| anyhow!("no dispatchable active lease found"))?;

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

    let next_generation: i64 = tx.query_row(
        "SELECT COALESCE(MAX(system_generation), 0) + 1 FROM event_log",
        [],
        |r| r.get(0),
    )?;

    let claim_payload = json!({
        "lease_id": lease_id,
        "worker_id": worker_id
    });

    tx.execute(
        "INSERT INTO event_log
         (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
         VALUES (?1, ?2, 0, ?3, ?4, 'WORKER_CLAIMED', ?5, ?6)",
        params![
            next_generation,
            next_generation,
            task_id,
            step_id,
            serde_json::to_string(&claim_payload)?,
            next_generation
        ],
    )?;

    tx.commit()?;

    println!("WORKER_CLAIM_OK");
    println!("WORKER: {}", worker_id);
    println!("STEP_CLAIMED: {}", step_id);
    Ok(())
}

pub fn start_step(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    let step_kind = parse_step_kind_from_step_id(step_id)?;
    let worker_capability = capability_for_worker_id(worker_id)?;
    let required_capability = required_capability_for_step(&step_kind);

    if worker_capability != required_capability && worker_capability != WorkerCapability::LegacyGeneric {
        return Err(anyhow!(
            "worker capability mismatch for step: worker={:?}, required={:?}, step_id={}",
            worker_capability,
            required_capability,
            step_id
        ));
    }

    let mut conn = Connection::open(db)?;
    let tx = conn.transaction()?;

    let lease_id: String = tx
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
        .optional()?
        .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

    let next_generation: i64 = tx.query_row(
        "SELECT COALESCE(MAX(system_generation), 0) + 1 FROM event_log",
        [],
        |r| r.get(0),
    )?;

    let payload = json!({
        "lease_id": lease_id,
        "worker_id": worker_id
    });

    tx.execute(
        "INSERT INTO event_log
         (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
         VALUES (?1, ?2, 0, ?3, ?4, 'STEP_RUNNING', ?5, ?6)",
        params![
            next_generation,
            next_generation,
            task_id,
            step_id,
            serde_json::to_string(&payload)?,
            next_generation
        ],
    )?;

    tx.commit()?;

    println!("STEP_RUNNING_OK");
    println!("WORKER: {}", worker_id);
    println!("STEP_RUNNING: {}", step_id);
    Ok(())
}

pub fn heartbeat(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    let mut conn = Connection::open(db)?;
    let tx = conn.transaction()?;

    let lease_id: String = tx
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
        .optional()?
        .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

    let current_generation: i64 = tx.query_row(
        "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
        [],
        |r| r.get(0),
    )?;

    tx.execute(
        "UPDATE leases
         SET expires_at_generation = ?1
         WHERE lease_id = ?2
           AND worker_id = ?3
           AND state = 'active'",
        params![current_generation + 2, lease_id, worker_id],
    )?;

    let next_generation: i64 = current_generation + 1;
    let payload = json!({
        "lease_id": lease_id,
        "worker_id": worker_id,
        "expires_at_generation": current_generation + 2
    });

    tx.execute(
        "INSERT INTO event_log
         (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
         VALUES (?1, ?2, 0, ?3, ?4, 'LEASE_HEARTBEAT', ?5, ?6)",
        params![
            next_generation,
            next_generation,
            task_id,
            step_id,
            serde_json::to_string(&payload)?,
            next_generation
        ],
    )?;

    tx.commit()?;

    println!("LEASE_HEARTBEAT_OK");
    println!("WORKER: {}", worker_id);
    println!("STEP: {}", step_id);
    Ok(())
}

pub fn fail_step(
    db: &str,
    task_id: &str,
    worker_id: &str,
    step_id: &str,
    reason: &str,
) -> Result<()> {
    let mut conn = Connection::open(db)?;
    let tx = conn.transaction()?;

    let lease_id: String = tx
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
        .optional()?
        .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

    let next_generation: i64 = tx.query_row(
        "SELECT COALESCE(MAX(system_generation), 0) + 1 FROM event_log",
        [],
        |r| r.get(0),
    )?;

    let outcome = classify_failure_outcome(reason);
    let fail_payload = json!({
        "lease_id": lease_id,
        "worker_id": worker_id,
        "reason": reason,
        "outcome": format!("{:?}", outcome)
    });

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
            serde_json::to_string(&fail_payload)?,
            next_generation
        ],
    )?;

    tx.execute(
        "UPDATE step_status
         SET status = 'rejected'
         WHERE task_id = ?1 AND step_id = ?2 AND status = 'dispatched'",
        params![task_id, step_id],
    )?;

    tx.execute(
        "UPDATE leases
         SET state = 'released'
         WHERE lease_id = ?1
           AND worker_id = ?2
           AND state = 'active'",
        params![lease_id, worker_id],
    )?;

    tx.commit()?;

    println!("STEP_FAIL_OK");
    println!("WORKER: {}", worker_id);
    println!("STEP_FAILED_BY_WORKER: {}", step_id);
    println!("REASON: {}", reason);
    Ok(())
}

pub fn complete_step(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    let mut conn = Connection::open(db)?;
    let tx = conn.transaction()?;

    let lease_id: String = tx
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
        .optional()?
        .ok_or_else(|| anyhow!("no active lease owned by worker for step"))?;

    let next_generation: i64 = tx.query_row(
        "SELECT COALESCE(MAX(system_generation), 0) + 1 FROM event_log",
        [],
        |r| r.get(0),
    )?;

    let outcome = StepOutcome::Success;
    let complete_payload = json!({
        "lease_id": lease_id,
        "worker_id": worker_id,
        "result": "ok",
        "outcome": format!("{:?}", outcome)
    });

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
            serde_json::to_string(&complete_payload)?,
            next_generation
        ],
    )?;

    tx.execute(
        "UPDATE step_status
         SET status = 'committed'
         WHERE task_id = ?1 AND step_id = ?2 AND status = 'dispatched'",
        params![task_id, step_id],
    )?;

    tx.execute(
        "UPDATE leases
         SET state = 'completed'
         WHERE lease_id = ?1
           AND worker_id = ?2
           AND state = 'active'",
        params![lease_id, worker_id],
    )?;

    tx.commit()?;

    println!("STEP_COMPLETE_OK");
    println!("WORKER: {}", worker_id);
    println!("STEP_COMPLETED_BY_WORKER: {}", step_id);
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::{classify_failure_outcome, outcome_to_event_type};
    use crate::workflow::contract::StepOutcome;

    #[test]
    fn classify_failure_outcome_maps_retry_prefixes() {
        assert_eq!(
            classify_failure_outcome("retry: network blip"),
            StepOutcome::RetryableFailure
        );
        assert_eq!(
            classify_failure_outcome("timeout waiting for lock"),
            StepOutcome::RetryableFailure
        );
    }

    #[test]
    fn classify_failure_outcome_maps_blocked_prefixes() {
        assert_eq!(
            classify_failure_outcome("blocked: waiting on dependency"),
            StepOutcome::Blocked
        );
    }

    #[test]
    fn classify_failure_outcome_defaults_to_terminal_failure() {
        assert_eq!(
            classify_failure_outcome("syntax error"),
            StepOutcome::TerminalFailure
        );
    }

    #[test]
    fn outcome_to_event_type_maps_success_and_blocked() {
        assert_eq!(outcome_to_event_type(StepOutcome::Success), "STEP_COMPLETED");
        assert_eq!(outcome_to_event_type(StepOutcome::Blocked), "STEP_FAILED");
    }
}
