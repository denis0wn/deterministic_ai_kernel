# TODO Phase 1 — Hardening

## 1. Deterministic replay hardening
- Property test: seed × StepKind permutations -> identical replay.
- Batch test: 10k random seeds replay stability.
- Snapshot equality: bit-level comparison enforcement.
- Failure mode: any drift = test panic (no soft asserts).

## 2. Fuzz semantic boundary
- Fuzz StepKind parser with malformed inputs.
- Random invalid step sequences.
- Duplicate-heavy sequences stress test.
- Ensure: system never diverges, only rejects or normalizes.

## 3. Replay equivalence contract
- Define ReplayEquivalence test module.
- Assert:
  - same seed + same input -> identical snapshot graph;
  - ordering independence guaranteed.
