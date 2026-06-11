# TODO Phase 4 — Guardrails

## 10. Contract regression lock
- Test: no structural changes to V1 allowed.
- Any field addition -> requires version bump.

## 11. Architecture invariants tests
- semantic != execution
- CLI != truth
- snapshot != runtime state
- artifacts immutable after creation

## 12. Drift detection
- detect explain format changes
- detect replay divergence
- detect CLI surface expansion
