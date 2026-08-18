# client_settlement — HARD synthetic client (rounding accumulation)

A harder synthetic client (no customer data) modelling a classic, realistic
financial defect: **rounding accumulation**.

## The defect (A1)

`total_fees` truncates each line-item fee to whole cents
(`int(fee * 100)`) and accumulates, instead of summing the EXACT fees and
rounding HALF-UP to the cent ONCE. The error grows with the number of line
items — the drift a reconciliation desk actually sees.

## Why this is hard

- It is a **rounding-stage** defect (rounding applied at the wrong point),
  not a one-token typo — the model must restructure the logic to sum the
  exact fees and round ONCE at the end.
- The single-item test forces **HALF-UP specifically** (banker's
  `round(0.125, 2)` gives 0.12, but the policy requires 0.13), so a naive
  `round()`-at-the-end fix still fails.
- Detected by the `money-truncation` rule (`int(fee * 100)`), guarded by a
  monetary invariant; remediation is gated accordingly.

## Tests

`test_aggregator.py` FAILS on the seeded defect and defines correct
behavior (sum exact fees, round HALF-UP once).
