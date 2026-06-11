# Test Runners

## Fast gate
Run these frequently during development:

```bash
cargo test --test capture_capsule_save_cli_json
cargo test --test cli_json_contract
cargo test --test integrity_json_cli
cargo test --test kernel_types_contract
cargo test --test plan_identity_contract
cargo test --test model_manifest_env
cargo test --test doctor_json_cli
cargo test --test lm_control_contract
cargo test --test lm_control_doctor_contract
cargo test --test lm_control_doctor_json_contract
cargo test --test lm_control_policy
cargo test --test semantic_artifact_contract
cargo test --test semantic_bias_observability_contract
cargo test --test verification_graph_reuse_policy
```

## Medium gate
Run these for subsystem validation:

```bash
cargo test --test capture_capsule_cli
cargo test --test compare_capsules_cli
cargo test --test compare_capsules_cli_divergent
cargo test --test compare_capsules_cli_explain_divergent
cargo test --test compare_capsules_cli_explain_identical
cargo test --test compare_capsules_cli_explain_invalid
cargo test --test compare_capsules_cli_invalid
cargo test --test compare_capsules_cli_json_divergent
cargo test --test compare_capsules_cli_json_identical
cargo test --test compare_capsules_cli_json_invalid
cargo test --test latest_bias_artifact_cli
cargo test --test restore_snapshot_artifact_refs_cli
cargo test --test snapshot_artifacts_cli
cargo test --test semantic_artifacts
cargo test --test semantic_artifacts_list_cli
cargo test --test replay_capsule_cli
cargo test --test replay_capsule_cli_json
cargo test --test replay_capsule_cli_persistence
cargo test --test replay_capsule_cli_validation
cargo test --test lm_control_auto_route
cargo test --test lm_control_blocked
cargo test --test lm_control_doctor
cargo test --test lm_control_safe_switch
cargo test --test lm_control_switch
cargo test --test scheduler_integration
cargo test --test event_bus_kernel_bridge
cargo test --test semantic_bias_replay
cargo test --test semantic_bias_replay_equivalence
cargo test --test reuse_policy_matrix
cargo test --test replay_snapshot
cargo test --test replay_capsule_content
cargo test --test replay_capsule_diff
cargo test --test replay_capsule_structured_diff
cargo test --test replay_capsule_validation
cargo test --test replay_capsule_classification
```

## Slow gate
Run these separately as long-duration validation:

```bash
cargo test --test determinism
cargo test --test golden_replay_corpus
cargo test --test replay_long_chain_equivalence
cargo test --test replay_seed_matrix_equivalence
cargo test --test snapshot_replay_compatibility
cargo test --test snapshot_version_matrix
cargo test --test versioned_snapshot_compatibility
cargo test --test event_ordering_fuzz
cargo test --test bias_explain_cli
cargo test --test bias_explain_help_cli
cargo test --test bias_explain_invalid_cli
cargo test --test bias_artifact_cli
cargo test --test bias_v1_snapshot
cargo test --test emit_bias_artifact_invalid_step_kind
cargo test --test lm_control_live_lmstudio
cargo test --test lm_control_policy_negative
cargo test --test semantic_bias_seed_matrix
cargo test --test semantic_bias_v1_invariants
```
