use crate::providers::storage::StorageProvider;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::planner_pipeline::pipeline::Pipeline;
use crate::planner_pipeline::replay::{ReplayTape, Replayer};
use crate::planner_pipeline::PipelineContext;

// ── Step-level result ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepStatus {
    Ok,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub index: usize,
    pub description: String,
    pub status: StepStatus,
    pub duration_ms: u64,
}

// ── Execution report ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub plan_id: String,
    pub seed: u64,
    pub steps: Vec<StepResult>,
    pub total_duration_ms: u64,
    pub success: bool,
    #[serde(default)]
    pub final_answer: String,
    #[serde(default)]
    pub critique_status: String,
}

impl ExecutionReport {
    pub fn failed_steps(&self) -> Vec<&StepResult> {
        self.steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Failed(_)))
            .collect()
    }
    pub fn skipped_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == StepStatus::Skipped)
            .count()
    }
}

// ── Step executor trait (injectable / testable) ──────────────────────────────

pub trait StepExecutor: Send + Sync {
    fn execute(&self, index: usize, description: &str) -> Result<StepStatus>;
    fn is_default(&self) -> bool {
        false
    }
}

/// Default executor: validates the step description is non-empty, then marks Ok.
pub struct DefaultStepExecutor;

impl StepExecutor for DefaultStepExecutor {
    fn execute(&self, _index: usize, description: &str) -> Result<StepStatus> {
        if description.trim().is_empty() {
            bail!("step description is empty");
        }
        Ok(StepStatus::Ok)
    }
    fn is_default(&self) -> bool {
        true
    }
}

// ── State Machine Hardening ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    None,
    Created,
    PlanCreated,
    StepReady,
    StepRunning,
    PrimitiveExecuting,
    StepCompleted,
    TaskCompleted,
}

impl TaskState {
    pub fn transition_allowed(self, next: Self) -> bool {
        !matches!(
            (self, next),
            (Self::TaskCompleted, _)
                | (Self::StepCompleted, Self::StepRunning)
                | (Self::StepRunning, Self::StepRunning)
        )
    }
}

pub fn get_current_task_state(db: &str, task_id: &str) -> Result<TaskState> {
    let events = crate::providers::storage_for(db).query_events(task_id)?;
    let mut state = TaskState::None;
    for row in events {
        let ev_type = row.event_type.as_str();
        match ev_type {
            "TASK_CREATED" => state = TaskState::Created,
            "PLAN_CREATED" => state = TaskState::PlanCreated,
            "STEP_READY" => state = TaskState::StepReady,
            "STEP_STARTED" | "STEP_RUNNING" => state = TaskState::StepRunning,
            "PRIMITIVE_EXECUTING" => state = TaskState::PrimitiveExecuting,
            "STEP_COMPLETED" | "STEP_FAILED" | "PRIMITIVE_EXECUTED" => {
                state = TaskState::StepCompleted
            }
            "TASK_COMPLETED" => state = TaskState::TaskCompleted,
            _ => {}
        }
    }
    Ok(state)
}

pub fn check_and_emit_transition(
    db: &str,
    task_id: &str,
    step_id: Option<&str>,
    event_type: &str,
    execution_id: &str,
    details: serde_json::Value,
) -> Result<()> {
    let current = get_current_task_state(db, task_id)?;

    let next_state = match event_type {
        "TASK_CREATED" => TaskState::Created,
        "PLAN_CREATED" => TaskState::PlanCreated,
        "STEP_READY" => TaskState::StepReady,
        "STEP_STARTED" | "STEP_RUNNING" => TaskState::StepRunning,
        "PRIMITIVE_EXECUTING" => TaskState::PrimitiveExecuting,
        "STEP_COMPLETED" | "STEP_FAILED" | "PRIMITIVE_EXECUTED" => TaskState::StepCompleted,
        "TASK_COMPLETED" => TaskState::TaskCompleted,
        _ => current,
    };

    if current != next_state && !current.transition_allowed(next_state) {
        anyhow::bail!(
            "State Machine Violation: Transition from {:?} to {:?} is forbidden for task '{}'",
            current,
            next_state,
            task_id
        );
    }

    emit_event(db, task_id, step_id, event_type, execution_id, details);
    Ok(())
}

// ── Event Emitter Helper ─────────────────────────────────────────────────────

