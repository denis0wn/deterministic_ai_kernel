# ANALYZER ROADMAP — развитие детектора поверх frozen-ядра

Создано 2026-08-16. Текущее состояние: базовые слои построены и доказаны
(ANALYZER_BUILD_RECORD.md), сквозной demo зелёный.

## СДЕЛАНО (v0.1.0)
- `src/analyzer/ingestion.rs` — read-only инвентарь (пути, язык, размер, BLAKE3)
- `src/analyzer/scan_primitives.rs` — 6 статических правил (money-truncation,
  money-round-bare, offbyone-range, none-arith с lookahead, dangerous-eval,
  todo-marker); найдены ровно 3/3 заложенных дефекта toy-репо
- `src/analyzer/triage.rs` — детерминированный severity-ranking (деньги/деньги-пути
  повышают критичность), provenance (detector/confidence/version)
- `src/analyzer/task_emitter.rs` — TaskContract v0 для executor (6-шаговый CodeFix)
- `src/analyzer/audit_log.rs` — журнал запусков анализатора
- `src/bin/analyzer_demo.rs` — сквозной CLI
- `analyzer_examples/billing_python/` — toy-репо с 3 дефектами + падающие тесты
- 12 unit-тестов; suite репо 723/0; fmt/clippy/check/release чистые

## СЛЕДУЮЩИЕ ШАГИ (зафиксированы, НЕ реализуются сейчас)

### 1. Мультиязычность (Python → JVM/Go/JS)
- ingestion уже классифицирует jvm/go/javascript/typescript; нужны правила
  сканирования под каждый язык (свои паттерны truncate/round/off-by-one/null);
- контракт TaskContract не меняется — язык прозрачен для executor, но
  run_tests_v1 нужен раннер под язык (сейчас python_test_file/cargo_test).

### 2. Подключение внешних SAST в scan_primitives
- интерфейс Candidate уже универсален: внешний инструмент → маппинг в
  Candidate с provenance.detector="external";
- кандидаты от внешних SAST идут через тот же triage/task_emitter —
  executor по-прежнему не доверяет finding'у.

### 3. Модельный проход (model-scan)
- LLM-проход по модулю → кандидаты с detector="model", confidence < static;
- строго как hints: триаж смешивает источники, но приоритет у детерминированных.

### 4. Интеграция с executor как library/stable API
- сейчас связка ручная (TaskContract JSON → pipeline-run); следующий шаг —
  вызов executor'а из анализатора как внешний процесс с контрактом, либо
  потребление ядра как library-зависимости (а не копии в этом репо).

### 5. Repo-scale контекст
- текущий retrieval toy-уровня; для реальной кодовой базы нужен нормальный
  код-поиск/индексация (см. R8-выводы в репо исполнителя).

## ВЕРСИИ

### v0.2 Enterprise Pilot Evidence Package — DONE (2026-08-16)
Completed when ALL of the following hold (see
/tmp/dek_ai_matrix/ANALYZER_V02_ENTERPRISE_PILOT_RECORD.md):
- `evidence_manifest_v1` module: byte-stable manifest, no timestamps in
  content-hashed artifacts, snapshot BLAKE3, deterministic run id,
  generated-path exclusions — unit-tested.
- Finding model v0.2: stable finding_id, rule_id, candidate_statement,
  snapshot-anchored evidence_locations, detector provenance
  (static|model|external), mandatory `model_hint_unverified` for model
  hints, per-rule limitations — TaskContract v0 not broken.
- Deterministic deduplication: identical (rule_id, location, normalized
  evidence) collapses regardless of input order — tested.
- Financial ruleset `python-fintech-rules/0.2.0`: 5 documented Python
  rules with positive+negative tests and FP/FN profiles
  (docs/ANALYZER_RULES_CATALOG.md); todo-marker scoped to finance paths.
- Readiness contract: `candidate_only` by default; `remediation_ready`
  only with operator-provided reproducible test + snapshot match +
  complete evidence; `manual_review_required` always true.
