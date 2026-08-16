# ROLES — разделение системы на два проекта

Создано 2026-08-15 копированием `deterministic_ai_kernel_clean` (HEAD f9ddfbc).

## Проект 1 — ИСПОЛНИТЕЛЬ (executor)
**Репо:** `/Users/denissmoliakov/projects/deterministic_ai_kernel_clean`
**Ветка:** `orchestrator-rebuild` · remote: github.com/denis0wn/deterministic_ai_kernel

Детерминированное ядро исполнения задач: pipeline-run → parser → routing → planner →
model → effects → validation. Все гарантии живут здесь:
- LLM = untrusted input; никаких shell/filesystem эффектов из LLM-текста;
- мутирующие эффекты только через kernel-owned примитивы (apply_patch_v1 с
  context-проверенными hunk'ами, run_tests_v1 с allowlist-argv);
- grounding-гейт (unverified_claim), RAG, streaming-клиент с idle-таймаутами,
  wedge-recovery, бенчмарк-гейт (tests/acceptance/), ops-профиль (ops/).

**Правило:** этот репо не получает эвристику поиска дефектов. Все фиксы ядра
(R2–R9, security) живут и развиваются здесь.

## Проект 2 — АНАЛИЗАТОР (detector) — ЭТОТ РЕПО
**Ветка:** `analyzer` · remote: отсутствует (создастся отдельно)

Слой обнаружения дефектов: сканирование кодовой базы → кандидаты → триаж →
структурированные задачи для исполнителя. По природе вероятностный — поэтому
отделён от детерминированного ядра.

### Слои, которые нужно построить (в этом репо)
1. **Repo ingestion** — обход репозитория, инвентарь файлов/модулей, снимок состояния.
2. **Scan primitives** — поиск кандидатов в дефекты (статические эвристики +
   модельные проходы; модель здесь может быть любой, включая более крупные).
3. **Triage/ranking** — оценка критичности, дедупликация, приоритизация.
4. **Task emitter** — преобразование кандидата в контракт задачи исполнителя.

### Ядро в этом репо — ЗАМОРОЖЕННЫЙ СЛОЙ
Код ядра (src/, tests/) здесь — стартовая база и будущая зависимость. Новая
логика анализатора строится СВЕРХУ (новые модули/бинарники), ядро не правится;
при необходимости фиксов ядра — они сначала идут в репо исполнителя и
переносятся сюда. Цель: со временем потреблять ядро как library/git-зависимость,
а не как копию.

## КОНТРАКТ АНАЛИЗАТОР → ИСПОЛНИТЕЛЬ (v0)
Анализатор передаёт исполнителю только то, что тот умеет потреблять:

```json
{
  "task_kind": "codefix | answer_question",
  "workspace": "<абсолютный путь к репо/модулю>",
  "target_files": ["src/billing/fees.py"],
  "finding": {
    "id": "FEE-ROUND-001",
    "severity": "critical|high|medium|low",
    "description": "округление комиссий в пользу клиента при отрицательных ставках",
    "evidence": ["src/billing/fees.py:42-51"]
  },
  "codefix_steps": [
    "Step 1 read repository <workspace>/<target>",
    "Step 2 find bug",
    "Step 3 patch code",
    "Step 4 apply patch",
    "Step 5 run tests",
    "Step 6 validate patch"
  ],
  "tests_contract": "существующие тесты модуля + новый тест на finding",
  "provenance": {
    "detector": "static|model",
    "confidence": 0.0,
    "analyzer_version": "..."
  }
}
```

Инварианты контракта:
- исполнитель НЕ доверяет finding'у: патч валидируется context_before, тесты
  реально исполняются, гейты применяются как обычно;
- finding без воспроизводимого теста не считается закрытым;
- анализатор никогда не вызывает эффекты напрямую — только через
  pipeline-run исполнителя.

## Статус
- [x] копия создана, ветка analyzer, роли зафиксированы
- [ ] сборка копии (cargo build) — не выполнялась
- [ ] слои 1–4 анализатора
- [ ] сквозной demo: анализатор нашёл → исполнитель починил → evidence-отчёт
