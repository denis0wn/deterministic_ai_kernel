# Pilot Scope and Limitations

## In scope

- One isolated Python service/module chosen by the client.
- Read-only analyzer pass: snapshot inventory, static candidate detection,
  deterministic triage, task contract emission with readiness assessment.
- Operator-reviewed remediation attempts (1–3) executed by the deterministic
  executor inside an isolated workspace copy.
- Evidence package: `evidence_manifest_v1`, findings, task contracts,
  operational log, pilot report — all content-hashed and reproducible.

## Out of scope

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
