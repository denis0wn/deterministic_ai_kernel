# Deterministic AI Kernel — Specification v1

> This document defines **what the system is**. It is not a roadmap, not a TODO list, and not a design discussion.
> Source of truth: GOVERNANCE.md §27.

---

## 1. System Boundary

Deterministic AI Kernel (DAK) is a standalone deterministic execution runtime.
It accepts an `ExecSpec` and produces a verifiable event tape.
It does not generate plans, does not own workflows, and does not execute side effects directly.

### Inputs
- `ExecSpec` — immutable execution specification (JSON, content-hashed).
- Runtime seed (`u64`) — deterministic identity source.
- Provider capabilities — injected via trait objects at composition root.

### Outputs
- Append-only event tape in `EventStore`.
- `ReplayCapsule` — point-in-time state derivation from event tape.
- Structured JSON reports via `cli_json` facade.

---

## 2. Module Map & Ownership

| Module | Owner Of | Forbidden |
|--------|----------|-----------|
| `exec_spec` | Execution specification types, hashing, validation | Runtime imports, domain logic |
| `execution_abi` | Primitive types, trust context | Scheduler, worker, LLM |
| `kernel_types` | ReplayCapsule, StateGraph, ExecutionEvent, DecisionPoint | Business logic |
| `kernel_error` | Error taxonomy | Runtime/domain imports |
| `event_bus` | Event persistence facade over StorageProvider | Business logic, scheduling |
| `scheduler` | Next-step selection only | Execution, lease management |
| `worker` | Lease lifecycle (claim, heartbeat, complete, fail) | Scheduling decisions |
| `execution/` | Primitive execution, caching, solver | Scheduler, workflow, LLM routing |
| `workflow/` | Canonical task classes, step kinds, contracts, compiler | Runtime execution |
| `replay/` | State reconstruction from event stream | Side effects, planning |
| `snapshot` | Point-in-time state capture/restore | Business logic |
| `planner_pipeline/` | NL→Plan parsing, normalization, critic, replay verification | Direct DB access, lease ops |
| `fingerprint/` | Environment fingerprint computation | Temporal data, randomness |
| `lm_control/` | LM role routing policy, doctor diagnostics | Execution, scheduling |
| `semantic_bias/` | Deterministic step-ordering bias (seeded, versioned) | Non-deterministic sources |
| `providers/` | Trait definitions + default implementations | Business logic |
| `api` | Public facade over event_bus, replay, snapshot | Internal module coupling |
| `cli_json` | Structured JSON output schema | Business logic |
| `metrics/` | Observability counters | Control flow |
| `schema/` | JSON schema validation for semantic bias | Domain knowledge |
| `models/` | Artifact type registry | Execution logic |
| `registry/` | Model registry resolution | Execution, scheduling |
| `reconstruction/` | State derivation from events | Side effects |

---

## 3. Data Flow

```
User Request
     │
     ▼
Normalizer ──► Parser ──► Critic ──► ExecSpec
                                        │
                                        ▼
                              ExecutionEngine
                                   │
                          ┌────────┼────────┐
                          ▼        ▼        ▼
                     Executor   Cache   EventStore
                          │        │        │
                          ▼        ▼        ▼
                      Result    Hit/Miss  Event Tape
                                        │
                                        ▼
                                   Replayer
                                        │
                                        ▼
                               ReplayCapsule
```

---

## 4. Contracts & ABI Status

| Contract | Version | Status | Test Coverage |
|----------|---------|--------|---------------|
| Event ABI (event_log schema) | v1 | Frozen | `replay_verifies_stable_run`, `replayer_catches_tampered_id` |
| Execution ABI (primitives) | v1 | Frozen | `primitives_roundtrip_json`, `step_kind_to_primitive_kind_mapping` |
| Workflow ABI (task classes, step kinds) | v1 | Frozen | `contract_v1_metadata_is_stable`, `every_step_kind_is_present_in_exactly_one_canonical_flow` |
| Semantic Bias Schema | v1 | Frozen | `bias_configuration_roundtrip_json`, `semantic_bias_is_deterministic_across_seed_and_input_matrix` |
| CLI JSON Output | v1 | Draft | `capsule_summary_report_has_expected_shape`, `comparison_report_has_expected_shape_and_diff_payloads` |
| Replay Capsule | v1 | Accepted | `replayer_verifies_multiple_entries`, `replayer_handles_empty_tape` |
| Snapshot | v1 | Accepted | `snapshotVersionMatrixV1` (verification pipeline) |

