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


# monetary-invariant: MONEY-TRUNCATION-LEDGER-15
# Property-based guard for the post_amount truncation finding: money
# results must never carry a sub-cent remainder, for ALL probe values.
_PROBES = [0.0, 0.005, 0.01, 0.125, 0.5, 0.995, 1.0, 1.005, 2.5, 3.333, 10.0, 99.999]


def test_invariant_post_amount_cent_precision():
    for a in _PROBES:
        r = post_amount(a)
        assert abs(r * 100 - round(r * 100)) < 1e-9, (
            "sub-cent remainder for %r -> %r" % (a, r)
        )


def test_invariant_post_amount_determinism():
    for a in _PROBES:
        assert post_amount(a) == post_amount(a), "non-deterministic for %r" % a
