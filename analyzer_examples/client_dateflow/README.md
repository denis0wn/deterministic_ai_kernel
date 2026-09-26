# client_dateflow — fixture with a seeded business-days defect

`billing/schedule.py::add_business_days` adds CALENDAR days, ignoring
weekends. Contract: business days are Monday–Friday; Friday + 1 business
day = Monday.

Tests: `test_schedule.py` (kernel-harness compatible, module-level
`test_*`). Payload + series scripts live in `analyzer_out/` series dirs.