- `analyzer_pilot_report` CLI: full evidence package
  (manifest/findings/contracts/report + operational log), workspace
  untouched, repeated runs byte-identical — demonstrated on
  analyzer_examples/pilot_fintech (seeded candidates + clean negative
  control).
- Client docs: ENTERPRISE_PILOT_ONE_PAGER.md,
  PILOT_SCOPE_AND_LIMITATIONS.md, docs/ANALYZER_RULES_CATALOG.md.
- Full regression green: cargo test / fmt / clippy (0 warnings in new
  files) / check / release.

### v0.3 External SAST ingestion — DONE (2026-08-16)
Semgrep/Bandit ONLY as a read-only candidate source (the analyzer never
runs the tools). Completed evidence:
- `src/analyzer/external_sast.rs`: JSON ingestion with format
  auto-detection (semgrep/bandit), fail-closed on unknown formats,
  deterministic normalization into `Candidate` (rule_id
  `semgrep:<check_id>` / `bandit:<test_id>`, `detector: external`,
  tool severity carried through), workspace path-traversal rejection
  with counted `rejected_paths`, report timestamps ignored.
- Triage: conservative external classification (ERROR/HIGH→High 0.55,
  WARNING/MEDIUM→Medium 0.45, else Low 0.35; never Critical), and a
  deterministic tie-breaker static < external < model at equal severity.
- `evidence_manifest_v1.external_sources`: tool + report BLAKE3 +
  candidate/rejection counts — external input is part of the audit
  trail and changes the run id.
- CLI: `analyzer_pilot_report --external-sast <report.json>`
  (repeatable); argument order does not affect artifacts.
- Fixtures: `analyzer_examples/external_reports/` (hand-authored
  deterministic sample reports incl. traversal probe) — no third-party
  tool installed or executed in tests.
- Tests: normalization/determinism/traversal/fail-closed unit tests,
  merge byte-stability + CLI double-run integration tests; full
  regression green (count in the v0.3 record).

### v0.4-pilot-ops — Review Gate + Work Order + Evidence Chain — DONE (2026-08-16)
The human boundary and the independent verification, formalized as
tamper-evident artifacts (no executor invocation anywhere):
- `review_gate.rs` + `analyzer_review` CLI — schema `review_decision_v1`:
  byte-stable decision core + content-hash id; timestamps only in the
  operational envelope. Fail-closed: package integrity (evidence hashes
  vs manifest), approve only for `remediation_ready` unless an explicit
  recorded `--override-candidate-only`, repro paths must exist in the
  snapshot, reviewer/rationale mandatory.
- `work_order.rs` + `analyzer_work_order` CLI — schema `work_order_v1`:
  passive byte-stable handoff document (contract embedded with BLAKE3,
  target snapshot hashes, operator checklist); generated only for
  approved decisions and only if the package has not drifted since the
  decision. Executes nothing.
- `evidence_chain.rs` + `analyzer_chain_verify` CLI — schema
  `evidence_chain_v1`: five links (contract integrity, pre-state vs
  snapshot, post-state presence/change, structural `test_report_v1`
  validation against the executor's real TestReportV1 schema, optional
  event log); four overall statuses (`chain_consistent_remediation_
  evidenced`, `chain_consistent_no_change`, `chain_inconsistent`,
  `chain_incomplete`). All hashes recomputed from bytes; executor
  statuses never believed; the verifier speaks only about chain
  consistency, never about fix correctness.
- `analyzer_examples/simulated_executor_evidence/` — explicitly labeled
  SIMULATED executor evidence for verifier tests (no real executor run).
- `PILOT_OPS_RUNBOOK.md` — the operator workflow end to end.
- Analyzer version 0.4.0; TaskContract v0 unchanged.

### Model-quality experiments (next phase; gated — added after the 2026-08-16 real negative run)
The first real-model attempt series (3 attempts) ended in an accepted
negative outcome: fail-safe behavior confirmed, remediation success not
achieved (model patch semantically wrong; real tests failed; no fake
success). Before ANY further remediation attempts:
- Pre-registered experiment design: fixed attempt budget declared before
  the first attempt; seed list fixed in advance; no seed-hunting, no
  retries-until-green (see PILOT_OPS_RUNBOOK.md attempt discipline).
