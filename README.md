# deterministic_ai_kernel

A deterministic Rust kernel for task execution, replay, snapshotting, and local model control.

## Core concepts

- `event_bus`: persistence layer for events and semantic artifacts.
- `scheduler`: selection layer only.
- `worker`: ownership and lease execution.
- `execution`: execution-only runtime.
- `workflow`: canonical task classes and step contracts.

## Semantic artifacts

Supported artifact types are:

- `analysis_seed`
- `retrieval_result`
- `classification`

CLI commands:

- `semantic-artifacts <task_id> [step_id]`
- `latest-analysis-seed <task_id> [step_id]`

## Useful tests

- `cargo test --test replay_snapshot -- --nocapture`
- `cargo test --test scheduler_integration -- --nocapture`
- `cargo test --test semantic_artifacts -- --nocapture`
- `cargo test --test semantic_artifacts_cli -- --nocapture`
- `cargo test --test semantic_artifacts_list_cli -- --nocapture`
- `cargo test --test semantic_artifact_contract -- --nocapture`

## Current status

- Replay/snapshot on clean DB is stable.
- Default DB path is absolute.
- Semantic artifact contract is explicitly tested.
