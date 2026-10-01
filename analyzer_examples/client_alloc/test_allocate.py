# run_tests_v1-compatible: module-level test_* functions invoked by the
# kernel harness (no pytest needed).
import math

from ledger.allocate import split_amount

# (total, parts) cases: indivisible cents, one-cent totals, large part
# counts (where "fix the last share" drifts several cents), zero total,
# and the trivial single share.
CASES = [
    (100.0, 3),
    (10.0, 3),
    (1.0, 7),
    (0.05, 2),
    (0.03, 5),
    (999.99, 12),
    (123.45, 1),
    (50.0, 8),
    (0.0, 5),
]


def _check(total, parts):
    shares = split_amount(total, parts)
    assert isinstance(shares, list), f"expected list, got {type(shares)}"
    assert len(shares) == parts, f"expected {parts} shares, got {len(shares)}"
    assert all(isinstance(s, float) for s in shares), (
        f"every share must be float, got {shares!r}"
    )
    assert all(s >= 0.0 for s in shares), f"negative share in {shares!r}"
    assert round(math.fsum(shares), 2) == total, (
        f"shares {shares!r} sum to {math.fsum(shares)}, expected exactly {total}"
    )
    spread = max(shares) - min(shares)
    assert spread <= 0.01 + 1e-9, (
        f"spread {spread:.4f} exceeds one cent for ({total}, {parts}): {shares!r}"
    )
    # Whole-cent granularity — added after a measured plain-arm patch passed
    # the original suite with SUB-CENT shares (33.3333... for a 3-way split;
    # evidence: analyzer_out/mq_alloc_plain_weak_2026-10-01). Money that
    # cannot be paid is not a solution. Tests-as-judge is only as strong as
    # the suite.
    for s in shares:
        assert abs(s * 100 - round(s * 100)) < 1e-6, (
            f"share {s!r} is not a whole number of cents for ({total}, {parts})"
        )


def test_indivisible_three_way_sums_exactly():
    _check(100.0, 3)


def test_small_amount_three_way():
    _check(10.0, 3)


def test_one_dollar_seven_ways():
    _check(1.0, 7)


def test_five_cents_two_ways():
    _check(0.05, 2)


def test_three_cents_five_ways_has_zero_shares():
    # More parts than cents: some shares must be 0.0, still summing exactly.
    _check(0.03, 5)


def test_twelve_way_split_keeps_one_cent_spread():
    # Large part counts expose last-share-correction drift (several cents).
    _check(999.99, 12)


def test_single_share_is_total():
    shares = split_amount(123.45, 1)
    assert shares == [123.45], f"single share must equal total, got {shares!r}"


def test_even_split_unaffected():
    _check(50.0, 8)


def test_zero_total_all_zero_shares():
    shares = split_amount(0.0, 5)
    assert shares == [0.0] * 5, f"zero total must give zero shares, got {shares!r}"


def test_shares_are_whole_cents():
    # Hole-closer (see note in _check): sub-cent shares such as 1/3 cent each
    # sum back exactly but cannot be paid. Every share must be whole cents.
    for total, parts in [(100.0, 3), (0.05, 7), (1.0, 3)]:
        _check(total, parts)


def test_invariant_sum_exact_over_grid():
    totals = [0.01, 0.07, 0.99, 3.33, 17.97, 250.0, 888.88]
    for total in totals:
        for parts in (2, 3, 4, 6, 9, 12):
            shares = split_amount(total, parts)
            assert round(math.fsum(shares), 2) == total, (
                f"({total}, {parts}): shares {shares!r} do not sum to total"
            )
            assert max(shares) - min(shares) <= 0.01 + 1e-9, (
                f"({total}, {parts}): spread exceeds one cent: {shares!r}"
            )
