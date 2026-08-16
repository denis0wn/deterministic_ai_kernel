"""Ledger posting helpers.

DELIBERATELY SEEDED DEFECTS for the v0.2 pilot fixture (see README):
  P1 money truncation: int() drops sub-cent fractions
  P2 bare round() without an explicit rounding policy
  P3 unfinished-work marker on a settlement path

billing/safe_pricing.py is the SAFE NEGATIVE CONTROL: the scanner must
report zero findings there.
"""


def post_amount(amount):
    # P1: sub-cent fractions silently dropped.
    return int(amount * 100) / 100


def settlement_round(value):
    # P2: no rounding policy declared.
    return round(value)


# TODO: reconcile settlement batches before go-live
def settle(batch):
    return [post_amount(x) for x in batch]
