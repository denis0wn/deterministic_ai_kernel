# W1 measurement — two new defect classes — 2026-09-26

Sanctioned local model (Ministral-3-14B-Reasoning-2512), loop OFF, 8 seeds
per arm, canonical payload form. Fixtures: `analyzer_examples/client_dateflow`
(business-days defect), `analyzer_examples/client_scorer` (null-safety defect).

## Results

| Class | plain | + class hints |
|---|---|---|
| scorer (null-safety) | 8/8 | 8/8 |
| dateflow, weak suite (first run) | 8/8 | 3/8 |
| **dateflow, strengthened suite** | **0/8** | **3/8** |

## What the evidence showed (and fixed)

1. **The first dateflow suite had a hole** — no weekend-START cases — and a
   plain-arm patch (`if weekend: +2 days`) passed every test while being
   wrong for Saturday starts. Found by reading the patch, fixed by adding
   `test_saturday_plus_one_is_monday` / `test_sunday_plus_two_is_tuesday`
   (fixture `test_schedule.py` carries the note). Tests-as-judge is only as
   strong as the suite — and the evidence chain is what exposed it.
2. With the strengthened suite: plain 0/8 (the hole-exploiting patches now
   fail honestly), hints 3/8 (the completed ones are genuinely correct —
   `while weekday() >= 5: advance` — verified by reading).
3. scorer (null-safety) is easy for this model: 8/8 both arms.

## Files

- `run_w1_series.sh` — the harness (payloads + hint recipes verbatim)
- `mq_<class>_<plain|hints>_2026-09-26/seed*.out|diff` — per-seed evidence

Layer-2 note: loop was OFF in all arms (clean attribution).
