# client_scorer — fixture with a seeded null-safety defect

`reports/scores.py::average_score` computes `sum(scores) / len(scores)` —
crashes on `None` entries and counts them in the denominator. Contract:
skip Nones; empty or all-None input returns 0.0; always returns float.

Tests: `test_scores.py` (kernel-harness compatible, module-level
`test_*`). Payload + series scripts live in `analyzer_out/` series dirs.
