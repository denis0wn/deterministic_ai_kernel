# PILOT DELIVERY RUNBOOK

> Operator-facing process for delivering a Deterministic Remediation pilot
> to a Client. Complements `PILOT_OPS_RUNBOOK.md` (tool operation) — this
> document is the ENGAGEMENT lifecycle. Every step preserves the invariants:
> human review mandatory, no production deployment, evidence over claims.

## Phase 0 — Qualification & scoping (before signing)

1. Confirm the target is a **Python-first, risk-critical** module (fees,
   limits, settlement, pricing, discounts). Decline non-Python or
   non-isolated targets.
2. Confirm an **isolated environment** is available (on-prem/local model;
   Client code does not leave it).
3. Agree the module, the pilot window (2–4 weeks), and the number of
   remediation attempts (1–3). Fill `PILOT_SOW_TEMPLATE.md`.
4. Set expectations explicitly: the pilot demonstrates the evidence trail
   and fail-safe behavior, not a fix-count guarantee.

## Phase 1 — Baseline evidence package

1. Take a read-only snapshot of the isolated module
   (`analyzer_pilot_report`). Record the workspace BLAKE3 snapshot hash.
2. Produce the findings register. For each finding: evidence, severity,
   confidence, and limitation notes.
3. Deliver the findings register to the Client. **Findings are candidates,
   not proven defects** — say so.

## Phase 2 — Human review gate

1. The Client (or delegated reviewer) reviews each finding.
2. For each finding to attempt: record an **approve** decision via
   `analyzer_review` with reviewer + rationale. `candidate_only` findings
   need an operator-supplied reproducible test or an explicit, recorded
   override.
3. Record **reject**/**defer** for findings not attempted. Every finding
   gets a disposition; none silently dropped.

## Phase 3 — Remediation attempts (isolated)

For each approved finding (see `PILOT_OPS_RUNBOOK.md` for tool detail):
1. Restore/confirm the isolated workspace is pristine.
2. Generate the work order (`analyzer_work_order`) — passive document.
3. Run the executor in the isolated copy with the kernel-owned boundary.
4. Capture: event log, pre/post BLAKE3 hashes, `test_report_v1`, the
   applied patch (or the rejection reason).
5. **Accept any honest outcome**: a green cycle, a fail-closed rejection,
   or a tests-failed attempt are all valid results. Never relabel.

## Phase 4 — Independent chain verification

1. For each attempt, run `analyzer_chain_verify` over the evidence.
2. Record the overall status per attempt:
   `chain_consistent_remediation_evidenced` /
   `chain_consistent_no_change` / `chain_inconsistent` /
   `chain_incomplete`.
3. Re-run the post-state assertions OUTSIDE the executor where possible
   (independent verification).

## Phase 5 — Final pilot report & retrospective

1. Assemble the **pilot delivery report** (see
   `PILOT_DELIVERY_REPORT_EXAMPLE.md`): scope, findings register,
   per-attempt evidence + chain status, honest limitations, next steps.
2. Retrospective with the Client: what was demonstrated, what was not,
   model/scope limitations, and recommended next steps.
3. State the commercially honest claim (from the one-pager); never the
   forbidden claims (no guarantees, no certification, no "fixes all bugs").

## Cross-cutting rules

- **Human approval gate** before every remediation attempt.
- **No production deployment** of anything produced.
- **Publish all attempts** — successes and failures alike.
- **Evidence integrity**: every artifact BLAKE3-hashed; the package is
  byte-reproducible for the same snapshot + analyzer version.
- **Escalate honestly**: if the executor rejects or the model fails, report
  it with evidence; do not retry-until-green.
