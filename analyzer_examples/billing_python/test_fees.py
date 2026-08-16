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
