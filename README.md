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

## Demo workflow

Example end-to-end walkthrough:

1. Analyze a task:
   - `cargo run -- analyze-task demo-task "Investigate scheduler retry semantics"`

2. Read the latest semantic seed:
   - `cargo run -- latest-analysis-seed demo-task analyze_task`

3. List semantic artifacts for the task:
   - `cargo run -- semantic-artifacts demo-task`

4. Rebuild snapshot state:
   - `cargo run -- snapshot demo-task`

5. Replay persisted state:
   - `cargo run -- replay demo-task`

This demonstrates the intended loop: analyze -> persist semantic artifact -> inspect artifact state -> snapshot -> replay.

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
