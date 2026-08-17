# PILOT ACCEPTANCE CRITERIA

> Acceptance is measured on EVIDENCE INTEGRITY and FAIL-SAFE BEHAVIOR,
> deliberately NOT on "number of bugs fixed." Each criterion is binary and
> checkable from the delivered artifacts. A pilot that fixes zero findings
> but meets all criteria below is a SUCCESSFUL pilot; a pilot that "fixes"
> something without verifiable evidence is NOT acceptable.

## A. Evidence integrity (must ALL hold)

- [ ] **A1.** Workspace snapshot captured; BLAKE3 snapshot hash recorded in
      `evidence_manifest_v1`; manifest has no timestamps in content-hashed
      fields and is byte-reproducible for the same snapshot + analyzer
      version.
- [ ] **A2.** Every finding has: stable id, rule_id, severity, confidence,
      snapshot-anchored evidence location (file + lines + file BLAKE3),
      detector provenance, and documented limitations.
- [ ] **A3.** Every remediation attempt has a tamper-evident task contract
      (content-hashed) and a human review decision (approve/reject/defer)
      with reviewer + rationale.
- [ ] **A4.** Every attempted patch has BLAKE3 pre/post hashes and the
      kernel event log preserved.
- [ ] **A5.** Every completed test run has a structurally valid
      `test_report_v1` extracted from the executor's own artifacts (never
      hand-authored).
- [ ] **A6.** Every attempt has a chain-of-custody report
      (`evidence_chain_v1`) with an overall status and per-link results.

## B. Fail-safe behavior (must ALL hold)

- [ ] **B1.** No mutation occurred outside the isolated workspace (verified
      by diff/hash of the source-of-truth before vs after).
- [ ] **B2.** Every malformed/hallucinated patch was rejected BEFORE
      application (fail-closed), evidenced in the event log.
- [ ] **B3.** No attempt was relabeled: a tests-failed or rejected attempt
      is reported as such; `fake_success == 0` across the pilot.
- [ ] **B4.** No remediation ran without a recorded human approval.
- [ ] **B5.** Nothing produced during the pilot was deployed to production.

## C. Process completeness (must ALL hold)

- [ ] **C1.** Every finding received a disposition (approved / rejected /
      deferred); none silently dropped.
- [ ] **C2.** All attempts (successes and failures) are published in the
      final report; success rate, if quoted, is over ALL attempts.
- [ ] **C3.** The final pilot report contains: scope, findings register,
      per-attempt evidence + chain status, honest limitations, and the
      commercially honest claim (no forbidden claims).
- [ ] **C4.** Independent verification (post-state assertions re-run
      outside the executor) was performed for each green attempt.

## D. Honest-framing checks (must ALL hold)

- [ ] **D1.** The report does NOT contain any forbidden claim: no "AI
      guarantees security/compliance," no "automatically fixes any/all
      bugs," no "certifies absence of errors," no "hallucinations fully
      eliminated."
- [ ] **D2.** Findings are described as candidates, not proven defects.
- [ ] **D3.** Model and scope limitations are stated (Python-only;
      pattern-level heuristics; prose-hallucination residual outside gated
      classes).

## Outcome interpretation

- **All of A + B + C + D satisfied** → pilot accepted as a successful
  demonstration of deterministic, evidenced, fail-safe remediation — 
  regardless of how many findings were actually fixed.
- **Any B-item violated** (mutation outside boundary, relabeled failure,
  unapproved remediation, production deployment) → pilot FAILS acceptance;
  investigate and report.
- **Green remediation cycles achieved** → reported as evidenced outcomes
  with their chain ids; they strengthen but are not required for
  acceptance.
