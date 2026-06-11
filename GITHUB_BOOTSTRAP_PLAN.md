# GitHub Bootstrap Plan

## Current branch
Current branch: `bias-v1`.[cite:82]

## Tracked modified files
The repository currently has modified tracked files in two main areas.[cite:82]

### Kernel-adjacent / CLI surface
- `src/cli_json.rs`[cite:82]
- `src/main.rs`[cite:82]
- `src/model_manifest.rs`[cite:82]

### Generated verification artifacts
- `artifacts/verification_plan.deep.invalid_env.json`[cite:82]
- `artifacts/verification_plan.deep.json`[cite:82]
- `artifacts/verification_plan.fast.invalid_env.json`[cite:82]
- `artifacts/verification_plan.fast.json`[cite:82]
- `artifacts/verification_plan.identity_contract.json`[cite:82]
- `artifacts/verification_plan.invalid_env.json`[cite:82]
- `artifacts/verification_plan.json`[cite:82]
- `artifacts/verification_verdict.deep.warmup.json`[cite:82]
- `artifacts/verification_verdict.deep_same_fingerprint_allows_reuse.json`[cite:82]
- `artifacts/verification_verdict.fast.warmup.json`[cite:82]
- `artifacts/verification_verdict.fast_same_fingerprint_allows_reuse.json`[cite:82]
- `artifacts/verification_verdict.identity_contract.json`[cite:82]
- `artifacts/verification_verdict.json`[cite:82]

## Untracked files
The repository also has untracked files across documentation, logs, and a new policy-layer directory.[cite:82]

### Policy layer
- `src/lm_policy_layer/`[cite:82]

### Documentation / analysis / specs
- `CLI_STDOUT_ISOLATION_FIX_REPORT.md`[cite:82]
- `LM_POLICY_CLIENT_RS.rs`[cite:82]
- `LM_POLICY_LAYER_IMPLEMENTATION.md`[cite:82]
- `POLICY_APPLIER_ENGINE.rs`[cite:82]
- `POLICY_SURFACE_MAP.md`[cite:82]
- `POLICY_VERSIONING_SPEC.md`[cite:82]
- `PROJECT_FULL_STATE_ANALYSIS.md`[cite:82]
- `STDOUT_SOURCES_MAP.md`[cite:82]
- `TEST_RUNNERS.md`[cite:82]
- `TEST_STRATIFICATION.md`[cite:82]

### Local logs / ephemeral outputs
- `capture_capsule_debug.log`[cite:82]
- `cargo_test_policy.log`[cite:82]
- `cli_json_contract.log`[cite:82]
- `full_cargo_test.log`[cite:82]

## Classification of current changes

### Kernel
The currently modified tracked source files `src/cli_json.rs`, `src/main.rs`, and `src/model_manifest.rs` should be treated as kernel-adjacent or CLI-surface changes because they live in `src/` and are part of the executable/runtime surface.[cite:82]

### Policy layer
The untracked directory `src/lm_policy_layer/` is the clearest policy-layer change currently present in the working tree.[cite:82]

### Documentation
The markdown reports and analysis files are documentation artifacts and should not be mixed into the first core code commit without an explicit publication decision.[cite:82]

### Test infrastructure
`TEST_RUNNERS.md` and `TEST_STRATIFICATION.md` belong to test infrastructure/documentation, while the `artifacts/verification_*` files are generated outputs from verification/test activity rather than durable source changes.[cite:82]

## Safe first-fixation strategy
The safest first GitHub fixation is to publish only durable source and repo-structure changes first, while leaving generated logs and unstable artifacts local.[cite:82]

Recommended principle:
1. Publish source-bearing changes first.
2. Keep generated logs local.
3. Keep verification artifacts out of the first push unless the repository intentionally versions them.
4. Publish documentation only if it reflects stable repository state rather than transient debugging context.

## Proposed branch structure
- `main` or protected default branch: stable published baseline.
- `bootstrap/policy-layer` or `bootstrap/repo-normalization`: first publication branch for durable source additions.
- `docs/analysis-bootstrap`: optional separate branch for reports and analysis if those documents are intended for version control.
- `local/debug-only`: not pushed; used only for logs, transient artifacts, and experimental reports.

## First commit plan

### Commit 1 — durable source bootstrap
Include only durable source changes that define repository state:
- `src/lm_policy_layer/`[cite:82]
- any intentional source edits in `src/cli_json.rs`, `src/main.rs`, `src/model_manifest.rs` after manual review against desired baseline[cite:82]

### Commit 2 — test organization docs
Include only stable test-ops documentation:
- `TEST_STRATIFICATION.md`[cite:82]
- `TEST_RUNNERS.md`[cite:82]

### Commit 3 — optional architecture/docs publication
Include only durable architecture/spec docs if they are intended to live in the repo long-term:
- `LM_POLICY_LAYER_IMPLEMENTATION.md`[cite:82]
- `POLICY_SURFACE_MAP.md`[cite:82]
- `POLICY_VERSIONING_SPEC.md`[cite:82]
- `CLI_STDOUT_ISOLATION_FIX_REPORT.md`[cite:82]
- `STDOUT_SOURCES_MAP.md`[cite:82]
- `PROJECT_FULL_STATE_ANALYSIS.md`[cite:82]

## What should go into the first push
The first push should contain only durable repository state:
- reviewed source changes in `src/`[cite:82]
- the new `src/lm_policy_layer/` directory if it is intended as part of the product baseline[cite:82]
- optionally the stable test runner/stratification docs if the team wants operational documentation in-repo[cite:82]

## What should remain local
The following should remain local for the first publication wave:
- `capture_capsule_debug.log`[cite:82]
- `cargo_test_policy.log`[cite:82]
- `cli_json_contract.log`[cite:82]
- `full_cargo_test.log`[cite:82]
- generated `artifacts/verification_*` files unless the repository explicitly tracks them as canonical outputs[cite:82]
- speculative or transient `.md`/`.rs` analysis artifacts unless they are intentionally part of the repository narrative[cite:82]

## GitHub publication readiness
No Git remote is currently configured in this working copy, so GitHub push or pull request readiness is not confirmed from the repository tools at this time.[cite:82]

Because the remote list is empty, write access to GitHub is not verified here, and a publication plan can only be prepared conceptually rather than executed.[cite:82]
