# Test Stratification Proposal

## Fast gate
- tests/capture_capsule_save_cli_json.rs
- tests/cli_json_contract.rs
- tests/integrity_json_cli.rs
- tests/kernel_types_contract.rs
- tests/plan_identity_contract.rs
- tests/model_manifest_env.rs
- tests/doctor_json_cli.rs
- tests/lm_control_contract.rs
- tests/lm_control_doctor_contract.rs
- tests/lm_control_doctor_json_contract.rs
- tests/lm_control_policy.rs
- tests/semantic_artifact_contract.rs
- tests/semantic_bias_observability_contract.rs
- tests/verification_graph_reuse_policy.rs

## Medium gate
- tests/capture_capsule_cli.rs
- tests/compare_capsules_cli.rs
- tests/compare_capsules_cli_divergent.rs
- tests/compare_capsules_cli_explain_divergent.rs
- tests/compare_capsules_cli_explain_identical.rs
- tests/compare_capsules_cli_explain_invalid.rs
- tests/compare_capsules_cli_invalid.rs
- tests/compare_capsules_cli_json_divergent.rs
- tests/compare_capsules_cli_json_identical.rs
- tests/compare_capsules_cli_json_invalid.rs
- tests/latest_bias_artifact_cli.rs
- tests/restore_snapshot_artifact_refs_cli.rs
- tests/snapshot_artifacts_cli.rs
- tests/semantic_artifacts.rs
- tests/semantic_artifacts_list_cli.rs
- tests/replay_capsule_cli.rs
- tests/replay_capsule_cli_json.rs
- tests/replay_capsule_cli_persistence.rs
- tests/replay_capsule_cli_validation.rs
- tests/lm_control_auto_route.rs
- tests/lm_control_blocked.rs
- tests/lm_control_doctor.rs
- tests/lm_control_safe_switch.rs
- tests/lm_control_switch.rs
- tests/scheduler_integration.rs
- tests/event_bus_kernel_bridge.rs
- tests/semantic_bias_replay.rs
- tests/semantic_bias_replay_equivalence.rs
- tests/reuse_policy_matrix.rs
- tests/replay_snapshot.rs
- tests/replay_capsule_content.rs
- tests/replay_capsule_diff.rs
- tests/replay_capsule_structured_diff.rs
- tests/replay_capsule_validation.rs
- tests/replay_capsule_classification.rs

## Slow gate
- tests/determinism.rs
- tests/golden_replay_corpus.rs
- tests/replay_long_chain_equivalence.rs
- tests/replay_seed_matrix_equivalence.rs
- tests/snapshot_replay_compatibility.rs
- tests/snapshot_version_matrix.rs
- tests/versioned_snapshot_compatibility.rs
- tests/event_ordering_fuzz.rs
- tests/seed_interpreter_props.rs.disabled
- tests/bias_explain_cli.rs
- tests/bias_explain_help_cli.rs
- tests/bias_explain_invalid_cli.rs
- tests/bias_artifact_cli.rs
- tests/bias_v1_snapshot.rs
- tests/emit_bias_artifact_invalid_step_kind.rs
- tests/lm_control_live_lmstudio.rs

## Rules
- Each test belongs to exactly one gate.
- If a test is ambiguous across multiple gates, it is assigned to the slow gate only.
- `tests/seed_interpreter_props.rs.disabled` is currently disabled and excluded from runnable command groups.
