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
