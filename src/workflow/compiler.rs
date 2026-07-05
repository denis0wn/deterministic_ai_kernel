use anyhow::Result;

use crate::event_bus::EventBus;
use crate::llm;
use crate::workflow::contract::{step_specs_to_steps, task_class_to_flow, Step, TaskClass};
use crate::workflow::pipeline::{PipelineInput, PipelineOutput, PlannerPipeline};
use crate::workflow::planner::{apply_semantic_bias_from_seed, parse_steps, validate_steps};
use crate::workflow::planner_types::PlannerManifest;

pub struct Workflow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeFixSourceType {
    CompileError,
    TestFailure,
    LintReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeFixInput {
    pub source_type: CodeFixSourceType,
    pub artifact_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskInput {
    Generic(String),
    PlannerHardening(String),
    CodeFix(CodeFixInput),
}

impl TaskInput {
    pub fn generic(task: impl Into<String>) -> Self {
        TaskInput::Generic(task.into())
    }

    pub fn planner_hardening(task: impl Into<String>) -> Self {
        TaskInput::PlannerHardening(task.into())
    }

    pub fn from_compile_error(artifact_ref: impl Into<String>) -> Self {
        TaskInput::CodeFix(CodeFixInput {
            source_type: CodeFixSourceType::CompileError,
            artifact_ref: artifact_ref.into(),
        })
    }

    pub fn from_test_failure(artifact_ref: impl Into<String>) -> Self {
        TaskInput::CodeFix(CodeFixInput {
            source_type: CodeFixSourceType::TestFailure,
            artifact_ref: artifact_ref.into(),
        })
    }

    pub fn from_lint_report(artifact_ref: impl Into<String>) -> Self {
        TaskInput::CodeFix(CodeFixInput {
            source_type: CodeFixSourceType::LintReport,
            artifact_ref: artifact_ref.into(),
        })
    }

    pub fn task_class(&self) -> TaskClass {
        match self {
            TaskInput::Generic(_) => TaskClass::Generic,
            TaskInput::PlannerHardening(_) => TaskClass::PlannerHardening,
            TaskInput::CodeFix(_) => TaskClass::CodeFix,
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            TaskInput::Generic(task) | TaskInput::PlannerHardening(task) => {
                let normalized = task.trim();
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                }
            }
            TaskInput::CodeFix(_) => None,
        }
    }
}

impl Workflow {
    pub fn build_steps(input: &TaskInput) -> Vec<Step> {
        let task_class = input.task_class();
        let step_specs = task_class_to_flow(task_class);

        if task_class == TaskClass::Generic {
            return step_specs_to_steps(&step_specs, input.detail());
        }

        step_specs_to_steps(&step_specs, None)
    }

    /// Canonical executor entry point. Deterministic, event-sourced.
    /// Use this for all new call sites.
    pub fn build_via_pipeline(
        task_id: &str,
        input: &TaskInput,
        bus: &EventBus,
        seed: u64,
    ) -> Result<PipelineOutput> {
        let manifest = PlannerManifest::v1();
        let task_text = input.detail().unwrap_or("").to_string();
        PlannerPipeline::new(bus).run(PipelineInput {
            task_id: task_id.to_string(),
            task_text,
            seed,
            manifest,
        })
    }

    /// Legacy LLM-augmented path. Kept as fallback — prefer `build_via_pipeline`.
    #[allow(dead_code)]
    pub async fn build_from_task_llm(input: &TaskInput) -> Result<Vec<Step>> {
        let normalized = input.detail().unwrap_or("");

        if normalized.is_empty() {
            return Ok(Self::build_steps(input));
        }

        let deterministic = Self::build_steps(input);

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

        let text = llm::task_planner(&prompt).await?;
        let mut steps = deterministic.clone();
        let parsed = apply_semantic_bias_from_seed(parse_steps(&text), None);

        for kind in parsed {
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

    use super::{TaskInput, Workflow};

    #[test]
    fn llm_planning_task_uses_canonical_ir_steps() {
        let steps = Workflow::build_steps(&TaskInput::planner_hardening(
            "Add LLM-powered task planning to the kernel",
        ));
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
        let steps = Workflow::build_steps(&TaskInput::generic("Refactor scheduler reconciliation"));
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

    #[test]
    fn planner_hardening_input_maps_to_planner_hardening_task_class() {
        let input = TaskInput::planner_hardening("Add LLM-powered task planning to the kernel");

        assert_eq!(
            input.task_class(),
            crate::workflow::contract::TaskClass::PlannerHardening
        );
    }

    #[test]
    fn codefix_input_maps_to_codefix_task_class() {
        let input = TaskInput::from_compile_error("cargo-check.log");

        assert_eq!(
            input.task_class(),
            crate::workflow::contract::TaskClass::CodeFix
        );
    }

    #[test]
    fn codefix_input_uses_canonical_codefix_flow() {
        let steps =
            Workflow::build_steps(&TaskInput::from_test_failure("scheduler_integration.log"));
        let kinds: Vec<StepKind> = steps.into_iter().map(|s| s.kind).collect();

        assert_eq!(
            kinds,
            vec![
                StepKind::ReadRepository,
                StepKind::LocateBug,
                StepKind::PatchCode,
                StepKind::RunTests,
                StepKind::ValidatePatch,
            ]
        );
    }

    #[test]
    fn lint_report_codefix_input_maps_to_codefix_task_class() {
        let input = TaskInput::from_lint_report("clippy.log");

        assert_eq!(
            input.task_class(),
            crate::workflow::contract::TaskClass::CodeFix
        );
    }

    #[test]
    fn all_codefix_failure_signals_converge_to_same_canonical_flow() {
        let compile_steps =
            Workflow::build_steps(&TaskInput::from_compile_error("cargo-check.log"));
        let test_steps =
            Workflow::build_steps(&TaskInput::from_test_failure("scheduler_integration.log"));
        let lint_steps = Workflow::build_steps(&TaskInput::from_lint_report("clippy.log"));

        let compile_kinds: Vec<StepKind> = compile_steps.into_iter().map(|s| s.kind).collect();
        let test_kinds: Vec<StepKind> = test_steps.into_iter().map(|s| s.kind).collect();
        let lint_kinds: Vec<StepKind> = lint_steps.into_iter().map(|s| s.kind).collect();

        assert_eq!(compile_kinds, test_kinds);
        assert_eq!(test_kinds, lint_kinds);
    }
}
