# 60-Second Demo Script (verified commands, September 2026)

**Recording:** `demo_60s.cast` (asciinema, recorded live 2026-09-26 with
`docs/demo_runner.sh`; events re-timed to a readable 36 s pace — content
is the raw live output; `docs/demo_runner.sh` re-records anywhere).

Audience: one skeptical engineering lead. Setup: a terminal, the kernel
binary built (`cargo build`; binary at `./target/debug/deterministic_ai_kernel`),
the `client_northpay` fixture (a payment library with a seeded
money-rounding defect). Everything shown is live.

---

**[0:00] The claim.**
"This is a deterministic execution engine for AI code fixes. It does not
trust the model. It runs the real tests, keeps the receipts, and replays
the whole thing on demand. Watch."

**[0:05] The defect.**
Show `analyzer_examples/client_northpay/clearing/fees.py`:
`proportional_refund` truncates instead of rounding HALF-UP —
`1.0 × 1/8 = 0.125` must become `0.13`.

**[0:10] Unassisted model, first.**
"Even a frontier-class cloud model fails this class unassisted — we
measured 0 of 14 runs." (Show `analyzer_out/mq_northpay_cloud_2026-09-26/series.log`.)

**[0:20] The kernel run.**
```
./target/debug/deterministic_ai_kernel pipeline-run \
  --payload "Step 1 read repository <ws>/clearing/fees.py
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
Fix proportional_refund: it truncates instead of rounding HALF-UP to the cent. Contract: 0.125 -> 0.13.
<hints block>" --seed 42
```
"Six bounded steps. The model proposes; the kernel disposes. The tests
that decide are the repository's own — executed sandboxed, network denied,
writes confined to the workspace."

**[0:35] The receipts.**
```
./target/debug/deterministic_ai_kernel semantic-artifacts <task_id>
```
"Every step persisted an artifact. Every model call is stored with its
full prompt, its full response, and the seed. The test report names the
exact tests that ran and their outcome."

**[0:45] The replay.**
```
./target/debug/deterministic_ai_kernel replay-capsule <task_id> --json
```
"Same task, replayed against its recorded evidence. Deterministic — not
'usually similar'. And the measured result on this defect class with our
hint layer: 14 of 14 runs complete with the correct HALF-UP fix."

**[0:55] The honest close.**
"When the fix is wrong, the kernel says `tests_failed` and stops — it has
never fabricated a pass in any recorded run. The failure episodes are in
the same evidence directory; we'll scroll them if you want."

---

## Notes for the presenter

- Runs in this script are live, not canned. If the model misbehaves during
  the demo, that IS the product working: the failure will be classified,
  evidenced, and honest. Say so.
- Keep `DAK_FEEDBACK_LOOP=off` for the scripted run (Layer-2 feedback is
  proof-of-concept, not the pilot surface).
- The reference evidence directories cited above are committed in-repo
  under `analyzer_out/` and can be re-generated with the committed series
  scripts.
