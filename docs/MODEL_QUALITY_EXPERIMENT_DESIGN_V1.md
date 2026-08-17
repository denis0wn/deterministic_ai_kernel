# Model-Quality Experiment Design — v1 (PRE-REGISTRATION DRAFT)

Status: DRAFT. This design is **pre-registered at the moment it is
committed**. Any change after the first attempt is a protocol deviation
and must be recorded as a new version with a deviation note. No attempts
run before registration.

Experiment log (factual, append-only): 2026-08-16 — first bounded
task-level experiment under this design's guardrails executed
(mq1-rounding-truncation-3att; 3 attempts, seeds 100/101/102; result
3/3 malformed_patch, fail-closed, zero effects; records in
/tmp/dek_ai_matrix/MODEL_QUALITY_EXPERIMENT_PLAN.md and
MODEL_QUALITY_EXPERIMENT_FINAL_RECORD.md). The 40-attempt multi-arm
series of §2 has NOT started.
2026-08-16 (later) — mq2-patch-shape-3att executed: arm A2
(patch-contract facts incl. exact target path + letter-case rule),
seeds 103/104/105, budget 3. Result: 3/3 malformed_patch with the
IDENTICAL failure signature as mq1 (model capitalizes the workspace
directory in the patch target path); the prose-context lever is
ineffective for this defect on this model; zero mutations, zero fake
successes, all attempts preserved
(/tmp/dek_ai_matrix/MODEL_QUALITY_EXPERIMENT_PLAN_MQ2.md,
MODEL_QUALITY_EXPERIMENT_MQ2_FINAL_RECORD.md,
model_quality_experiment_mq2/). Combined mq1+mq2: 6/6 seeds.
2026-08-17 — mq3-structural-levers-3att executed: workspace renamed to
`isolatedws` (H1 naming trigger), A2 facts under renamed workspace, and
a structural PatchV1 shape example (H3); seeds 106/107/108, budget 3.
Result: 3/3 malformed_patch, IDENTICAL signature — H1 REJECTED, A2
ineffective, H3 REJECTED. All prompt-level levers tested so far fail to
fix this model's patch-target-path capitalization; combined mq1+mq2+mq3:
9/9 seeds. Zero mutations, zero fake successes, all attempts preserved
(/tmp/dek_ai_matrix/MODEL_QUALITY_EXPERIMENT_PLAN_MQ3.md,
MODEL_QUALITY_EXPERIMENT_MQ3_FINAL_RECORD.md,
model_quality_experiment_mq3/). Seed 109 remains reserved.
2026-08-17 — mq4-model-variable-3att (model as the variable: coder
fine-tune gemma-4-12b-coder-fable5-composer2.5-4bit; seeds 200/201/202,
budget 3): attempt 1 (seed 200) ended as infrastructure_failure — the
artifact is unservable on this machine (installed mlx_lm: `ValueError:
Model type gemma4_unified not supported`, reproduced by direct load);
per the registered stop rule seeds 201/202 were NOT executed. No
model-quality conclusion drawn; zero mutations; fake_success 0. Closed
as ENVIRONMENT LIMITATION
(/tmp/dek_ai_matrix/MODEL_QUALITY_EXPERIMENT_PLAN_MQ4.md,
MODEL_QUALITY_EXPERIMENT_MQ4_FINAL_RECORD.md,
model_quality_experiment_mq4/). Open item: pilot_metrics_v1 lacks an
infrastructure_failure counter (documented schema gap).
2026-08-17 (later) — env-mlxlm-upgrade-1 (preregistered upgrade of the
serving runtime to unblock the model-variable track): CLOSED WITHOUT
EXECUTION — the environment already runs the latest released mlx_lm
(0.31.3; mlx 0.31.2; Python 3.13.2), so there is nothing to upgrade to;
`gemma4_unified` is unsupported by the newest released runtime as well
(upstream support gap, not local staleness). No environment change made.
Record: /tmp/dek_ai_matrix/MLX_LM_UPGRADE_EXPERIMENT_PLAN.md. Remaining
paths (each needs separate approval): prerelease mlx_lm from git main as
its own experiment; a different artifact supported by 0.31.3; or close
the model-variable track.
2026-08-17 (later) — mq5-model-variable-r2-3att executed: model
variable round 2 with mlx-community/Qwen2.5-Coder-7B-Instruct-4bit
(qwen2; compatibility verified pre-registration; direct load verified
pre-series), seeds 203/204/205, budget 3, everything else identical to
the mq1 baseline. Result: 3/3 malformed_patch with the SAME signature
(`Isolated_ws` capitalized path). KEY UPDATE: combined mq1+mq2+mq3+mq5
= 12/12 across TWO model families — the mq4-era "gemma4-specific"
interpretation is REFUTED; the defect is task-presentation-robust; the
explanatory layer likely lives in the frozen kernel's patch_code
presentation (read-only analysis track proposed, no code changes).
(/tmp/dek_ai_matrix/MODEL_QUALITY_EXPERIMENT_PLAN_MQ5.md,
MODEL_QUALITY_EXPERIMENT_MQ5_FINAL_RECORD.md,
model_quality_experiment_mq5/). Reserved seeds now: 109, 201, 202.
2026-08-17 (later) — READ-ONLY root-cause analysis of the patch target
path failure (executor source @ 6eebb0f, no code changes):
PATCH_PROMPT gives the model a symbolic `<FILE>` placeholder in the JSON
template plus the real path on a FILE: line, so the model must RE-TYPE
the path; both families reconstruct the only "name-like" component (the
workspace basename) with a leading capital — a task-presentation
property + universal LLM prior, not a model defect. Target mismatch is
terminal with no corrective retry. Kernel-side mitigations enumerated but
require a separate security review; NO legal analyzer-side knob exists
(the patch prompt is kernel-owned). Record:
/tmp/dek_ai_matrix/PATCH_TARGET_PATH_ROOT_CAUSE_ANALYSIS.md.