- Failure taxonomy for model patches: structural rejection vs semantic
  test failure vs partial fix; per-finding analysis of why the patch was
  wrong (e.g. invented API signatures, wrong rounding mode).
- Metrics via docs/PILOT_METRICS_V1.md only; attempt lists complete and
  published, success rate over ALL attempts.
- Candidate quality levers to evaluate (each separately, with the same
  evidence requirements): richer contract context (test expectations
  embedded in the work order), patch-shape constraints/examples, and
  per-runner test semantics notes. No change may weaken a kernel gate;
  executor code stays frozen pending its own review.
- Acceptance for this phase: a pre-declared metrics target over the
  whole attempt series, OR an accepted negative result — both are valid
  outcomes; only fake success is not.
- Status (2026-08-16): first bounded experiment executed
  (mq1-rounding-truncation-3att, preregistered plan + 3 attempts,
  seeds 100/101/102): 3/3 attempts rejected fail-closed at patch_code
  as malformed_patch (systematic model error: capitalized directory in
  the patch target path); zero mutations, zero fake successes, all
  evidence preserved (/tmp/dek_ai_matrix/model_quality_experiment/,
  FINAL_RECORD + METRICS + PLAN).
- Status (2026-08-16, later): second bounded experiment executed
  (mq2-patch-shape-3att, arm A2 patch-contract facts incl. exact path
  restatement; seeds 103/104/105): 3/3 malformed_patch with the SAME
  signature — the prose-context lever does not fix this defect on this
  model; combined 6/6 across seeds, always fail-closed pre-effect
  (/tmp/dek_ai_matrix/model_quality_experiment_mq2/).
- Status (2026-08-17): third bounded experiment executed
  (mq3-structural-levers-3att; workspace renamed to isolatedws, A2
  under renamed workspace, structural PatchV1 shape example; seeds
  106/107/108): 3/3 malformed_patch, IDENTICAL signature — H1 naming
  hypothesis REJECTED, H3 structural example REJECTED; combined
  mq1+mq2+mq3: 9/9 seeds, always fail-closed pre-effect
  (/tmp/dek_ai_matrix/model_quality_experiment_mq3/). Prompt-level
  levers for the path-capitalization defect are exhausted within the
  tested space. Next (registered plan + explicit go-ahead required):
  model/runtime change as the variable, or a workspace-relative target
  framing ONLY if supported without executor changes, or accept as a
  documented model constraint. Seed 109 reserved.
- Status (2026-08-17, later): mq4-model-variable-3att attempted (model
  as the variable: coder fine-tune artifact). Closed as ENVIRONMENT
  LIMITATION: the artifact is unservable by the installed mlx_lm
  (`Model type gemma4_unified not supported`, confirmed by direct load);
  attempt 1 = infrastructure_failure, seeds 201/202 preserved by the
  registered stop rule; no model-quality conclusion drawn
  (/tmp/dek_ai_matrix/model_quality_experiment_mq4/). Options pending
  approval: compatible second model artifact, or a preregistered mlx_lm
  upgrade experiment (with runtime re-validation), or close the track.
- Status (2026-08-17, later): the preregistered mlx_lm upgrade
  (env-mlxlm-upgrade-1) closed WITHOUT EXECUTION — the installed mlx_lm
  0.31.3 already IS the latest PyPI release, so `gemma4_unified` is an
  upstream support gap, not staleness; no environment change made
  (/tmp/dek_ai_matrix/MLX_LM_UPGRADE_EXPERIMENT_PLAN.md). Remaining
  paths (separate approval each): prerelease mlx_lm from git main as its
  own experiment with full re-validation; a different artifact supported
  by the released runtime (seeds 203+ under a fresh plan); or close the
  model-variable track.
