# Enterprise Pilot One-Pager — Deterministic Remediation & Audit-Trail Engine

**Positioning:** Deterministic remediation and audit-trail engine for
Python-first risk-critical systems.

This is **not** a finished "audit platform". Audit here is a *service* built
around three separated components: a **read-only analyzer** (candidate
detection), **human review**, and a **deterministic executor** (controlled,
evidence-producing remediation). Nothing claims to replace any of the three
with the other two.

## Who this is for

Python-first fintech / risk teams with critical calculation paths:
fees, limits, settlement, pricing, discounts — where a rounding or
boundary mistake is a money event, not a code smell.

## What the pilot demonstrates

A verifiable chain, not a promise:

```
workspace snapshot (BLAKE3) → read-only finding (evidence + limitations)
→ tamper-evident task contract → executor boundary (context-verified patch,
real allowlisted tests, fail-closed validation) → remediation evidence
(event log, pre/post hashes, test_report_v1)
```

Every artifact is content-hashed; the package is byte-reproducible for the
same input snapshot, analyzer version and rule set.

## Properties that already exist (demonstrated, not promised)

- **On-prem / local model** operation; the model is an untrusted input.
- **Kernel-owned effects only**: no shell, filesystem or network execution
  from LLM text; mutations go through `apply_patch_v1` (context-verified
  hunks) and `run_tests_v1` (allowlisted argv, real tests) exclusively.
- **Fail-closed validation**: hallucinated patch context is rejected before
  any change; failing or rigged tests cannot produce a "pass".
- **Evidence, not narrative**: BLAKE3 pre/post hashes, event log, replay,
  `test_report_v1`, grounding gates.
- **Tamper-evident analyzer trail**: `evidence_manifest_v1` with workspace
  snapshot hash, ruleset version, deterministic run id; no timestamps inside
  content-hashed artifacts.

## Honest limitations

- Model-layer: prose hallucinations outside gated classes and slow reasoning
  on some prompts remain possible; kernel gates contain them but do not
  eliminate them.
- Scope-layer: analyzer rules are pattern-level static heuristics with
  documented false-positive/false-negative profiles; findings are candidates,
  not proven defects; Python only in this phase.

## Proposed pilot format

- **2–4 weeks**, one isolated Python service or module.
- **Manual review is mandatory** for every finding before any remediation
  attempt.
- **No production deployments** during the pilot.
- Deliverables: findings register, evidence package, 1–3 selected
  remediation attempts with full executor evidence, final retrospective.

Pricing is a separate commercial document and is intentionally not included
here.
