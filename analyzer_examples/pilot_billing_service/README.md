# pilot_billing_service — multi-file synthetic pilot fixture

A more realistic Python-first billing service for pilot demonstrations
(synthetic, no customer data). Seeded defects span several files and
rules; `billing/rates.py` is a SAFE NEGATIVE CONTROL that must produce
zero findings.

## Seeded defects

| File | Defect | Rule |
|---|---|---|
| billing/charges.py | `to_cents` truncates via `int(amount * 100)` | money-truncation |
| billing/charges.py | `split_fee` uses floor division `//` | floor-div-money |
| billing/settlement.py | `balance == 0.01` float equality | float-equality |
| billing/settlement.py | `record.get("fee")` then arithmetic (None risk) | none-arith |
| billing/limits.py | `range(1, len(tiers))` skips first tier | offbyone-range |
| billing/limits.py | `# TODO:` marker on a risk path | todo-marker |

## Negative control

`billing/rates.py` uses Decimal with explicit ROUND_HALF_UP policy and
defaulted `.get()`; no pilot rule should fire there.

## Known documented false positive

`float-equality` also fires in `test_billing.py` (the test asserts
`to_cents(0.125) == 12.5`). A float `==` inside a test assertion is a
legitimate test, not a money defect — this is the rule's documented
false-positive class (pattern rules cannot tell test assertions from
production comparisons). It is expected and illustrates why findings are
candidates requiring human review, not proven defects.

## Tests

`test_billing.py` fails on the seeded truncation defect
(`to_cents(0.125)` returns 12, the test expects 12.5), giving a
reproducible failing test for remediation.
