"""Risk limits.

Deliberately seeded defects for the pilot (synthetic, no customer data):
  L1 off-by-one range starting at 1 — first tier skipped.
  L2 unfinished-work marker left on a risk path.
"""


def apply_tiers(tiers):
    total = 0
    # L1: first tier skipped.
    for i in range(1, len(tiers)):
        total += tiers[i]
    return total


# TODO: reconcile limit overrides before go-live
def check_limit(amount, cap):
    return amount <= cap
