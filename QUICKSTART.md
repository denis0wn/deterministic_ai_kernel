# Quickstart

## Build

```bash
cargo build
```

## Run demo command

```bash
./target/debug/deterministic-ai-kernel replay-capsule task-replay-json --json
```

## Run replay command

```bash
./target/debug/deterministic-ai-kernel replay-capsule task-replay-json --json
```

## Expected output format

The command returns exactly one JSON object with this shape:

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
