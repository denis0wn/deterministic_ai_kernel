# Pilot Metrics — schema `pilot_metrics_v1`

Purpose: report pilot outcomes in a form that cannot cherry-pick. Every
attempt counts, negative outcomes are first-class, and the `fake_success`
counter is the pilot's self-destruct wire: any non-zero value invalidates
the pilot result.

## Schema

```json
{
  "schema_version": "pilot_metrics_v1",
  "pilot_id": "...",
  "attempts_total": 0,
  "attempts": {
    "fail_closed_before_effect": 0,
    "terminal_resume_rejected": 0,
    "patch_applied_tests_failed": 0,
    "patch_applied_tests_passed": 0,
    "fake_success": 0
  },
  "findings": {
    "reviewed": 0,
    "approved": 0,
    "rejected": 0,
    "deferred": 0
  },
  "evidence": {
    "snapshot_continuity_verified": 0,
    "chain_incomplete": 0,
    "chain_inconsistent": 0,
    "chain_consistent_remediation_evidenced": 0
  }
}
```

## Field semantics

### attempts

- `attempts_total` — every pipeline attempt executed, including
  configuration mistakes, terminal failures and resumes. Nothing is
  excluded.
- `fail_closed_before_effect` — attempts stopped by a gate BEFORE any
  workspace mutation (missing authorization, malformed/hallucinated
  patch, invalid shape). These are SAFELY SUCCESSFUL gate activations,
  not failed remediations.
- `terminal_resume_rejected` — attempts that resumed a terminally failed
  task and correctly produced no new effect (terminal state preserved).
- `patch_applied_tests_failed` — a patch passed the structural boundary
  and was applied inside the authorized workspace, then REAL tests
  failed. Honest negative remediation.
- `patch_applied_tests_passed` — a patch was applied and REAL tests
  passed with a structurally valid `test_report_v1`. Only this category
  feeds any success narrative.
- `fake_success` — attempts where success was claimed without real
  passing tests, or where evidence was relabeled/altered. MUST stay 0;
  any value > 0 invalidates the pilot and triggers incident review.

### findings

Counts of human review decisions recorded through `analyzer_review`
(approve/reject/defer), plus `reviewed` = all decisions recorded.
Approvals with `override_candidate_only` are counted in `approved` and
must be listed separately in the pilot narrative.

### evidence

- `snapshot_continuity_verified` — chain runs where the executor's
  pre-image hash matched the analyzer snapshot hash for every target
  file (cross-boundary continuity).
- `chain_incomplete` / `chain_inconsistent` /
  `chain_consistent_remediation_evidenced` — verifier overall statuses,
  counted exactly as emitted. `chain_consistent_no_change` attempts are
  honest outcomes too; report them in the narrative (they do not enter
  this schema's counters).

## Counting rules

1. `attempts_total` == sum of the five `attempts` counters. Any mismatch
   is a reporting defect.
2. Success rate = `patch_applied_tests_passed / attempts_total`, over
   ALL attempts. Best-seed reporting is prohibited.
3. Every attempt referenced in metrics must have preserved artifacts
   (executor log, evidence chain inputs, decision/work-order ids).
4. Metrics are produced AFTER the attempt series closes; counters are
   never revised silently — corrections are additive notes.

## Worked example — pilot run 2026-08-16 (real negative outcome)

Pilot id: `pilot_demo_pilot_fintech_2026_08_16`

```json
{
  "schema_version": "pilot_metrics_v1",
  "pilot_id": "pilot_demo_pilot_fintech_2026_08_16",
  "attempts_total": 3,
  "attempts": {
    "fail_closed_before_effect": 1,
    "terminal_resume_rejected": 1,
    "patch_applied_tests_failed": 1,
    "patch_applied_tests_passed": 0,
    "fake_success": 0
  },
  "findings": {
    "reviewed": 1,
    "approved": 1,
    "rejected": 0,
    "deferred": 0
  },
  "evidence": {
    "snapshot_continuity_verified": 1,
    "chain_incomplete": 1,
    "chain_inconsistent": 0,
    "chain_consistent_remediation_evidenced": 0
  }
}
```

Reading of the example: 1 finding approved for exactly one controlled
attempt series; 3 attempts executed; the boundary stopped 1 unauthorized
path, preserved 1 terminal failure, and honestly failed 1 applied patch;
snapshot continuity was verified once; the final chain status is
`chain_incomplete`; remediation success was not achieved and not
claimed. See NEGATIVE_REMEDIATION_ACCEPTANCE_RECORD.md.
