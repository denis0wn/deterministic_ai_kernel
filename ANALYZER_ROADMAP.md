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
