use deterministic_ai_kernel::exec_spec::*;
use deterministic_ai_kernel::execution_identity::*;
use std::collections::BTreeMap;

// ── Test: Execute same spec twice → identical evidence ─────────────────────

#[test]
fn same_spec_produces_identical_hash_twice() {
    let build_spec = || {
        ExecSpec::new(
            1,
            vec![
                StepSpec {
                    step_id: "00_analyze".to_string(),
                    required_capability: "Planner".to_string(),
                    detail: Some("analyze the task".to_string()),
                    primitive: None,
                    constraints: vec![Constraint {
                        target: "worker".to_string(),
                        key: "required_capability".to_string(),
                        value: "Planner".to_string(),
                    }],
                    artifact_requirements: vec![],
                    inputs: vec![],
                    outputs: vec![],
                    metadata: serde_json::Value::Null,
                },
                StepSpec {
                    step_id: "01_execute".to_string(),
                    required_capability: "Executor".to_string(),
                    detail: Some("execute the plan".to_string()),
                    primitive: None,
                    constraints: vec![Constraint {
                        target: "worker".to_string(),
                        key: "required_capability".to_string(),
                        value: "Executor".to_string(),
                    }],
                    artifact_requirements: vec![],
                    inputs: vec![],
                    outputs: vec![],
                    metadata: serde_json::Value::Null,
                },
            ],
            vec![],
            vec![Dependency {
                step_id: "01_execute".to_string(),
                depends_on: vec!["00_analyze".to_string()],
            }],
            vec![Policy {
                name: "timeout".to_string(),
                value: "120s".to_string(),
            }],
            BTreeMap::new(),
        )
    };

    let spec1 = build_spec();
    let spec2 = build_spec();

    assert_eq!(
        spec1.spec_id, spec2.spec_id,
        "identical specs must produce identical spec_id"
    );
    assert_eq!(
        spec1.calculate_hash(),
        spec2.calculate_hash(),
        "hash must be stable across invocations"
    );
}

// ── Test: Serialization stability ──────────────────────────────────────────

#[test]
fn exec_spec_serialization_is_deterministic() {
    let spec = ExecSpec::new(
        1,
        vec![StepSpec {
            step_id: "step_a".to_string(),
            required_capability: "Planner".to_string(),
            detail: None,
            primitive: None,
            constraints: vec![],
            artifact_requirements: vec![],
            inputs: vec![],
            outputs: vec![],
            metadata: serde_json::Value::Null,
        }],
        vec![],
        vec![],
        vec![],
        BTreeMap::new(),
    );

    let json1 = serde_json::to_string(&spec).expect("test failure");
    let json2 = serde_json::to_string(&spec).expect("test failure");
    assert_eq!(
        json1, json2,
        "JSON serialization must be byte-identical across invocations"
    );

    // Deserialize and re-serialize → must be identical
    let restored: ExecSpec = serde_json::from_str(&json1).expect("test failure");
    let json3 = serde_json::to_string(&restored).expect("test failure");
    assert_eq!(json1, json3, "roundtrip serialization must be stable");
}

#[test]
fn exec_spec_hash_is_stable_after_roundtrip() {
    let spec = ExecSpec::new(
        1,
        vec![StepSpec {
            step_id: "s1".to_string(),
            required_capability: "Executor".to_string(),
            detail: Some("detail".to_string()),
            primitive: None,
            constraints: vec![Constraint {
                target: "w".to_string(),
                key: "k".to_string(),
                value: "v".to_string(),
            }],
            artifact_requirements: vec![ArtifactRequirement {
                artifact_type: "report".to_string(),
                schema: Some("json".to_string()),
            }],
            inputs: vec![],
            outputs: vec![],
            metadata: serde_json::Value::Null,
        }],
        vec![TransitionRule {
            step_id: "s2".to_string(),
            depends_on: vec!["s1".to_string()],
        }],
        vec![Dependency {
            step_id: "s2".to_string(),
            depends_on: vec!["s1".to_string()],
        }],
        vec![Policy {
            name: "retry".to_string(),
            value: "3".to_string(),
        }],
        {
            let mut m = BTreeMap::new();
            m.insert("output".to_string(), "text/plain".to_string());
            m
        },
    );

    let hash_before = spec.calculate_hash();
    let json = serde_json::to_string(&spec).expect("test failure");
    let restored: ExecSpec = serde_json::from_str(&json).expect("test failure");
    let hash_after = restored.calculate_hash();

    assert_eq!(hash_before, hash_after, "hash must survive JSON roundtrip");
}

