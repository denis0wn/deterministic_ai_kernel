//! Phase 4 §5 — Explain Drift Detection
//!
//! Locks the structure and keys of SemanticBias::explain_lines().
//! If a key is renamed, added, or removed the output format drifts —
//! breaking any downstream parser that depends on explain output.

use deterministic_ai_kernel::workflow::contract::StepKind;
use deterministic_ai_kernel::workflow::semantic::bias::SemanticBias;

fn neutral_bias() -> SemanticBias {
    let domain = &[
        StepKind::AnalyzeTask,
        StepKind::PlanExecution,
        StepKind::ExecuteChanges,
    ];
    SemanticBias::neutral_for(domain)
}

#[test]
fn explain_lines_contains_version_key() {
    let lines = neutral_bias().explain_lines();
    assert!(
        lines.iter().any(|l| l.starts_with("bias.version=")),
        "explain_lines must contain 'bias.version='"
    );
}

#[test]
fn explain_lines_contains_seed_key() {
    let lines = neutral_bias().explain_lines();
    assert!(
        lines.iter().any(|l| l.starts_with("bias.seed=")),
        "explain_lines must contain 'bias.seed='"
    );
}

#[test]
fn explain_lines_contains_preferred_key() {
    let lines = neutral_bias().explain_lines();
    assert!(
        lines.iter().any(|l| l.starts_with("bias.preferred=")),
        "explain_lines must contain 'bias.preferred='"
    );
}

#[test]
fn explain_lines_contains_meta_keys() {
    let lines = neutral_bias().explain_lines();
    let required = [
        "bias.meta.version=",
        "bias.meta.seed=",
        "bias.meta.preferred_count=",
        "bias.meta.weighted_count=",
    ];
    for key in &required {
        assert!(
            lines.iter().any(|l| l.starts_with(key)),
            "explain_lines must contain '{}'",
            key
        );
    }
}

#[test]
fn explain_lines_contains_weight_keys_for_domain() {
    let lines = neutral_bias().explain_lines();
    let weight_lines: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("bias.weight."))
        .collect();
    assert_eq!(
        weight_lines.len(),
        3,
        "neutral_for 3-step domain must produce 3 weight lines"
    );
}

#[test]
fn explain_lines_minimum_count_is_locked() {
    let lines = neutral_bias().explain_lines();
    // 7 fixed keys + 3 weight keys = 10 minimum
    assert!(
        lines.len() >= 10,
        "explain_lines must produce at least 10 lines for a 3-step domain, got {}",
        lines.len()
    );
}

#[test]
fn explain_lines_version_matches_bias_version() {
    let bias = neutral_bias();
    let lines = bias.explain_lines();
    let version_line = lines
        .iter()
        .find(|l| l.starts_with("bias.version="))
        .unwrap();
    assert_eq!(
        version_line.as_str(),
        "bias.version=v1",
        "bias.version in explain output must match BiasVersion::V1 display"
    );
}
