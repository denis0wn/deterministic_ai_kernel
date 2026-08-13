//! Phase 2, Task 1 — JSON Contract Verification
//!
//! Verifies that schema files are valid JSON Schemas and that existing
//! contract-level tests still pass.

use serde_json::{json, Value};

// ── 1. Schema Validity ──────────────────────────────────────────────────────

#[test]
fn execution_contract_v1_is_valid_json_schema() {
    let schema_str = include_str!("../schema/execution_contract_v1.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    // Must have $schema field
    assert!(
        schema.get("$schema").is_some(),
        "execution_contract_v1.json must have $schema field"
    );
    // Must have $id field
    assert!(
        schema.get("$id").is_some(),
        "execution_contract_v1.json must have $id field"
    );
    // Must have type field
    assert!(
        schema.get("type").is_some(),
        "execution_contract_v1.json must have type field"
    );
    // Must have properties
    assert!(
        schema.get("properties").is_some(),
        "execution_contract_v1.json must have properties field"
    );
}

#[test]
fn execution_event_v1_is_valid_json_schema() {
    let schema_str = include_str!("../schema/execution_event_v1.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    // Must have $schema field
    assert!(
        schema.get("$schema").is_some(),
        "execution_event_v1.json must have $schema field"
    );
    // Must have $id field
    assert!(
        schema.get("$id").is_some(),
        "execution_event_v1.json must have $id field"
    );
    // Must have type field
    assert!(
        schema.get("type").is_some(),
        "execution_event_v1.json must have type field"
    );
    // Must have properties
    assert!(
        schema.get("properties").is_some(),
        "execution_event_v1.json must have properties field"
    );
}

#[test]
fn semantic_bias_v1_is_valid_json_schema() {
    let schema_str = include_str!("../schema/semantic_bias_v1.schema.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    // Must have $schema field
    assert!(
        schema.get("$schema").is_some(),
        "semantic_bias_v1.schema.json must have $schema field"
    );
    // Must be parseable by jsonschema crate
    let compiled = jsonschema::validator_for(&schema).expect("schema must be compilable");

    // Real contract assertions (the previous `P || !P` tautology was a
    // hollow test — audit finding "tautological assertions"):
    let all_kinds = [
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
    ];

    // 1. A fully-populated payload validates.
    let weights: serde_json::Map<String, Value> = all_kinds
        .iter()
        .map(|k| (k.to_string(), json!(1.0)))
        .collect();
    let valid = json!({
        "version": "v1",
        "seed": 42,
        "preferred": ["AnalyzeTask"],
        "weights": Value::Object(weights),
    });
    assert!(
        compiled.is_valid(&valid),
        "fully-populated v1 payload must validate"
    );

    // 2. Partial weights are rejected (all 13 kinds are required).
    let partial = json!({
        "version": "v1",
        "seed": 42,
        "preferred": ["AnalyzeTask"],
        "weights": { "AnalyzeTask": 1.0 },
    });
    assert!(
        !compiled.is_valid(&partial),
        "partial weights must be rejected"
    );

    // 3. Unknown schema versions are hard-rejected.
    let bad_version = json!({
        "version": "v2",
        "seed": 42,
        "preferred": [],
        "weights": {},
    });
    assert!(
        !compiled.is_valid(&bad_version),
        "unknown version must be rejected"
    );

    // 4. Unknown top-level keys are rejected (additionalProperties: false).
    let mut extra = valid.as_object().unwrap().clone();
    extra.insert("intruder".to_string(), json!(1));
    assert!(
        !compiled.is_valid(&Value::Object(extra)),
        "unknown top-level keys must be rejected"
    );
}

// ── 2. Contract Backward Compatibility ──────────────────────────────────────

#[test]
fn execution_contract_preserves_required_fields() {
    let schema_str = include_str!("../schema/execution_contract_v1.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    // Verify task_envelope required fields (inside properties.task_envelope.required)
    let task_required = schema["properties"]["task_envelope"]["required"]
        .as_array()
        .expect("task_envelope must have required field");
    assert!(task_required.contains(&json!("task_id")));
    assert!(task_required.contains(&json!("run_id")));
    assert!(task_required.contains(&json!("task_class")));
    assert!(task_required.contains(&json!("input")));
    assert!(task_required.contains(&json!("requested_capabilities")));
    assert!(task_required.contains(&json!("contract_version")));

    // Verify event_envelope required fields (inside properties.event_envelope.required)
    let event_required = schema["properties"]["event_envelope"]["required"]
        .as_array()
        .expect("event_envelope must have required field");
    assert!(event_required.contains(&json!("event_id")));
    assert!(event_required.contains(&json!("run_id")));
    assert!(event_required.contains(&json!("task_id")));
    assert!(event_required.contains(&json!("event_type")));
    assert!(event_required.contains(&json!("ts")));
    assert!(event_required.contains(&json!("payload")));
    assert!(event_required.contains(&json!("contract_version")));

    // Verify outcomes
    let terminal = schema["properties"]["terminal_outcomes"]["const"]
        .as_array()
        .expect("terminal_outcomes must have const field");
    assert!(terminal.contains(&json!("Success")));
    assert!(terminal.contains(&json!("TerminalFailure")));

    let non_terminal = schema["properties"]["non_terminal_outcomes"]["const"]
        .as_array()
        .expect("non_terminal_outcomes must have const field");
    assert!(non_terminal.contains(&json!("RetryableFailure")));
    assert!(non_terminal.contains(&json!("Blocked")));
}

#[test]
fn execution_event_preserves_event_types() {
    let schema_str = include_str!("../schema/execution_event_v1.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    let event_types = schema["properties"]["event_envelope"]["properties"]["event_type"]["enum"]
        .as_array()
        .expect("event_type must have enum field");
    assert!(event_types.contains(&json!("task.created")));
    assert!(event_types.contains(&json!("task.started")));
    assert!(event_types.contains(&json!("task.progress")));
    assert!(event_types.contains(&json!("task.succeeded")));
    assert!(event_types.contains(&json!("task.failed")));
    assert!(event_types.contains(&json!("task.blocked")));
}

// ── 3. Contract Version Stability ───────────────────────────────────────────

#[test]
fn contract_version_is_locked_to_1() {
    let contract_str = include_str!("../schema/execution_contract_v1.json");
    let contract: serde_json::Value = serde_json::from_str(contract_str).unwrap();
    assert_eq!(
        contract["properties"]["contract_version"]["const"],
        json!(1)
    );

    let event_str = include_str!("../schema/execution_event_v1.json");
    let event: serde_json::Value = serde_json::from_str(event_str).unwrap();
    assert_eq!(event["properties"]["contract_version"]["const"], json!(1));
}
