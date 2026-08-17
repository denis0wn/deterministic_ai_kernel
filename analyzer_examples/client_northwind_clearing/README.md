# client_northwind_clearing — simulated client (SYNTHETIC)

A synthetic "client" for a full end-to-end pilot simulation. **No
customer data** — models the shape of a real settlement path.

## The client's problem

The Northwind risk desk reports nightly settlement fees come out slightly
LOW and reconciliation flags a growing shortfall. The contractual fee
schedule is `fee = gross * rate`, rounded **HALF-UP** to the cent.

## The defect (not labeled in the code, the system must find it)

`settlement_fee` computes the fee in cents via `int(... * 100) / 100`,
which TRUNCATES toward zero instead of rounding HALF-UP. On fee values
with a sub-cent remainder this under-charges, and the error accumulates
across a batch — exactly the reconciliation shortfall the desk sees.

## The oracle

`test_settlement.py` encodes the contractual policy and FAILS on the
current implementation (midpoint fee must round up; batch net must match
the schedule). A correct remediation rounds HALF-UP to the cent.

## Why this is a good complex task

- The bug is a rounding-STAGE defect, not a one-token typo — fixing it
  requires understanding the domain policy (half-up, not banker's, not
  truncation).
- The fix must not break exact-amount behavior (`test_fee_exact_…`).
- The batch test ties per-transaction correctness to an aggregate outcome.
