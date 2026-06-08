use crate::event_bus::EventBus;
use crate::execution::runtime::Runtime;
use crate::workflow::contract::{Step, StepKind};
use serde_json::json;

#[allow(dead_code)]
pub struct ExecutionEngine {
    bus: EventBus,
}

#[allow(dead_code)]
impl ExecutionEngine {
    pub fn new(bus: EventBus) -> Self {
        Self { bus }
    }

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

    pub async fn run_plan(&self, task_id: &str, plan: &[Step]) {
        let runtime = Runtime::new(self.bus.clone());

        for (i, step) in plan.iter().enumerate() {
            let step_id = format!("{:02}_{}", i, Self::step_slug(step));
            let effect_id = format!("{task_id}/{step_id}/dispatch");
            let step_text = step.as_text();

            let mut events = vec![
                (
                    "LEASE_ACQUIRED".to_string(),
                    json!({"worker":"kernel", "step": step_text}),
                ),
                (
                    "EFFECT_RESERVED".to_string(),
                    json!({"effect_id": effect_id.clone(), "step": step_text}),
                ),
                (
                    "STEP_DISPATCHED".to_string(),
                    json!({"agent":"generic", "step": step_text}),
                ),
            ];

            match step.kind {
                StepKind::AnalyzeTask => {
                    match runtime.execute_step(task_id, step).await {
                        Ok(()) => events.push((
                            "STEP_COMPLETED".to_string(),
                            json!({"effect_id": effect_id.clone(), "status":"ok", "step": step_text}),
                        )),
                        Err(e) => events.push((
                            "STEP_FAILED".to_string(),
                            json!({"effect_id": effect_id.clone(), "status":"error", "reason":e.to_string(), "step": step_text}),
                        )),
                    }
                }
                _ => {
                    let terminal_event = if i == 1 {
                        (
                            "STEP_FAILED".to_string(),
                            json!({"effect_id": effect_id.clone(), "status":"error", "reason":"simulated failure", "step": step_text}),
                        )
                    } else {
                        (
                            "STEP_COMPLETED".to_string(),
                            json!({"effect_id": effect_id.clone(), "status":"ok", "step": step_text}),
                        )
                    };
                    events.push(terminal_event);
                }
            }

            self.bus
                .commit_causal_unit(task_id, &step_id, events)
                .unwrap();
        }

        self.bus
            .append_event(task_id, None, "DONE", &json!({}))
            .unwrap();
    }
}
