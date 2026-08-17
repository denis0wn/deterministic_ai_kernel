"""Authoritative tests for the NorthPay fee & refund engine.

Encode the contractual refund policy (HALF-UP to the cent) and guard the
correct helper functions against unnecessary "fixes". run_tests_v1-
compatible: module-level test_* functions invoked by the kernel harness.
"""

from clearing.fees import loyalty_bonus, proportional_refund, split_count


def test_refund_basic_third():
    # 10.0 * (1/3) = 3.3333... -> 3.33 under any sane rounding.
    assert proportional_refund(10.0, 1, 3) == 3.33


def test_refund_midpoint_half_up_not_bankers():
    # 1.0 * (1/8) = 0.125 EXACTLY. HALF-UP -> 0.13.
    # Python's built-in round() uses banker's rounding and would give
    # 0.12 here, so a naive round()-based "fix" must FAIL this test.
    assert proportional_refund(1.0, 1, 8) == 0.13


def test_refund_full_share():
    # Full share refunds the whole fee exactly.
    assert proportional_refund(5.0, 4, 4) == 5.0


def test_split_count_decoy_intact():
    # Floor division is the CORRECT behavior; a "fix" that changes it
    # breaks this guard.
    assert split_count(10, 3) == 3
    assert split_count(9, 3) == 3


def test_loyalty_bonus_decoy_intact():
    # The zero sentinel comparison is correct; do not alter it.
    assert loyalty_bonus(0.0) == 1.0
    assert loyalty_bonus(50.0) == 1.25
