# Adapter Storage Layout

## Purpose
This document defines the filesystem and artifact storage layout for datasets, LoRA adapters, evaluations, and rollout metadata.

## Storage principles
- immutable artifacts are append-only
- base model weights are never mutated
- adapters are stored separately from base models
- datasets, evaluations, and approvals are auditable by version
- storage layout must support rollback and retention without rewriting history

## Recommended top-level structure

```text
learning/
  datasets/
  training_runs/
  adapters/
  evaluations/
  approvals/
  routing/
  archive/
```

## Dataset storage structure

```text
learning/datasets/
  <dataset_id>/
    manifest.json
    train.jsonl
    validation.jsonl
    holdout.jsonl
    quality_report.json
    provenance.json
```

### Dataset naming convention
Recommended pattern:
- `dataset_<task_type>_<yyyy_mm_dd>_<rev>`

Example:
- `dataset_planner_2026_06_11_r1`

## Training run storage structure

```text
learning/training_runs/
  <training_run_id>/
    run_manifest.json
    config.json
    stdout.log
    stderr.log
    metrics.json
    linked_dataset.txt
    linked_adapter.txt
```

### Training run naming convention
Recommended pattern:
- `train_<task_type>_<base_model_id>_<yyyy_mm_dd>_<rev>`

## Adapter artifact storage structure

```text
learning/adapters/
  <adapter_family>/
    <adapter_version>/
      adapter_manifest.json
      adapter.safetensors
      adapter_config.json
      metrics_summary.json
      evaluation_link.json
```

### Adapter naming convention
Recommended pattern:
- family: `planner`, `classifier`, `summarizer`, `policy_suggester`
- version: `v001`, `v002`, ... or timestamped monotonic revision
- full identity: `<family>__<base_model_id>__<version>`

Example:
- `planner__llama3_8b_instruct__v003`

## Evaluation artifact structure

```text
learning/evaluations/
  <evaluation_report_id>/
    evaluation_manifest.json
    benchmark_results.json
    regression_comparison.json
    safety_checks.json
    recommendation.json
```

## Approval artifact structure

```text
learning/approvals/
  <approval_record_id>.json
```

Approval record should contain approver identity, decision timestamp, linked evaluation id, and rollout authorization scope.

## Routing storage structure

```text
learning/routing/
  routing_registry.json
  routing_history/
    <routing_change_id>.json
```

## Retention policy

### Keep indefinitely
- base model manifests
- adapter manifests
- adapter artifacts
- evaluation reports
- approval records
- routing history
- dataset manifests

### May be archived
- raw training stdout/stderr logs after summary extraction
- intermediate caches
- temporary transformed datasets once canonical dataset version is preserved

### Must never be deleted during normal lifecycle
- active adapter artifact
- superseded adapter artifact with rollback eligibility
- evaluation reports linked to active or historical deployment decisions

## Assumptions
- filesystem-backed local storage is acceptable for initial implementation
- object-store migration can be added later without changing logical identities

## Risks
- co-locating mutable logs with immutable manifests may blur lifecycle boundaries
- inconsistent naming can make rollback and audit difficult
- missing artifact hashes may weaken integrity checks

## Unresolved questions
- whether to shard large datasets into multiple files
- whether adapter binaries should be mirrored to external object storage
- whether routing registry should remain file-backed or later move to structured metadata store
