# Remediation Work Order (MONEY-TRUNCATION-LEDGER-15)

- Work order id (content hash): `12bd44ee05dca953c4212f184ab6218d7fc99b0353562c08a5460f23e774d2df`
- Approved via decision id: `e7332f36ee9fd870ecb10d9cb2391b540fd1a29436145e7cda07de9bb9a4bb39`
- Contract BLAKE3: `1e8ccfbf7777660b8bc4838188f060014701ef1f2c5dd323fc0ffcd1ce21d6d5`
- Workspace snapshot BLAKE3: `3ad6d6316471f31333efc6cd9d1750d7b40c524d8d2fe906860b420298291a0d`
- Reproducible test: test_ledger.py

This work order is a passive document. It executes nothing, invokes no executor, and does not modify any workspace.

## Target files (snapshot hashes must match before the attempt)

- `billing/ledger.py` — BLAKE3 `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985`

## Operator checklist

1. Copy the target workspace to a fresh ISOLATED directory. Never run the executor on the original workspace.
2. Hand the embedded TaskContract v0 JSON to the executor run in that isolated copy (manual pipeline-run; this document invokes nothing).
3. The executor may honestly reject the contract (hallucinated context, failing validation). A rejection is a valid outcome.
4. Preserve executor evidence: event log, BLAKE3 hashes of every target file BEFORE and AFTER the attempt, and test_report_v1.
5. Bring the isolated pre-copy, post-copy and evidence files back and run analyzer_chain_verify.
6. No production deployment of anything produced by the attempt.

## TaskContract v0 (verbatim, executor input)

```json
{"task_kind":"codefix","workspace":"/Users/denissmoliakov/projects/deterministic_ai_kernel_clean_2/analyzer_examples/pilot_fintech","target_files":["billing/ledger.py"],"finding":{"id":"MONEY-TRUNCATION-LEDGER-15","severity":"critical","description":"money amount truncated via int() — sub-cent fractions lost","evidence":["billing/ledger.py:15-15"]},"codefix_steps":["Step 1 read repository /Users/denissmoliakov/projects/deterministic_ai_kernel_clean_2/analyzer_examples/pilot_fintech/billing/ledger.py","Step 2 find bug","Step 3 patch code","Step 4 apply patch","Step 5 run tests","Step 6 validate patch"],"tests_contract":"existing module tests + one new test reproducing this finding","provenance":{"detector":"static","confidence":0.7,"analyzer_version":"0.4.0"}}
```
