# PILOT DELIVERY REPORT — WORKED EXAMPLE

> **Status: WORKED EXAMPLE.** Assembled from REAL internal evidence (the
> R2 autonomous green cycle) on the SYNTHETIC `pilot_fintech` module,
> which stands in for a Client module. Real Client pilots follow the same
> structure with the Client's module. No Customer/Private data is used.

---

## 1. Pilot scope

- **Target module (stand-in):** `pilot_fintech` — a synthetic
  Python billing/rounding module (isolated, no customer data).
- **Snapshot BLAKE3:** `7b7cdceb704f076405bb30434c55ee42b774f28da4f9eb0721d398286ce757b2`
- **Ruleset:** python-fintech-rules/0.2.0 (5 financial rules).
- **Window / attempts:** single finding, 2 remediation attempts shown
  (one fail-closed, one green).
- **Positioning reminder:** this demonstrates the evidence trail and
  fail-safe behavior, not a fix-count guarantee.

## 2. Findings register (read-only analysis)

| Finding | Rule | Severity | Location | Disposition |
|---|---|---|---|---|
| MONEY-TRUNCATION-ROUNDING-14 | money-truncation | Critical | billing/rounding.py:14 | **approved** for remediation (reproducible test supplied) |

- Candidate statement: "money amount truncated via int() — sub-cent
  fractions lost."
- Evidence: file BLAKE3 anchored to the snapshot; limitations documented
  (known FP/FN profile for money-truncation).
- Finding is a **candidate**, not a proven defect, until the remediation
  evidence confirms behavior change.

## 3. Human review gate

- Review decision recorded via `analyzer_review`: **approve**, reviewer
  "mq-experiment-operator", rationale cited; reproducible test
  `test_rounding.py` present in the snapshot. Manual review was performed
  before any attempt (invariant upheld).

## 4. Remediation attempts & evidence

### Attempt A — fail-closed (honest rejection)
- A malformed/target-mismatched patch was rejected **before any
  application** (`malformed_patch`, terminal). Workspace diff: unchanged.
- Demonstrates the kernel boundary stops bad patches pre-effect.

### Attempt B — autonomous GREEN cycle (real)
- All six steps committed: read_repository → locate_bug → patch_code →
  apply_patch → run_tests → validate_patch.
- **Patch applied:** `float(Decimal(str(amount)).quantize(Decimal('0.01'),
  rounding=ROUND_HALF_UP))`.
- **Real tests:** `classification=tests_passed`, `exit_code=0`,
  allowlisted argv `python3 - test_rounding.py`.
- **Pre/post BLAKE3:** pre-image equals the snapshot file hash; post-image
  differs (change applied).
- **Independent verification:** the three rounding assertions re-run
  OUTSIDE the executor on the post-state — PASS.
- **Chain of custody:** `analyzer_chain_verify` overall =
  **`chain_consistent_remediation_evidenced`**,
  chain id `040b3ba458f194a3e5d43a08523e10d2fc3fd3af6a04619b10e69e4b1eacf2a8`;
  all five links verified; inputs untouched.

## 5. Acceptance mapping (per PILOT_ACCEPTANCE_CRITERIA.md)

- Evidence integrity (A1–A6): satisfied — manifest, findings, contract,
  review decision, pre/post hashes, event log, test_report_v1, and chain
  report all present and hash-anchored.
- Fail-safe behavior (B1–B5): satisfied — no mutation outside the isolated
  workspace; malformed patch rejected pre-effect; no relabeling
  (fake_success=0); human approval recorded; no production deployment.
- Process completeness (C1–C4): satisfied — finding dispositioned, all
  attempts published, independent verification done.
- Honest framing (D1–D3): satisfied — no forbidden claims; findings stated
  as candidates; limitations disclosed.

## 6. Honest limitations (disclosed)

- The green cycle used task context that included the expected test
  outcomes (tests are the ground-truth spec). Fully context-free
  remediation is not what is demonstrated.
- Demonstrated on one finding, one model (gemma4-reasoning); breadth
  across findings/models is unproven.
- Findings are pattern-level candidates; absence of a finding is not proof
  of absence of defects. Python-only in this phase.

## 7. Recommended next steps

- Extend to additional findings in the Client module under the same
  human-review gate.
- Wire the Client's own regression suite as the authoritative test oracle.
- If broader model autonomy is desired, run the executor on the Client's
  isolated module (R2 unblocked lowercase workspaces).

## 8. Commercially honest claim

> The system produces a reproducible evidence trail from a read-only
> finding to a controlled remediation. It does not guarantee the absence
> of defects and does not replace human review.
