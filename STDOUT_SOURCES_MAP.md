# STDOUT Sources Map

## capture-capsule-save JSON path

| File | Function | Output type | Stream |
|---|---|---|---|
| `src/main.rs` | `capture-capsule-save` JSON branch | JSON envelope emission | stdout |
| `src/cli_json.rs` | `print_json_report()` | pretty-printed JSON | stdout |
| `src/main.rs` | other CLI branches | human-readable status lines | stdout/stderr depending on path |
| `src/lm_control.rs` | memory/model status commands | diagnostic/status lines | stdout |
| `src/snapshot.rs` | snapshot/restore commands | status lines and payloads | stdout |
| `src/scheduler.rs` | reconcile/schedule commands | status lines | stdout |
| `src/worker.rs` | worker lifecycle commands | status lines | stdout |
| `src/llm.rs` | smoke/test commands | status lines | stdout |

## Diagnosis
The failing JSON mode must isolate the `capture-capsule-save --json` path to exactly one stdout write. All other diagnostics and warnings must stay out of stdout or be disabled for JSON mode.[cite:53][cite:46]