fn emit_event(
    db: &str,
    task_id: &str,
    step_id: Option<&str>,
    event_type: &str,
    execution_id: &str,
    details: serde_json::Value,
) {
    let payload_str = details.to_string();
    let payload_hash = blake3::hash(payload_str.as_bytes()).to_hex().to_string();
    let event_id = format!(
        "evt_{}",
        &blake3::hash(
            format!(
                "{}:{}:{}:{}",
                task_id, execution_id, event_type, payload_hash
            )
            .as_bytes()
        )
        .to_hex()[..16]
    );

    let payload = serde_json::json!({
        "event_id": event_id,
        "task_id": task_id,
        "execution_id": execution_id,
        "generation": 0,
        "timestamp": "2026-07-12T05:30:00Z",
        "payload_hash": payload_hash,
        "event_type": event_type,
        "deterministic": true,
        "details": details
    });
    if let Err(e) =
        crate::providers::storage_for(db).append_event(task_id, step_id, event_type, &payload)
    {
        eprintln!("engine event append failed ({}): {}", event_type, e);
    }
}

pub fn calculate_primitive_input_hash(
    prim: &crate::execution_abi::primitives::PrimitiveSpec,
) -> String {
    match prim.kind {
        crate::execution_abi::primitives::PrimitiveKind::Read => {
            let path = prim
                .payload
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("dummy.txt");
            blake3::hash(path.as_bytes()).to_hex().to_string()
        }
        crate::execution_abi::primitives::PrimitiveKind::Write => {
            let path = prim
                .payload
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("dummy.txt");
            let content = prim
                .payload
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            blake3::hash(format!("{}:{}", path, content).as_bytes())
                .to_hex()
                .to_string()
        }
        crate::execution_abi::primitives::PrimitiveKind::Compute => {
            let cmd = prim
                .payload
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            blake3::hash(cmd.as_bytes()).to_hex().to_string()
        }
        _ => {
            let detail = prim
                .payload
                .get("detail")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            blake3::hash(detail.as_bytes()).to_hex().to_string()
        }
    }
}

// ── Primitive Hash Computation ────────────────────────────────────────────────
// PURE specification hashing for the cache layer.
//
// This function MUST NOT perform side effects: no file reads/writes, no
// process spawning, no network. It hashes the normalized primitive
// specification (kind + payload identity) so the cache layer can recognize
// identical work across runs. The previous implementation executed the
// command / wrote the file "to compute the output hash", which turned hash
// computation into arbitrary command execution (audit finding C5). If the
// engine needs real execution, it must go through an explicit, authorized
// executor — never through hashing.

pub(crate) fn compute_primitive_hashes(
    prim: &crate::execution_abi::primitives::PrimitiveSpec,
    _execution_id: &str,
) -> (String, String, String, String, u64) {
    let t_start = Instant::now();
    let prim_id = prim.id.0.clone();
    let kind = prim.kind;

    let (input_hash, output_hash) = match kind {
        crate::execution_abi::primitives::PrimitiveKind::Read => {
            let path = prim
                .payload
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("dummy.txt");
            (
                blake3::hash(format!("Read:{}", path).as_bytes())
                    .to_hex()
                    .to_string(),
                blake3::hash(format!("Read-output:{}", path).as_bytes())
                    .to_hex()
                    .to_string(),
            )
        }
        crate::execution_abi::primitives::PrimitiveKind::Write => {
            let path = prim
                .payload
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("dummy.txt");
            let content = prim
                .payload
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            (
                blake3::hash(format!("Write:{}:{}", path, content).as_bytes())
                    .to_hex()
                    .to_string(),
                blake3::hash(format!("Write-output:{}", path).as_bytes())
                    .to_hex()
                    .to_string(),
            )
        }
        crate::execution_abi::primitives::PrimitiveKind::Compute => {
            let cmd = prim
                .payload
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("echo 'hello'");
            (
                blake3::hash(format!("Compute:{}", cmd).as_bytes())
                    .to_hex()
                    .to_string(),
                blake3::hash(format!("Compute-output:{}", cmd).as_bytes())
                    .to_hex()
                    .to_string(),
            )
        }
        _ => {
            let detail = prim
                .payload
                .get("detail")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            (
                blake3::hash(detail.as_bytes()).to_hex().to_string(),
                blake3::hash("ok".as_bytes()).to_hex().to_string(),
            )
        }
    };

    let duration_ms = t_start.elapsed().as_millis() as u64;
    (
        prim_id,
        input_hash,
        output_hash,
        "success".to_string(),
        duration_ms,
    )
}

// ── Async verification wrapper ────────────────────────────────────────────────

