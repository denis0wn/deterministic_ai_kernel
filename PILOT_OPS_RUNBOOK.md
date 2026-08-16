# Pilot Operations Runbook (v0.4-pilot-ops)

Operator workflow from read-only analysis to a verified evidence chain.
Every boundary crossing leaves a tamper-evident artifact. Nothing in
this runbook deploys anything to production.

## Roles and boundaries

| Boundary | Actor | Artifact | Proves |
|---|---|---|---|
| Analysis | analyzer (read-only) | evidence package (`evidence_manifest_v1`, findings, contracts) | what was scanned, what was found, with snapshot hashes |
| Approval | HUMAN | `review_decisions/<finding>.json` | who approved what, against which contract/snapshot hashes |
| Handoff | HUMAN carries the document | `work_order_v1.json` + `WORK_ORDER.md` | the exact contract + targets the attempt is scoped to |
| Remediation | HUMAN runs the executor in an ISOLATED copy | executor event log, pre/post files, `test_report_v1` | the attempt happened with kernel-owned evidence |
| Verification | analyzer_chain_verify (read-only) | `evidence_chain_v1.json` + `CHAIN_OF_CUSTODY.md` | the trail is hash-consistent end to end |

The analyzer never invokes the executor. The verifier never declares a
fix correct — only chain-consistent.

## Step 1 — Evidence package

```
cargo run --bin analyzer_pilot_report -- \
  --workspace <target_workspace> \
  --output analyzer_out/pilot \
  [--repro <FINDING_ID>=<workspace-relative-test.py> ...] \
  [--external-sast <report.json> ...]
```

## Step 2 — Human review decision

```
cargo run --bin analyzer_review -- \
  --package analyzer_out/pilot \
  --finding <FINDING_ID> \
  --decision approve|reject|defer \
  --reviewer "<name or role, free text>" \
  --rationale "<why>" \
  [--repro <workspace-relative-test.py>] \
  [--override-candidate-only] \
  --out analyzer_out/pilot
```

Rules enforced fail-closed:
- approving a `candidate_only` finding is refused without the explicit
  `--override-candidate-only` flag (the override is recorded in the
  artifact forever);
- tampered packages are refused (finding evidence hashes must match the
  manifest snapshot);
- `--repro` paths must exist in the snapshot inventory.

## Step 3 — Work order (passive document)

```
cargo run --bin analyzer_work_order -- \
  --package analyzer_out/pilot \
  --decision analyzer_out/pilot/review_decisions/<FINDING_ID>.json \
  --out analyzer_out/pilot
```

Refused unless the decision is `approve` and the package has not
drifted since the decision.

## Step 4 — Isolated executor attempt (manual)

1. Copy the target workspace to a fresh isolated directory (`pre-copy`:
   keep one untouched copy for verification).
2. Run the executor in the isolated copy with the work order's embedded
   TaskContract v0 JSON. The executor may honestly reject the task.
3. Preserve: the executor event log, the pre-copy, the post-copy, and
   the `test_report_v1` artifact.

## Step 5 — Chain verification

```
cargo run --bin analyzer_chain_verify -- \
  --package analyzer_out/pilot \
  --decision analyzer_out/pilot/review_decisions/<FINDING_ID>.json \
  --pre-dir <pre-copy> --post-dir <post-copy> \
  --test-report <test_report_v1.json> \
  [--event-log <event_log.json>] \
  --out analyzer_out/chain
```

Possible overall statuses and how to handle them:

- `chain_consistent_remediation_evidenced` — files changed and a real
  passing test report is structurally validated. Proceed to human
  retrospective. This still does NOT mean the fix is semantically
  correct — review it.
- `chain_consistent_no_change` — the executor rejected or no-op'ed.
  Honest outcome; decide next step (another finding, or close).
- `chain_incomplete` — mandatory evidence missing (e.g. no test report).
  Collect it or abort the attempt.
- `chain_inconsistent` — hashes or the test report do not line up.
  Treat as a failed attempt; investigate; do not retry blindly.

## After the attempt

Retrospective with the client: findings register delta, evidence chain,
what the executor rejected and why. No production deployment of
anything produced during the pilot.

## Real Negative Remediation Flow

A negative outcome is a NORMAL acceptance result when the system behaves
fail-safe. This is the canonical scenario, demonstrated for real on
2026-08-16 (see /tmp/dek_ai_matrix/NEGATIVE_REMEDIATION_ACCEPTANCE_RECORD.md):

1. Human review approves ONE controlled isolated attempt only — approval
   never generalizes to other findings, workspaces, or retries.
2. Operator creates a pristine executor workspace (fresh copy of the
   target; the original is never touched).
3. Operator saves pre-run hashes (the package manifest already anchors
   them; additionally keep an untouched pre-copy for the chain).
4. The executor receives the work order's contract and runs ONLY inside
   the authorized isolated workspace (`DAK_CODEFIX_WORKSPACE` or the
   payload workspace — nothing else is mutable).
5. An unauthorized or malformed patch path fails closed: no workspace
   authorized, hallucinated context, or invalid patch shape → rejection
   BEFORE any mutation.
6. A validly-shaped but semantically wrong patch is applied, and the REAL
   tests fail (non-zero exit, `tests_failed`). The validation gate
   refuses to record success («no fabricated success»).
7. Operator preserves ALL evidence of the failed attempt: event log,
   patch evidence (pre/context/replacement/post hashes), and the failed
   test status. Failed-attempt evidence is first-class; it is never
   discarded.
8. The chain verifier recomputes every hash from bytes over the preserved
   evidence.
9. The final status is NOT success: `chain_incomplete` (no passing test
   report exists) or `chain_inconsistent` (evidence disagrees), strictly
   per facts. `chain_incomplete` is never relabeled.
10. Operator runs the retrospective: boundary behavior, model patch
    quality, test outcome, and evidence-chain result are reported as
    separate items — never blended into one «attempt failed» line.
11. No repeat attempts without a separate, pre-registered model-quality
    experiment (see ANALYZER_ROADMAP.md).

### Attempt discipline rules

- Do NOT repeat seeds hunting for a green answer. Retries require a
  separately approved model-quality experiment with a pre-registered
  attempt budget.
- EVERY attempt is preserved (logs, artifacts, evidence chain input),
  including configuration mistakes and terminal failures.
- Publishing only successful attempts is prohibited; attempt lists are
  always complete.
- Repeated attempts must have a pre-declared limit before the first
  attempt of the series.
- Success rate is computed over ALL attempts, never over the best seed.
- A negative outcome with fail-safe behavior is a valid acceptance
  result; a green outcome with any skipped gate is not.
