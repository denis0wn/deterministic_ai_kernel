"""Unit tests asserting CORRECT behavior — they FAIL on the seeded defects.

Compatible with the executor's run_tests_v1 harness (module-level test_*
functions are invoked by the kernel-owned runner).
"""

from billing.fees import apply_discount, compute_fee


def test_first_tier_fee_is_charged():
    # D2 regression: first tier must not be skipped.
    assert compute_fee(500) == 10.0


def test_fee_rounding_is_not_truncated():
    # D1 regression: 20 + 33.33*0.015 = 20.49995 -> correct rounding 20.5,
    # truncation yields 20.49.
    assert compute_fee(1033.33) == 20.50


def test_missing_discount_defaults_to_zero():
    # D3 regression: absent discount must not crash and must mean 0.
    assert apply_discount(100.0, {"name": "cust-1"}) == 100.0


# monetary-invariant: MONEY-TRUNCATION-FEES-29
# Property-based guards for compute_fee (money-math).
_PROBES = [0.0, 0.005, 0.01, 0.125, 0.5, 0.995, 1.0, 1.005, 2.5, 3.333, 10.0, 99.999]


def test_invariant_compute_fee_cent_precision():
    for a in _PROBES:
        r = compute_fee(a)
        assert abs((r * 100) % 1) < 1e-9, (
            "sub-cent remainder for %r -> %r" % (a, r)
        )


def test_invariant_compute_fee_determinism():
    for a in _PROBES:
        assert compute_fee(a) == compute_fee(a), "non-deterministic for %r" % a
