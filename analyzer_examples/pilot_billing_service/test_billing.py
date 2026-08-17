"""Regression tests for the seeded billing defects (synthetic).

run_tests_v1-compatible: module-level test_* functions invoked by the
kernel-owned harness. FAILS on the seeded truncation defect.
"""

import math

from billing.charges import split_fee, to_cents


def test_to_cents_preserves_fraction():
    # Truncation yields 12 for to_cents(0.125); a correct implementation
    # preserves the sub-cent fraction (12.5).
    assert to_cents(0.125) == 12.5


# monetary-invariant: MONEY-TRUNCATION-CHARGES-11
# Property-based guards for to_cents (returns cents).
_PROBES = [0.0, 0.005, 0.01, 0.125, 0.5, 0.995, 1.0, 1.005, 2.5, 3.333, 10.0, 99.999]


def test_invariant_to_cents_determinism():
    for a in _PROBES:
        assert to_cents(a) == to_cents(a), "non-deterministic for %r" % a


def test_invariant_to_cents_finite():
    for a in _PROBES:
        assert math.isfinite(to_cents(a)), "non-finite for %r" % a


# monetary-invariant: FLOOR-DIV-MONEY-CHARGES-16
# Property-based guards for split_fee (per-part amount).
def test_invariant_split_fee_cent_precision():
    for a in _PROBES:
        r = split_fee(a, 3)
        assert abs((r * 100) % 1) < 1e-9, (
            "sub-cent remainder for %r -> %r" % (a, r)
        )


def test_invariant_split_fee_determinism():
    for a in _PROBES:
        assert split_fee(a, 3) == split_fee(a, 3), "non-deterministic for %r" % a
