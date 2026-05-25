#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    TightenPlannerPrompt,
    NormalizePlannerOutput,
    AddLlmFallbackHandling,
    AddPlannerTestCoverage,
    ValidatePlannerOutput,
    AnalyzeTask,
    PlanExecution,
    ExecuteChanges,
    ReadRepository,
    LocateBug,
    PatchCode,
    RunTests,
    ValidatePatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub detail: Option<String>,
}

impl Step {
    pub fn as_text(&self) -> String {
        match (&self.kind, &self.detail) {
            (StepKind::TightenPlannerPrompt, _) => "tighten planner prompt".into(),
            (StepKind::NormalizePlannerOutput, _) => "normalize planner output".into(),
            (StepKind::AddLlmFallbackHandling, _) => "add llm fallback handling".into(),
            (StepKind::AddPlannerTestCoverage, _) => "add planner test coverage".into(),
            (StepKind::ValidatePlannerOutput, _) => "validate planner output".into(),
            (StepKind::AnalyzeTask, Some(detail)) => format!("analyze task: {}", detail),
            (StepKind::PlanExecution, Some(detail)) => format!("plan execution: {}", detail),
            (StepKind::ExecuteChanges, Some(detail)) => format!("execute changes: {}", detail),
            (StepKind::ReadRepository, Some(detail)) => format!("read repository: {}", detail),
            (StepKind::LocateBug, Some(detail)) => format!("locate bug: {}", detail),
            (StepKind::PatchCode, Some(detail)) => format!("patch code: {}", detail),
            (StepKind::RunTests, Some(detail)) => format!("run tests: {}", detail),
            (StepKind::ValidatePatch, Some(detail)) => format!("validate patch: {}", detail),
            (StepKind::AnalyzeTask, None) => "analyze task".into(),
            (StepKind::PlanExecution, None) => "plan execution".into(),
            (StepKind::ExecuteChanges, None) => "execute changes".into(),
            (StepKind::ReadRepository, None) => "read repository".into(),
            (StepKind::LocateBug, None) => "locate bug".into(),
            (StepKind::PatchCode, None) => "patch code".into(),
            (StepKind::RunTests, None) => "run tests".into(),
            (StepKind::ValidatePatch, None) => "validate patch".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClass {
    Generic,
    PlannerHardening,
    CodeFix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerCapability {
    Planner,
    Executor,
    Verifier,
    LegacyGeneric,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSpec {
    pub kind: StepKind,
    pub required_capability: WorkerCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    Success,
    RetryableFailure,
    TerminalFailure,
    Blocked,
}

pub fn terminal_outcome(outcome: &StepOutcome) -> bool {
    matches!(outcome, StepOutcome::Success | StepOutcome::TerminalFailure)
}

pub fn required_capability_for_step(step_kind: &StepKind) -> WorkerCapability {
    match step_kind {
        StepKind::TightenPlannerPrompt
        | StepKind::NormalizePlannerOutput
        | StepKind::AddLlmFallbackHandling
        | StepKind::AddPlannerTestCoverage
        | StepKind::AnalyzeTask
        | StepKind::PlanExecution
        | StepKind::ReadRepository
        | StepKind::LocateBug => WorkerCapability::Planner,
        StepKind::ExecuteChanges
        | StepKind::PatchCode
        | StepKind::RunTests => WorkerCapability::Executor,
        StepKind::ValidatePlannerOutput
        | StepKind::ValidatePatch => WorkerCapability::Verifier,
    }
}


pub fn task_class_to_flow(task_class: TaskClass) -> Vec<StepSpec> {
    match task_class {
        TaskClass::Generic => vec![
            StepSpec { kind: StepKind::AnalyzeTask, required_capability: required_capability_for_step(&StepKind::AnalyzeTask) },
            StepSpec { kind: StepKind::PlanExecution, required_capability: required_capability_for_step(&StepKind::PlanExecution) },
            StepSpec { kind: StepKind::ExecuteChanges, required_capability: required_capability_for_step(&StepKind::ExecuteChanges) },
        ],
        TaskClass::PlannerHardening => vec![
            StepSpec { kind: StepKind::TightenPlannerPrompt, required_capability: required_capability_for_step(&StepKind::TightenPlannerPrompt) },
            StepSpec { kind: StepKind::NormalizePlannerOutput, required_capability: required_capability_for_step(&StepKind::NormalizePlannerOutput) },
            StepSpec { kind: StepKind::AddLlmFallbackHandling, required_capability: required_capability_for_step(&StepKind::AddLlmFallbackHandling) },
            StepSpec { kind: StepKind::AddPlannerTestCoverage, required_capability: required_capability_for_step(&StepKind::AddPlannerTestCoverage) },
            StepSpec { kind: StepKind::ValidatePlannerOutput, required_capability: required_capability_for_step(&StepKind::ValidatePlannerOutput) },
        ],
        TaskClass::CodeFix => vec![
            StepSpec { kind: StepKind::ReadRepository, required_capability: required_capability_for_step(&StepKind::ReadRepository) },
            StepSpec { kind: StepKind::LocateBug, required_capability: required_capability_for_step(&StepKind::LocateBug) },
            StepSpec { kind: StepKind::PatchCode, required_capability: required_capability_for_step(&StepKind::PatchCode) },
            StepSpec { kind: StepKind::RunTests, required_capability: required_capability_for_step(&StepKind::RunTests) },
            StepSpec { kind: StepKind::ValidatePatch, required_capability: required_capability_for_step(&StepKind::ValidatePatch) },
        ],
    }
}

pub fn step_specs_to_steps(step_specs: &[StepSpec], detail: Option<&str>) -> Vec<Step> {
    step_specs
        .iter()
        .map(|spec| Step {
            kind: spec.kind.clone(),
            detail: detail.map(|d| d.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{required_capability_for_step, step_specs_to_steps, task_class_to_flow, terminal_outcome, StepKind, StepOutcome, TaskClass, WorkerCapability};

    #[test]
    fn generic_task_class_maps_to_default_execution_flow() {
        let kinds: Vec<StepKind> = task_class_to_flow(TaskClass::Generic)
            .into_iter()
            .map(|step| step.kind)
            .collect();

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
    fn planner_hardening_task_class_maps_to_canonical_planner_flow() {
        let kinds: Vec<StepKind> = task_class_to_flow(TaskClass::PlannerHardening)
            .into_iter()
            .map(|step| step.kind)
            .collect();

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
    fn codefix_task_class_maps_to_canonical_codefix_flow() {
        let kinds: Vec<StepKind> = task_class_to_flow(TaskClass::CodeFix)
            .into_iter()
            .map(|step| step.kind)
            .collect();

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
    fn analyze_and_plan_steps_require_planner_capability() {
        assert_eq!(
            required_capability_for_step(&StepKind::AnalyzeTask),
            WorkerCapability::Planner
        );
        assert_eq!(
            required_capability_for_step(&StepKind::PlanExecution),
            WorkerCapability::Planner
        );
    }

    #[test]
    fn execute_changes_requires_executor_capability() {
        assert_eq!(
            required_capability_for_step(&StepKind::ExecuteChanges),
            WorkerCapability::Executor
        );
    }

    #[test]
    fn validate_planner_output_requires_verifier_capability() {
        assert_eq!(
            required_capability_for_step(&StepKind::ValidatePlannerOutput),
            WorkerCapability::Verifier
        );
    }

    #[test]
    fn codefix_steps_require_expected_capabilities() {
        assert_eq!(
            required_capability_for_step(&StepKind::ReadRepository),
            WorkerCapability::Planner
        );
        assert_eq!(
            required_capability_for_step(&StepKind::LocateBug),
            WorkerCapability::Planner
        );
        assert_eq!(
            required_capability_for_step(&StepKind::PatchCode),
            WorkerCapability::Executor
        );
        assert_eq!(
            required_capability_for_step(&StepKind::RunTests),
            WorkerCapability::Executor
        );
        assert_eq!(
            required_capability_for_step(&StepKind::ValidatePatch),
            WorkerCapability::Verifier
        );
    }

    #[test]
    fn step_specs_can_be_lowered_to_steps_with_detail() {
        let specs = task_class_to_flow(TaskClass::Generic);
        let steps = step_specs_to_steps(&specs, Some("Refactor scheduler reconciliation"));

        let rendered: Vec<String> = steps.into_iter().map(|s| s.as_text()).collect();

        assert_eq!(
            rendered,
            vec![
                "analyze task: Refactor scheduler reconciliation".to_string(),
                "plan execution: Refactor scheduler reconciliation".to_string(),
                "execute changes: Refactor scheduler reconciliation".to_string(),
            ]
        );
    }

    #[test]
    fn success_and_terminal_failure_are_terminal_outcomes() {
        assert!(terminal_outcome(&StepOutcome::Success));
        assert!(terminal_outcome(&StepOutcome::TerminalFailure));
    }

    #[test]
    fn retryable_failure_and_blocked_are_not_terminal_outcomes() {
        assert!(!terminal_outcome(&StepOutcome::RetryableFailure));
        assert!(!terminal_outcome(&StepOutcome::Blocked));
    }

}
