"""Rate management — SAFE NEGATIVE CONTROL.

Uses Decimal with an explicit rounding policy and defaulted access; none
of the pilot rules should fire in this file.
"""

from decimal import ROUND_HALF_UP, Decimal

TWO_PLACES = Decimal("0.01")


def apply_rate(amount, rate):
    return float(
        Decimal(str(amount)) * Decimal(str(rate)).quantize(TWO_PLACES, rounding=ROUND_HALF_UP)
    )


def get_rate(table, key):
    return table.get(key, Decimal("0"))
