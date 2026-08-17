# PILOT STATEMENT OF WORK — TEMPLATE

> Deterministic Remediation & Audit-Trail Pilot
> Python-first risk-critical systems
>
> This is a fill-in template. Bracketed items are completed per
> engagement. Nothing here is a guarantee; see §6 (What is NOT provided).

---

## 1. Parties

- **Provider:** [Provider legal name] ("Provider")
- **Client:** [Client legal name] ("Client")
- **Effective date:** [date] · **Pilot window:** [2–4 weeks], from [start] to [end]

## 2. Objective

Demonstrate, on ONE isolated Client Python module, a verifiable chain from
read-only defect candidate to controlled, evidence-producing remediation:

```
workspace snapshot (BLAKE3) → read-only finding → human review →
tamper-evident task contract → executor boundary (context-verified patch,
real allowlisted tests, fail-closed validation) → remediation evidence
(event log, pre/post hashes, test_report_v1) → independent chain check
```

The pilot's value is the **evidence trail and the fail-safe behavior**, not
a promise that every finding is fixed.

## 3. Scope

**In scope:**
- One isolated Python module/service selected by Client: [module name].
- Read-only static analysis; deterministic findings with evidence and
  documented false-positive/false-negative profiles.
- Human-reviewed remediation attempts on up to [1–3] selected findings.
- Deterministic evidence package per attempt (hashes, event log, test
  report, chain-of-custody report).
- Final pilot report and retrospective.

**Out of scope:**
- Any language other than Python.
- Whole-codebase semantic analysis or repo-scale retrieval.
- Production deployment of anything produced during the pilot.
- Automated remediation without human approval.
- Compliance, legal, or security certification of any kind.
- Fixing findings the executor honestly rejects or the human reviewer
  declines.

## 4. Environment & data handling

- On-prem / local-model operation; Client code does not leave the agreed
  environment. [Specify environment.]
- No Customer/Private data is required beyond the target module itself.
- The analyzer is read-only toward the Client workspace; mutations occur
  only in an isolated copy through the kernel-owned boundary.

## 5. Deliverables

1. **Findings register** — deterministic candidates with evidence,
   severity, confidence, and limitation notes.
2. **Evidence package** — per remediation attempt: evidence manifest,
   task contract, pre/post BLAKE3 hashes, event log, `test_report_v1`,
   chain-of-custody report.
3. **Remediation attempts** — up to [1–3], each either a documented
   green cycle or an honest, evidenced rejection/failure.
4. **Final pilot report & retrospective** — what was demonstrated, what
   was not, and recommended next steps.

## 6. What is NOT provided (explicit)

- No guarantee that defects are found, or that all findings are fixed.
- No certification or attestation (security, compliance, financial).
- No "AI guarantees correctness." Remediation success is evidenced per
  attempt by real tests and a hash-verified chain, never asserted.
- No production-ready system; the pilot is an evaluation engagement.
- Model limitations are disclosed (prose hallucination outside gated
  classes; findings are candidates, not proofs).

## 7. Roles & responsibilities

- **Client:** selects the module and findings to attempt; performs or
  delegates human review; provides environment access; accepts results.
- **Provider:** operates analyzer + executor; preserves evidence; never
  bypasses the human-review gate; reports honest failures, including
  executor rejections and model mistakes.
- **Human approval gate:** no remediation attempt starts without a named
  human approving the specific finding and patch scope.

## 8. Acceptance

Pilot acceptance is against the measurable criteria in
`PILOT_ACCEPTANCE_CRITERIA.md` (evidence integrity, fail-safe behavior,
process completeness) — NOT against "number of bugs fixed."

## 9. Commercials

- Pricing, payment terms, and any liability cap are set in a separate
  commercial agreement and intentionally excluded here.
- No production deployment rights are granted by this pilot.

## 10. Term & termination

- The pilot runs for the stated window. Either party may end it early;
  evidence produced to date is delivered as-is.

## Signatures

| Role | Name | Title | Date | Signature |
|---|---|---|---|---|
| Provider | [ ] | [ ] | [ ] | [ ] |
| Client | [ ] | [ ] | [ ] | [ ] |
