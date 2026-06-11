# Model Registry Extension Specification

## Purpose
This specification extends the current model registry boundary so that immutable base models and versioned LoRA adapters can coexist without altering deterministic execution behavior. The current repository already contains a purpose-based resolver in `src/model_registry.rs`; this specification extends around that surface rather than replacing execution logic.[cite:79][cite:80]

## Current boundary
The current registry surface resolves model purpose to `base_url`, `api_key`, and `model` through `ModelPurpose` and `ModelConfig`.[cite:80] This is sufficient for base model selection but does not yet encode adapter identity, version lineage, approval state, or rollback metadata.[cite:80]

## Extension principles
- Base model records are immutable.
- LoRA adapters are versioned append-only records.
- Routing metadata is separate from model and adapter registries.
- Rollback is metadata-driven and must not mutate historical records.
- Registry extension must remain outside execution-core semantics.

## Base model records
Recommended record structure:

```text
BaseModelRecord
- base_model_id
- model_purpose_set
- backend
- base_url
- api_key_ref
- model_name
- runtime_format
- context_window
- quantization
- compatibility_tags
- status: active | deprecated | retired
- created_at
- deprecated_at?
- notes?
```

### Notes
- `base_model_id` is the stable identity used by adapters and routing.
- `api_key_ref` should reference configuration, not embed secrets in registry files.
- `status` affects eligibility for new adapter training but does not delete historical compatibility.

## LoRA adapter records
Recommended record structure:

```text
LoraAdapterRecord
- adapter_id
- adapter_family
- adapter_version
- task_type
- base_model_id
- dataset_id
- training_run_id
- evaluation_report_id
- approval_record_id?
- artifact_uri
- artifact_hash
- created_at
- created_by
- deployment_status: candidate | approved | active | superseded | deprecated | rolled_back | rejected
- rollback_parent_adapter_id?
- supersedes_adapter_id?
- notes?
```

## Adapter metadata
Additional metadata should be tracked as a separate attached structure or inline subdocument:

```text
AdapterMetadata
- training_backend
- training_config_hash
- epochs
- rank
- alpha
- dropout
- target_modules
- source_capsule_window
- source_dataset_summary
- metrics_summary
- safety_gate_version
- schema_version
```

## Version relationships
Version relationships must be explicit and directional.

Required relationships:
- adapter -> base model
- adapter -> dataset
- adapter -> training run
- adapter -> evaluation report
- adapter -> approval record
- adapter -> superseded adapter (optional)
- adapter -> rollback parent (optional)

### Versioning rule
Adapter identity must be immutable. A promoted adapter is never edited in place; a new record is appended for every new candidate or corrected rebuild.

## Routing references
Routing must not be embedded in base or adapter records. Use a separate routing registry with entries like:

```text
RoutingRecord
- task_type
- environment
- selected_base_model_id
- selected_adapter_id?
- rollout_mode
- activated_at
- activated_by
- previous_routing_record_id?
```

## Rollback references
Rollback requires explicit lineage.

Required rollback references:
- `rollback_parent_adapter_id` on the failed adapter
- `previous_routing_record_id` on the routing change
- incident / evaluation / rejection report link if rollback followed degraded production evaluation

Rollback must restore prior routing metadata only. No historical registry record may be deleted or rewritten.

## Recommended persistence shape
Use three append-oriented registries:
- `base_models.json`
- `lora_adapters.json`
- `routing_registry.json`

Alternative: a directory-per-record layout with immutable manifests. Either option must preserve stable ids and append-only history.

## Assumptions
- the current `ModelPurpose` resolver remains a compatibility surface for base-model selection.[cite:80]
- LM Studio or an adjacent compatible backend remains the runtime target for model serving.[cite:79]

## Risks
- overloading the current registry with rollout state would mix model lifecycle with deployment lifecycle
- storing secrets directly in registry manifests would create unsafe artifact handling
- omitting explicit rollback lineage would make reversions ambiguous

## Unresolved questions
- whether registry persistence should be JSON manifests or SQLite-backed metadata
- whether task type taxonomy should be identical to current purpose taxonomy or a richer superset
- whether environments (`local`, `staging`, `prod`) require separate routing registries or shared registry with environment key
