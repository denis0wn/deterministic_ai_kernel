# deterministic_ai_kernel — Pilot Overview

**Deterministic execution engine for AI code-fix workflows — every run is
replayable, every result carries an evidence chain, and the system never
reports success it cannot prove.**

## The problem we solve

Teams adopting LLM-driven code fixing hit the same wall: the model's output
cannot be trusted, replayed, or shown to an auditor.

- The same defect gets a different fix every run — or a plausible-looking
  patch that silently fails the real test suite.
- When something breaks, there is no record of what the model was asked,
  what it answered, and what actually executed.
- "It worked in the demo" does not survive a compliance review.

## What the kernel does

Runs code-fix tasks through a canonical, fully-persisted pipeline:
read → locate → patch → apply → **run the real test suite** → validate.
Every step writes an evidence artifact; every model call is recorded with
its full prompt, response, and seed. A task can be replayed end-to-end and
verified against its evidence chain.

Measured behavior on our reference defect class (money rounding, Python):

| Configuration | Success rate |
|---|---|
| Frontier-class cloud model, unassisted | 0/14 |
| Local reasoning model, unassisted | 0/14 |
| **Kernel + Layer-1 domain hints** | **14/14** |

And the honesty property that matters more than the rate: when the fix is
wrong, the kernel says `tests_failed` — it has never fabricated a passing
result in any recorded run (verified across 70+ live episodes, including
attempted sandbox escapes reproduced and closed in security review).

## Why this is different from observability tooling

LangSmith, Langfuse, Braintrust and peers show you traces and dashboards.
The kernel is an **execution boundary with proof**: bounded retries,
workspace-confined and network-denied test execution (macOS Seatbelt),
deterministic replay, and a verifiable artifact chain per task. It does not
watch your pipeline — it *is* the pipeline, with receipts.

## Current state (September 2026)

- 936 automated tests green, CI green on every merge (checked, not assumed).
- Security review passed with documented conditions; two high-severity
  findings found and fixed in-review (fabricated-success vector and
  unsandboxed test execution).
- Reference implementation: Python code-fix pipeline (NorthPay fixture).
- Runs local (Apple Silicon, MLX) or cloud (OpenAI-compatible) models;
  model-agnostic by design.

## What a pilot looks like

See `PILOT_OFFER.md`. In short: we take one of your recurring code-fix or
migration task classes, wrap it in the kernel, and deliver per-task evidence
packs your reviewers and auditors can verify independently.
