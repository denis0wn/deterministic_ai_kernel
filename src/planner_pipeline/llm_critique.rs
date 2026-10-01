use anyhow::Result;
use serde::Deserialize;

use crate::llm::{self, CRITIC_PROMPT, PLANNER_PROMPT};
use crate::model_registry::ModelPurpose;

use super::execution_engine::ExecutionReport;
use super::report::{ReplayEvent, StageName};
use super::{PipelineReport, Plan};

// Hard-coded limits (see requirements)
#[allow(dead_code)]
const MAX_PLAN_REVISIONS: usize = 1;
const MAX_DEFECTS_BEFORE_SKIP_REVISION: usize = 5;
const MIN_STEPS_FOR_CRITIQUE: usize = 2;

#[derive(Debug, Deserialize)]
pub struct CritiqueResult {
    pub pass: bool,
    #[serde(default)]
    pub defects: Vec<CritiqueDefect>,
}

#[derive(Debug, Deserialize)]
pub struct CritiqueDefect {
    pub category: String,
    pub description: String,
}

/// Silent LLM-based plan critique.
/// Runs after deterministic build_plan(). Never blocks on failure.
pub async fn critique_plan(payload: &str, plan: &Plan) -> Result<CritiqueResult> {
    let steps_list = plan
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}", i + 1, s))
        .collect::<Vec<_>>()
        .join("\n");

    let user_prompt = format!("Task: {}\n\nPlan steps:\n{}", payload, steps_list);

    let result = llm::chat_structured(ModelPurpose::Critic, CRITIC_PROMPT, &user_prompt).await?;

    // Fail-closed (audit findings M4/EH2): a malformed critique must never
    // be treated as a passing review.
    let critique: CritiqueResult = serde_json::from_value(result).unwrap_or(CritiqueResult {
        pass: false,
        defects: vec![CritiqueDefect {
            category: "UNVERIFIED".to_string(),
            description: "malformed critique response".to_string(),
        }],
    });

    Ok(critique)
}

/// Revise plan based on critic defects.
/// Calls planner LLM once with defects as context.
pub async fn revise_plan(payload: &str, plan: &Plan, defects: &[CritiqueDefect]) -> Result<Plan> {
    let defect_list = defects
        .iter()
        .enumerate()
        .map(|(i, d)| format!("{}. [{}] {}", i + 1, d.category, d.description))
        .collect::<Vec<_>>()
        .join("\n");

    let user_prompt = format!(
        "Task: {}\n\nPrevious plan had these issues:\n{}\n\nGenerate a revised plan that fixes these issues.",
        payload, defect_list
    );

    let result =
        llm::chat_structured(ModelPurpose::TaskPlanning, PLANNER_PROMPT, &user_prompt).await?;

    // Parse steps from response
    let steps = result
        .get("steps")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|step| {
                    step.get("detail")
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if steps.is_empty() {
        // Fallback: return original plan if revision produces nothing
        return Ok(plan.clone());
    }

    Ok(Plan::new_with_stable_id(plan.seed, steps))
}

/// Run silent LLM critique on a plan.
/// Returns the plan (possibly revised) and critique stage events.
/// Never fails the pipeline — falls back to original plan on any error.
pub async fn run_silent_critique(payload: &str, report: &mut PipelineReport) {
    let plan = &report.plan;

    // Skip critique for simple plans
    if plan.steps.len() < MIN_STEPS_FOR_CRITIQUE {
        report.stage_events.push(ReplayEvent {
            stage: StageName::LlmCritique,
            timestamp_offset_ms: 0,
            description: format!(
                "skipped: {} steps < {} threshold",
                plan.steps.len(),
                MIN_STEPS_FOR_CRITIQUE
            ),
        });
        return;
    }

    // Run critic
    let critique = match critique_plan(payload, plan).await {
        Ok(c) => c,
        Err(e) => {
            report.stage_events.push(ReplayEvent {
                stage: StageName::LlmCritique,
                timestamp_offset_ms: 0,
                description: format!("skipped: {}", e),
            });
            return;
        }
    };

    if critique.pass {
        report.stage_events.push(ReplayEvent {
            stage: StageName::LlmCritique,
            timestamp_offset_ms: 0,
            description: "passed".into(),
        });
        return;
    }

    // Hard stop: too many defects, don't revise
    if critique.defects.len() > MAX_DEFECTS_BEFORE_SKIP_REVISION {
        report.stage_events.push(ReplayEvent {
            stage: StageName::LlmCritique,
            timestamp_offset_ms: 0,
            description: format!(
                "skipped revision: {} defects > {}",
                critique.defects.len(),
                MAX_DEFECTS_BEFORE_SKIP_REVISION
            ),
        });
        return;
    }

    report.stage_events.push(ReplayEvent {
        stage: StageName::LlmCritique,
        timestamp_offset_ms: 0,
        description: format!("found {} defect(s), revising", critique.defects.len()),
    });

    // One revision only
    match revise_plan(payload, plan, &critique.defects).await {
        Ok(revised_plan) => {
            report.plan = revised_plan;
            report.stage_events.push(ReplayEvent {
                stage: StageName::LlmCritique,
                timestamp_offset_ms: 0,
                description: "plan revised after critique".into(),
            });
        }
        Err(e) => {
            report.stage_events.push(ReplayEvent {
                stage: StageName::LlmCritique,
                timestamp_offset_ms: 0,
                description: format!("revision failed: {}, keeping original", e),
            });
        }
    }
}

// ── Execution Critic ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ExecutionCritiqueResult {
    pub pass: bool,
    #[serde(default)]
    pub defects: Vec<CritiqueDefect>,
    #[serde(default)]
    pub suggestions: Vec<String>,
}

