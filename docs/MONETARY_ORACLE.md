# Monetary Invariant Oracle (v0.4.1)

## Why it exists

Example-based tests (`input -> expected output`) can miss subtle
rounding/scale errors when they don't cover the exact edge case. The
NorthPay simulation showed a plausible-looking fix with a rounding-scale
error that only a property check would guarantee to catch. For money we
need **properties that hold for all values**, not just examples.

## The hard guarantee (fail-safe)

A finding produced by a **money-math rule** (`money-truncation`,
`money-round-bare`, `floor-div-money`) is `remediation_ready` **only if**
a monetary-invariant test guards it in the workspace. Without it the
finding stays `candidate_only` forever — even if a reproducible failing
test is provided. **Money therefore cannot be remediated without
property-based checks.** This is a hard gate, not a suggestion.

Non-money findings are not gated (the criterion is vacuously present).

## How it works

1. **Marker.** The operator places a marker next to the invariant test(s)
   in a workspace test file:
   ```python
   # monetary-invariant: <FINDING_ID>
   ```
2. **Detection.** The analyzer (read-only) scans workspace Python files
   for the marker with the exact finding id
   (`monetary_oracle::invariant_present`).
3. **Readiness.** `task_emitter::assess_readiness` requires
   `monetary_invariant_present` for money-math findings before
   `remediation_ready`.
4. **Generation.** `analyzer_monetary_oracle --finding <ID>` prints a
   ready-to-adapt invariant test template.

## The invariants

The template ships two always-on invariants that need no expected values:

- **Cent precision** — a money result must never carry a sub-cent
  remainder: `abs((r * 100) % 1) < 1e-9`. This catches the rounding-scale
  error class (e.g. a result of `0.125` where a cent-rounded `0.13` is
  required). The **modulo form** is used deliberately instead of
  `round(r*100)`: a bare `round(...)` inside the guard would itself trip
  the `money-round-bare` rule when the test lives in a finance-path file.
- **Determinism** — `f(x) == f(x)` for all probes.

plus a skeleton **half-up boundary** check the operator adapts to the
function's contract (half-cent midpoints must round HALF-UP, never to
even, never truncate).

## Important scope note

Cent-precision/determinism catch the **scale-error** class. They do NOT
catch a **rounding-direction** error (e.g. truncation vs half-up both
produce cent-precision values) — that class is caught by the example
tests (e.g. asserting `0.125 -> 0.13`). The invariants are an ADDITIONAL
safety net on top of example tests, not a replacement. A robust monetary
test suite has both: example tests pin the policy (direction), invariants
guarantee structural properties (precision, determinism) for all values.

## Current coverage (as of v0.4.1, 2026-08-18)

Every REAL money-math defect across all monetary fixtures now carries a
monetary-invariant test; money therefore passes the gate and can reach
`remediation_ready`:

| Fixture | Money-math finding(s) guarded |
|---|---|
| pilot_fintech | `post_amount` (money-truncation), `settlement_round` (money-round-bare) |
| pilot_billing_service | `to_cents` (money-truncation), `split_fee` (floor-div-money) |
| client_northwind_clearing | `settlement_fee` (money-truncation) |
| client_northpay | `proportional_refund` (money-truncation) |
| billing_python | `compute_fee` (money-truncation) |

**Deliberate exceptions (no invariant, by design):**
- `client_northpay::split_count` (floor-div-money) and
  `client_northpay::loyalty_bonus` (float-equality) are DECOYS — correct
  code that must be REJECTED at review, not remediated. They are
  intentionally left without invariants so the gate keeps them
  `candidate_only`.
- `float-equality` is NOT a money-math rule (not in the gated set), so its
  findings (including the documented false positives on test-file
  assertions) are not gated by the oracle.

## Read-only invariant

The analyzer only DETECTS the marker and PRINTS a template; it never
writes into the scanned workspace. The operator adds the invariant test;
the EXECUTOR runs it via `run_tests_v1` like any other test.
