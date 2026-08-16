"""NEGATIVE CONTROL: compliant money handling the scanner must NOT flag.

Uses Decimal with an explicit rounding policy, defaulted .get(), and
range(len(...)) — none of the pilot rule patterns apply here. If this
file ever produces findings, either a rule regressed or the control was
edited.
"""

from decimal import ROUND_HALF_UP, Decimal

TWO_PLACES = Decimal("0.01")


def price_with_fee(amount, rate):
    fee = Decimal(str(amount)) * Decimal(str(rate))
    return fee.quantize(TWO_PLACES, rounding=ROUND_HALF_UP)


def apply_discount(amount, customer):
    discount = customer.get("discount", 0)
    return amount - amount * discount


def walk_tiers(tiers):
    total = 0
    for i in range(len(tiers)):
        total += tiers[i]
    return total
