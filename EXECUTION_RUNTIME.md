# Deterministic Execution Runtime (DAK)

This document describes the design, architecture, and constraints of the production-ready deterministic execution runtime of the Deterministic AI Kernel.

---

## 1. Deterministic Execution Model

DAK enforces strict determinism across all stages of request processing, planning, and primitive execution. By tying the execution to a stable random seed, strict normalizer transformations, and system context parameters, the kernel guarantees:
* Identical parsed step sequences for the same payload and seed.
* Identical execution planning results (`ExecSpec` containing structured `StepSpec`).
* Identical event tapes (with matching hashes and sequence numbering).
* Consistent replay results, even when verified on matching physical nodes.

```
[User Request] 
      │
      ▼
[Normalizer] ──► [Parser & Planner] ──► [ExecSpec] ──► [Deterministic Runtime]
                                                             │
                                                             ▼
                                                    [SQLite Event Tape]
```

---

## 2. Execution State Machine

The runtime hardening enforces a strict lifecycle state machine. Transitions are validated against the current state stored in the SQLite event log, preventing illegal state changes.

### State Transitions Diagram
```mermaid
stateDiagram-model
    [*] --> TASK_CREATED
    TASK_CREATED --> PLAN_CREATED
    PLAN_CREATED --> STEP_READY
    STEP_READY --> STEP_RUNNING
    STEP_RUNNING --> PRIMITIVE_EXECUTING
    PRIMITIVE_EXECUTING --> STEP_COMPLETED
    PRIMITIVE_EXECUTING --> STEP_FAILED
    STEP_COMPLETED --> STEP_RUNNING : Next Step
    STEP_COMPLETED --> TASK_COMPLETED : All Steps Done
    STEP_FAILED --> TASK_COMPLETED
    TASK_COMPLETED --> [*]
```

### Transition Enforcement Rules
* **Illegal Transitions**: Any repeats of active/running states (e.g. `STEP_RUNNING` to `STEP_RUNNING`), jumping back to ready states, or changing state after a task transitions to `TASK_COMPLETED` will result in a fatal `State Machine Violation` error.
* **Transitions validation**: Triggered automatically on event bus writes via `check_and_emit_transition`.

---

## 3. Event Sourcing & Event Bus

Every execution mutation writes to an append-only transaction log in SQLite.

### Event Schema
The schema for `event_log` is defined with a unique, content-hashed `event_id` to guarantee uniqueness and prevent duplication:

```sql
CREATE TABLE IF NOT EXISTS event_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT UNIQUE,
    system_generation BIGINT NOT NULL,
    causal_unit_id BIGINT NOT NULL,
    sequence_in_unit INTEGER NOT NULL,
    task_id TEXT NOT NULL,
    step_id TEXT,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    logical_generation BIGINT NOT NULL,
    UNIQUE(causal_unit_id, sequence_in_unit)
);
```

### Event Metadata Structure (`EventEnvelope`)
All event bus envelopes encapsulate the following metadata fields:
* `event_id`: Stable UUID-like string hashed via `evt_BLAKE3(...)` using transaction payload contents.
* `task_id`: Identifier of the execution pipeline context.
* `execution_id`: Identifier of the specific run iteration.
* `generation`: Sourcing generation index.
* `timestamp`: ISO 8601 string representation.
* `payload_hash`: Content hash verification token.

---

## 4. Replay Guarantees & Drift Verification

The `Replayer::verify` interface provides rigorous validation of execution logs against current configurations. It performs **6 distinct checks** to detect any system drift:

1. **Tape Plan ID Match**: Compares the plan ID recorded in the replay tape entry with the generated runtime plan ID.
2. **Database Plan ID Match**: Asserts the generated plan ID matches the historical `PLAN_CREATED` event log value.
3. **Environment Fingerprint Match**: Verifies the current environment fingerprint matches the fingerprint written at plan creation time.
4. **Step Sequence Match**: Checks the exact sequence of planned steps (and fallback steps).
5. **Primitive Sequence, Input, and Output Hashes**: Verifies the list of executed primitives, their parameter hashes, and their resultant output hashes.
6. **Chronological Event Types sequence**: Asserts that every logical state transition is matched in order.

Detailed drift reports highlighting `expected`, `actual`, `first_difference`, and `event_index` are output when verification fails.

---

## 5. Deterministic Cache Layer

To speed up operations and ensure idempotency of execution primitives (e.g. file operations, computation steps), DAK implements a persistent caching layer.

### Cache Key Computation
The cache key uniquely combines the task, plan structure, primitive configuration, parameters, and host environment:
$$\text{key} = \text{BLAKE3}(\text{task\_id} + \text{plan\_id} + \text{primitive\_id} + \text{input\_hash} + \text{environment\_fingerprint})$$

### Cache Semantics
* **Cache Hits**: When a matching cache key is found, the physical operation is bypassed completely, saving execution time. The engine emits a `CACHE_HIT` event containing the cached output details.
* **Cache Misses**: The primitive is executed physically, its output hash is saved to the SQLite `execution_cache` table, and execution resumes.

---

## 6. Environment Fingerprinting

To prevent cross-environment cache corruption and unsafe replays on mismatched system topologies, a system configuration fingerprint is dynamically constructed using non-temporal host metadata:
* OS type (`std::env::consts::OS`)
* Target Architecture (`std::env::consts::ARCH`)
* Active Rust version (`rustc --version`)
* Host Kernel version (`uname -r`)
* Relevant compiler/runtime env overrides (excluding temporary paths, timestamps, or system random tokens).

This fingerprint is locked inside the `PLAN_CREATED` event payload.
