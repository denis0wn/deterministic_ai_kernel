# Pilot Scope and Limitations

## In scope

- One isolated Python service/module chosen by the client.
- Read-only analyzer pass: snapshot inventory, static candidate detection,
  deterministic triage, task contract emission with readiness assessment.
- Operator-reviewed remediation attempts (1–3) executed by the deterministic
  executor inside an isolated workspace copy.
- Evidence package: `evidence_manifest_v1`, findings, task contracts,
  operational log, pilot report — all content-hashed and reproducible.

## Review gate and evidence chain (v0.4-pilot-ops)

A finding crosses to remediation only through a recorded human decision
(`review_decision_v1`): approving a `candidate_only` finding requires an
explicit, permanently recorded override. The handoff is a passive work
order document; the analyzer never invokes the executor. After an
isolated attempt, `analyzer_chain_verify` recomputes every hash from
bytes and structurally validates the executor's `test_report_v1`.

**Boundary of verification:** a consistent evidence chain proves the
trail is hash-coherent end to end. It does NOT prove the remediation is
semantically correct — correctness rests on the real tests and human
review. Chain statuses are exhaustive: `chain_consistent_remediation_
evidenced`, `chain_consistent_no_change`, `chain_inconsistent`,
`chain_incomplete`.

## External candidate sources (v0.3)

Pre-produced Semgrep/Bandit JSON reports may be ingested as an
additional read-only candidate source (`--external-sast`). The analyzer
never executes those tools. External findings are untrusted third-party
claims: capped below static confidence, never Critical on tool severity
alone, recorded in the evidence manifest with the report's BLAKE3 hash,
and subject to the same readiness gates and executor verification as
static findings.

## Out of scope

- Running third-party SAST tools (operators run them; we ingest reports).
- Any other language (JVM/Go/JS are backlog; do not plan work against them).
- Whole-codebase semantic analysis or repo-scale retrieval.
- Production deployment of anything during the pilot.
- Automatic remediation without human approval.
- Compliance, legal or security certification of any kind.

## Responsibilities

**Client:** selects the target module; provides workspace access on-prem;
performs manual review of every finding; decides which findings (if any)
proceed to remediation; owns acceptance of results.

**Operator:** runs analyzer and executor; preserves evidence artifacts;
never bypasses the manual review gate; reports honest failures, including
executor rejections and model mistakes.

## Human approval gate

No remediation attempt starts without a named human approving the specific
finding and patch scope. The analyzer cannot promote a finding to
`remediation_ready` without an operator-provided reproducible test, and even
`remediation_ready` keeps `manual_review_required: true`.

## No-production-deployment rule

All remediation attempts run on isolated copies. Nothing produced during the
pilot is deployed to production by either party.

## Privacy / on-prem assumptions

The stack runs locally/on-prem with a local model server; code samples leave
the client infrastructure only if the client explicitly decides so. **No
compliance certifications are claimed** (no SOC2/ISO/PCI attestation is
implied or offered by this document).

## Legal disclaimer

This pilot is an engineering evaluation. It is **not** legal advice,
financial advice, or a security certification. Absence of findings is not
proof of absence of defects.

## Model residuals (disclosed)

- Prose hallucinations outside the kernel's gated classes can occur in
  model output; kernel gates catch the effect-bearing classes but do not
  eliminate model error entirely.
- Slow reasoning on individual prompts is possible (model/server layer).

## Language scope

**Python only** in the current pilot. Claims about any other language are
out of scope and must not be made from this pilot's results.
