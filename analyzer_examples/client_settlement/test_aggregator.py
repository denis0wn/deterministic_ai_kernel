"""Authoritative tests for the settlement fee aggregation.

Encode the contractual policy: sum the EXACT per-item fees, then round
HALF-UP to the cent ONCE. FAIL on the seeded accumulation defect.
run_tests_v1-compatible: module-level test_* functions.
"""

from settlement.aggregator import total_fees


def test_total_fees_rounds_once_half_up():
    # 3 items, each exact fee 0.125 -> exact total 0.375 -> HALF-UP 0.38.
    items = [
        {"amount": 1.0, "rate": 0.125},
        {"amount": 1.0, "rate": 0.125},
        {"amount": 1.0, "rate": 0.125},
    ]
    assert total_fees(items) == 0.38


def test_total_fees_single_item():
    # Single item: exact fee 0.125 -> HALF-UP 0.13. This case forces
    # HALF-UP specifically (banker's round(0.125, 2) would give 0.12).
    items = [{"amount": 1.0, "rate": 0.125}]
    assert total_fees(items) == 0.13


# monetary-invariant: MONEY-TRUNCATION-AGGREGATOR-22
# Property-based guards for total_fees (money-math).
_PROBES_AMOUNTS = [0.0, 0.125, 0.5, 1.0, 2.5, 10.0, 99.999]


def test_invariant_total_fees_cent_precision():
    for a in _PROBES_AMOUNTS:
        r = total_fees([{"amount": a, "rate": 0.125}])
        assert abs((r * 100) % 1) < 1e-9, (
            "sub-cent remainder for %r -> %r" % (a, r)
        )


def test_invariant_total_fees_determinism():
    for a in _PROBES_AMOUNTS:
        assert total_fees([{"amount": a, "rate": 0.125}]) == total_fees(
            [{"amount": a, "rate": 0.125}]
        ), "non-deterministic for %r" % a
