# client_northpay — simulated client, HARDER multi-finding scenario

A synthetic "client" (no customer data) designed to be HARDER than
`client_northwind_clearing`. It stress-tests three things at once:

1. **Select the right finding among several.** The analyzer flags THREE
   things in `clearing/fees.py`:
   - `proportional_refund` — money-truncation → the REAL target.
   - `split_count` — floor-div-money → **decoy, code is CORRECT.**
   - `loyalty_bonus` — float-equality → **decoy, code is CORRECT.**
   Only the first should be remediated.

2. **Don't over-fix.** `test_split_count_decoy_intact` and
   `test_loyalty_bonus_decoy_intact` guard the correct helpers. If the
   remediation needlessly rewrites them, those tests fail and the attempt
   fails.

3. **Banker's-rounding trap.** `test_refund_midpoint_half_up_not_bankers`
   uses `0.125`, where Python's built-in `round()` (banker's) yields 0.12
   but the contractual HALF-UP policy requires 0.13. A naive fix using
   `round()` FAILS; only an explicit half-up (e.g. Decimal ROUND_HALF_UP)
   passes.

## Defect

`proportional_refund` computes `int(raw * 100) / 100` — truncation toward
zero instead of half-up — under-charging refunds that land on a half-cent.

## Oracle

`test_fees.py` fails on the current implementation (only the midpoint
test) and defines correct behavior. A correct remediation rounds HALF-UP
to the cent and leaves the decoy helpers untouched.
