# LoRA Lifecycle Specification

## Purpose
This specification defines the lifecycle of LoRA adapters as versioned, rollback-capable artifacts attached to immutable base models. Base models are never modified directly; only adapters evolve over time.[conversation_history:1][cite:80]

## Base model registry
Base models are immutable records with stable identifiers. The existing repository already resolves model purpose to model configuration through `src/model_registry.rs`; this lifecycle extends around that surface rather than replacing it.[cite:80]

Base model fields:
- `base_model_id`
- `model_name`
- `backend`
- `base_url`
- `format`
- `supported_purposes`
- `status`
- `created_at`
- `deprecated_at` (optional)

## LoRA registry
Each LoRA adapter is a separately versioned artifact.

Adapter fields:
- `adapter_id`
- `task_type`
- `base_model_id`
- `dataset_id`
- `training_config_hash`
- `evaluation_report_id`
- `approval_status`
- `deployment_status`
- `previous_adapter_id` (optional)
- `artifact_uri`
- `created_at`

## Versioning scheme
Recommended version pattern:
- adapter semantic name, e.g. `planner`
- monotonically increasing revision, e.g. `planner_lora_v3`
- timestamp or build id, e.g. `planner_lora_v3_2026_06_11`

Version identity should be immutable and should always bind:
- adapter artifact
- dataset version
- base model id
- evaluation report

## Promotion process
1. candidate adapter registered as `candidate`
2. offline evaluation completed
3. safety thresholds checked
4. human approval recorded
5. routing entry updated
6. adapter status becomes `active`
7. previous active adapter becomes `superseded`

Promotion must never overwrite previous adapter metadata.

## Deprecation process
An adapter may be deprecated when:
- superseded by a newer approved adapter
- tied to a deprecated base model
- shown to underperform or violate safety constraints

Deprecation changes only lifecycle metadata. The artifact and its lineage remain preserved for audit and rollback.

## Rollback process
Rollback is metadata-driven and routing-driven:
1. identify last known good adapter for the task type
2. freeze current candidate or active adapter deployment
3. update routing to point back to the last approved adapter
4. mark failed adapter as `rolled_back` or `rejected_post_deploy`
5. preserve evaluation and incident report links

## Assumptions
- base-model resolution stays compatible with the current purpose-based registry surface.[cite:80]
- LoRA adapters are served by an LM Studio-compatible or adjacent local backend boundary.[cite:79]

## Risks
- ambiguous adapter naming may break rollback clarity
- coupling promotion to training rather than evaluation may skip safety review
- allowing multiple mutable routing authorities may create drift

## Unresolved questions
- whether multiple active adapters per task type are allowed by environment
- whether routing supports canary rings or only single active mapping
- where artifact storage authority lives: filesystem, object store, or registry database

## Recommended implementation order
1. define registry record schemas
2. define adapter state machine
3. define approval metadata fields
4. define rollback metadata and audit links
5. implement registry persistence
