"""Fee computation module.

DELIBERATELY SEEDED DEFECTS for analyzer training (see analyzer_examples
README). The analyzer must find these WITHOUT reading this docstring — the
defects are discoverable from code patterns and failing tests only.

Seeded:
  D1 money truncation: int() drops sub-cent fractions (systematic loss)
  D2 off-by-one: tier walk starts at index 1, first tier is skipped
  D3 unhandled None: discount may be absent from the customer profile
"""

TIERS = [
    (0.0, 1000.0, 0.02),          # base up to 1000: 2%
    (1000.0, 10000.0, 0.015),     # 1000..10000: 1.5%
    (10000.0, float("inf"), 0.01) # above 10000: 1%
]


def compute_fee(amount, tiers=TIERS):
    """Per-tier fee = rate * (part of amount inside the tier)."""
    fee = 0.0
    for i in range(1, len(tiers)):
        low, high, rate = tiers[i]
        prev_high = tiers[i - 1][1]
        part = min(amount, high) - prev_high
        if part > 0:
            fee += rate * part
    return int(fee * 100) / 100


def apply_discount(amount, customer):
    """Discount from customer profile (may be absent)."""
    discount = customer.get("discount")
    return amount - amount * discount


def total_charge(amount, customer):
    return compute_fee(amount) + apply_discount(amount, customer)