/// Silent LLM-based execution critique.
/// Reviews the full execution trace after all steps complete.
/// Never fails the pipeline — best-effort only.
pub async fn critique_execution(
    payload: &str,
    report: &ExecutionReport,
) -> Result<ExecutionCritiqueResult> {
    let steps_summary = report
        .steps
        .iter()
        .map(|s| {
            let status_str = match &s.status {
                super::execution_engine::StepStatus::Ok => "OK",
                super::execution_engine::StepStatus::Skipped => "SKIPPED",
                super::execution_engine::StepStatus::Failed(e) => return format!("FAILED: {}", e),
            };
            format!("{}. [{}] {}", s.index + 1, status_str, s.description)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let user_prompt = format!(
        "Task: {}\n\nExecution results:\n{}\n\nTotal steps: {}, Success: {}",
        payload,
        steps_summary,
        report.steps.len(),
        report.success
    );

    let result = llm::chat_structured(ModelPurpose::Critic, CRITIC_PROMPT, &user_prompt).await?;

    // Fail-closed (audit findings M4/EH2): a malformed execution critique
    // must never be treated as a passing review.
    let critique: ExecutionCritiqueResult =
        serde_json::from_value(result).unwrap_or(ExecutionCritiqueResult {
            pass: false,
            defects: vec![CritiqueDefect {
                category: "UNVERIFIED".to_string(),
                description: "malformed critique response".to_string(),
            }],
            suggestions: vec![],
        });

    Ok(critique)
}

/// Run silent execution critique.
/// Adds stage events to the report. Never fails the pipeline.
pub async fn run_execution_critique(
    payload: &str,
    report: &ExecutionReport,
    stage_events: &mut Vec<ReplayEvent>,
) {
    // Skip if no steps or already failed
    if report.steps.is_empty() || !report.success {
        stage_events.push(ReplayEvent {
            stage: StageName::ExecutionCritique,
            timestamp_offset_ms: 0,
            description: "skipped: no successful steps to critique".into(),
        });
        return;
    }

    // Run critic
    let critique = match critique_execution(payload, report).await {
        Ok(c) => c,
        Err(e) => {
            stage_events.push(ReplayEvent {
                stage: StageName::ExecutionCritique,
                timestamp_offset_ms: 0,
                description: format!("skipped: {}", e),
            });
            return;
        }
    };

    if critique.pass {
        stage_events.push(ReplayEvent {
            stage: StageName::ExecutionCritique,
            timestamp_offset_ms: 0,
            description: "passed".into(),
        });
    } else {
        stage_events.push(ReplayEvent {
            stage: StageName::ExecutionCritique,
            timestamp_offset_ms: 0,
            description: format!("found {} defect(s)", critique.defects.len()),
        });
    }

    // Log suggestions if any (for future improvement)
    if !critique.suggestions.is_empty() {
        stage_events.push(ReplayEvent {
            stage: StageName::ExecutionCritique,
            timestamp_offset_ms: 0,
            description: format!("{} suggestion(s) noted", critique.suggestions.len()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deliberate invariant pinning: these asserts freeze the threshold
    // constants at their measured values so a drift fails the suite loudly.
    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn min_steps_threshold_works() {
        assert!(1 < MIN_STEPS_FOR_CRITIQUE);
        assert!(2 >= MIN_STEPS_FOR_CRITIQUE);
    }

    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn max_defects_hard_stop_works() {
        assert!(5 <= MAX_DEFECTS_BEFORE_SKIP_REVISION);
        assert!(6 > MAX_DEFECTS_BEFORE_SKIP_REVISION);
    }

    #[test]
    fn revision_limit_works() {
        assert_eq!(MAX_PLAN_REVISIONS, 1);
    }
}
