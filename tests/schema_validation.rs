use deterministic_ai_kernel::workflow::contract::StepKind;
use deterministic_ai_kernel::workflow::semantic::bias::SemanticBias;

fn all_step_kinds() -> Vec<StepKind> {
    vec![
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
    ]
}

#[test]
fn round_trip_valid_bias() {
    let domain = all_step_kinds();
    let bias = SemanticBias::neutral_for(&domain);
    let json = bias.to_json().expect("serialization failed");
    let restored = SemanticBias::from_json(&json).expect("validation + deserialization failed");
    assert_eq!(bias, restored);
}

#[test]
fn rejects_invalid_bias_via_public_api() {
    let bad = serde_json::json!({
        "version": "v1",
        "seed": 0,
        "preferred": ["foobar"],
        "weights": {
            "TightenPlannerPrompt": 1.0,
            "NormalizePlannerOutput": 1.0,
            "AddLlmFallbackHandling": 1.0,
            "AddPlannerTestCoverage": 1.0,
            "ValidatePlannerOutput": 1.0,
            "AnalyzeTask": 1.0,
            "PlanExecution": 1.0,
            "ExecuteChanges": 1.0,
            "ReadRepository": 1.0,
            "LocateBug": 1.0,
            "PatchCode": 1.0,
            "RunTests": 1.0,
            "ValidatePatch": 1.0
        }
    });
    assert!(SemanticBias::from_json(&bad).is_err());
}
