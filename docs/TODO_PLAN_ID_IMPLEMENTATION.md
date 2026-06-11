# TODO: PLAN_ID implementation

## Objective

Refactor the verification graph system so identity and reuse legality are derived from three explicit IDs:

- `PLAN_ID`
- `ENVIRONMENT_ID`
- `EXECUTION_ORDER_ID`

## Phase 1 — Environment identity

- Add `ENVIRONMENT_ID` as a strict fingerprint derived only from:
  - python version
  - cargo version
  - rustc version
- Keep current platform, machine, system, release, cwd as loose debug metadata only.
- Do not use loose metadata in reuse legality checks.

### Done when

- planner output contains both:
  - `environment_id`
  - `environment_debug`
- reuse policy no longer depends on loose machine metadata

## Phase 2 — Execution order identity

- Compute canonical ordered node keys after:
  - pipeline selection
  - selected-only filtering
  - dependency closure
  - version resolution
  - deterministic ordering
- Add:
  - `execution_order`
  - `execution_order_id`

### Done when

- plan serializes canonical ordered node keys
- order-dependent tests can assert `execution_order_id`

## Phase 3 — Unified plan identity

- Replace partial identity logic with:
  - `plan_id = hash(spec_hash + selected_pipeline + selected_only + resolved_nodes + ordering_policy + environment_id)`
- Keep `plan_hash` only if needed as transitional alias.
- Treat `PLAN_ID` as the only reuse identity input.

### Done when

- reuse legality starts from `plan_id`
- mismatch reasons are diagnostic only

## Phase 4 — Ownership cleanup

- Plan owns:
  - `plan_id`
  - `environment_id`
  - `execution_order_id`
  - environment snapshot
  - resolved nodes
- Verdict references:
  - `plan_id`
  - node results
  - timings
  - status
- Remove duplicated environment ownership from verdict where possible.

### Done when

- verdict no longer redefines planning identity
- runner only executes plan, does not reconstruct identity

## Phase 5 — Test migration

Update tests so they assert:
- `environment_id`
- `execution_order_id`
- `plan_id`

Keep existing semantic assertions, but move away from scattered field coupling.

## Non-goals

Do not add during this refactor:
- new CI jobs
- new policy families
- new fuzz suites
- new graph abstraction layers

## Target end state

`spec -> plan -> result`

Where:
- `plan = f(spec, env)`
- `result = run(plan)`

And the system exposes exactly:
- `PLAN_ID`
- `ENVIRONMENT_ID`
- `EXECUTION_ORDER_ID`