---

## 5. Determinism Guarantees

### 5.1 Identity
- `plan_id = BLAKE3(seed || normalized_steps)` — stable across runs with same seed.
- `event_id = evt_BLAKE3(task_id:event_type:generation)` — content-derived, no UUID v4.
- `cache_key = BLAKE3(task_id || plan_id || primitive_id || input_hash || env_fingerprint)`.

### 5.2 Ordering
- All maps use `BTreeMap` (deterministic iteration order).
- Step ordering governed by `semantic_bias` (seeded, replay-safe).
- Event log ordered by `(causal_unit_id, sequence_in_unit, id)`.

### 5.3 Prohibited Sources
The following are forbidden in kernel core (`src/` excluding `providers/`, `bin/`, `main.rs`):
- `Uuid::new_v4()`, `Uuid::new_v1()`
- `SystemTime::now()`, `Instant::now()` (except metrics instrumentation)
- `rand::thread_rng()`, `OsRng`
- `std::fs::*` direct calls (must go through `FilesystemProvider`)
- `reqwest`, `hyper`, `TcpStream` (must go through `LlmProvider`)

### 5.4 Replay Equivalence
For any valid event tape T and ExecSpec S:
```
Replay(T, S) == OriginalExecution(T, S)
```
Verified by `Replayer::verify()` performing 6 checks (see EXECUTION_RUNTIME.md §4).

---

## 6. State Machine

```
[*] → TASK_CREATED → PLAN_CREATED → STEP_READY → STEP_RUNNING
      → PRIMITIVE_EXECUTING → STEP_COMPLETED → (next step or TASK_COMPLETED)
                            → STEP_FAILED → TASK_COMPLETED
TASK_COMPLETED → [*]
```

Transitions enforced by `check_and_emit_transition` on event bus writes.
Illegal transitions produce fatal `State Machine Violation` error.

---

## 7. Provider Interfaces

| Trait | Purpose | Implementations |
|-------|---------|-----------------|
| `StorageProvider` | Event persistence, cache, leases, snapshots | `DefaultStorage` (SQLite) |
| `FilesystemProvider` | File read/write/create_dir | `DefaultFilesystem` (std::fs) |
| `LlmProvider` | LLM chat, embedding | `DefaultLlm` (MLX/OpenAI) |

All providers registered via `OnceLock` at composition root.
Kernel core depends only on trait objects, never on concrete types.

---

## 8. Configuration Surface

| Variable | Purpose | Default |
|----------|---------|---------|
| `KERNEL_DB_PATH` | SQLite database path | `kernel.db` |
| `DAK_LM_BACKEND` | LM backend selector (`mock` / `mlx` / `openai`) | Auto-detect |
| `OPENAI_API_KEY` | OpenAI API authentication | — |
| `OPENAI_BASE_URL` | OpenAI-compatible endpoint | — |
| `OPENAI_MODEL_VERSION` | Model version override | — |

No other environment variables are read by kernel core.

---

## 9. Verification Pipeline

```
integrityV1 (preflight)
    ├── snapshotVersionMatrixV1 (contract)
    ├── goldenReplayCorpusV1 (contract)
    └── schedulerIntegrationV1 (contract)
```

All nodes must pass with `ok: true` before a phase is considered complete.

---

## 10. Phase Status

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Core types, event bus, scheduler, worker, replay engine | ✅ DONE |
| 2 | pub(crate) refactor, public facade, integration tests | ✅ DONE |
| 3 | Semantic bias v1 schema, sealed enum, registry API | ✅ DONE |
| 4 | Regression lock, architecture invariants, drift detection | 🔄 IN PROGRESS |
