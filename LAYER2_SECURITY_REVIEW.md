# Layer 2 — Security Review (pre-POC)

Date: 2026-09-25. Reviewer: Qwen (forensic pass, code-path evidence).
Scope: `LAYER2_VERIFIER_FEEDBACK_SPEC.md` loop design + the merged
prerequisites C0–C3 (`fc53c26`, `58401e1`, `ad9e879`, `6756ac4`) post-hoc +
interaction with the 2026-09-24 audit's open findings.
Method: full call-chain tracing on the diff `0af4947..78a014d`, threat
model = model output is adversarial, workspace content is model-influenced,
operator payload is trusted.

## Verdict: YES — with conditions C1–C4 below

The prerequisites introduced no capability regression; the loop design is
safe to implement **provided** the four conditions hold. One HIGH finding
(F5, fabricated success via `os._exit(0)`) was found and **fixed in this
review cycle**; the remaining structural risk is pre-existing (M-2) and is
*amplified* by the loop.

## CONFIRMED findings (5)

### F5 (High, FIXED in this review) — exit-code spoofing via in-process SUT

**Reproduced, not theoretical.** The kernel harness executes the test file
AND the model-patched SUT it imports **in the harness's own process**
(`runpy.run_path`, `src/tools/test_runner.rs:56`). A patched module
containing `import os; os._exit(0)` kills the harness at import time with
a success exit code — zero tests ran, yet `run_tests` classified exit 0 as
`tests_passed`. Reproduced against the real harness byte-for-byte:

```
sut.py: import os; os._exit(0)
test_x.py: from sut import *; def test_real(): assert True
$ python3 harness.py test_x.py  →  exit 0   (fabricated success)
```

This attacked the kernel's core invariant ("never declare tests passed on
the model's word — only on the real exit status") through the harness's
own process, and pre-dates the Layer-2 work (the loop would have amplified
it: up to 3 spoof opportunities per task).

**Fix (this cycle):** for the `python_test_file` runner, `passed`
additionally requires the harness's own `kernel test harness: OK` line on
stdout, printed only after every `test_*` function returned
(`classify_outcome`, test_runner.rs). Exit 0 without the marker →
`tests_failed`. Regression test: `os_exit_zero_from_sut_does_not_
fabricate_pass` (asserts exit_code == 0 AND classification ==
tests_failed). **Remaining ceiling, honestly stated:** in-process
adversarial code can print the marker string itself before exiting —
unforgeable signalling from a compromised process is impossible; the
structural fix is M-2 isolation (out-of-process verdict). The marker
requirement kills the trivial/accidental class and any spoof that does
not know the exact string.

### F1 (Low, reproduced) — C0 failure-name marker is forgeable via `atexit`

`src/tools/test_runner.rs` harness prints `DAK_TEST_FAILURES_V1 <json>` as
the last line of *harness-controlled* output, and the kernel parses the
last marker line. But workspace code (the model-patched SUT, imported by
the test module) can register an `atexit` handler; those run at
interpreter shutdown — **after** the harness printed its marker — so a
forged later marker shadows the real one. Reproduced byte-for-byte:

```
$ python3 harness.py test_x.py   # sut.py registers atexit print
stderr: DAK_TEST_FAILURES_V1 ["test_real"]
stderr: DAK_TEST_FAILURES_V1 ["forged_by_atexit"]   ← parsed (last wins)
```

- Reachability: requires model-written code to deliberately register
  `atexit`; reachable in the Layer-2 threat model.
- Impact: feedback receives wrong test *names* — the loop is misdirected.
  **Cannot fabricate success**: pass/fail authority is the process exit
  code plus (after the F5 fix) the OK marker; a forged names marker only
  degrades repair quality. No workspace escape, no privilege change.
- Ceiling: any signal channel from a process running adversarial code is
  forgeable (the path/token variants are readable via `sys.argv` /
  `os.environ` / inherited fds). Names are therefore *advisory*; the
  structural boundary is M-2 isolation. Documented as accepted limitation.

