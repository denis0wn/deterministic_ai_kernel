//! Fingerprint determinism contract.
//!
//! Guarantees that same inputs produce the same fingerprint and changed
//! inputs produce a different fingerprint, for all three fingerprint types.

use deterministic_ai_kernel::fingerprint::{PlanFingerprint, StepFingerprint, TaskFingerprint};
use serde_json::json;

// ── TaskFingerprint ────────────────────────────────────────────────────────────

#[test]
fn task_fingerprint_same_input_same_hash() {
    let a = TaskFingerprint {
        normalized_input: "analyse the repo",
        environment_fingerprint: "env-abc",
        manifest_version: "1.0.0",
        kernel_version: "0.1.0",
    }
    .compute();
    let b = TaskFingerprint {
        normalized_input: "analyse the repo",
        environment_fingerprint: "env-abc",
        manifest_version: "1.0.0",
        kernel_version: "0.1.0",
    }
    .compute();
    assert_eq!(a, b, "TaskFingerprint must be deterministic");
}

#[test]
fn task_fingerprint_changed_input_different_hash() {
    let a = TaskFingerprint {
        normalized_input: "analyse the repo",
        environment_fingerprint: "env-abc",
        manifest_version: "1.0.0",
        kernel_version: "0.1.0",
    }
    .compute();
    let b = TaskFingerprint {
        normalized_input: "fix the bug",
        environment_fingerprint: "env-abc",
        manifest_version: "1.0.0",
        kernel_version: "0.1.0",
    }
    .compute();
    assert_ne!(
        a, b,
        "different inputs must produce different TaskFingerprint"
    );
}

// ── PlanFingerprint ────────────────────────────────────────────────────────────

#[test]
fn plan_fingerprint_same_steps_same_hash() {
    let steps = vec![
        "read".to_string(),
        "compute".to_string(),
        "write".to_string(),
    ];
    let a = PlanFingerprint {
        task_fingerprint: "abc123",
        ordered_steps: &steps,
        planner_version: "1.0.0",
    }
    .compute();
    let b = PlanFingerprint {
        task_fingerprint: "abc123",
        ordered_steps: &steps,
        planner_version: "1.0.0",
    }
    .compute();
    assert_eq!(a, b);
}

#[test]
fn plan_fingerprint_reordered_steps_different_hash() {
    let steps_a = vec!["read".to_string(), "compute".to_string()];
    let steps_b = vec!["compute".to_string(), "read".to_string()];
    let a = PlanFingerprint {
        task_fingerprint: "abc123",
        ordered_steps: &steps_a,
        planner_version: "1.0.0",
    }
    .compute();
    let b = PlanFingerprint {
        task_fingerprint: "abc123",
        ordered_steps: &steps_b,
        planner_version: "1.0.0",
    }
    .compute();
    assert_ne!(a, b, "step order matters for PlanFingerprint");
}

// ── StepFingerprint ────────────────────────────────────────────────────────────

#[test]
fn step_fingerprint_same_input_same_hash() {
    let input = json!({ "operation": "echo hello" });
    let a = StepFingerprint {
        primitive_type: "Compute",
        primitive_version: "1",
        canonical_input: &input,
        dependency_hash: None,
        environment_fingerprint: "env-xyz",
    }
    .compute();
    let b = StepFingerprint {
        primitive_type: "Compute",
        primitive_version: "1",
        canonical_input: &input,
        dependency_hash: None,
        environment_fingerprint: "env-xyz",
    }
    .compute();
    assert_eq!(a, b);
}

#[test]
fn step_fingerprint_changed_env_different_hash() {
    let input = json!({ "operation": "echo hello" });
    let a = StepFingerprint {
        primitive_type: "Compute",
        primitive_version: "1",
        canonical_input: &input,
        dependency_hash: None,
        environment_fingerprint: "env-1",
    }
    .compute();
    let b = StepFingerprint {
        primitive_type: "Compute",
        primitive_version: "1",
        canonical_input: &input,
        dependency_hash: None,
        environment_fingerprint: "env-2",
    }
    .compute();
    assert_ne!(a, b, "env change must produce different StepFingerprint");
}

#[test]
fn step_fingerprint_changed_dep_hash_different_hash() {
    let input = json!({ "path": "src/" });
    let a = StepFingerprint {
        primitive_type: "Read",
        primitive_version: "1",
        canonical_input: &input,
        dependency_hash: Some("dep-hash-aaa"),
        environment_fingerprint: "env-1",
    }
    .compute();
    let b = StepFingerprint {
        primitive_type: "Read",
        primitive_version: "1",
        canonical_input: &input,
        dependency_hash: Some("dep-hash-bbb"),
        environment_fingerprint: "env-1",
    }
    .compute();
    assert_ne!(a, b, "dependency hash change must alter StepFingerprint");
}
