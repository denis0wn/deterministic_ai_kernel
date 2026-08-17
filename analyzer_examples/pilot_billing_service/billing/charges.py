"""Charge calculation.

Deliberately seeded defects for the pilot (synthetic, no customer data):
  C1 money truncation via int() — sub-cent fractions dropped.
  C2 floor division on a monetary amount — truncates the per-part fee.
"""


def to_cents(amount):
    # C1: sub-cent fractions silently dropped.
    return int(amount * 100)


def split_fee(amount, parts):
    # C2: floor division truncates the per-part fee.
    return amount // parts
