#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    Ai,
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

#[allow(dead_code)]
pub const CONTRACT_VERSION: u32 = 1;

#[allow(dead_code)]
pub fn contract_version() -> u32 {
    CONTRACT_VERSION
}

#[allow(dead_code)]
pub fn task_class_names() -> &'static [&'static str] {
    &["Generic", "PlannerHardening", "CodeFix"]
}

#[allow(dead_code)]
pub fn step_kind_names() -> &'static [&'static str] {
    &[
        "TightenPlannerPrompt",
        "NormalizePlannerOutput",
        "AddLlmFallbackHandling",
        "AddPlannerTestCoverage",
        "ValidatePlannerOutput",
        "AnalyzeTask",
        "PlanExecution",
        "ExecuteChanges",
        "ReadRepository",
        "LocateBug",
        "PatchCode",
        "RunTests",
        "ValidatePatch",
    ]
}

#[allow(dead_code)]
pub fn outcome_names() -> &'static [&'static str] {
    &["Success", "RetryableFailure", "TerminalFailure", "Blocked"]
}

#[allow(dead_code)]
pub fn event_type_names() -> &'static [&'static str] {
    &[
        "task.created",
        "task.started",
        "task.progress",
        "task.succeeded",
        "task.failed",
        "task.blocked",
    ]
}

/// Advisory-only capability hint from the static workflow contract.
/// Runtime execution is lease-authorized; this mapping is metadata, not an execution guard.
#[allow(dead_code)]
pub fn advisory_capability_for_step(step_kind: &StepKind) -> WorkerCapability {
    required_capability_for_step(step_kind)
}

const GENERIC_FLOW: &[(StepKind, WorkerCapability)] = &[
    (StepKind::AnalyzeTask, WorkerCapability::Planner),
    (StepKind::PlanExecution, WorkerCapability::Planner),
    (StepKind::ExecuteChanges, WorkerCapability::Executor),
];

const PLANNER_HARDENING_FLOW: &[(StepKind, WorkerCapability)] = &[
    (StepKind::TightenPlannerPrompt, WorkerCapability::Planner),
    (StepKind::NormalizePlannerOutput, WorkerCapability::Planner),
    (StepKind::AddLlmFallbackHandling, WorkerCapability::Planner),
    (StepKind::AddPlannerTestCoverage, WorkerCapability::Planner),
    (StepKind::ValidatePlannerOutput, WorkerCapability::Verifier),
];

const CODEFIX_FLOW: &[(StepKind, WorkerCapability)] = &[
    (StepKind::ReadRepository, WorkerCapability::Planner),
    (StepKind::LocateBug, WorkerCapability::Planner),
    (StepKind::PatchCode, WorkerCapability::Executor),
    (StepKind::RunTests, WorkerCapability::Executor),
    (StepKind::ValidatePatch, WorkerCapability::Verifier),
];

fn flow_table(task_class: TaskClass) -> &'static [(StepKind, WorkerCapability)] {
    match task_class {
        TaskClass::Generic => GENERIC_FLOW,
        TaskClass::PlannerHardening => PLANNER_HARDENING_FLOW,
        TaskClass::CodeFix => CODEFIX_FLOW,
    }
}

pub fn required_capability_for_step(step_kind: &StepKind) -> WorkerCapability {
    for (kind, capability) in GENERIC_FLOW
        .iter()
        .chain(PLANNER_HARDENING_FLOW.iter())
        .chain(CODEFIX_FLOW.iter())
    {
        if kind == step_kind {
            return *capability;
        }
    }

    panic!("missing capability mapping for step kind")
}

pub fn task_class_to_flow(task_class: TaskClass) -> Vec<StepSpec> {
    flow_table(task_class)
        .iter()
        .map(|(kind, capability)| StepSpec {
            kind: kind.clone(),
            required_capability: *capability,
        })
        .collect()
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
    use super::{
        contract_version, event_type_names, outcome_names, required_capability_for_step,
        step_kind_names, step_specs_to_steps, task_class_names, task_class_to_flow,
        terminal_outcome, StepKind, StepOutcome, TaskClass, WorkerCapability,
    };

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
    fn every_task_class_has_a_non_empty_canonical_flow() {
        for task_class in [
            TaskClass::Generic,
            TaskClass::PlannerHardening,
            TaskClass::CodeFix,
        ] {
            assert!(
                !task_class_to_flow(task_class).is_empty(),
                "expected non-empty canonical flow for {:?}",
                task_class
            );
        }
    }

    #[test]
    fn every_step_kind_is_present_in_exactly_one_canonical_flow() {
        let all_step_kinds = [
            StepKind::TightenPlannerPrompt,
            StepKind::NormalizePlannerOutput,
            StepKind::AddLlmFallbackHandling,
            StepKind::AddPlannerTestCoverage,
            StepKind::ValidatePlannerOutput,
            StepKind::AnalyzeTask,
            StepKind::PlanExecution,
            StepKind::ExecuteChanges,
            StepKind::ReadRepository,
            StepKind::LocateBug,
            StepKind::PatchCode,
            StepKind::RunTests,
            StepKind::ValidatePatch,
        ];

        let flows = [
            task_class_to_flow(TaskClass::Generic),
            task_class_to_flow(TaskClass::PlannerHardening),
            task_class_to_flow(TaskClass::CodeFix),
        ];

        let mut seen: Vec<StepKind> = Vec::new();

        for flow in &flows {
            for step in flow {
                let duplicates = seen.iter().filter(|kind| **kind == step.kind).count();
                assert_eq!(
                    duplicates, 0,
                    "step kind {:?} appears in more than one canonical flow",
                    step.kind
                );
                seen.push(step.kind.clone());
            }
        }

        for expected in all_step_kinds {
            let count = seen.iter().filter(|kind| **kind == expected).count();
            assert_eq!(
                count, 1,
                "expected step kind {:?} to appear exactly once",
                expected
            );
        }

        assert_eq!(
            seen.len(),
            13,
            "unexpected extra step kinds in canonical flows"
        );
    }

    #[test]
    fn contract_v1_metadata_is_stable() {
        assert_eq!(contract_version(), 1);
        assert_eq!(
            task_class_names(),
            &["Generic", "PlannerHardening", "CodeFix"]
        );
        assert_eq!(
            step_kind_names(),
            &[
                "TightenPlannerPrompt",
                "NormalizePlannerOutput",
                "AddLlmFallbackHandling",
                "AddPlannerTestCoverage",
                "ValidatePlannerOutput",
                "AnalyzeTask",
                "PlanExecution",
                "ExecuteChanges",
                "ReadRepository",
                "LocateBug",
                "PatchCode",
                "RunTests",
                "ValidatePatch",
            ]
        );
        assert_eq!(
            outcome_names(),
            &["Success", "RetryableFailure", "TerminalFailure", "Blocked"]
        );
    }

    #[test]
    fn event_type_names_match_execution_event_v1_contract() {
        assert_eq!(
            event_type_names(),
            &[
                "task.created",
                "task.started",
                "task.progress",
                "task.succeeded",
                "task.failed",
                "task.blocked",
            ]
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
