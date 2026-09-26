# W1 measurement — two new defect classes — 2026-09-26

Sanctioned local model (Ministral-3-14B-Reasoning-2512), loop OFF, 8 seeds
per arm, canonical payload form. Fixtures: `analyzer_examples/client_dateflow`
(business-days defect), `analyzer_examples/client_scorer` (null-safety defect).

## Results

| Class | plain | + class hints v1 | + hints v2 (recipe fixed) |
|---|---|---|---|
| scorer (null-safety) | 8/8 | 8/8 | — |
| dateflow, weak suite (first run) | 8/8 | 3/8 | — |
| **dateflow, strengthened suite** | **0/8** | 3/8 | **8/8** |

## What the evidence showed (and fixed)

1. **The first dateflow suite had a hole** — no weekend-START cases — and a
   plain-arm patch (`if weekend: +2 days`) passed every test while being
   wrong for Saturday starts. Found by reading the patch, fixed by adding
   `test_saturday_plus_one_is_monday` / `test_sunday_plus_two_is_tuesday`
   (fixture `test_schedule.py` carries the note). Tests-as-judge is only as
   strong as the suite — and the evidence chain is what exposed it.
2. With the strengthened suite: plain 0/8 (the hole-exploiting patches now
   fail honestly), hints 3/8.
3. **The v1 hint recipe was itself the defect**: it described the iteration
   ("advance day-by-day, check weekday") but not the invariant — models
   produced count-days-and-skip loops that land on weekends. The v2 recipe
   states the invariant ("only business-day LANDINGS count; while added <
   days: advance one calendar day, count only landings") → 8/8, patches
   verified correct by reading. **Hint recipes are validated by measurement
   or not at all.**
4. scorer (null-safety) is easy for this model: 8/8 both arms.

Note: the `mq_dateflow_hints_2026-09-26/` directory holds the hints-v2
evidence (the v1 runs are in git history, commit `e0ddcc3`).

## Files

- `run_w1_series.sh` — the harness (payloads + hint recipes verbatim)
- `mq_<class>_<plain|hints>_2026-09-26/seed*.out|diff` — per-seed evidence

Layer-2 note: loop was OFF in all arms (clean attribution).