Companion documents: PILOT_OPS_RUNBOOK.md (attempt discipline),
docs/PILOT_METRICS_V1.md (metrics), ANALYZER_ROADMAP.md (model-quality
phase gate), /tmp/dek_ai_matrix/NEGATIVE_REMEDIATION_ACCEPTANCE_RECORD.md
(baseline series).

## 1. Question and hypotheses

**Primary question.** With the executor kernel completely frozen (no gate
changes, no source changes), can contract-context quality levers raise the
real-model remediation success rate on the pilot finding above the baseline,
without weakening any safety property?

- Baseline (series 2026-08-16, control conditions): 0/3 remediation
  successes; 3/3 fail-safe behaviors; model patch semantically wrong
  (invented API signature, wrong rounding mode).
- H0: lever arms perform the same as control.
- H1 (directional): at least one lever arm shows a higher success rate.

This experiment evaluates **model + context**, never the gates. A lever
that would require weakening a kernel gate is out of scope by definition.

## 2. Fixed elements (registered before any attempt)

- **Target finding:** MONEY-TRUNCATION-LEDGER-15 on the `pilot_fintech`
  fixture. Success semantics are registered as: ALL module tests in
  `test_ledger.py` pass under the kernel-owned harness — because
  `run_tests_v1` gates on the workspace's derived test command, a
  "remediation success" for this finding necessarily includes the P2
  rounding behavior the tests encode. This is documented here, not
  discovered mid-flight.
- **Model/runtime:** real local gemma4-reasoning via MLX, kernel
  lifecycle-managed; kernel determinism settings unchanged (temp 0);
  idle unload after the series.
- **Workspace:** a FRESH isolated copy per attempt + pristine pre-copy
  for the chain; the original fixture is never the mutation target.
- **Seed list (fixed, 10 seeds):** 100, 101, 102, 103, 104, 105, 106,
  107, 108, 109. The previously used 42/43 are excluded on purpose; no
  seed outside this list may be run in this experiment.
- **Budget (hard cap):** 4 arms × 10 seeds = 40 attempts, plus up to 4
  operator-error repeats if an attempt is invalidated by operator
  misconfiguration (such repeats still count in `attempts_total`).
- **Stop rule:** the series ends when the budget is exhausted — report
  whatever the outcome is. Early stop is permitted ONLY for a safety
  violation (boundary breach or any fake-success event), never because
  results look good or bad.
- **Registration artifact:** the committed JSON block in §8.

## 3. Arms (one lever per arm; control included)

| Arm | Payload content | What it tests |
|---|---|---|
| A0 control | the six contract steps verbatim (v0.4 format) | baseline reproduction on the new seed list |
| A1 evidence context | A0 + finding candidate statement + evidence line + repro test expectations (input/output pairs) | does the model fail less when the observable contract is explicit |
| A2 patch-contract facts | A0 + kernel contract facts only: PatchV1 shape, context_before must match file bytes exactly, no invented APIs, tests run unmodified | does structural failure drop when the boundary is explained |
| A3 combined | A0 + A1 + A2 context | additive effect |

**Coaching boundary (registered).** Allowed added context: facts from the
analyzer package (finding statement, evidence coordinates, snapshot
hashes), the repro test's expected behaviors (already present in the
workspace), and kernel patch-contract facts. **Prohibited:** any form of
reference solution, hint at the correct code, or the fixed file content.
If a payload variant crosses this line it is rejected in preflight.

**Routing preflight (no model runs).** Each arm payload must be shown —
against the executor's existing router forensics corpus and parser rules,
read-only — to still route to the canonical CodeFix chain. If an arm
payload does not route, that arm is dropped BEFORE the series (recorded
as a preflight exclusion, not a mid-flight change).

## 4. Outcome taxonomy (pre-registered)

