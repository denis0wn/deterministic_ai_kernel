# kernel-tui — Operator TUI for the Deterministic Kernel

Status: **functional** observation + control interface (first production-oriented version).
Binary: `kernel_tui` (`src/bin/kernel_tui.rs` + `src/bin/kernel_tui/{adapter,model,view}.rs`).

This document describes the implemented architecture. It does not describe the
separate, untracked WIP `replay_os` console (`src/bin/replay_os.rs` +
`src/bin/tui/*`), which remains a pipeline-runner tool and is independent.

## Architecture

```
                 USER
                   |
                   v
          kernel-tui (this binary)
          +-----------------------------+
          | view.rs      (render only)  |
          | model.rs     (UI state only)|
          | adapter.rs   (kernel calls) |
          +-----------------------------+
                   |
                   v
            Kernel API layer
        storage_for(db) / scheduler facades
        (explicit DB routing, no globals)
                   |
                   v
             Existing Kernel
                   |
                   v
              Event Store (event_log)
                   |
                   v
                TaskFold
```

Hard rules enforced by this design:

- The TUI contains **no task/step/lease/effect state machine**. All canonical
  state is read from kernel queries:
  - task state: `StorageProvider::task_state` (derived from the canonical fold)
  - step states: `StorageProvider::get_current_status_map`
  - exec spec: `StorageProvider::load_exec_spec`
  - events: `StorageProvider::event_rows`
  - leases: `StorageProvider::list_leases`
  - effects: `StorageProvider::effect_ledger_rows`
  - artifacts: `StorageProvider::list_semantic_artifacts`
  - replay verdict + violations: `StorageProvider::replay_validate` /
    `replay_violations` (the same fold; the TUI implements no validator)
  - stats: `StorageProvider::print_stats`
- The TUI never derives terminal state, never re-implements replay, and never
  treats hidden UI as authorization.
- DB routing is explicit: `--db <path>`, else `KERNEL_DB_PATH`, else
  `./kernel.db`. Every query goes through `providers::storage_for(db)`.
  No thread-local overrides, no global mutable routing (audit finding M3).

## Modules

| Module | Responsibility |
| --- | --- |
| `adapter.rs` | The only component that talks to the kernel. Builds plain view-model structs (`Snapshot`, `TaskRowVm`, `TaskDetailVm`, …) and exposes the safe control ops (`submit_task`, `schedule_task`, `rebuild_snapshot`), all via canonical kernel APIs. |
| `model.rs` | Pure presentation state (screen, selection, filters, input, toasts) and the pure `update(model, msg) -> OutAction` transition function. No kernel state lives here. |
| `view.rs` | Thin ratatui rendering of the snapshot. Colors are display styling over canonical status strings only. |
| `kernel_tui.rs` | Entry point: arg parsing, terminal setup, input/render loop, and the background worker thread. |

## Concurrency / refresh

- Kernel queries run on a **background worker thread**. The input/render loop
  never blocks on the database.
- Communication is message-based (`std::sync::mpsc`):
  - UI → worker: `Refresh(params)` and control `Action`s.
  - worker → UI: `Snapshot` results and action outcomes.
- The worker auto-collects periodically (~2 s) and after every action, so the
  UI tracks live kernel activity (new events, lease changes, scheduler work)
  without corrupting UI state. Snapshots replace the previous data atomically;
  selections are clamped.

## Control operations

Only safe operations are exposed, and they use the same enforcement boundary
as the CLI:

| Key | Screen | Operation | Kernel path |
| --- | --- | --- | --- |
| `n` | Tasks | submit new task (prompt) | `insert_task` + `scheduler::schedule` |
| `s` | Tasks | schedule selected task | `scheduler::schedule` |
| `b` | Task Detail | rebuild snapshot | `StorageProvider::rebuild_snapshot` |
| `enter`/`r` | Replay | validate selected task | `replay_validate`/`replay_violations` |

The TUI exposes **no shell execution** and no destructive operations
(no reset/vacuum/integrity-delete). Side-effecting tools, if ever used, must
go through `tools::registry::execute_tool(..., confirmed)` with the existing
confirmation/timeout/security policy; the TUI itself is not a security
boundary and does not rely on "the button is hidden" as authorization.

## Keyboard controls

```
q        quit            1..6     switch screens
j / ↓    next            k / ↑    previous
enter    open / validate esc      back
r        refresh         ?        help

Tasks screen:
  /      task-id filter (case-insensitive substring)
  f      cycle canonical-state filter (all→pending→running→completed→failed→all)
  c      cycle task-class filter (over classes present in kernel data)
  n      new task (prompt → submit_task via canonical kernel API)
  s      schedule selected task — asks y/n confirmation first

Events screen (filters apply to the loaded bounded window):
  /      task filter (query-level, canonical event_rows filter)
  f      event-type filter (case-insensitive substring)
  t      text query over task/step/payload (case-insensitive)
  selected event → detail pane (payload pretty-printed, or raw with
                   an explicit [unparsed payload] marker — never a panic)

Replay / Integrity screen:
  enter/r  validate selected task (kernel fold)
  i        run the canonical integrity self-check (non-destructive)

Task Detail:
  b      rebuild snapshot — asks y/n confirmation first

Confirmation: side-effecting actions (s, b) stage a `confirm: … (y/n)`
prompt; `y` dispatches the canonical kernel action, any other key cancels.
Authorization itself is enforced below the UI by the kernel — the prompt
only protects against accidental activation.
```

Filtering policy: task-id and event filters are case-insensitive substring
matches; state and class filters are exact matches against canonical kernel
strings. Filtering is a display projection over kernel-derived records — the
TUI never infers state from event text.

## Screens

1. **Dashboard** — canonical counters (tasks by state, active leases, workers,
   events/units/generations, replay validity) + recent events from the event log.