- Status (2026-08-17, later): mq5-model-variable-r2-3att executed with
  a compatible second model (Qwen2.5-Coder-7B-Instruct-4bit, qwen2;
  seeds 203/204/205): 3/3 malformed_patch with the SAME signature as
  all gemma4-reasoning attempts. COMBINED 12/12 across two model
  families — the "gemma4-specific" interpretation is REFUTED; the
  defect is task-presentation-robust
  (/tmp/dek_ai_matrix/model_quality_experiment_mq5/). Next (each needs
  explicit approval): read-only root-cause analysis of the kernel's
  patch_code target-path presentation (no code changes); an
  analyzer-side presentation experiment only if a legal knob is found;
  or close remediation-attempt tracks at the current evidence level.
  Reserved seeds: 109, 201, 202.
- Status (2026-08-17, later): READ-ONLY root-cause analysis completed
  (/tmp/dek_ai_matrix/PATCH_TARGET_PATH_ROOT_CAUSE_ANALYSIS.md): the
  kernel's PATCH_PROMPT makes the model re-type the target path from a
  `<FILE>` placeholder; both model families capitalize the only
  name-like path component (the workspace basename) during
  reconstruction; mismatch is terminal with no corrective retry. This
  is a task-presentation property of the FROZEN kernel prompt layer;
  kernel-side mitigations exist but each requires a separate security
  review; no legal analyzer-side knob exists. Decision pending from the
  owner: open a kernel presentation review track, or keep remediation
  attempts closed and run the pilot as analysis + evidence-chain only.

### Layer 1 (rule-driven proactive hints) — DONE & VALIDATED (2026-08-18)
The analyzer itself now generates the corrective guidance instead of a
human per task (owner directive: the system must produce the right
hints itself; no manual per-task hinting). Analyzer-side only — no
executor/kernel change, no gate weakened, read-only invariant intact.
- `src/analyzer/hint_engine.rs`: deterministic rule→hints mapping (no
  LLM). Covered rules: money-truncation (3 hints: use decimal module
  with ROUND_HALF_UP; add the import INSIDE the fixed function to keep
  the patch a single contiguous region; sum exact values then round
  once), money-round-bare, floor-div-money, none-arith,
  offbyone-range. All other rules → empty (fail-closed to no hints).
- `EmittedTask.hints: Vec<String>` (serde default — TaskContract v0
  consumers unaffected); analyzer version 0.4.1.
- Root cause addressed: PatchV1 is a SINGLE contiguous region, so a
  fix needing a top-of-file import + a lower function change cannot be
  one clean patch; the model's hard-task failure was exactly a missing
  `from decimal import Decimal, ROUND_HALF_UP` → NameError → honest
  tests_failed.
- Controlled validation experiment on client_settlement (hard fixture):
  WITHOUT hints 3/3 tests_failed (seeds 900/901/902, same missing-
  import signature); WITH Layer-1 hints GREEN at seed 950 — chain
  `chain_consistent_remediation_evidenced`, chain_id 1261be0e…,
  executor task 0e52b9bd0dd74fa9; model applied the in-function import
  + Decimal HALF-UP fix; independent re-verification passed
  (total_fees(3×0.125)=0.38, 1×0.125=0.13); real tests exit 0. No
  seed-hunting: the contrast is hints-vs-no-hints, not seed search.
  Record: /tmp/dek_ai_matrix/LAYER1_HINTS_EXPERIMENT_RECORD.md.

### Layer 2 (verifier-driven feedback loop) — DEFERRED / NOT JUSTIFIED (2026-08-18)
Forensic review: /tmp/dek_ai_matrix/LAYER2_FORENSIC_REVIEW_REPORT.md
(verdict CONDITIONAL GO). The justification experiment (C4) did NOT
find what was needed, so Layer 2 is closed as DEFERRED — not as
"doesn't work":
- C4 NorthPay branch (WITH money-truncation hints, seeds 1051–1053,
  pre-registered, canonical pipeline-run path): 3/3 HONEST SHAPE-
  NEGATIVE — byte-identical `malformed patch: invalid escape at line 1
  column 290` at 02_patch_code; tests never ran; workspace untouched.
  Record: /tmp/dek_ai_matrix/C4_NORTHPAY_WITH_HINTS_RECORD.md.
