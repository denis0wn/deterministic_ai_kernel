# Chain of Custody — Evidence Verification

- Chain id (content hash): `401f0ff6564d7b49113637cf1db9b4dc3f64d56b4631a48ce932d2745cf1f2a0`
- Decision id: `e7332f36ee9fd870ecb10d9cb2391b540fd1a29436145e7cda07de9bb9a4bb39`
- Contract BLAKE3: `1e8ccfbf7777660b8bc4838188f060014701ef1f2c5dd323fc0ffcd1ce21d6d5`
- Overall status: **chain_inconsistent**

Chain consistency is NOT proof that the remediation is correct. Correctness is established by the real tests and human review; this verifier only cross-checks hashes and structure.

| Link | Status | Detail |
|---|---|---|
| contract_integrity | verified | decision matches the current package (manifest, snapshot, contract) |
| pre_state_matches_snapshot | verified | 1 target file(s) match the snapshot |
| post_state_present_and_changed | verified | at least one target file differs from pre-state |
| test_report_passed | mismatch | classification is 'tests_failed', not tests_passed |

## Target files (recomputed hashes)

| Path | Snapshot | Pre | Post | Changed |
|---|---|---|---|---|
| `billing/ledger.py` | `e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985` | e92c388f131d4103238b25aa328ff484feabd470661a3dc57bbade93384b1985 | 159e3a7813b09d1e4c4e5ddd1f9a67d1a86477905658f833ddc982521d4cb9b4 | yes |

All hashes above were recomputed by this verifier from file bytes. No executor self-reported status was accepted without structural validation.
