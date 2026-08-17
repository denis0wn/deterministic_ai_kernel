"""Regression tests for the seeded billing defects (synthetic).

run_tests_v1-compatible: module-level test_* functions invoked by the
kernel-owned harness. FAILS on the seeded truncation defect.
"""

from billing.charges import to_cents


def test_to_cents_preserves_fraction():
    # Truncation yields 12 for to_cents(0.125); a correct implementation
    # preserves the sub-cent fraction (12.5).
    assert to_cents(0.125) == 12.5