// ── Test: ExecutionId stability ────────────────────────────────────────────

#[test]
fn execution_id_stable_hash_is_deterministic() {
    let id1 = ExecutionId::new("task-12345");
    let id2 = ExecutionId::new("task-12345");

    assert_eq!(
        id1.stable_hash(),
        id2.stable_hash(),
        "same input must produce same hash"
    );

    let id3 = ExecutionId::new("task-99999");
    assert_ne!(
        id1.stable_hash(),
        id3.stable_hash(),
        "different input must produce different hash"
    );
}

#[test]
fn primitive_id_deterministic_across_invocations() {
    let p1 = PrimitiveId::new("prim-001");
    let p2 = PrimitiveId::new("prim-001");
    assert_eq!(p1, p2, "same primitive id must be equal");

    let p3 = PrimitiveId::new("prim-002");
    assert_ne!(p1, p3, "different primitive ids must be unequal");
}

// ── Test: Modified spec hash rejected ──────────────────────────────────────

#[test]
fn modified_spec_detected_by_hash() {
    let mut spec = ExecSpec::new(
        1,
        vec![StepSpec {
            step_id: "step_a".to_string(),
            required_capability: "Planner".to_string(),
            detail: None,
            primitive: None,
            constraints: vec![],
            artifact_requirements: vec![],
            inputs: vec![],
            outputs: vec![],
            metadata: serde_json::Value::Null,
        }],
        vec![],
        vec![],
        vec![],
        BTreeMap::new(),
    );

    let original_hash = spec.spec_id.clone();

    // Tamper with the spec
    spec.steps[0].required_capability = "Executor".to_string();

    let new_hash = spec.calculate_hash();
    assert_ne!(
        original_hash, new_hash,
        "tampering must change the calculated hash"
    );
    assert_ne!(
        spec.spec_id, new_hash,
        "spec_id must not match after tampering"
    );
}

#[test]
fn validate_hash_rejects_tampered_spec() {
    let mut spec = ExecSpec::new(
        1,
        vec![StepSpec {
            step_id: "s1".to_string(),
            required_capability: "Planner".to_string(),
            detail: None,
            primitive: None,
            constraints: vec![],
            artifact_requirements: vec![],
            inputs: vec![],
            outputs: vec![],
            metadata: serde_json::Value::Null,
        }],
        vec![],
        vec![],
        vec![],
        BTreeMap::new(),
    );

    // Valid spec should pass
    assert!(
        spec.validate_hash().is_ok(),
        "untampered spec must pass validation"
    );

    // Tamper
    spec.steps[0].detail = Some("injected".to_string());
    assert!(
        spec.validate_hash().is_err(),
        "tampered spec must fail validation"
    );
}

// ── Test: Spec validation catches structural errors ────────────────────────

#[test]
fn validate_catches_missing_dependency_target() {
    let spec = ExecSpec::new(
        1,
        vec![StepSpec {
            step_id: "step_a".to_string(),
            required_capability: "Planner".to_string(),
            detail: None,
            primitive: None,
            constraints: vec![],
            artifact_requirements: vec![],
            inputs: vec![],
            outputs: vec![],
            metadata: serde_json::Value::Null,
        }],
        vec![],
        vec![Dependency {
            step_id: "step_a".to_string(),
            depends_on: vec!["nonexistent_step".to_string()],
        }],
        vec![],
        BTreeMap::new(),
    );

    let errors = spec.validate();
    assert!(!errors.is_empty(), "must detect missing dependency target");

    let msgs: Vec<String> = errors.iter().map(|e| format!("{}", e)).collect();
    assert!(
        msgs.iter().any(|m| m.contains("nonexistent_step")),
        "error message must mention the missing step"
    );
}

// ── Test: BTreeMap ensures stable key ordering ─────────────────────────────

#[test]
fn btreemap_key_order_is_stable() {
    let mut m1 = BTreeMap::new();
    m1.insert("zebra".to_string(), "z".to_string());
    m1.insert("alpha".to_string(), "a".to_string());
    m1.insert("middle".to_string(), "m".to_string());

    let mut m2 = BTreeMap::new();
    m2.insert("middle".to_string(), "m".to_string());
    m2.insert("alpha".to_string(), "a".to_string());
    m2.insert("zebra".to_string(), "z".to_string());

    let json1 = serde_json::to_string(&m1).expect("test failure");
    let json2 = serde_json::to_string(&m2).expect("test failure");
    assert_eq!(
        json1, json2,
        "BTreeMap serialization must be order-independent of insertion order"
    );
}
