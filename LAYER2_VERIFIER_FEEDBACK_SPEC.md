# Layer 2 — Verifier-Driven Feedback Loop Spec

Status: PROPOSED (2026-09-25). Gate E0 PASSED the same day (see §3):
Layer 1 does not eliminate the class. Prerequisites C0–C3 are next.

## 1. Justification — the resume condition is met

The deferral record required: *a reproducible executable semantic failure
that Layer 1 does not eliminate and the verifier can diagnose.*

Evidence (`analyzer_out/mq_northpay_2026-09-25/`, live gemma4-reasoning,
canonical `pipeline-run` path, 14 runs):

- **14/14 runs**: well-formed patch → `patch_shape_validation=ok` → applied
  → real tests failed semantically → honest `classification=tests_failed`,
  no fabricated success. `repair_report` absent 14/14; `malformed_patch`
  0/14. Layer 1 (shape validation, patch repair) never fires — the class
  is invisible to it.
- **Reproducible at classification level**: seeds 42 and 102 reproduced
  `tests_failed` across 2026-09-24 and 2026-09-25. Patch-level identity
  did NOT hold (seed 42: `round(x*100+0.5)` vs `round(x*100)`; seed 102:
  Decimal vs `round(x*100+0.5)`) — consistent with the payload
  reconstruction confound; cross-day artifact identity is NOT claimed.
- **Diagnosable by the verifier**: the failing check has a stable name
  (`test_refund_basic_third`) and the defect is localizable from it
  (`round(x+0.5)` used where `floor(x+0.5)` is required).

Sub-condition CLOSED 2026-09-25 (gate E0,
`analyzer_out/mq_northpay_hints_2026-09-25/`): the same 14-seed series WITH
the money-truncation hint block converted **0/14** — hints changed the
failure's shape (one uniform Decimal patch, which additionally
hallucinates `round(x, 2, rounding=...)` — builtin `round` has no such
parameter) but not its class. Layer 1 is exhausted for this class.

## 2. What Layer 2 is

A **bounded, evidence-emitting repair loop** inside the canonical CodeFix
chain. When `04_run_tests` fails with `classification=tests_failed` and
the failing test identity is known, the kernel feeds the *located rung*
back into a bounded number of additional `02_patch_code` attempts instead
of failing terminally on the first bad patch.

Explicitly NOT:
- not a statistical monitor, not anomaly telemetry (rejected by the
  deferral record; 6-step episodes give no post-onset horizon);
