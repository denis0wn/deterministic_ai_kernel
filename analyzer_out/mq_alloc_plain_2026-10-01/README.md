# allocation-remainder measurement — NEW defect class — 2026-10-01

Sanctioned local model (Ministral-3-14B-Reasoning-2512), loop OFF, 8 seeds
per arm, canonical payload form. Fixture:
`analyzer_examples/client_alloc` (money-allocation defect:
`split_amount` rounds every share independently, so shares do not sum back
to the total).

## Results

| Suite revision | plain | + invariant hints |
|---|---|---|
| weak suite (round 1, no granularity pin) | 1/8 | 8/8 |
| **strengthened suite (whole-cent granularity)** | **0/8** | **8/8** |

## What the evidence showed (and fixed)

1. **Round 1 exposed a hole in the fixture's own suite** — same class of
   finding as dateflow 2026-09-26. Plain seed42 passed the weak suite with
   SUB-CENT shares (33.3333... each for a 3-way split: exact sum, spread 0 —
   but money that cannot be paid). Fixed by adding the whole-cent assertion
   to `_check` and as `test_shares_are_whole_cents`; the fixture carries the
   note. Tests-as-judge is only as strong as the suite — and the evidence
   chain is what exposed it.
2. With the strengthened suite all 8 plain seeds fail honestly
   (`classification=tests_failed`, no fabricated success); seed42's
   sub-cent patch now fails the granularity assertion.
3. **The hint recipe converted 8/8** and every patch is byte-identical
   (md5 across all 8 diffs): the canonical integer-cents largest-remainder
   construction (`cents = round(total * 100); base = cents // parts;`
   remainder cents to the first r shares). Zero repairs needed. Recipe
   states the invariant (exactly r shares get the extra cent), following
   the business-days lesson — hint recipes are validated by measurement or
   not at all; this recipe is now shipped in `hint_engine.rs`
   (`allocation-remainder`).
4. Correct alternative policies still pass the suite (verified externally:
   cumulative rounding passes, last-share-correction fails on spread,
   sub-cent fails on granularity) — the suite judges the contract, not one
   implementation.

## Files

- `run_series.sh` — the harness (payload + hint recipe verbatim)
- `mq_alloc_{plain,hints}_weak_2026-10-01/` — round-1 evidence (the hole)
- `mq_alloc_{plain,hints}_2026-10-01/seed*.out|diff` — per-seed evidence

Layer-2 note: loop was OFF in all arms (clean attribution; W3: the loop
does not convert at temp 0 — this class needs recipe knowledge).
