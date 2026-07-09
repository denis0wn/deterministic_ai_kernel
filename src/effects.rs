use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::event_bus::EventBus;
use crate::scheduler::{next_ready_step, schedule};
use crate::worker;
use crate::workflow::contract::{Step, StepKind};

fn step_from_step_id(step_id: &str, payload: &str) -> Result<Step> {
    let slug = step_id
        .split_once('_')
        .map(|(_, rest)| rest)
        .unwrap_or(step_id);

    let kind = match slug {
        "tighten_planner_prompt" => StepKind::TightenPlannerPrompt,
        "normalize_planner_output" => StepKind::NormalizePlannerOutput,
        "add_llm_fallback_handling" => StepKind::AddLlmFallbackHandling,
        "add_planner_test_coverage" => StepKind::AddPlannerTestCoverage,
        "validate_planner_output" => StepKind::ValidatePlannerOutput,
        "analyze_task" => StepKind::AnalyzeTask,
        "plan_execution" => StepKind::PlanExecution,
        "execute_changes" => StepKind::ExecuteChanges,
        "read_repository" => StepKind::ReadRepository,
        "locate_bug" => StepKind::LocateBug,
        "patch_code" => StepKind::PatchCode,
        "run_tests" => StepKind::RunTests,
        "validate_patch" => StepKind::ValidatePatch,
        _ => return Err(anyhow!("unknown step id: {}", step_id)),
    };

    Ok(Step {
        kind,
        detail: Some(payload.to_string()),
    })
}

fn payload_for_task(task_id: &str) -> Result<String> {
    Ok(std::fs::read_to_string(format!(
        "artifacts/pipeline_input.{}.txt",
        task_id
    ))?)
}

fn default_worker_for_step(step_id: &str) -> &'static str {
    let slug = step_id
        .split_once('_')
        .map(|(_, rest)| rest)
        .unwrap_or(step_id);

    match slug {
        "analyze_task" | "plan_execution" | "read_repository" | "locate_bug" => "worker-planner",
        "execute_changes" | "patch_code" | "run_tests" => "worker-executor",
        "validate_patch" | "validate_planner_output" => "worker-verifier",
        _ => "worker-planner",
    }
}

pub fn execute_effects(db: &str, task_id: &str) -> Result<()> {
    std::fs::create_dir_all("artifacts")?;
    let payload = payload_for_task(task_id)?;

    loop {
        schedule(db, task_id)?;

        let step_id = if let Some(step_id) = next_ready_step(db, task_id)? {
            step_id
        } else {
            let conn = Connection::open(db)?;
            let dispatched: Option<String> = conn
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
            drop(conn);

            match dispatched {
                Some(step_id) => step_id,
                None => break,
            }
        };

        let worker_id = default_worker_for_step(&step_id).to_string();

        worker::claim_worker(db, task_id, &worker_id)?;
        worker::start_step(db, task_id, &worker_id, &step_id)?;

        let _step = step_from_step_id(&step_id, &payload)?;
        let _ = EventBus::new(db)?;

        worker::complete_step(db, task_id, &worker_id, &step_id)?;
    }

    let conn = Connection::open(db)?;
    let mut stmt = conn.prepare(
        "SELECT e.effect_id, e.task_id, e.step_id, e.state
         FROM effect_ledger e
         LEFT JOIN external_effects x ON x.effect_id = e.effect_id
         WHERE e.task_id = ?1
           AND e.state IN ('committed','rejected')
           AND x.effect_id IS NULL
         ORDER BY e.effect_id",
    )?;

    let rows = stmt.query_map([task_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    let mut count = 0_i64;

    for row in rows {
        let (effect_id, task_id, step_id, observed_state) = row?;

        let result_payload = if observed_state == "committed" {
            json!({
                "effect_id": effect_id,
                "action": "send_to_external_system",
                "status": "executed"
            })
        } else {
            json!({
                "effect_id": effect_id,
                "action": "skip_external_side_effect",
                "status": "suppressed_due_to_rejection"
            })
        };

        conn.execute(
            "INSERT INTO external_effects
             (effect_id, task_id, step_id, observed_state, result_payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                effect_id,
                task_id,
                step_id,
                observed_state,
                serde_json::to_string(&result_payload)?
            ],
        )?;

        count += 1;
    }

    println!("EFFECT_EXECUTION_OK");
    println!("EXECUTED_EFFECT_ROWS: {}", count);
    Ok(())
}