### F2 (Low) — C1 storage error can mask failure classification

`src/effects.rs` (C1 block): on step error we persist the failing report
with `storage.append_semantic_artifact(...)?`. If THAT write fails (db
locked/full), the returned error is the storage error, not the original
`fatal: tests_failed` — the terminal/retryable classification of the
original failure is lost.

- Reachability: requires the db to break mid-pipeline; in that state
  `fail_step` would have failed anyway, so no *safety* property changes.
- Impact: error-classification fidelity on an already-broken system.
  Accepted as-is; noted for operators (a storage failure during a failing
  task reports as a storage failure).

### F3 (Info) — `llm_calls` persists full prompts, including file content

C2 records full prompt + response per patch-step call into the step
artifact. The prompt embeds the full target-file content. If a workspace
file contains secrets, they now persist in `kernel.db` under
`llm_calls[].prompt` — but this is the **same exposure class** the kernel
already had: the read step persists file content into semantic artifacts,
and the content is sent to the model endpoint regardless. No new class;
operators should treat `kernel.db` as containing workspace content (it
already does).

### F4 (Medium, pre-existing, AMPLIFIED) — M-2 × attempt count

M-2 (2026-09-24 audit, still open): `src/tools/test_runner.rs:56` executes
workspace python via `runpy.run_path` unsandboxed; test-file selection
`run_tests` :226-252 picks a workspace `*.py`. A model can land code in the
workspace via the patch path and have it executed. The feedback loop
re-enters `04_run_tests`, multiplying executions of model-influenced code
by up to 3 (budget: 2 feedback attempts).

- The loop adds **no new capability** — but it increases the exposure
  count of an open Medium finding.
- Condition C2 below.

## REJECTED (checked, not issues)

1. **Forged marker fabricating success** — impossible: classification
   derives from the child process exit code only; the marker feeds names
   to a prompt, never the pass bit. (test_runner.rs `report_tail`.)
2. **Prompt injection via feedback names** — names are constrained to
   `[A-Za-z0-9_]{1,200}`, count-capped at 64, fail-closed on anomaly
   (`parse_failures`). The feedback block adds only names + the operator's
   own defect contract + a blake3 hex digest.
3. **Confinement regression from C0–C3** — `git diff 0af4947..78a014d` on
   `src/tools/file_tools.rs` and `src/execution/patch_apply.rs` is empty;
   the 22 security tests from `02b8736` are in the green 915/0.
4. **`DAK_KERNEL_SEED` env race** — set once in the `pipeline-run` arm
   before any model call; read-only afterwards.
5. **Seed on the wire** — opaque metadata; server may ignore it.
6. **Loop widening success** — by design it cannot: conversion requires a
   real exit-0 test run, `05_validate_patch` is unchanged for
   loop-produced patches, budget is hard-coded at 2, identical-patch
   retry stops the cycle (temp-0 futility).
7. **Event channel abuse (C3)** — vocabulary only; emitters land with the
   POC and will be reviewed with it.

## Conditions for the YES

- **C1 (POC mechanics)**: the loop must re-enter the existing executor
  path (`execute_effects` step machinery) — no new write/patch/test
  channel. Verified at POC review; the spec already mandates this (§5.1).
- **C2 (M-2 amplification)**: until M-2 is closed, the POC runs only
  against controlled fixtures (`analyzer_examples/client_northpay`), not
  arbitrary workspaces. An M-2 fix (or a documented risk acceptance)
  precedes any production use of the loop.
- **C3 (feedback names are advisory)**: the POC must treat
  `failures[]` as a hint, never as evidence: pass/fail authority stays
  with exit code + OK marker (F5 fix), and no success path may consume
  the names list. (The earlier idea of a random-path marker file was
  dropped during this review: in-process adversarial code reads
  `os.environ`/`sys.argv`, so no in-process channel is unforgeable;
  M-2 isolation is the only structural boundary.)
- **C4 (documentation)**: operator docs state that `kernel.db` artifacts
  contain workspace file content (already true; now also under
  `llm_calls[].prompt`).
