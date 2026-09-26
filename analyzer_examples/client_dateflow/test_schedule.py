# run_tests_v1-compatible: module-level test_* functions invoked by the
# kernel harness (no pytest needed).
from datetime import date

from billing.schedule import add_business_days


def test_friday_plus_one_is_monday():
    assert add_business_days(date(2026, 9, 25), 1) == date(2026, 9, 28)


def test_wednesday_plus_two_is_friday():
    assert add_business_days(date(2026, 9, 23), 2) == date(2026, 9, 25)


def test_thursday_plus_one_is_friday():
    assert add_business_days(date(2026, 9, 24), 1) == date(2026, 9, 25)


def test_monday_plus_five_is_next_monday():
    assert add_business_days(date(2026, 9, 21), 5) == date(2026, 9, 28)


def test_zero_days_is_same_day():
    assert add_business_days(date(2026, 9, 23), 0) == date(2026, 9, 23)


# Weekend-START cases — added after a measured plain-arm patch passed every
# original test while being wrong for Saturday starts (evidence:
# analyzer_out/mq_dateflow_plain_2026-09-26). Tests-as-judge is only as
# strong as the suite.
def test_saturday_plus_one_is_monday():
    assert add_business_days(date(2026, 9, 26), 1) == date(2026, 9, 28)


def test_sunday_plus_two_is_tuesday():
    assert add_business_days(date(2026, 9, 27), 2) == date(2026, 9, 29)


def test_invariant_never_lands_on_weekend():
    for y, m, d in [(2026, 9, 21), (2026, 9, 25), (2026, 9, 26), (2026, 12, 31)]:
        for n in (1, 2, 3, 7, 10):
            result = add_business_days(date(y, m, d), n)
            assert result.weekday() < 5, f"{y}-{m}-{d} + {n} -> {result} is a weekend"