/// Run execution with step verification.
/// After each step, runs a verifier gate (best-effort, never blocks pipeline).
pub async fn run_with_verification(
    engine: &ExecutionEngine,
    payload: &str,
    ctx: &PipelineContext,
) -> Result<ExecutionReport> {
    let report = engine.run(payload, ctx)?;

    // Verify each completed step (best-effort, silently)
    let mut verified_steps = Vec::with_capacity(report.steps.len());
    for step in &report.steps {
        if matches!(step.status, StepStatus::Ok) {
            // Attempt verification (don't fail pipeline on verification error)
            let verdict = super::step_verifier::verify_step(
                "step",
                &step.description,
                &format!("step {} completed successfully", step.index),
            )
            .await;

            match verdict {
                Ok(v) if v.verdict.to_uppercase() == "FAIL" => {
                    // Verification failed — mark as failed but keep going
                    verified_steps.push(StepResult {
                        index: step.index,
                        description: step.description.clone(),
                        status: StepStatus::Failed(format!("verification: {}", v.reason)),
                        duration_ms: step.duration_ms,
                    });
                }
                _ => {
                    // PASS or verification error — keep original status
                    verified_steps.push(step.clone());
                }
            }
        } else {
            verified_steps.push(step.clone());
        }
    }

    let success = verified_steps
        .iter()
        .all(|s| matches!(s.status, StepStatus::Ok | StepStatus::Skipped));

    let final_report = ExecutionReport {
        plan_id: report.plan_id,
        seed: report.seed,
        steps: verified_steps,
        total_duration_ms: report.total_duration_ms,
        success,
        final_answer: String::new(),
        critique_status: String::new(),
    };

    // Run execution critic (best-effort, never fails pipeline)
    let mut critique_events = Vec::new();
    super::llm_critique::run_execution_critique(payload, &final_report, &mut critique_events).await;

    // Attach critique events to the report (for observability)
    // Note: ExecutionReport doesn't have stage_events field, so we log them.
    for event in &critique_events {
        println!(
            "observability: component=execution_critique stage={} description={}",
            event.stage, event.description
        );
    }

    Ok(final_report)
}

// ── Engine ───────────────────────────────────────────────────────────────────

pub struct ExecutionEngine {
    pipeline: Pipeline,
    executor: Box<dyn StepExecutor>,
    // Explicit database routing: the engine never relies on process-global
    // override state (audit finding M3).
    db_path: String,
}

fn default_db_path() -> String {
    std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| "kernel.db".to_string())
}

impl ExecutionEngine {
    pub fn new(pipeline: Pipeline, executor: Box<dyn StepExecutor>) -> Self {
        Self::new_with_db(pipeline, executor, &default_db_path())
    }

    pub fn new_with_db(pipeline: Pipeline, executor: Box<dyn StepExecutor>, db_path: &str) -> Self {
        Self {
            pipeline,
            executor,
            db_path: db_path.to_string(),
        }
    }

    pub fn with_default_executor(pipeline: Pipeline) -> Self {
        Self::new(pipeline, Box::new(DefaultStepExecutor))
    }

    pub fn with_default_executor_db(pipeline: Pipeline, db_path: &str) -> Self {
        Self::new_with_db(pipeline, Box::new(DefaultStepExecutor), db_path)
    }

    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    fn storage(&self) -> crate::providers::storage::DefaultStorage {
        crate::providers::storage_for(&self.db_path)
    }

