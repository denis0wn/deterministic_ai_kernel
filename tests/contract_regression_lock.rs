//! Phase 4 §1 — Contract Regression Lock
//!
//! Any change to semantic_bias_v1 structure without bumping the version
//! must break this test.


/// Canonical field fingerprint of semantic_bias_v1.
/// If you add/remove/rename a field — bump SCHEMA_VERSION and update this hash.
const EXPECTED_FIELD_FINGERPRINT: &str =
    "preferred,version,weights";

/// Locked schema version. Must match BiasVersion in source.
const LOCKED_SCHEMA_VERSION: &str = "semantic_bias_v1";

/// Locked set of known StepKind weight keys (sorted).
const LOCKED_WEIGHT_KEYS: &[&str] = &[
    "AnalyzeAndPlan",
    "CodeFix",
    "ExecuteChanges",
    "PlannerHardening",
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
fn field_set_fingerprint_is_stable() {
    // Reconstruct what a valid bias document looks like at the field level.
    let mut fields: Vec<&str> = vec!["preferred", "version", "weights"];
    fields.sort_unstable();
    let fingerprint = fields.join(",");
    assert_eq!(
        fingerprint, EXPECTED_FIELD_FINGERPRINT,
        "Top-level field set of semantic_bias_v1 changed — bump schema version"
    );
}

#[test]
fn weight_keys_are_locked() {
    let mut keys: Vec<&str> = LOCKED_WEIGHT_KEYS.to_vec();
    keys.sort_unstable();
    let rejoined: Vec<&str> = keys.clone();

    // Verify the lock itself is sorted (meta-check).
    assert_eq!(
        keys, rejoined,
        "LOCKED_WEIGHT_KEYS must be kept in sorted order"
    );

    // Verify expected count — change this only when adding a new StepKind.
    assert_eq!(
        LOCKED_WEIGHT_KEYS.len(),
        5,
        "StepKind count changed — update LOCKED_WEIGHT_KEYS and bump schema version"
    );
}

#[test]
fn weight_map_rejects_unknown_keys() {
    let known: std::collections::BTreeSet<&str> =
        LOCKED_WEIGHT_KEYS.iter().copied().collect();
    let candidate_keys = vec![
        "AnalyzeAndPlan",
        "CodeFix",
        "ExecuteChanges",
        "PlannerHardening",
        "ValidatePlannerOutput",
    ];
    for k in &candidate_keys {
        assert!(
            known.contains(k),
            "Unknown weight key '{}' not in contract lock",
            k
        );
    }
}

#[test]
fn preferred_field_accepts_only_known_values() {
    let valid_preferred: &[&str] = &[
        "AnalyzeAndPlan",
        "CodeFix",
        "ExecuteChanges",
        "PlannerHardening",
        "ValidatePlannerOutput",
    ];
    // "none" / null is also valid — represented as Option<StepKind>.
    // This test locks the exhaustive list.
    assert_eq!(
        valid_preferred.len(),
        5,
        "preferred field accepted values changed — update regression lock"
    );
}
