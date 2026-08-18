"""Fee aggregation for settlement.

Deliberately seeded defect (synthetic, no customer data):
  A1 rounding accumulation via truncation: each line-item fee is
     truncated to whole cents and accumulated, instead of summing the
     EXACT fees and rounding HALF-UP to the cent ONCE at the end. The
     error grows with the number of line items — exactly the drift a
     reconciliation desk sees.
"""


def total_fees(line_items):
    """Total fee across line items.

    Each item is a dict: {'amount': float, 'rate': float}. The exact fee
    per item is amount * rate. The total must be the sum of the EXACT
    fees, rounded HALF-UP to the cent ONCE.
    """
    total = 0
    for item in line_items:
        fee = item["amount"] * item["rate"]
        total += int(fee * 100)
    return total / 100
