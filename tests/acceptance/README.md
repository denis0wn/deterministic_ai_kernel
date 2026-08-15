# Acceptance & Benchmark Corpus (R9)

Versioned corpus for the formal benchmark suite — moved out of `/tmp`
into the repo per ROADMAP_DETERMINISTIC_KERNEL_R7_R9.md §3 (the benchmark
must survive reboots and be reviewable in git).

## Contents
- `benchmark_suite.json` — fixed benchmark: 33 cases across arithmetic,
  logic, scheduling, anti-hallucination, technical (RAG), codefix,
  long-reasoning. Each case carries explicit ground truth
  (`expect.kind` ∈ contains_any / not_contains / refusal_or_rejected /
  completed_committed / blocked_truthful / completed_any) and a run count
  (arithmetic runs 3× to measure variance at temp=0).
- `kb/` — test knowledge base for the technical_rag category (includes the
  hostile `inject_me.md` prompt-injection fixture).
- `run_benchmark.sh` — runner: executes the suite against the REAL local
  model (no mocks), writes CSV + summary + pass/fail gate.
- `../r9_benchmark_schema.rs` — cargo-test schema validation of the corpus
  (runs on every `cargo test`, model not involved).

## Running
```bash
# model must be available (mlx_lm.server, auto-loaded by the kernel)
bash tests/acceptance/run_benchmark.sh [out_dir]
# default out_dir: /tmp/dek_ai_matrix/r9_benchmark
# exit code = gate: 0 PASS, 1 FAIL
```
Outputs: `benchmark_results_<ts>.csv` (per case/run: exit, state, wall,
verdict, detail) and `benchmark_summary_<ts>.txt` (per-category accuracy,
hallucination accounting, OVERALL GATE).

## Gate thresholds (fixed in the suite JSON)
- arithmetic/logic/scheduling accuracy ≥ 0.9 (temp=0 ⇒ expect ~1.0)
- anti_hallucination: **0 recorded hallucinations** (kernel catches and
  honest refusals both count as contained)
- technical_rag accuracy ≥ 0.75
- codefix: positive chain commits AND negative chain blocks truthfully
- long_reasoning: both awkward-prompt generations complete

## Schedule
- before every major change (R-phase), and
- periodically (weekly) to track model/configuration drift.
Results are comparable across runs because the corpus, seeds, thresholds
and temperature are pinned.
