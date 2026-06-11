# deterministic_ai_kernel

Deterministic AI execution engine with replayable results.

Run AI tasks that can be replayed with identical results every time.

## Why it matters
- deterministic execution of AI tasks
- replay of executed workflows
- structured JSON output contract

## Quick start
Build the binary:

```bash
cargo build
```

Run the demo flow:

```bash
./target/debug/deterministic-ai-kernel replay-capsule task-replay-json --json
```

## Example output

```json
{
  "ok": true,
  "schema_version": "cli-json-v1",
  "command": "replay-capsule",
  "report": {
    "task_id": "task-replay-json",
    "capsule_id": "<capsule_id>",
    "valid": true,
    "events": 2,
    "nodes": 2,
    "edges": 1
  }
}
```

## Replay command

Replay the same task again:

```bash
./target/debug/deterministic-ai-kernel replay-capsule task-replay-json --json
```

## Determinism guarantee
For the same saved task/capsule input, replay returns the same structured JSON result for task identity, capsule identity, validity, and graph/event counts.[cite:89][cite:90]
