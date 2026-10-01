# client_alloc — fixture with a seeded money-allocation defect

`ledger/allocate.py::split_amount` rounds every share independently
(`round(total / parts, 2)` repeated), so the shares do not sum back to the
total: splitting 100.00 into 3 yields 33.33 × 3 = 99.99. Contract: exactly
`parts` float shares, every share a whole number of cents, summing exactly
to `total`, no share differing from any other by more than 0.01 (one-cent
spread).

Suite history (same lesson as client_dateflow): the first revision did not
pin cent granularity, and a measured plain-arm patch passed it with SUB-CENT
shares (33.3333... each for a 3-way split — exact sum, impossible payout;
evidence: `analyzer_out/mq_alloc_plain_weak_2026-10-01`). The whole-cent
assertion was added to `_check` and as `test_shares_are_whole_cents`.

The suite is invariant-based (sum-exactness, one-cent spread, length, type),
deliberately policy-neutral: any allocation policy satisfying the contract
passes, so a correct cumulative or largest-remainder implementation is not
penalized. The spread invariant is what kills the "fix the last share"
shortcut once `parts` is large enough (12-way splits drift several cents).

Tests: `test_allocate.py` (kernel-harness compatible, module-level
`test_*`). Payload + series scripts live in `analyzer_out/` series dirs.
