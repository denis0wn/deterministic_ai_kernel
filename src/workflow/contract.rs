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

impl StepKind {
    pub fn to_primitive_kind(&self) -> crate::execution_abi::primitives::PrimitiveKind {
        use crate::execution_abi::primitives::PrimitiveKind;
        match self {
            StepKind::TightenPlannerPrompt => PrimitiveKind::Compute,
            StepKind::NormalizePlannerOutput => PrimitiveKind::Compute,
            StepKind::AddLlmFallbackHandling => PrimitiveKind::Compute,
            StepKind::AddPlannerTestCoverage => PrimitiveKind::Compute,
            StepKind::ValidatePlannerOutput => PrimitiveKind::Route,
            StepKind::AnalyzeTask => PrimitiveKind::Reasoning,
            StepKind::PlanExecution => PrimitiveKind::Reasoning,
            StepKind::ExecuteChanges => PrimitiveKind::ToolExecution,
            StepKind::ReadRepository => PrimitiveKind::Read,
            StepKind::LocateBug => PrimitiveKind::Reasoning,
            StepKind::PatchCode => PrimitiveKind::Write,
            StepKind::RunTests => PrimitiveKind::Compute,
            StepKind::ValidatePatch => PrimitiveKind::Route,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Step {
    pub kind: StepKind,
    pub detail: Option<String>,
    pub primitive_binding: Option<String>,
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

impl TaskClass {
    pub fn to_exec_spec(&self, detail: Option<&str>) -> crate::exec_spec::ExecSpec {
        let specs = task_class_to_flow(*self);
        let steps_lowered = step_specs_to_steps(&specs, detail);
        steps_to_exec_spec(&steps_lowered)
    }
}

fn extract_command(desc: &str) -> Option<String> {
    let lower = desc.to_lowercase();
    if let Some(idx) = lower.find("command ") {
        return Some(desc[idx + 8..].to_string());
    }
    if let Some(idx) = lower.find("run ") {
        return Some(desc[idx + 4..].to_string());
    }
    if let Some(idx) = lower.find("execute ") {
        return Some(desc[idx + 8..].to_string());
    }
    None
}

pub fn try_steps_to_exec_spec(steps: &[Step]) -> anyhow::Result<crate::exec_spec::ExecSpec> {
    let mut steps_specs = Vec::new();
    let mut transitions = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        let slug = match step.kind {
            StepKind::TightenPlannerPrompt => "tighten_planner_prompt",
            StepKind::NormalizePlannerOutput => "normalize_planner_output",
            StepKind::AddLlmFallbackHandling => "add_llm_fallback_handling",
            StepKind::AddPlannerTestCoverage => "add_planner_test_coverage",
            StepKind::ValidatePlannerOutput => "validate_planner_output",
            StepKind::AnalyzeTask => "analyze_task",
            StepKind::PlanExecution => "plan_execution",
            StepKind::ExecuteChanges => "execute_changes",
            StepKind::ReadRepository => "read_repository",
            StepKind::LocateBug => "locate_bug",
            StepKind::PatchCode => "patch_code",
            StepKind::RunTests => "run_tests",
            StepKind::ValidatePatch => "validate_patch",
        };
        let step_id = format!("{:02}_{}", i, slug);

        let cap = required_capability_for_step(&step.kind);
        let required_capability = format!("{:?}", cap);

        let constraint = crate::exec_spec::Constraint {
            target: "worker".to_string(),
            key: "required_capability".to_string(),
            value: required_capability.clone(),
        };

        let primitive_kind = step.kind.to_primitive_kind();
        let raw_detail = step.detail.as_deref().unwrap_or("");
        let prim_kind = primitive_kind;

        let primitive_binding = step.primitive_binding.as_deref();
        let prim_payload = if let Some(binding) = primitive_binding {
            crate::tool_registry::materialize_allowed_tool(binding, step.detail.as_deref())?
        } else {
            match primitive_kind {
                crate::execution_abi::primitives::PrimitiveKind::Compute => {
                    let cmd =
                        extract_command(raw_detail).unwrap_or_else(|| "echo 'hello'".to_string());
                    serde_json::json!({
                        "binding": "process.compute",
                        "tool_version": "v1",
                        "command": cmd,
                    })
                }
                _ => {
                    serde_json::json!({
                        "requires_llm": matches!(
                            step.kind,
                            StepKind::ExecuteChanges
                                | StepKind::PatchCode
                                | StepKind::PlanExecution
                                | StepKind::LocateBug
                        ),
                        "operation": if step.kind == StepKind::AnalyzeTask {
                            "semantic_embedding"
                        } else {
                            "none"
                        },
                        "detail": step.detail.clone(),
                        "step_kind": format!("{:?}", step.kind)
                    })
                }
            }
        };

        let primitive = Some(crate::execution_abi::primitives::PrimitiveSpec {
            id: crate::execution_abi::primitives::PrimitiveId(step_id.clone()),
            kind: prim_kind,
            payload: prim_payload,
        });

        let inputs = match step.kind {
            StepKind::AnalyzeTask => vec!["task_description".to_string()],
            StepKind::PlanExecution => vec!["analysis_seed".to_string()],
            StepKind::ExecuteChanges => vec!["execution_plan".to_string()],
            _ => vec![],
        };
        let outputs = match step.kind {
            StepKind::AnalyzeTask => vec!["analysis_seed".to_string()],
            StepKind::PlanExecution => vec!["execution_plan".to_string()],
            StepKind::ExecuteChanges => vec!["changes_committed".to_string()],
            _ => vec![],
        };
        let metadata = serde_json::json!({
            "step_kind": format!("{:?}", step.kind)
        });

        steps_specs.push(crate::exec_spec::StepSpec {
            step_id: step_id.clone(),
            required_capability,
            detail: step.detail.clone(),
            primitive,
            constraints: vec![constraint],
            artifact_requirements: vec![],
            inputs,
            outputs,
            metadata,
        });

        if i > 0 {
            let prev_slug = match steps[i - 1].kind {
                StepKind::TightenPlannerPrompt => "tighten_planner_prompt",
                StepKind::NormalizePlannerOutput => "normalize_planner_output",
                StepKind::AddLlmFallbackHandling => "add_llm_fallback_handling",
                StepKind::AddPlannerTestCoverage => "add_planner_test_coverage",
                StepKind::ValidatePlannerOutput => "validate_planner_output",
                StepKind::AnalyzeTask => "analyze_task",
                StepKind::PlanExecution => "plan_execution",
                StepKind::ExecuteChanges => "execute_changes",
                StepKind::ReadRepository => "read_repository",
                StepKind::LocateBug => "locate_bug",
                StepKind::PatchCode => "patch_code",
                StepKind::RunTests => "run_tests",
                StepKind::ValidatePatch => "validate_patch",
            };
            let prev_id = format!("{:02}_{}", i - 1, prev_slug);
            transitions.push(crate::exec_spec::TransitionRule {
                step_id,
                depends_on: vec![prev_id],
            });
        }
    }

    let mut dependencies = Vec::new();
    for t in &transitions {
        dependencies.push(crate::exec_spec::Dependency {
            step_id: t.step_id.clone(),
            depends_on: t.depends_on.clone(),
        });
    }

    Ok(crate::exec_spec::ExecSpec::new(
        1,
        steps_specs,
        transitions,
        dependencies,
        vec![],
        std::collections::BTreeMap::new(),
    ))
}

pub fn steps_to_exec_spec(steps: &[Step]) -> crate::exec_spec::ExecSpec {
    try_steps_to_exec_spec(steps)
        .expect("workflow contract invariant violated: unable to materialize exec spec")
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
            primitive_binding: None,
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
    fn registry_materializes_allowed_tools() {
        let payload = crate::tool_registry::materialize_allowed_tool(
            "repo.run_tests.cargo_all_targets",
            Some("ignored"),
        )
        .unwrap();

        assert_eq!(payload["binding"], "repo.run_tests.cargo_all_targets");
        assert_eq!(payload["tool_version"], "v1");
        assert_eq!(payload["executable_tool"], "cargo test --all-targets");
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