Every attempt receives exactly one class:

- `F0 fail_closed_before_effect` — stopped before mutation (missing
  authorization, malformed/hallucinated context, invalid patch shape).
- `F1 terminal_resume_rejected` — resumed terminal state, no new effect
  (not expected with fresh tasks; kept for completeness).
- `F2 patch_applied_tests_failed`, sub-classified by the reviewer from
  preserved evidence with these rules:
  - `F2a invented API/signature` (e.g. nonexistent kwargs),
  - `F2b wrong semantics` (e.g. wrong rounding mode),
  - `F2c partial fix` (strict subset of module tests now passing),
  - `F2d regression introduced` (previously passing behavior broken).
- `S patch_applied_tests_passed` — all module tests pass with a
  structurally valid report.

Classification happens AFTER the series closes, from artifacts only, by
the registered reviewer. `S` additionally requires the chain verifier to
emit `chain_consistent_remediation_evidenced` for that attempt; an `S`
without that chain status is reclassified as a protocol anomaly and
investigated before any publication.

## 5. Metrics and acceptance criteria

Metrics strictly via `pilot_metrics_v1`, one object per arm plus an
aggregate over ALL attempts.

- **Primary:** success rate = `S / attempts_total`, per arm and overall,
  over ALL attempts (operator-error repeats included).
- **Registered interpretation (honest limitation):** with 10 attempts per
  arm this is an operational signal, not statistical significance. A lever
  is declared "effective" only if its success rate exceeds control by at
  least 20 percentage points AND no safety counter broke; otherwise the
  result is reported as inconclusive/negative. No post-hoc redefinition
  of this threshold.
- **Safety counters (must stay 0):** `fake_success` for every arm;
  any non-zero value aborts the experiment and becomes an incident.
- **Secondary:** structural rejection rate (F0 share), F2 subclass
  distribution per arm, per-seed outcome table (published in full).

**Acceptance for the phase (from the roadmap):** either a pre-declared
metrics outcome over the whole series, or an accepted negative result.
Both are valid; only fake success is not.

## 6. Procedure and governance

1. **Register:** commit this document + §8 JSON. Registration timestamp
   = commit time. Distribute the registered JSON to the experiment
   directory.
2. **Preflight (zero model attempts):** routing check (§3), fixture
   semantics check on copies (pristine fails the tests; no model
   involved), lifecycle smoke.
3. **Per attempt:** fresh isolated copy → analyzer package for that copy
   → ONE `analyzer_review` approval referencing this registered design
   (reviewer: experiment operator; rationale must cite the experiment id)
   → work order → executor run with the arm payload + registered seed →
   preserve ALL evidence → `analyzer_chain_verify`.
4. **After the series:** classify all attempts (§4), compute metrics
   (§5), write the experiment report incl. the full attempt table;
   update the acceptance record (success → chain evidence; negative →
   negative acceptance record v2).
5. **Deviations:** any departure from this document (extra seed, payload
   edit, re-run of an attempt outside the registered repeat rule) is
   logged as a protocol deviation in the report and the affected attempt
   is flagged, never silently dropped.

## 7. Risks and honesty clauses

- Small N: rates are directional; the report says so verbatim.
- Same seed across arms does NOT imply same model behavior; paired
  per-seed comparisons are heuristic, not proof.
- Operator-error attempts count in attempts_total (discipline rule:
  success rate over ALL attempts).
- No attempt is excluded from publication; no arm is stopped because it
  is losing.
- The experiment creates no new execution path: everything runs through
  the existing work order + isolated workspace + gates.

## 8. Registration block (machine-readable)

```json
{
  "schema_version": "mq_experiment_v1",
  "experiment_id": "mq1-pilot-fintech-truncation",
  "registered_by_commit": "<filled at commit time>",
  "target_finding": "MONEY-TRUNCATION-LEDGER-15",
  "fixture": "analyzer_examples/pilot_fintech",
  "success_definition": "all module tests in test_ledger.py pass under run_tests_v1 AND chain_consistent_remediation_evidenced",
  "model": "gemma4-reasoning (local MLX, kernel lifecycle-managed)",
  "seed_list": [100, 101, 102, 103, 104, 105, 106, 107, 108, 109],
  "excluded_seeds": [42, 43],
  "arms": ["A0_control", "A1_evidence_context", "A2_patch_contract_facts", "A3_combined"],
  "attempts_budget": 40,
  "operator_error_repeats_max": 4,
  "effectiveness_margin_pp": 20,
  "stop_rules": {
    "budget_exhausted": "always stop and report",
    "early_stop_only_on": ["boundary_breach", "fake_success"]
  },
  "prohibited_context": ["reference solutions", "fixed file content", "hints at correct code"],
  "metrics_schema": "pilot_metrics_v1",
  "publication_rule": "all attempts published; success rate over ALL attempts"
}
```
