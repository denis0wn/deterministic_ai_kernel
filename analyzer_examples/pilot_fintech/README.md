# pilot_fintech — v0.2 Enterprise Pilot fixture

Python-first fintech fixture for the deterministic analyzer pilot.

## Seeded candidates (billing/ledger.py)

- **P1** money truncation: `int(amount * 100) / 100` drops sub-cent fractions.
- **P2** bare `round()` on a settlement path without an explicit rounding policy.
- **P3** unfinished-work marker (`TODO`) on a settlement path.

## Negative control (billing/safe_pricing.py)

Compliant money handling: `Decimal.quantize` with explicit `ROUND_HALF_UP`,
defaulted `.get()`, `range(len(...))`. The scanner must report **zero**
findings in this file — it proves detection is pattern-driven, not
"flag everything in sight".

## Reproducible failing tests (test_ledger.py)

Fail on the seeded defects (executor-harness compatible). An operator may
submit this file with `--repro <finding_id>=test_ledger.py` to promote a
finding to `remediation_ready`; the analyzer never claims that status on
its own.

The analyzer is READ-ONLY against this directory: no file may change as a
result of any scan or report run.
