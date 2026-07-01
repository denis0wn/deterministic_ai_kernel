use deterministic_ai_kernel::workflow::contract::StepKind;
use deterministic_ai_kernel::workflow::semantic::bias::{SemanticBias, BIAS_VERSION_V1};
use std::collections::BTreeMap;

#[test]
fn bias_v1_explain_snapshot_is_stable() {
    let mut weights = BTreeMap::new();
    weights.insert("AnalyzeTask".to_string(), 0.91);
    weights.insert("ExecuteChanges".to_string(), 0.42);

    let bias = SemanticBias {
        version: BIAS_VERSION_V1,
        seed: 123,
        preferred: vec![StepKind::AnalyzeTask],
        weights,
    };

    let rendered = bias.explain_lines().join("\n");
    let expected = [
        "bias.version=1".to_string(),
        "bias.seed=123".to_string(),
        "bias.preferred=[AnalyzeTask]".to_string(),
        "bias.meta.version=1".to_string(),
        "bias.meta.seed=123".to_string(),
        "bias.meta.preferred_count=1".to_string(),
        "bias.meta.weighted_count=2".to_string(),
        "bias.weight.AnalyzeTask=0.910000".to_string(),
        "bias.weight.ExecuteChanges=0.420000".to_string(),
    ]
    .join("\n");

    assert_eq!(rendered, expected);
}

#[test]
fn bias_v1_weight_lines_are_stably_sorted() {
    let mut weights = BTreeMap::new();
    weights.insert("RunTests".to_string(), 0.30);
    weights.insert("AnalyzeTask".to_string(), 0.90);
    weights.insert("ExecuteChanges".to_string(), 0.40);

    let bias = SemanticBias {
        version: BIAS_VERSION_V1,
        seed: 7,
        preferred: vec![StepKind::AnalyzeTask],
        weights,
    };

    let weight_lines: Vec<String> = bias
        .explain_lines()
        .into_iter()
        .filter(|line| line.starts_with("bias.weight."))
        .collect();

    assert_eq!(
        weight_lines,
        vec![
            "bias.weight.AnalyzeTask=0.900000".to_string(),
            "bias.weight.ExecuteChanges=0.400000".to_string(),
            "bias.weight.RunTests=0.300000".to_string(),
        ]
    );
}
