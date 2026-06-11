# Demo Flow Specification

## Selected demo path

Flow: `integrity-json → replay-capsule --json → structural equivalence check`

This is the only path that satisfies all selection criteria:
- CLI command already implemented and tested
- JSON output fully structured and envelope-validated by existing contract tests
- Replay mechanism already implemented and passing
- No new code required[cite:89][cite:90]

## Step 1 — Execute

### Command
```
./target/debug/deterministic-ai-kernel integrity-json
```

### Expected JSON output structure
```json
{
  "ok": true,
  "schema_version": "cli-json-v1",
  "command": "integrity-json",
  "task_id": "integrity-task",
  "report": {
    "task_id": "integrity-task",
    "...": "..."
  }
}
```

Contraints verified by tests:
- `schema_version` must equal `"cli-json-v1"`
- `ok` must equal `true`
- `command` must equal `"integrity-json"`
- `report.task_id` must equal `"integrity-task"`
- single JSON object, parseable by `serde_json::from_str`[cite:89][cite:90]

## Step 2 — Capture capsule and replay

### Capture command
```
./target/debug/deterministic-ai-kernel capture-capsule <task_id> --save
```

### Replay command
```
./target/debug/deterministic-ai-kernel replay-capsule <task_id> --json
```

### Expected replay output structure
```json
{
  "ok": true,
  "schema_version": "cli-json-v1",
  "command": "replay-capsule",
  "report": {
    "task_id": "<task_id>",
    "capsule_id": "<capsule_id>",
    "valid": true,
    "events": <n>,
    "nodes": <n>,
    "edges": <n>
  }
}
```

## Step 3 — Deterministic equivalence condition

Two executions of `replay-capsule <task_id> --json` against the same persisted capsule are deterministic if and only if:
- `ok` is `true` in both runs
- `report.capsule_id` is identical
- `report.valid` is `true` in both runs
- `report.events`, `report.nodes`, `report.edges` are identical

Structural identity of the JSON is sufficient for demo correctness. Byte-for-byte equivalence of the formatted output also holds because serialization order is stable in the current envelope.

## Step 4 — Existing tests that validate this flow

| Test | File | Status |
|------|------------------------------------------------------------|--------|
| `integrity_json_emits_valid_cli_json_contract` | `tests/cli_json_contract.rs` | passing[cite:89] |
| `integrity_json_cli_emits_valid_json_report` | `tests/integrity_json_cli.rs` | passing[cite:89] |
| `replay_capsule_json_reports_saved_capsule` | `tests/replay_capsule_cli_json.rs` | passing[cite:89] |
| `replay_long_chain_matches_after_snapshot_restore` | `tests/replay_long_chain_equivalence.rs` | passing[cite:89] |
| `seed_matrix_replay_equivalence_matches_after_snapshot_restore` | `tests/replay_seed_matrix_equivalence.rs` | passing[cite:89] |
| `seed_matrix_long_chain_replay_equivalence_matches_after_snapshot_restore` | `tests/replay_seed_matrix_equivalence.rs` | passing[cite:89] |

All 7 tests pass without code modification.[cite:89]

## Current stabilization status

All selected demo tests pass without any changes. There are no stdout contamination issues, JSON envelope issues, or replay mismatches in the selected path.

No code changes are required to reach demo readiness for this exact flow.

## Demo narrative

```
1. Run:    deterministic-ai-kernel integrity-json
   Get:    single JSON with ok:true, schema_version, task_id

2. Run:    deterministic-ai-kernel replay-capsule <task_id> --json
   Get:    single JSON with capsule_id, valid:true, events, nodes, edges

3. Run:    deterministic-ai-kernel replay-capsule <task_id> --json   (again)
   Get:    structurally identical JSON
   Proves: deterministic replay
```

This demonstrates: "AI execution engine that deterministically replays the same result."
