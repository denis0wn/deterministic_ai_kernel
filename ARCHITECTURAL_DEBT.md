# Архитектурный долг (Architectural Debt)

В данном документе фиксируются все элементы устаревшей архитектуры, нарушающие инварианты целевой архитектуры **Execution Kernel**.

---

## 1. Прямой доступ к БД из воркера в обход шины событий
*   **Статус: ЗАКРЫТО.** Проверено 2026-09-24 (в `worker.rs`/`leases.rs` ноль SQL-ключевых слов) и доочищено 2026-09-25 (`c744602`): реальный остаток сидел в `src/main.rs` — операторские `reset_db` (13× DELETE + VACUUM) и INSERT-сайты задач. Весь SQL перенесён в `StorageProvider` (trait `reset_db` расширен до полного 13-табличного M2-списка; добавлены `upsert_task_exec_spec`, `insert_semantic_bias_artifact`, `task_exists`, `effect_ledger_counts`). `grep -E 'query_row|execute_batch|INSERT|DELETE' src/main.rs` = 0.
*   **Модули:** ~~`worker.rs`, `leases.rs`~~ → фактически `src/main.rs`
*   **Проблема:** Воркер напрямую выполняет SQL-запросы `INSERT` и `UPDATE` в таблицы `leases` и `step_status`.
*   **Нарушение:** Нарушает инвариант каноничности шины событий и скрывает переходы состояний от `EventBus` (раздел 3.1 `ARCHITECTURE_INVARIANTS.md`).
*   **Сложность миграции:** Medium.
*   **Оценка трудозатрат:** 4-6 часов.

## 2. Остаточные fallback-пути (StepKind & TaskClass)
*   **Статус: ЗАКРЫТО 2026-09-25.** Fallback удалён из обоих мест, где он жил: `ordered_step_ids` и `load_exec_spec` в `src/providers/storage.rs` — задача без persisted ExecSpec теперь fail-closed («publish a plan before scheduling»), а ПОВРЕЖДЁННАЯ спека — ошибка, а не молчаливая регенерация дефолтного флоу (раньше `if let Ok(spec)` глотал corruption). Попутно закрыт баг несогласованных веток (NULL-exec_spec не принимал класс `Question`, пустая строка — принимал). Потоки с легальным дефолтом (`submit-task`, TUI `submit_task`) теперь выписывают `TaskClass::Generic.to_exec_spec(None)` явно на записи. Двухфазный поток (`analyze-task` → `pipeline-run`) не затронут: спека заполняется через upsert до планирования. Тест `unknown_task_class_is_rejected` заменён на `specless_task_is_rejected` (класс больше не гейтит планирование — гейтит наличие спеки). Сами перечисления `StepKind`/`TaskClass` остаются как классификация/отображение (TUI, `classify_task_class`); их полное удаление из дистрибутива — отдельный проект (миграция колонки `tasks.task_class` + TUI-фильтры), здесь не в скоупе.
*   **Модули:** ~~`worker.rs`, `scheduler.rs`, `workflow/contract.rs`~~ → фактически `src/providers/storage.rs`
*   **Проблема:** В коде ядра сохранены fallback-пути парсинга `StepKind` и `TaskClass` для обеспечения обратной совместимости с существующими тестами и базами данных.
*   **Нарушение:** Препятствует полному удалению перечислений `StepKind` и `TaskClass` из бинарного дистрибутива ядра.
*   **Сложность миграции:** Low (требуется полный переход внешних систем на ExecSpec с последующим удалением fallback-кода).
*   **Оценка трудозатрат:** 2-3 часа.
