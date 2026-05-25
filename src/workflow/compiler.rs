use anyhow::Result;

use crate::llm;
use crate::workflow::contract::{step_specs_to_steps, task_class_to_flow, Step, TaskClass};
use crate::workflow::planner::{parse_steps, validate_steps};

pub struct Workflow;

impl Workflow {
    fn classify_task(task: &str) -> TaskClass {
        let normalized = task.trim();
        let lower = normalized.to_ascii_lowercase();

        if lower.contains("llm") && lower.contains("plan") {
            return TaskClass::PlannerHardening;
        }

        TaskClass::Generic
    }

    pub fn build_steps(task: &str) -> Vec<Step> {
        let normalized = task.trim();
        let task_class = Self::classify_task(task);
        let step_specs = task_class_to_flow(task_class);

        if normalized.is_empty() {
            return step_specs_to_steps(&step_specs, None);
        }

        if task_class == TaskClass::Generic {
            return step_specs_to_steps(&step_specs, Some(normalized));
        }

        step_specs_to_steps(&step_specs, None)
    }

    pub async fn build_from_task_llm(task: &str) -> Result<Vec<Step>> {
        let normalized = task.trim();

        if normalized.is_empty() {
            return Ok(Self::build_steps(task));
        }

        let deterministic = Self::build_steps(task);

        let prompt = format!(
            "You are improving an EXISTING Rust kernel planner. \
Refine the provided draft plan into short implementation steps. \
Keep the same scope, stay grounded in the existing code, and return one step per line. \
No numbering, no bullets, no commentary.\n\nTask: {}\n\nDraft plan:\n{}",
            normalized,
            deterministic
                .iter()
                .map(|s| s.as_text())
                .collect::<Vec<_>>()
                .join("\n")
        );

        let text = llm::coding_assistant(&prompt).await?;
        let mut steps = deterministic.clone();

        for kind in parse_steps(&text) {
            let step = Step { kind, detail: None };
            if !steps.iter().any(|existing| existing == &step) {
                steps.push(step);
            }

            if steps.len() == deterministic.len() {
                break;
            }
        }

        Ok(validate_steps(steps))
    }
}

#[cfg(test)]
mod tests {
    use crate::workflow::contract::StepKind;

    use super::Workflow;

    #[test]
    fn llm_planning_task_uses_canonical_ir_steps() {
        let steps = Workflow::build_steps("Add LLM-powered task planning to the kernel");
        let kinds: Vec<StepKind> = steps.into_iter().map(|s| s.kind).collect();

        assert_eq!(
            kinds,
            vec![
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
                StepKind::AddLlmFallbackHandling,
                StepKind::AddPlannerTestCoverage,
                StepKind::ValidatePlannerOutput,
            ]
        );
    }

    #[test]
    fn generic_task_uses_default_execution_flow() {
        let steps = Workflow::build_steps("Refactor scheduler reconciliation");
        let kinds: Vec<StepKind> = steps.into_iter().map(|s| s.kind).collect();

        assert_eq!(
            kinds,
            vec![
                StepKind::AnalyzeTask,
                StepKind::PlanExecution,
                StepKind::ExecuteChanges,
            ]
        );
    }
}
