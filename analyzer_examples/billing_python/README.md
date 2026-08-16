# billing_python — toy repository for analyzer training

Minimal Python billing module with THREE seeded defects (billing/fees.py):
D1 money truncation via int(), D2 off-by-one in the tier walk, D3 unhandled
None discount. `test_fees.py` asserts the CORRECT behavior, so all three
tests fail until the defects are fixed.

Intended flow (ROLES.md contract):
1. analyzer (this repo) scans the module and emits TaskContracts;
2. executor (deterministic_ai_kernel_clean) consumes a codefix task and
   fixes ONE defect per task via apply_patch_v1 + run_tests_v1 + gates.

Note for humans: defects are described in the fees.py docstring for
training transparency; the analyzer itself must not need that docstring —
its detectors work from code patterns only.
