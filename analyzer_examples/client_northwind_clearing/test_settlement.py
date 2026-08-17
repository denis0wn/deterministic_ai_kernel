"""Authoritative tests for the Northwind settlement fee schedule.

These encode the desk's contractual rounding policy (HALF-UP to the
cent). They FAIL on the current truncating implementation and define the
correct behavior the remediation must achieve. run_tests_v1-compatible:
module-level test_* functions invoked by the kernel-owned harness.
"""

from clearing.settlement import batch_settlement, settlement_fee


def test_fee_half_up_at_midpoint():
    # 100.0 * 0.01005 = 1.005 exactly -> HALF-UP to cents = 1.01.
    # Truncation toward zero yields 1.00, which under-charges.
    assert settlement_fee(100.0, 0.01005) == 1.01


def test_fee_exact_amounts_unchanged():
    # 250.0 * 0.02 = 5.00 exactly; no rounding ambiguity.
    assert settlement_fee(250.0, 0.02) == 5.0


def test_batch_settlement_matches_schedule():
    txns = [
        {"gross": 100.0, "rate": 0.01005},
        {"gross": 250.0, "rate": 0.02},
    ]
    # net = (100.0 - 1.01) + (250.0 - 5.00) = 98.99 + 245.00 = 343.99
    assert batch_settlement(txns) == 343.99


# monetary-invariant: MONEY-TRUNCATION-SETTLEMENT-28
# Property-based guards for settlement_fee (HALF-UP to the cent).
_PROBES = [0.0, 0.005, 0.01, 0.125, 0.5, 0.995, 1.0, 1.005, 2.5, 3.333, 10.0, 99.999]


def test_invariant_settlement_fee_cent_precision():
    for a in _PROBES:
        r = settlement_fee(a, 0.01005)
        assert abs((r * 100) % 1) < 1e-9, (
            "sub-cent remainder for %r -> %r" % (a, r)
        )


def test_invariant_settlement_fee_determinism():
    for a in _PROBES:
        assert settlement_fee(a, 0.01005) == settlement_fee(a, 0.01005), (
            "non-deterministic for %r" % a
        )
