# run_tests_v1-compatible: module-level test_* functions invoked by the
# kernel harness (no pytest needed).
from reports.scores import average_score


def test_skips_none_entries():
    assert average_score([1.0, None, 3.0]) == 2.0


def test_empty_list_is_zero():
    assert average_score([]) == 0.0


def test_all_none_is_zero():
    assert average_score([None, None]) == 0.0


def test_single_value():
    assert average_score([4.0]) == 4.0


def test_returns_float():
    result = average_score([1, 2, 3])
    assert isinstance(result, float)


def test_invariant_no_crash_on_none_mix():
    for scores in ([None], [None, 1.0], [2.0, None, 4.0, None], []):
        result = average_score(scores)
        assert isinstance(result, float), f"crashed or wrong type for {scores}"
