# Product Definition

## Product description
Deterministic AI execution engine with replayable results.

## Problem statement
- AI execution results are not reproducible.
- Deterministic replay of AI workflows is typically missing.

## Target users
- AI developers

## Core value
- deterministic execution of AI tasks
- replay of executed workflows
- structured JSON output contract

## One-line demo claim
“Run AI tasks that can be replayed with identical results every time”

## Complete demo flow

### CLI command
```bash
./target/debug/deterministic-ai-kernel replay-capsule task-replay-json --json
```

### JSON output example
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

### Replay command
```bash
./target/debug/deterministic-ai-kernel replay-capsule task-replay-json --json
```

### Deterministic equivalence condition
The demo is successful if repeated replay of the same saved task produces the same structured JSON result for `ok`, `schema_version`, `command`, `report.task_id`, `report.capsule_id`, `report.valid`, `report.events`, `report.nodes`, and `report.edges`.[cite:89][cite:90]
