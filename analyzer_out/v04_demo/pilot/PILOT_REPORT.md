# Deterministic Remediation Pilot — Evidence Report

Analyzer version: 0.4.0 · schema: evidence_manifest_v1 · findings: 3 · remediation-ready: 1

## 1. Scope and limitations

- Workspace: `/Users/denissmoliakov/projects/deterministic_ai_kernel_clean_2/analyzer_examples/pilot_fintech`
- Workspace snapshot BLAKE3: `3ad6d6316471f31333efc6cd9d1750d7b40c524d8d2fe906860b420298291a0d`
- Files inventoried: 5
- Ruleset: `python-fintech-rules/0.2.0` — rules: dangerous-eval, money-round-bare, money-truncation, none-arith, offbyone-range, todo-marker
- Run id (content hash): `6a0e39d486f06084e52f2841c38dcdc0bd1310bf35879b5fa0ee728461446b55`

**This is read-only candidate detection, not a guarantee of absence of
defects.** A finding is a candidate statement, not a proven defect.

Model hints (if enabled in future runs) are unverified: the model never
executes commands and never modifies code. This pilot run uses the
deterministic static pipeline only.

Supported scope: **Python-first demo/risk logic only.** Explicit
limitations:

- No multi-language coverage (JVM/Go/JS are backlog, not supported).
- No repo-scale semantic retrieval — pattern-level static rules only.
- Every finding must pass human review before any remediation attempt.
- The LLM used by the executor can still be wrong outside the kernel's
  gated classes; the executor's gates contain, not eliminate, that risk.
- Rules carry documented false-positive/false-negative profiles (see
  section 2 and docs/ANALYZER_RULES_CATALOG.md).

## 2. Findings

Findings are CANDIDATES with evidence, not proven defects. Each carries
its detector's known false-positive/false-negative profile.

### MONEY-TRUNCATION-LEDGER-15

- Candidate statement: money amount truncated via int() — sub-cent fractions lost
- Rule: `money-truncation` · Severity: Critical · Confidence: 0.70
- Detector: `static` (model_hint_unverified: false)
- Evidence: `billing/ledger.py:15-15` — file snapshot BLAKE3 `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985`
- Why it matters: sub-cent fractions are silently dropped; repeated over many transactions this becomes systematic money loss
- Remediation readiness: **remediation_ready** (manual review required: true)
- Analysis limitations:
  - false positives: int(x * 100) used for non-monetary scaling (percent formatting, basis points display)
  - false negatives: truncation via // operator, math.floor, format specs, or Decimal(int(...)) is not matched

### MONEY-ROUND-BARE-LEDGER-20

- Candidate statement: bare round() without explicit rounding policy — banker's-rounding surprises on money
- Rule: `money-round-bare` · Severity: Critical · Confidence: 0.70
- Detector: `static` (model_hint_unverified: false)
- Evidence: `billing/ledger.py:20-20` — file snapshot BLAKE3 `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985`
- Why it matters: rounding without a declared policy produces banker's-rounding surprises on amounts customers are charged or credited
- Remediation readiness: **candidate_only** (manual review required: true)
- Analysis limitations:
  - false positives: round() on non-monetary values inside a finance-named file
  - false negatives: rounding hidden in format()/f-strings, numpy, or Decimal local contexts is not matched

### TODO-MARKER-LEDGER-23

- Candidate statement: TODO/FIXME marker left in a money/risk/limit/fee/settlement path
- Rule: `todo-marker` · Severity: Low · Confidence: 0.30
- Detector: `static` (model_hint_unverified: false)
- Evidence: `billing/ledger.py:23-23` — file snapshot BLAKE3 `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985`
- Why it matters: unfinished logic on a money/risk path is unquantified operational risk
- Remediation readiness: **candidate_only** (manual review required: true)
- Analysis limitations:
  - false positives: informational comments mentioning TODO without unfinished work
  - false negatives: alternate spellings (To Do, XXX, HACK) are not matched

## 3. Task contracts

A task contract is a PROPOSAL for the executor, never a command to
change code. The executor applies its own context/test/validation gates
and may honestly reject any contract.
- `MONEY-TRUNCATION-LEDGER-15` — contract BLAKE3 `1e8ccfbf7777660b8bc4838188f060014701ef1f2c5dd323fc0ffcd1ce21d6d5` · readiness: remediation_ready · targets: billing/ledger.py
- `MONEY-ROUND-BARE-LEDGER-20` — contract BLAKE3 `fec6d57b1d57048447ff3665bffb9291584ddc4c00eb8147c0dc4a53a885bb2e` · readiness: candidate_only · targets: billing/ledger.py
- `TODO-MARKER-LEDGER-23` — contract BLAKE3 `8a477945ed9a5ba364a94755f32397c682bcc0358bcfdfae8b40c0efabeacc5b` · readiness: candidate_only · targets: billing/ledger.py

## 4. Executor evidence boundary

- The analyzer fixes NOTHING. It only inventories, detects and proposes.
- The executor (deterministic kernel) treats every finding as untrusted:
  it verifies `context_before` of each patch hunk against the actual
  file, accepts only structured `apply_patch_v1` patches, runs real
  allowlisted tests (`run_tests_v1`) and can fail-closed reject the task.
- LLM output inside the executor is untrusted input: no shell, no
  filesystem, no network effects from model text — only kernel-owned
  primitives.
- Only a separate executor run can produce the `remediated` status,
  backed by event log, pre/post BLAKE3 hashes and `test_report_v1`.

## 5. Next pilot step

1. Select ONE `remediation_ready` finding (requires an operator-provided
   reproducible failing test; otherwise the finding stays `candidate_only`).
2. Operator performs manual review of the finding and the proposed patch
   scope.
3. Run the executor in an ISOLATED workspace copy.
4. Preserve executor evidence: event log, patch pre/post BLAKE3 hashes,
   `test_report_v1`.
5. Independent re-verification outside the executor.

## 6. Commercially honest claim

> The system produces a reproducible evidence trail from a read-only
> finding to a controlled remediation. It does not guarantee the absence
> of defects and does not replace human review.
