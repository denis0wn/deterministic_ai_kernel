# Deterministic Remediation Pilot — Evidence Report

Analyzer version: 0.3.0 · schema: evidence_manifest_v1 · findings: 5 · remediation-ready: 0

## 1. Scope and limitations

- Workspace: `/Users/denissmoliakov/projects/deterministic_ai_kernel_clean_2/analyzer_examples/pilot_fintech`
- Workspace snapshot BLAKE3: `3ad6d6316471f31333efc6cd9d1750d7b40c524d8d2fe906860b420298291a0d`
- Files inventoried: 5
- Ruleset: `python-fintech-rules/0.2.0` — rules: dangerous-eval, money-round-bare, money-truncation, none-arith, offbyone-range, todo-marker
- Run id (content hash): `34b55ec86ee1c7f273f9fcc2e14203813126f781b973badb2af70d11a7f06b46`

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

External candidate sources (read-only, untrusted third-party reports):
- bandit — report BLAKE3 `97c729bf1b9a1af7efef86ff14e85cee37f39f8115ddf3fc355bf391ada77afa` — 1 candidates, 0 rejected paths
- semgrep — report BLAKE3 `44d8e5fec9952fb66900f055163e39876b0cfeebf97dc59d6a0e04ed796ea670` — 1 candidates, 1 rejected paths
External findings are capped below static confidence and never reach Critical on tool severity alone.

## 2. Findings

Findings are CANDIDATES with evidence, not proven defects. Each carries
its detector's known false-positive/false-negative profile.

### MONEY-TRUNCATION-LEDGER-15

- Candidate statement: money amount truncated via int() — sub-cent fractions lost
- Rule: `money-truncation` · Severity: Critical · Confidence: 0.70
- Detector: `static` (model_hint_unverified: false)
- Evidence: `billing/ledger.py:15-15` — file snapshot BLAKE3 `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985`
- Why it matters: sub-cent fractions are silently dropped; repeated over many transactions this becomes systematic money loss
- Remediation readiness: **candidate_only** (manual review required: true)
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

### SEMGREP:PYTHON.FINTECH.MONEY-TRUNCATION-INT-LEDGER-15

- Candidate statement: int() truncates monetary amounts toward zero; sub-cent fractions are lost
- Rule: `semgrep:python.fintech.money-truncation-int` · Severity: High · Confidence: 0.55
- Detector: `external` (model_hint_unverified: false)
- Evidence: `billing/ledger.py:15-15` — file snapshot BLAKE3 `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985`
- Why it matters: see rule catalog
- Remediation readiness: **candidate_only** (manual review required: true)
- Analysis limitations:
  - external SAST finding: untrusted third-party tool output, not reproduced by this analyzer
  - severity/confidence are tool-reported and conservatively capped; external sources never reach Critical here

### BANDIT:B700-LEDGER-20

- Candidate statement: round() without an explicit rounding policy on a monetary path
- Rule: `bandit:B700` · Severity: Medium · Confidence: 0.45
- Detector: `external` (model_hint_unverified: false)
- Evidence: `billing/ledger.py:20-20` — file snapshot BLAKE3 `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985`
- Why it matters: see rule catalog
- Remediation readiness: **candidate_only** (manual review required: true)
- Analysis limitations:
  - external SAST finding: untrusted third-party tool output, not reproduced by this analyzer
  - severity/confidence are tool-reported and conservatively capped; external sources never reach Critical here

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
- `MONEY-TRUNCATION-LEDGER-15` — contract BLAKE3 `476ffae5918d547c5a866d31369d165a45f0f7d63771da0abb53fb47ac9bce75` · readiness: candidate_only · targets: billing/ledger.py
- `MONEY-ROUND-BARE-LEDGER-20` — contract BLAKE3 `f693ba33b6bb7cc4e76c30b3332261a71d04d88bc09d80c4c7f05a555d3895c4` · readiness: candidate_only · targets: billing/ledger.py
- `SEMGREP:PYTHON.FINTECH.MONEY-TRUNCATION-INT-LEDGER-15` — contract BLAKE3 `147e7002824ca7e1935ecce5b968ac34d50a50b54c198cd1b9ef2bfac006cba7` · readiness: candidate_only · targets: billing/ledger.py
- `BANDIT:B700-LEDGER-20` — contract BLAKE3 `2c0fd0bad2bea792e2f15c52c86a336b3c0092900350c2309e0aaf0114ee2ef1` · readiness: candidate_only · targets: billing/ledger.py
- `TODO-MARKER-LEDGER-23` — contract BLAKE3 `0f86b037257f7e4414c331cc8d70d3b0eba30ff0844e224a5c5fd9afee71a8b3` · readiness: candidate_only · targets: billing/ledger.py

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
