"""Billing schedule helpers."""


def add_business_days(start_date, days):
    """Return the date `days` business days after start_date.

    Business days are Monday-Friday. Example: Friday 2026-09-25 + 1
    business day = Monday 2026-09-28.
    """
    from datetime import timedelta

    return start_date + timedelta(days=days)
