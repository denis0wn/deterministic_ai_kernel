# external_reports — sample SAST reports for v0.3 ingestion tests

**Hand-authored, deterministic fixtures.** They mimic the JSON shapes of
`semgrep --json` and `bandit -f json` but were NOT produced by running the
tools. They exist so the ingestion path (`src/analyzer/external_sast.rs`)
is testable without installing or executing any third-party scanner —
consistent with the v0.3 invariant: the analyzer ingests reports, it never
runs tools.

Deliberate properties:

- `semgrep_sample.json`
  - one valid finding against `pilot_fintech/billing/ledger.py:15`;
  - one **path-traversal probe** (`../../outside_workspace.py`) that
    ingestion must reject and count (`rejected_paths: 1`);
  - timestamp-ish noise (`time.*`) that normalization must ignore.
- `bandit_sample.json`
  - one finding against `pilot_fintech/billing/ledger.py:20`;
  - `generated_at` noise that normalization must ignore.

Both reference the `pilot_fintech` fixture, so a pilot run over that
workspace with `--external-sast` yields 3 static + 2 external findings.
