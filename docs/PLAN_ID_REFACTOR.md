# PLAN_ID refactor

## Goal

Compress the verification system into a deterministic core with three explicit invariants:

- `PLAN_ID`
- `ENVIRONMENT_ID`
- `EXECUTION_ORDER_ID`

Target model:

- `plan = f(spec, env)`
- `result = run(plan)`

No hidden policy branches outside plan construction.

## Stable layers

### Layer 1 — Specification

Input file:

- `docs/verification_graph.json`

This layer owns only declarative graph intent:

- nodes
- dependencies
- stage
- pipeline
- version
- priority
- outputs
- ordering policy name

It must not own runtime verdict state.

### Layer 2 — Planning

Pure function:

- `plan = f(spec, env)`

Planner resolves:

- selected pipeline
- dependency closure
- node versions
- canonical node set
- deterministic execution order
- environment binding
- plan identity

Planner output must be the only source of truth for execution identity.

### Layer 3 — Execution

Pure runner:

- `result = run(plan)`

Runner responsibilities:

- execute ordered nodes
- collect exit status and timings
- stop on failure if policy says so
- emit verdict referencing the plan

Runner must not recompute planning decisions.

## New invariants

### 1. PLAN_ID

Define:

- `PLAN_ID = deterministic_hash(all_planning_inputs)`

Must include:

- normalized spec content
- selected pipeline
- selected-only filter
- resolved node keys with versions
- dependency closure result
- ordering policy name
- `ENVIRONMENT_ID`

This replaces partial identity checks spread across:

- `plan_hash`
- `selected_pipeline`
- ordered node list
- parts of reuse policy

Rule:

- reuse legality is decided from `PLAN_ID` compatibility, not ad hoc field comparisons.

### 2. ENVIRONMENT_ID

Split environment into two levels.

#### Strict environment fingerprint

Used for reuse legality:

- python version
- cargo version
- rustc version

This becomes:

- `ENVIRONMENT_ID`

#### Loose environment fingerprint

Used only for diagnostics:

- platform
- system
- release
- machine
- cwd

Rule:

- only strict fingerprint participates in plan identity
- loose fingerprint is debug metadata only

### 3. EXECUTION_ORDER_ID

Define:

- `EXECUTION_ORDER_ID = deterministic_hash(resolved_ordered_node_keys + ordering_policy_name)`

Purpose:

- make execution order explicit and testable
- separate graph identity from order identity
- make replay/debugging easier

## Ownership rules

### Plan owns

- spec snapshot or spec hash
- `PLAN_ID`
- `ENVIRONMENT_ID`
- `EXECUTION_ORDER_ID`
- selected pipeline
- selected-only filter
- resolved ordered nodes
- strict environment snapshot
- optional loose debug environment snapshot

### Verdict owns

- `PLAN_ID`
- execution status
- node results
- timings
- failure reason
- optional pointer to debug metadata

Rule:

- verdict must not duplicate full environment state if already owned by plan
- verdict references plan identity, not redefines it

## Reuse policy target

Current reuse policy is field-based.

Target policy:

- if prior `PLAN_ID` != current `PLAN_ID` => `invalid_reuse`
- if prior `PLAN_ID` == current `PLAN_ID` => reuse allowed

If needed for diagnostics, explain mismatch in terms of:

- pipeline mismatch
- strict environment mismatch
- node set mismatch
- order mismatch

But legality should derive from identity, not from scattered custom checks.

## Immediate refactor steps

1. Introduce explicit `ENVIRONMENT_ID` from strict toolchain versions only.
2. Define `EXECUTION_ORDER_ID` from canonical ordered node keys.
3. Redefine `PLAN_ID` to include:
   - spec hash
   - selected pipeline
   - resolved nodes
   - ordering policy
   - `ENVIRONMENT_ID`
4. Make planner emit these three IDs.
5. Make verdict reference `PLAN_ID` instead of duplicating environment ownership.
6. Change reuse policy to compare `PLAN_ID` first, reason strings second.
7. Keep current tests, but shift assertions toward the three IDs.

## What not to do during this refactor

Do not add:

- new policies
- new CI jobs
- new fuzz layers
- new graph abstractions

Focus only on compression of identity and ownership.

## Desired end state

The system should read like this:

- spec declares the graph
- planner creates a deterministic bound plan
- runner executes that plan
- verdict reports execution against that plan

Short form:

- `spec -> plan -> result`

With explicit identities:

- `PLAN_ID`
- `ENVIRONMENT_ID`
- `EXECUTION_ORDER_ID`
