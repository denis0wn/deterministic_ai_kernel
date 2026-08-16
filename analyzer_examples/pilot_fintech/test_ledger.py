"""Failing regression tests for the seeded pilot defects.

Compatible with the executor's run_tests_v1 harness (module-level test_*
functions are invoked by the kernel-owned runner). These tests FAIL on
the seeded code — that is exactly what makes the findings
reproducible when an operator submits this file via --repro.
"""

from billing.ledger import post_amount, settlement_round


def test_post_amount_keeps_sub_cent_fraction():
    # P1: 0.125 is exactly representable; truncation yields 0.12 while
    # explicit half-up rounding yields 0.13.
    assert post_amount(0.125) == 0.13


def test_settlement_round_is_not_bankers_rounding():
    # P2: policy-free round() uses banker's rounding: round(2.5) == 2.
    # With an explicit half-up policy the expected value is 3.
    assert settlement_round(2.5) == 3