- not traceback/values feedback — no expected values, no stdout/stderr
  content ever enters the prompt (T11 gaming surface and injection
  surface, per the deferral record's design constraints);
- not a retry-until-pass — no fabricated success under any budget
  exhaustion; the honesty invariant is untouched;
- not executor-side autonomy — every attempt is a first-class persisted,
  replayable, event-logged kernel episode.

## 3. Gate experiment E0 — DONE 2026-09-25, PASSED (hints convert 0/14)

Ran the 2026-09-25 series harness WITH the Layer-1 money-truncation
hint block appended to the payload (the three hints verbatim from
`hint_engine.rs`). Result: 14/14 `tests_failed`, evidence in
`analyzer_out/mq_northpay_hints_2026-09-25/`. Side finding (Layer-1
improvement item, not a Layer-2 blocker): the money-truncation hints
should state the return-type contract and forbid builtin
`round(..., rounding=)` — `rounding=` is legal only on
`Decimal.quantize`.

## 4. Prerequisites — DONE 2026-09-25

All four landed as separate commits on `analyzer` (suite 915/0, clippy
clean, CI watched):

- **C0 — failing-test identity** (`fc53c26`): `TestReportV1.failures`
  carries located failing-test names. The python harness emits a
  `DAK_TEST_FAILURES_V1 <json>` marker as the LAST stderr line (a forged
  marker from a test file cannot shadow it); the kernel parses the last
  marker, validates identifier shape, fails closed to empty. Names only.
- **C1 — failing report persisted** (`58401e1`): `execute_run_tests`
  failure returns `TestRunFailure { reason, report }`; the effects loop
  persists it as a `primitive_result_v1` artifact with
  `tests_passed=false` before the step dies. Error strings unchanged.
- **C2 — model calls recorded** (`ad9e879`): `ChatRequest.seed` (wire,
  optional) stamped from `DAK_KERNEL_SEED` set by `pipeline-run`; the
  PatchCode path records `llm_calls[]` (full prompt, full response,
  model, seed) into the step artifact.
- **C3 — cycle events** (`6756ac4`): `FEEDBACK_CYCLE_STARTED /
  FEEDBACK_ATTEMPT / FEEDBACK_EXHAUSTED / FEEDBACK_CONVERTED` locked in
  `src/execution/feedback.rs` with payload contracts; bus roundtrip
  tested. Emitters land with the POC.
- **Security review before merge of the loop itself** (carried over from
  the deferral record): the loop re-enters the patch path with model
  output; workspace confinement (`resolve_safe`, `authorized_workspace`)
  must hold per attempt exactly as it does for the first attempt.

## 5. Design

### 5.1 Where it lives

In the canonical effect loop (`src/effects.rs::execute_effects`, used by
`pipeline-run` via `main.rs:1265`), driven by failure-signature strategy
selection (`src/strategy.rs::admissible_strategies`) as a stage of the
existing PROGRESS UNTIL VERIFIED program (`src/progress.rs`, stage 2 is
detect-only today). NOT in the standalone `run` binary path — the misfire
audit (2026-08-18) documented its simulated lifecycles; no new writer may
appear outside the canonical path.

### 5.2 The loop

```
attempt 1: 02_patch_code → 03_apply_patch → 04_run_tests
  on tests_failed with failures[].name known and attempts left:
    feedback = { failing_tests: [names], defect_contract: <from payload>,
                 prior_patch_hash: <blake3 of attempt's patch> }
    re-enter 02_patch_code with prompt + feedback block
  on identical patch hash (temp-0 futility): stop — FEEDBACK_EXHAUSTED
    (at temperature 0 an identical prompt repeats identically; a repeated
    patch after changed feedback means the model cannot use the signal)
budget: max 2 feedback attempts (3 patch attempts total) — hard-coded,
  not configurable, for the POC
```

The feedback block contains: failing test **names**, the defect contract
sentence from the original payload, and the prior patch hash. Nothing
else. No test output, no traceback, no expected values.

### 5.3 Termination and honesty

- Exhaustion → the task ends exactly as it does today:
  `classification=tests_failed; no fabricated success`, terminal, with
  all attempts persisted. The loop can never widen the definition of
  success.
- A converted run (attempt N passes `04_run_tests`) proceeds to
  `05_validate_patch` unchanged — validation is not weakened for
  loop-produced patches.

### 5.4 Determinism and replay

- Each attempt is an independent persisted episode: patch artifact, test
  report artifact, model request/response records — all keyed with the
  attempt index.
- `execute_effects` remains the single writer; replay of a looped task
  replays attempts in order (event log is the source of truth).
- The kernel `--seed` continues to govern planning; model temperature
  stays 0.0. Loop decisions (retry/stop) depend only on persisted
  artifacts and hashes, never on wall-clock or RNG.

## 6. POC verification — DONE 2026-09-25

Loop implemented per §5 (`src/execution/feedback.rs`, effects-loop hook on
run_tests failure, prompt augmentation in the PatchCode branch;
`tests/feedback_loop_poc.rs`: conversion / honest exhaustion /
identical-patch stop / kill switch). Live Arm A on NorthPay (5 seeds,
same payload as the 2026-09-25 control): **conversion 0/5** — evidence
`analyzer_out/mq_northpay_loop_2026-09-25/`.

Outcome, honestly: the loop machinery is verified end-to-end (cycles,
rollback, attempt artifacts, events, futility stop), and the model does
not exploit located-rung feedback for this defect class — it pivots to
Decimal and keeps failing the return-type contract. Side signal: the
feedback prompt degraded JSON compliance (2/5 patch_error vs 0/19
baseline). The pilot blocker is model capability, not the loop. Two real
bugs found and fixed by the POC: stale `__pycache__` shadowing re-patched
sources (harness purge), and a latent stale-read in the `find_latest_*`
helpers (returned the OLDEST artifact; exposed by multiple reports per
task).

## 7. Security considerations

- Feedback content is kernel-derived (test names from C0), never model-
  or file-derived text — the prompt cannot be poisoned via traceback.
- No expected values in feedback → no hardcoding channel opened (T11).
- Each attempt re-runs the existing confined write/patch path; the loop
  adds no new filesystem capability.
- New events are append-only; nothing in the loop deletes or mutates
  prior artifacts.
- Security review (executor is frozen for it already) must cover this
  spec before implementation merges.

## 8. Non-goals

- Multi-defect or multi-file repair loops (single located rung only).
- Feedback into decomposition/planning (the plan is fixed once made).
- Temperature/scheduling tuning as a repair strategy.
- Layer 3 (policy/adaptation from loop outcomes) — separate track.

## 9. Open questions

- Should `admissible_strategies` own the retry budget per failure
  signature, or is the budget global for the POC? (Leaning global: one
  failure class demonstrated.)
- Does `05_validate_patch` need an attempt-aware report, or is the
  current single-report shape sufficient when only the final attempt can
  reach it?
- Hint composition order when E0 shows partial conversion: hints-then-
  loop, or loop-then-hints on exhaustion? (Leaning hints-first: cheaper
  signal first.)
