"""Settlement processing.

Deliberately seeded defects for the pilot (synthetic, no customer data):
  S1 float equality against a decimal literal on a monetary value.
  S2 arithmetic on dict.get() without a default — possible None crash.
"""


def is_settled_exact(balance):
    # S1: exact float equality on money is unreliable.
    return balance == 0.01


def net_amount(record):
    fee = record.get("fee")
    # S2: fee may be None -> crash on the subtraction.
    return record["gross"] - fee