    /// Run pipeline → execute every step → return ExecutionReport.
    pub fn run(&self, payload: &str, ctx: &PipelineContext) -> Result<ExecutionReport> {
        let t0 = Instant::now();

        // Structured observability logging
        let parser_start = Instant::now();
        let out = self.pipeline.run(payload, ctx)?;
        let plan = &out.plan;
        println!(
            "observability: component=parser operation=parse_and_plan duration_ms={}",
            parser_start.elapsed().as_millis()
        );

        let task_id = ctx.task_id.clone().unwrap_or_else(|| {
            format!("task_{}", &blake3::hash(payload.as_bytes()).to_hex()[..16])
        });

        // Emit TASK_CREATED
        check_and_emit_transition(
            &self.db_path,
            &task_id,
            None,
            "TASK_CREATED",
            &plan.id,
            serde_json::json!({
                "task_id": task_id,
                "payload": payload
            }),
        )?;

        // Emit PLAN_CREATED
        let env_fingerprint = crate::planner_pipeline::get_environment_fingerprint();
        check_and_emit_transition(
            &self.db_path,
            &task_id,
            None,
            "PLAN_CREATED",
            &plan.id,
            serde_json::json!({
                "plan_id": plan.id,
                "seed": plan.seed,
                "steps": plan.steps,
                "spec": plan.spec,
                "fingerprint": env_fingerprint
            }),
        )?;

        let mut steps = Vec::with_capacity(plan.spec.steps.len());
        let mut success = true;

        for (i, step_spec) in plan.spec.steps.iter().enumerate() {
            let step_t = Instant::now();
            let desc = step_spec.detail.as_deref().unwrap_or(&step_spec.step_id);
            let execution_id = format!("exec_{}_{}", plan.id, step_spec.step_id);

            // Emit STEP_READY
            check_and_emit_transition(
                &self.db_path,
                &task_id,
                Some(&step_spec.step_id),
                "STEP_READY",
                &execution_id,
                serde_json::json!({
                    "step_id": step_spec.step_id,
                }),
            )?;

            // Emit STEP_STARTED
            check_and_emit_transition(
                &self.db_path,
                &task_id,
                Some(&step_spec.step_id),
                "STEP_STARTED",
                &execution_id,
                serde_json::json!({
                    "step_id": step_spec.step_id,
                    "description": desc
                }),
            )?;

            let status = if self.executor.is_default() {
                if let Some(ref prim) = step_spec.primitive {
                    let prim_start = Instant::now();
                    // Emit PRIMITIVE_EXECUTING
                    check_and_emit_transition(
                        &self.db_path,
                        &task_id,
                        Some(&step_spec.step_id),
                        "PRIMITIVE_EXECUTING",
                        &execution_id,
                        serde_json::json!({
                            "primitive_id": prim.id.clone(),
                        }),
                    )?;

                    // Execute primitive (with Cache Layer check)
                    let ihash = calculate_primitive_input_hash(prim);
                    let env_fp = crate::planner_pipeline::get_environment_fingerprint();
                    let cache_key = blake3::hash(
                        format!("{}:{}:{}:{}:{}", task_id, plan.id, prim.id.0, ihash, env_fp)
                            .as_bytes(),
                    )
                    .to_hex()
                    .to_string();

                    let mut cache_hit = false;
                    let mut cached_status = "success".to_string();
                    let mut ohash = "".to_string();
                    let mut duration_ms = 0u64;

                    if let Ok(Some(record)) = self.storage().get_cache(&cache_key) {
                        cache_hit = true;
                        cached_status = record.execution_result;
                        ohash = record.output_hash;
                        duration_ms = record.duration_ms as u64;
                    }

                    let (prim_status, _prim_duration_ms) = if cache_hit {
                        // Emit CACHE_HIT event
                        check_and_emit_transition(
                            &self.db_path,
                            &task_id,
                            Some(&step_spec.step_id),
                            "CACHE_HIT",
                            &execution_id,
                            serde_json::json!({
                                "cache_key": cache_key,
                                "primitive_id": prim.id.clone(),
                                "output_hash": ohash,
                                "duration_ms": duration_ms
                            }),
                        )?;

                        // Emit PRIMITIVE_EXECUTED event representing cached state to keep replay valid
                        check_and_emit_transition(
                            &self.db_path,
                            &task_id,
                            Some(&step_spec.step_id),
                            "PRIMITIVE_EXECUTED",
                            &execution_id,
                            serde_json::json!({
                                "primitive_id": prim.id.0.clone(),
                                "primitive_kind": format!("{:?}", prim.kind),
                                "input_hash": ihash,
                                "output_hash": ohash,
                                "status": cached_status,
                                "duration_ms": duration_ms
                            }),
                        )?;

                        (cached_status, duration_ms)
                    } else {
                        let (prim_id, _, out_hash, status_str, dur_ms) =
                            compute_primitive_hashes(prim, &execution_id);
                        println!("observability: component=executor operation=compute_primitive_hashes duration_ms={}", prim_start.elapsed().as_millis());
                        ohash = out_hash.clone();

                        // Emit PRIMITIVE_EXECUTED
                        check_and_emit_transition(
                            &self.db_path,
                            &task_id,
                            Some(&step_spec.step_id),
                            "PRIMITIVE_EXECUTED",
                            &execution_id,
                            serde_json::json!({
                                "primitive_id": prim_id,
                                "primitive_kind": format!("{:?}", prim.kind),
                                "input_hash": ihash,
                                "output_hash": ohash,
                                "status": status_str,
                                "duration_ms": dur_ms
                            }),
                        )?;

                        // Save cache record
                        let record = crate::providers::storage::CacheRecord {
                            execution_result: status_str.clone(),
                            output_hash: ohash.clone(),
                            duration_ms: dur_ms as i64,
                            metadata: format!("prim_kind:{:?}", prim.kind),
                        };
                        let _ = self.storage().put_cache(&cache_key, &record);

                        (status_str, dur_ms)
                    };

                    if prim_status == "success" {
                        StepStatus::Ok
                    } else {
                        success = false;
                        StepStatus::Failed(format!("primitive execution failed: {}", prim.id.0))
                    }
                } else {
                    // Fallback step executor
                    match self.executor.execute(i, desc) {
                        Ok(s) => s,
                        Err(e) => {
                            success = false;
                            StepStatus::Failed(e.to_string())
                        }
                    }
                }
            } else {
                match self.executor.execute(i, desc) {
                    Ok(s) => s,
                    Err(e) => {
                        success = false;
                        StepStatus::Failed(e.to_string())
                    }
                }
            };

            if matches!(status, StepStatus::Failed(_)) {
                success = false;
                check_and_emit_transition(
                    &self.db_path,
                    &task_id,
                    Some(&step_spec.step_id),
                    "STEP_FAILED",
                    &execution_id,
                    serde_json::json!({
                        "step_id": step_spec.step_id,
                        "error": format!("{:?}", status)
                    }),
                )?;
            } else {
                check_and_emit_transition(
                    &self.db_path,
                    &task_id,
                    Some(&step_spec.step_id),
                    "STEP_COMPLETED",
                    &execution_id,
                    serde_json::json!({
                        "step_id": step_spec.step_id,
                        "duration_ms": step_t.elapsed().as_millis() as u64
                    }),
                )?;
            }

            steps.push(StepResult {
                index: i,
                description: desc.to_string(),
                status,
                duration_ms: step_t.elapsed().as_millis() as u64,
            });
        }

        // Emit TASK_COMPLETED
        check_and_emit_transition(
            &self.db_path,
            &task_id,
            None,
            "TASK_COMPLETED",
            &plan.id,
            serde_json::json!({
                "task_id": task_id,
                "success": success
            }),
        )?;

        println!(
            "observability: component=execution_engine operation=run_total duration_ms={}",
            t0.elapsed().as_millis()
        );
        Ok(ExecutionReport {
            plan_id: plan.id.clone(),
            seed: ctx.seed,
            steps,
            total_duration_ms: t0.elapsed().as_millis() as u64,
            success,
            final_answer: String::new(),
            critique_status: String::new(),
        })
    }

