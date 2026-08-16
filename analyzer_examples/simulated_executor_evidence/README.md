# simulated_executor_evidence — SIMULATED, not a real executor run

**⚠ This directory is hand-authored test data for the chain verifier
(`analyzer_chain_verify`). The real executor was NOT run to produce it.
Never present these files as actual executor evidence.**

Contents:

- `pre/billing/ledger.py` — byte-exact copy of
  `analyzer_examples/pilot_fintech/billing/ledger.py` (matches the pilot
  snapshot hashes).
- `post/billing/ledger.py` — simulated remediation of ONLY the
  MONEY-TRUNCATION-LEDGER-15 contract (explicit Decimal half-up policy);
  P2/P3 deliberately untouched.
- `test_report_v1.json` — simulated PASSING report in the executor's real
  `TestReportV1` schema (field names taken read-only from
  `src/tools/test_runner.rs` in the executor repo).
- `failing_report.json` — same schema, `tests_failed` — for
  fail-closed/negative tests.
- `event_log_sample.json` — optional-link demo data (presence only; the
  verifier does not interpret contents).

Purpose: prove that `evidence_chain` correctly distinguishes
`chain_consistent_remediation_evidenced`, `chain_consistent_no_change`,
`chain_inconsistent` and `chain_incomplete` without running any executor
or LLM.
