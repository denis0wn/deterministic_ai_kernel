"""Northwind Clearing — overnight settlement fee computation.

Client problem statement (reported by the Northwind risk desk):

    "Our nightly settlement fees come out slightly LOWER than the fee
    schedule agreed with the desk. The shortfall grows with the number
    of transactions, and reconciliation flags it every morning. Per the
    desk rounding policy, every fee must be rounded HALF-UP to the cent.
    Please make the computed fees match the schedule exactly."

Fee schedule (contractual):

    fee = gross_amount * fee_rate, rounded HALF-UP to 2 decimal places.

This module is synthetic (no customer data) but models the shape of a
real settlement path where a rounding-stage defect leaks real money.
"""


def settlement_fee(gross_amount, fee_rate):
    """Fee for a single transaction: gross * rate, HALF-UP to cents.

    The desk policy is HALF-UP rounding — neither truncation toward zero
    nor banker's rounding is acceptable. Example: a fee that computes to
    exactly 1.005 must be charged as 1.01.
    """
    # Fees are computed in cents then scaled back to dollars.
    return int(gross_amount * fee_rate * 100) / 100


def batch_settlement(transactions):
    """Net settlement over a batch: sum of (gross - fee) per transaction.

    Each transaction is a dict: {'gross': <float>, 'rate': <float>}.
    Returns the total net amount to be settled.
    """
    net = 0.0
    for t in transactions:
        fee = settlement_fee(t["gross"], t["rate"])
        net += t["gross"] - fee
    return net