    /// Run + record to tape + verify replay consistency + execution critique + final answer.
    pub fn run_with_replay(
        &self,
        payload: &str,
        ctx: &PipelineContext,
        tape: &mut ReplayTape,
    ) -> Result<ExecutionReport> {
        let mut report = self.run(payload, ctx)?;
        tape.record(payload, ctx.seed, &report.plan_id);

        let task_id = ctx.task_id.clone().unwrap_or_else(|| {
            format!("task_{}", &blake3::hash(payload.as_bytes()).to_hex()[..16])
        });

        // Only verify replay when historical events exist (skip on first run)
        let has_history = self
            .storage()
            .query_events(&task_id)
            .map(|e| !e.is_empty())
            .unwrap_or(false);

        if has_history {
            let verifier =
                Replayer::with_db(Pipeline::new(self.pipeline.bias.clone()), &self.db_path);
            let single = {
                let mut t = ReplayTape::new();
                t.record(payload, ctx.seed, &report.plan_id);
                t
            };
            verifier.verify(&single)?;
        }

        // Run execution critique (best-effort, never fails pipeline)
        let mut critique_events = Vec::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(super::llm_critique::run_execution_critique(
            payload,
            &report,
            &mut critique_events,
        ));
        for event in &critique_events {
            println!(
                "observability: component=execution_critique stage={} description={}",
                event.stage, event.description
            );
        }

        // Set critique_status from events
        report.critique_status = critique_events
            .iter()
            .map(|e| e.description.clone())
            .collect::<Vec<_>>()
            .join("; ");

        // Generate final_answer via LLM (best-effort)
        if report.success {
            let final_answer = rt.block_on(async {
                let steps_summary = report.steps.iter()
                    .map(|s| format!("{}. [{}] {}", s.index + 1,
                        match &s.status {
                            StepStatus::Ok => "OK",
                            StepStatus::Skipped => "SKIPPED",
                            StepStatus::Failed(_) => "FAILED",
                        },
                        s.description))
                    .collect::<Vec<_>>()
                    .join("\n");
                let user_prompt = format!(
                    "Task: {}\n\nExecution completed with {} step(s):\n{}\n\nProvide a clear, concise final answer to the original task. Do not describe the execution — answer the task directly.",
                    payload, report.steps.len(), steps_summary
                );
                crate::llm::chat(
                    "You are a helpful assistant. Answer the user's task directly and concisely. Do not explain your process — just give the answer.",
                    &user_prompt,
                ).await.unwrap_or_else(|e| format!("[final_answer synthesis failed: {}]", e))
            });
            report.final_answer = final_answer;
        }

        // Emit REPLAY_VALIDATED
        emit_event(
            &self.db_path,
            &task_id,
            None,
            "REPLAY_VALIDATED",
            &report.plan_id,
            serde_json::json!({
                "plan_id": report.plan_id,
                "tape_entries": tape.len()
            }),
        );

        Ok(report)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_pipeline::pipeline::Pipeline;
    use crate::planner_pipeline::PipelineContext;
    use crate::semantic_bias::{BiasConfiguration, BiasVersion, SemanticBiasRule};

    fn bias() -> BiasConfiguration {
        BiasConfiguration::new(
            "test",
            vec![SemanticBiasRule::new("r1", 1, "critical", "first")],
        )
    }
    fn ctx() -> PipelineContext {
        PipelineContext {
            seed: 42,
            bias_version: BiasVersion::V1,
            task_id: None,
        }
    }
    fn engine_db(db: &str) -> ExecutionEngine {
        ExecutionEngine::with_default_executor_db(Pipeline::new(bias()), db)
    }
    /// RAII cleanup for a temporary test database. Routing is explicit:
    /// engines under test are constructed with this path (audit finding M3 —
    /// no global override state).
    struct TestDbGuard {
        db_path: std::path::PathBuf,
    }
    impl TestDbGuard {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test failure")
                .as_nanos();
            let db_path = std::env::temp_dir().join(format!("dak_test_{}_{}.db", name, nanos));
            Self { db_path }
        }
        fn db_path_str(&self) -> String {
            self.db_path.to_str().expect("test failure").to_string()
        }
    }
    impl Drop for TestDbGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.db_path);
        }
    }

    #[test]
    fn runs_all_steps_successfully() {
        let guard = TestDbGuard::new("runs_all_steps_successfully");
        let r = engine_db(&guard.db_path_str())
            .run("step one\nstep two\ncritical step", &ctx())
            .expect("test failure");
        assert!(r.success);
        assert_eq!(r.steps.len(), 3);
        assert!(r.failed_steps().is_empty());
    }

    #[test]
    fn report_plan_id_matches_pipeline() {
        let guard = TestDbGuard::new("report_plan_id_matches_pipeline");
        let p = Pipeline::new(bias());
        let out = p.run("step one\nstep two", &ctx()).expect("test failure");
        let eng =
            ExecutionEngine::with_default_executor_db(Pipeline::new(bias()), &guard.db_path_str());
        let r = eng.run("step one\nstep two", &ctx()).expect("test failure");
        assert_eq!(r.plan_id, out.plan.id);
    }

    #[test]
    fn report_seed_is_preserved() {
        let guard = TestDbGuard::new("report_seed_is_preserved");
        let r = engine_db(&guard.db_path_str())
            .run("step one\nstep two", &ctx())
            .expect("test failure");
        assert_eq!(r.seed, 42);
    }

    #[test]
    fn failing_executor_marks_report_failed() {
        let guard = TestDbGuard::new("failing_executor_marks_report_failed");
        struct AlwaysFail;
        impl StepExecutor for AlwaysFail {
            fn execute(&self, _i: usize, _d: &str) -> Result<StepStatus> {
                Ok(StepStatus::Failed("boom".into()))
            }
        }
        let eng = ExecutionEngine::new_with_db(
            Pipeline::new(bias()),
            Box::new(AlwaysFail),
            &guard.db_path_str(),
        );
        let r = eng.run("step one\nstep two", &ctx()).expect("test failure");
        assert!(!r.success);
        assert_eq!(r.failed_steps().len(), 2);
    }

    #[test]
    fn skipping_executor_counts_correctly() {
        let guard = TestDbGuard::new("skipping_executor_counts_correctly");
        struct AllSkip;
        impl StepExecutor for AllSkip {
            fn execute(&self, _i: usize, _d: &str) -> Result<StepStatus> {
                Ok(StepStatus::Skipped)
            }
        }
        let eng = ExecutionEngine::new_with_db(
            Pipeline::new(bias()),
            Box::new(AllSkip),
            &guard.db_path_str(),
        );
        let r = eng
            .run("step one\nstep two\nstep three", &ctx())
            .expect("test failure");
        assert_eq!(r.skipped_count(), 3);
        // skipped ≠ failed → success still true
        assert!(r.success);
    }

    #[test]
    fn run_is_deterministic_same_seed() {
        let guard = TestDbGuard::new("run_is_deterministic_same_seed");
        let r1 = engine_db(&guard.db_path_str())
            .run("alpha\nbeta\ngamma", &ctx())
            .expect("test failure");
        // Clear events so second run doesn't violate state transition rule
        {
            let task_id = format!(
                "task_{}",
                &blake3::hash("alpha\nbeta\ngamma".as_bytes()).to_hex()[..16]
            );
            let conn = rusqlite::Connection::open(&guard.db_path).expect("test failure");
            conn.execute("DELETE FROM event_log WHERE task_id = ?1", [task_id])
                .expect("test failure");
        }
        let r2 = engine_db(&guard.db_path_str())
            .run("alpha\nbeta\ngamma", &ctx())
            .expect("test failure");
        assert_eq!(r1.plan_id, r2.plan_id);
        assert_eq!(r1.steps.len(), r2.steps.len());
    }

    #[test]
    fn run_with_replay_verifies_consistency() {
        let guard = TestDbGuard::new("run_with_replay_verifies_consistency");
        let mut tape = ReplayTape::new();
        let eng = engine_db(&guard.db_path_str());
        eng.run_with_replay("step one\nstep two", &ctx(), &mut tape)
            .expect("test failure");
        assert_eq!(tape.len(), 1);
    }

    #[test]
    fn run_with_replay_detects_tamper() {
        let mut tape = ReplayTape::new();
        tape.record("step one\nstep two", 42, "0000000000000000");
        let verifier = crate::planner_pipeline::replay::Replayer::new(Pipeline::new(bias()));
        assert!(verifier.verify(&tape).is_err());
    }

    #[test]
    fn step_results_have_correct_indices() {
        let guard = TestDbGuard::new("step_results_have_correct_indices");
        let r = engine_db(&guard.db_path_str())
            .run("a\nb\nc", &ctx())
            .expect("test failure");
        for (i, s) in r.steps.iter().enumerate() {
            assert_eq!(s.index, i);
        }
    }

    #[test]
    fn test_primitive_hashing_is_pure_no_side_effects() {
        // Regression for audit finding C5: hashing the primitive
        // specification must NEVER execute commands or write files.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test failure")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("dak_prim_test_{}.db", nanos));
        let db_path_str = db_path.to_str().expect("test failure").to_string();

        let path_str = format!(
            "{}/dak_prim_pure_{}.txt",
            std::env::temp_dir().display(),
            nanos
        );
        let probe_path = format!(
            "{}/dak_prim_cmd_probe_{}.txt",
            std::env::temp_dir().display(),
            nanos
        );
        let _ = std::fs::remove_file(&path_str);
        let _ = std::fs::remove_file(&probe_path);

        // 1. Write primitive: file must NOT be created.
        let payload = format!("Step 1 write file {} with hello_world_content", path_str);
        let eng = ExecutionEngine::with_default_executor_db(Pipeline::new(bias()), &db_path_str);
        let r_write = eng.run(&payload, &ctx()).expect("test failure");
        assert!(r_write.success);
        assert_eq!(r_write.steps.len(), 1);
        assert!(
            !std::path::Path::new(&path_str).exists(),
            "write primitive must not create files during hashing"
        );

        // 2. Read primitive: succeeds without touching the filesystem.
        let payload_read = format!("Step 1 read file {}", path_str);
        let r_read = eng.run(&payload_read, &ctx()).expect("test failure");
        assert!(r_read.success);

        // 3. Command primitive: the command must NOT be executed.
        let payload_cmd = format!("Step 1 run command touch {}", probe_path);
        let r_cmd = eng.run(&payload_cmd, &ctx()).expect("test failure");
        assert!(r_cmd.success);
        assert!(
            !std::path::Path::new(&probe_path).exists(),
            "compute primitive must not spawn processes during hashing"
        );

        // 4. Events still exist in the database for the command task.
        let task_id = format!(
            "task_{}",
            &blake3::hash(payload_cmd.as_bytes()).to_hex()[..16]
        );
        let events = crate::providers::storage_for(&db_path_str)
            .query_events(&task_id)
            .expect("test failure");

        let has_task_created = events.iter().any(|e| e.event_type == "TASK_CREATED");
        let has_plan_created = events.iter().any(|e| e.event_type == "PLAN_CREATED");
        let has_step_started = events.iter().any(|e| e.event_type == "STEP_STARTED");
        let has_primitive_executed = events.iter().any(|e| e.event_type == "PRIMITIVE_EXECUTED");
        let has_step_completed = events.iter().any(|e| e.event_type == "STEP_COMPLETED");

        assert!(has_task_created);
        assert!(has_plan_created);
        assert!(has_step_started);
        assert!(has_primitive_executed);
        assert!(has_step_completed);

        // 5. Replay still succeeds on a separate clean temp database.
        let db_path_replay = std::env::temp_dir().join(format!("dak_prim_test_rep_{}.db", nanos));
        let db_path_replay_str = db_path_replay.to_str().expect("test failure").to_string();

        let eng_rep =
            ExecutionEngine::with_default_executor_db(Pipeline::new(bias()), &db_path_replay_str);
        let mut tape = ReplayTape::new();
        let r_rep = eng_rep
            .run_with_replay(&payload_cmd, &ctx(), &mut tape)
            .expect("test failure");
        assert!(r_rep.success);

        // Clean up temp databases
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(db_path_replay);
    }

    #[test]
    fn test_state_machine_transition_hardening() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test failure")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("dak_sm_test_{}.db", nanos));
        let db_path_str = db_path.to_str().expect("test failure").to_string();

        let task_id = "test_sm_task";
        let execution_id = "test_exec";

        // Initial state is None
        let current = get_current_task_state(&db_path_str, task_id).expect("test failure");
        assert_eq!(current, TaskState::None);

        // 1. Transition TASK_CREATED is allowed
        check_and_emit_transition(
            &db_path_str,
            task_id,
            None,
            "TASK_CREATED",
            execution_id,
            serde_json::json!({}),
        )
        .expect("test failure");

        // 2. Transition TASK_COMPLETED
        check_and_emit_transition(
            &db_path_str,
            task_id,
            None,
            "TASK_COMPLETED",
            execution_id,
            serde_json::json!({}),
        )
        .expect("test failure");

        // 3. TASK_COMPLETED -> any change is forbidden
        let res = check_and_emit_transition(
            &db_path_str,
            task_id,
            None,
            "STEP_READY",
            execution_id,
            serde_json::json!({}),
        );
        assert!(res.is_err());
        assert!(res
            .expect_err("expected error")
            .to_string()
            .contains("State Machine Violation"));

        // Clean up
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn test_deterministic_cache_layer() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test failure")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("dak_cache_test_{}.db", nanos));
        let db_path_str = db_path.to_str().expect("test failure").to_string();

        let path_str = format!(
            "{}/dak_cache_primitive_{}.txt",
            std::env::temp_dir().display(),
            nanos
        );

        // Make sure it starts clean
        let _ = std::fs::remove_file(&path_str);

        // 1. First run: pure hashing, stores cache entry; NO physical write.
        let payload = format!("Step 1 write file {} with cached_hello_content", path_str);
        let task_id = format!("task_{}", &blake3::hash(payload.as_bytes()).to_hex()[..16]);
        let eng = ExecutionEngine::with_default_executor_db(Pipeline::new(bias()), &db_path_str);
        let r1 = eng.run(&payload, &ctx()).expect("test failure");
        assert!(r1.success);
        assert!(
            !std::path::Path::new(&path_str).exists(),
            "hashing must not write files"
        );

        // Delete all event log entries for this task to reset its state machine history
        {
            let conn = rusqlite::Connection::open(&db_path).expect("test failure");
            conn.execute("DELETE FROM event_log WHERE task_id = ?1", [&task_id])
                .expect("test failure");
        }

        // 2. Second run: hits the cache; still no physical side effect.
        let r2 = eng.run(&payload, &ctx()).expect("test failure");
        assert!(r2.success);
        assert!(!std::path::Path::new(&path_str).exists());

        // Verify CACHE_HIT event is present in the database
        let events = crate::providers::storage_for(&db_path_str)
            .query_events(&task_id)
            .expect("test failure");
        let has_cache_hit = events.iter().any(|e| e.event_type == "CACHE_HIT");
        assert!(has_cache_hit);

        // Clean up temp database
        let _ = std::fs::remove_file(db_path);
    }
}
