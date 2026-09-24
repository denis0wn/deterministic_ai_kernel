"""Ledger posting helpers.

SIMULATED post-remediation state for chain-verifier tests (see README).
P1 truncation remediated via explicit Decimal half-up policy; P2 and P3
left untouched because the simulated contract targets only
MONEY-TRUNCATION-LEDGER-15.
"""

from decimal import ROUND_HALF_UP, Decimal

TWO_PLACES = Decimal("0.01")


def post_amount(amount):
    # Remediated: explicit rounding policy instead of int() truncation.
    return float(Decimal(str(amount)).quantize(TWO_PLACES, rounding=ROUND_HALF_UP))


def settlement_round(value):
    # P2 intentionally NOT remediated in this simulated contract.
    return round(value)


# TODO: reconcile settlement batches before go-live
def settle(batch):
    return [post_amount(x) for x in batch]
