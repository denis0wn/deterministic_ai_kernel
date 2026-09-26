"""Review score aggregation."""


def average_score(scores):
    """Average of the non-None scores, as float.

    None entries are skipped. Empty input (or all-None) returns 0.0.
    Examples: [1.0, None, 3.0] -> 2.0; [] -> 0.0; [None] -> 0.0.
    """
    return sum(scores) / len(scores)
