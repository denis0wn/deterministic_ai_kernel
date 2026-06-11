# Policy Versioning Spec

## Storage
Applied LM policy updates are stored under `policy_versions/` as immutable JSON version records, plus an append-only `apply.log` file for chronological auditing.[cite:29][cite:30]

## Version record fields
Each version record stores:
- `version_id`
- `created_at_unix`
- `confidence`
- `snapshot`
- `diff`
- `rollback_pointer`

## Rollback model
Rollback is pointer-based: each new version stores the previous version id as `rollback_pointer`. This allows later tooling to restore an earlier policy snapshot without mutating execution-core code paths.[cite:30]

## Safety constraints
Only updates for `model_selection`, `ram_gating`, `planner`, and `env_sync` are valid. Any request targeting execution core or other unsupported surfaces must be rejected before apply.[cite:30]
