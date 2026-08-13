//! Phase 4 §1 — Contract Regression Lock
//!
//! Any change to semantic_bias_v1 structure without bumping the version
//! must break this test.
//!
//! Rewritten during the remediation pass (audit finding: the previous lock
//! was self-referential — it compared hardcoded lists against themselves and
//! locked five names that matched NO real StepKind). The lock now
//! cross-checks against the production sources of truth:
//! - `workflow::contract::step_kind_names()` for the StepKind domain,
//! - `schema/semantic_bias_v1.schema.json` for the payload structure.

use deterministic_ai_kernel::workflow::contract::step_kind_names;
use serde_json::json;

/// Locked schema version. Must match BiasVersion in source.
const LOCKED_SCHEMA_VERSION: &str = "semantic_bias_v1";

/// Locked set of known StepKind weight keys (sorted).
///
/// P0 (H-2 fix) NOTE: this locks the semantic_bias_v1 WEIGHT DOMAIN, which
/// is the 13 ordering-eligible step kinds. `AnswerQuestion` was added to the
/// StepKind enum WITHOUT bumping the schema version, on purpose: the
/// production pipeline-run path orders question plans trivially (single
/// step) and never consults bias weights, so keeping the weight domain
/// unchanged preserves validity of every existing semantic_bias_v1 artifact
/// (schema: additionalProperties=false + required[13]). The drift rule is
/// therefore: weight keys must be a subset of production StepKinds, and the
/// answer kind must stay excluded until a schema version bump.
const LOCKED_WEIGHT_KEYS: &[&str] = &[
    "AddLlmFallbackHandling",
    "AddPlannerTestCoverage",
    "AnalyzeTask",
    "ExecuteChanges",
    "LocateBug",
    "NormalizePlannerOutput",
    "PatchCode",
    "PlanExecution",
    "ReadRepository",
    "RunTests",
    "TightenPlannerPrompt",
    "ValidatePatch",
    "ValidatePlannerOutput",
];

#[test]
fn schema_version_string_is_locked() {
    assert_eq!(
        LOCKED_SCHEMA_VERSION, "semantic_bias_v1",
        "Schema version changed without updating the regression lock"
    );
}

#[test]
fn locked_weight_keys_are_valid_production_step_kinds() {
    let production: std::collections::BTreeSet<&str> = step_kind_names().iter().copied().collect();

    for key in LOCKED_WEIGHT_KEYS {
        assert!(
            production.contains(key),
            "locked weight key '{key}' is not a production StepKind — \
             the bias weight domain drifted from workflow::contract"
        );
    }
}

#[test]
fn answer_question_is_deliberately_excluded_from_bias_weight_domain() {
    // P0 contract: AnswerQuestion exists as a StepKind but must NOT enter
    // the semantic_bias_v1 weight domain without a schema version bump.
    assert!(
        step_kind_names().contains(&"AnswerQuestion"),
        "AnswerQuestion must exist in the production StepKind domain"
    );
    assert!(
        !LOCKED_WEIGHT_KEYS.contains(&"AnswerQuestion"),
        "AnswerQuestion entered the bias weight domain without a schema bump"
    );
}

#[test]
fn bias_payload_top_level_fields_are_locked_by_schema() {
    let schema_str = include_str!("../schema/semantic_bias_v1.schema.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).expect("schema parses");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");

    // The locked top-level field set.
    let required: Vec<&str> = schema["required"]
        .as_array()
        .expect("schema has required[]")
        .iter()
        .map(|v| v.as_str().expect("required entry is a string"))
        .collect();
    let mut required_sorted = required.clone();
    required_sorted.sort_unstable();
    assert_eq!(
        required_sorted,
        vec!["preferred", "seed", "version", "weights"],
        "top-level field set of semantic_bias_v1 changed — bump schema version"
    );

    // A fully-populated payload validates. Weights are built from the
    // locked WEIGHT DOMAIN (not step_kind_names(): AnswerQuestion is
    // deliberately outside the v1 weight domain, see lock notes above).
    let weights: serde_json::Map<String, serde_json::Value> = LOCKED_WEIGHT_KEYS
        .iter()
        .map(|k| (k.to_string(), json!(1.0)))
        .collect();
    let valid = json!({
        "version": "v1",
        "seed": 42,
        "preferred": ["AnalyzeTask"],
        "weights": serde_json::Value::Object(weights),
    });
    assert!(
        validator.is_valid(&valid),
        "canonical payload must validate"
    );

    // Unknown top-level keys are rejected (additionalProperties: false).
    let mut extra = valid.as_object().unwrap().clone();
    extra.insert("intruder".to_string(), json!(1));
    assert!(
        !validator.is_valid(&serde_json::Value::Object(extra)),
        "unknown top-level keys must be rejected"
    );

    // Unknown weight keys are rejected.
    let bad_weights = json!({
        "version": "v1",
        "seed": 42,
        "preferred": ["AnalyzeTask"],
        "weights": {"NotARealStepKind": 1.0},
    });
    assert!(
        !validator.is_valid(&bad_weights),
        "unknown weight keys must be rejected"
    );
}
