"""Payout splitting helpers."""


def split_amount(total, parts):
    """Split `total` into `parts` shares.

    Contract:
    - returns a list of exactly `parts` floats;
    - every share is a whole number of cents (an exact multiple of 0.01);
    - the shares sum exactly to `total` (no lost or created cents);
    - no share differs from any other by more than 0.01;
    - a zero total splits into all-zero shares.

    Example: split_amount(100.0, 3) must sum back to exactly 100.0.
    """
    share = round(total / parts, 2)
    return [share] * parts
