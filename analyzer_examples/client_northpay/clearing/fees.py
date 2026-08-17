"""NorthPay — fee & refund engine (SYNTHETIC client module).

Client problem statement (NorthPay finance operations):

    "Partial refunds are coming out slightly LOWER than the customer's
    proportional share of the fee. Customers notice whenever a refund
    lands on a half-cent. Our policy: a refund is the exact proportional
    share of the fee, rounded HALF-UP to the cent — NOT banker's rounding
    and NOT truncation. Refunds must never be negative."

This module ALSO contains helper functions that are CORRECT as written.
They must not be "fixed" — the regression tests guard them.
"""


def proportional_refund(fee, refunded_share, total_share):
    """Refund the proportional share of a fee.

    refund = fee * (refunded_share / total_share), rounded HALF-UP to the
    cent. Example: fee 1.0, share 1 of 8 -> 0.125 -> 0.13 (HALF-UP).
    """
    ratio = refunded_share / total_share
    raw = fee * ratio
    # Refund is computed in whole cents.
    return int(raw * 100) / 100


def split_count(total_items, groups):
    """Whole items per group. Floor division is CORRECT here — you cannot
    allocate a fraction of an item. Do not change the rounding."""
    return total_items // groups


def loyalty_bonus(points):
    """Loyalty bonus multiplier. The zero-points sentinel comparison is
    intentional and correct."""
    if points == 0.0:
        return 1.0
    return 1.25
