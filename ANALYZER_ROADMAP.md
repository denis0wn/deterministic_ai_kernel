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

### v0.3 (future) — external SAST ingestion
Semgrep/Bandit etc. ONLY as a read-only candidate source: no effects,
deterministic normalization into the Candidate shape, provenance
`detector: external`, same triage/emitter path, executor still does not
trust findings.

### v0.4 (future) — executor integration via stable API
Only after a separate security review. Until then the hand-off stays
manual (TaskContract JSON → executor pipeline-run); the analyzer never
invokes the executor automatically.

### Языковой scope
JVM/Go/JS remain explicitly BACKLOG — no support is claimed or implied
for them in v0.2–v0.4 planning; pilot claims are Python-only.

## ИНВАРИАНТЫ (неизменяемые)
- анализатор read-only к сканируемой базе;
- никакие эффекты без executor;
- модельные/эвристические результаты = untrusted hints;
- детерминизм triage/emitter при одинаковом входе.