- The demonstrated residual class (JSON escaping) is orthogonal to fix
  guidance and OUT of scope for verifier-driven feedback (no test
  signal exists); it belongs to the representation-robustness track.
- The settlement case produced a genuine tests_failed but is converted
  by Layer 1 hints (seed 950), so it cannot serve as evidence of need.
- Missing evidence class — Layer 2 stays DEFERRED until one exists:
  Layer 1 → valid executable candidate → independent verifier →
  SEMANTIC FAILURE → Layer 1 unable to fix it.
- Resume condition: a reproducible executable semantic failure that
  Layer 1 does not eliminate and the verifier can diagnose. Only then:
  prerequisites C1 (persist failing TestReportV1), C2 (record model
  calls — ChatRequest has no seed field today), C3 (event the feedback
  cycle), then a security review before any kernel change.
- Design constraints on resume (from arXiv 2608.02464, "Real-Time
  Detection and Repair of LLM Agent Failures"; notes in
  /tmp/dek_ai_matrix/PAPER_NOTES_2608.02464.md): (a) feedback payload
  must be the "located" rung — NAME of the failing check/test, NO
  values from traceback; this is the only feedback form that survived
  their correction against a resampling control (p=0.0005) and it
  simultaneously shrinks our T11 gaming surface (expected values in
  feedback = the hardcoding channel) and the injection surface;
  (b) any feedback POC must pair each feedback attempt with a
  same-prefix resample-without-feedback control and be credited only
  the margin above it — a loop that merely matches resample luck
  proves nothing. The paper's deterministic-verification-first result
  (0 false positives, cross-family transfer, no calibration) is
  independent external validation of this kernel's existing posture;
  statistical telemetry monitors are explicitly NOT adopted (6-step
  episodes give no post-onset horizon; per-deployment nulls violate
  the determinism posture; at temp 0 our failures are contract-
  checkable — the class the paper itself delegates to deterministic
  verifiers).

Work-selection principle adopted with this decision: do not build a
mechanism because it is architecturally elegant; build it only after a
reproducible failure that the mechanism can actually fix. Refusing the
next layer when evidence is absent is a good experimental result, not
lost progress.

Active track instead: representation robustness — localize WHY the
model emits malformed patch_v1 JSON on the NorthPay prompt but not on
the settlement prompt (task → prompt construction → model output →
serialization/escaping → parser), and audit the planner-pipeline
success-semantics misfire — VERIFIED in the C4 kernel.db: the
`run run` (planner-pipeline) path wrote a full simulated lifecycle for
a no-effect run into the SAME canonical event_log table the fold
reads — STEP_COMPLETED for 04_run_tests (29 ms), TASK_COMPLETED
success:true, REPLAY_VALIDATED — while the canonical effect-loop path
in the same DB honestly recorded TerminalFailure for the real task.
Two writers, one log, divergent success semantics. AUDITED
(/tmp/dek_ai_matrix/MISFIRE_AUDIT_REPORT.md): severity MEDIUM,
contained — the simulated path (standalone `run` binary, no-op
DefaultStepExecutor) writes full success lifecycles into the canonical
event_log, but the fold's unit-shape contract rejects them (replay
INVALID), nothing materializes into step_status/effect_ledger, and
chain attestation cannot be forged through it. Remediation items 1–5
in the audit report await owner decision; no code changed.

### v0.5 (future; was previously labeled v0.4) — executor integration via stable API
Only after a SEPARATE security review. Until then the hand-off stays
manual (work order → operator → executor pipeline-run in an isolated
copy); the analyzer never invokes the executor automatically.

### Языковой scope
JVM/Go/JS remain explicitly BACKLOG — no support is claimed or implied
for them in v0.2–v0.4 planning; pilot claims are Python-only.

## ИНВАРИАНТЫ (неизменяемые)
- анализатор read-only к сканируемой базе;
- никакие эффекты без executor;
- модельные/эвристические результаты = untrusted hints;
- детерминизм triage/emitter при одинаковом входе.