2. **Tasks** — table TASK ID / CLASS / STATE / CURRENT STEP / LEASE / GEN.
   Task STATE is the kernel `TaskState`; filtering by substring.
3. **Task Detail** — task header (canonical task state, exec spec), per-step
   canonical statuses, leases, effects, artifacts, recent events, replay verdict.
4. **Events** — chronological event_log rows (generation, causal unit, seq,
   type, task, step, worker projection) with optional task filter.
5. **Workers** — observable lease-derived status. Capability inference is
   displayed as informational only and is NOT authorization (kernel limitation L3).
6. **Replay / Integrity** — per-task `REPLAY OK` / `REPLAY INVALID` /
   `REPLAY CHECK FAILED` with the concrete violations returned by the kernel
   fold, plus an on-demand integrity self-check (key `i`) that runs through
   the canonical `api::integrity_json_report` against a disposable scratch DB
   (never the user's DB). A failed integrity check is shown as
   `INTEGRITY CHECK FAILED`, never conflated with a passing check.
7. **System** — db path, DB ACCESSIBLE/UNAVAILABLE read probe, schema tables
   present, event/unit/generation/task/lease counts, kernel version, refresh
   state.

## Running

```
cargo run --bin kernel_tui -- --db path/to/kernel.db
# or
KERNEL_DB_PATH=path/to/kernel.db cargo run --bin kernel_tui
```

## Testing

Headless tests (no real terminal required) live in the binary's `#[cfg(test)]`
modules and cover: model transitions, keyboard navigation, selection clamping,
filtering, canonical task-state mapping against a real seeded DB, event
rendering from real event data, replay valid/invalid rendering, error
rendering, empty-snapshot rendering, refresh/data replacement, and DB routing
isolation. View rendering is asserted against ratatui's `TestBackend`.

```
cargo test --bin kernel_tui
```

## Verified behavior (real pseudo-TTY, 2026-08-12)

The TUI was driven through a real pty (crossterm raw mode + alternate screen)
against disposable databases:

- startup renders the Dashboard with canonical counters and the db path;
- all 7 screens render; navigation (1-6, j/k, enter, esc, r, /, ?) works;
- task detail renders the canonical ExecSpec steps supplied by the kernel
  (verified with a real CodeFix task: 00_read_repository … 04_validate_patch —
  not hard-coded);
- auto-refresh picks up live kernel activity without restart (new tasks
  visible ~1-2 s after `submit-task`/`schedule`; failed/completed state
  transitions, worker leases, and new events all appear while the TUI runs);
- error handling: an unwritable DB path starts the UI with a visible error
  banner, keeps navigation working, and quits cleanly (exit 0, no panic);
- empty/fresh DB renders all screens without panic;
- resize (60x20 shrink, 160x40 grow, SIGWINCH) does not panic;
- replay screen renders the kernel's own verdict, including `REPLAY INVALID`
  with the concrete fold violations for a malformed event stream;
- clean quit restores the terminal and exits 0 in every scenario.

## Operator-oriented behaviors (hardening pass, 2026-08-12)

- Refresh retention: a failed refresh never wipes the view — the previous
  successful snapshot stays displayable under a yellow STALE VIEW banner; a
  successful refresh clears it. refresh_seq (a monotonic presentation
  counter, no wall clock) is shown on Dashboard and System screens.
- Replay screen distinguishes three outcomes: REPLAY OK, REPLAY INVALID
  (kernel fold violations listed), and REPLAY CHECK FAILED (the check itself
  could not run, e.g. DB access failure — never conflated with INVALID).
  Event count and generation range of the validated task are shown.
- System screen reports database ACCESSIBLE / UNAVAILABLE (real read probe)
  instead of silently showing zeros.
- Task Detail shows the current step (display projection of canonical
  statuses: first non-terminal step in spec order).
- Terminals smaller than 40x10 render a controlled "terminal too small"
  warning instead of a corrupted layout (verified against a real
  controlling TTY at 6x30).
- DB selection precedence: --db flag > KERNEL_DB_PATH env > ./kernel.db
  (resolve_db in kernel_tui.rs; explicit per-adapter routing, no global
  mutable state).

## Operator console behaviors (integration phase, 2026-08-12)

- Integrity self-check: the Replay/Integrity screen exposes key `i`, which
  runs the canonical `api::integrity_json_report` on the background worker
  (non-blocking). It exercises the snapshot/restore machinery against a
  disposable scratch DB and NEVER touches the user's database. Results render
  as `INTEGRITY OK` or `INTEGRITY CHECK FAILED` — never conflated, never
  faked. The result persists across subsequent non-integrity refreshes.
- Confirmation UX: side-effecting actions schedule (`s`) and rebuild snapshot
  (`b`) no longer fire on a single keypress. They stage a
  `confirm: <action> ? (y/n)` prompt; `y` dispatches the canonical kernel
  action, any other key cancels. Submit-task is exempt because typing a task
  id and pressing Enter is itself deliberate. Authorization remains enforced
  below the UI by the kernel; the prompt only guards against accidental
  activation.

Verified on a proper controlling TTY (setsid + TIOCSCTTY, continuous drain):
`i` produced a definitive `INTEGRITY OK`; `s` showed the confirmation banner,
`n` cancelled it, and `y` dispatched the schedule action; clean exit code 0.

## Known limitations

- No interactive long-running soak test has been performed; verification used
  scripted pty sessions.
- The effect ledger has no kernel-wide global count API; the System screen
  labels it per-task instead of fabricating a global number.
- Worker capability is displayed as informational only (kernel limitation L3);
  it is not authorization.
- Status: functional; production-readiness additionally depends on operator
  soak testing in a real terminal environment.
