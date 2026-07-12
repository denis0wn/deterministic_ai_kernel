# Kernel Product Readiness Audit

В данном документе описывается готовность ядра к использованию в качестве продукта и выявляются утечки доменных понятий в чистое ядро.

---

## 1. Утечки StepKind и TaskClass в Runtime/Execution

* **`src/worker.rs`**:
  * Функция `parse_step_kind_from_step_id` жестко мапит текстовые step_id на `StepKind`. Это привязывает воркер к конкретному набору шагов workflow.
* **`src/effects.rs`**:
  * Функция `step_from_step_id` дублирует логику маппинга текстовых step_id на `StepKind`.
  * Функция `default_worker_for_step` хардкодит соответствие шагов ролям воркеров (`worker-planner`, `worker-executor`, `worker-verifier`).
  * Жестко зашитая логика вызова LLM для определенных `StepKind` (`ExecuteChanges`, `PatchCode`, `AnalyzeTask`, `PlanExecution`).
* **`src/execution/runtime.rs`**:
  * Прямая проверка `step.kind == StepKind::AnalyzeTask` и вызов генерации эмбеддингов.

---

## 2. Вызовы LLM и файловой системы в обход провайдеров

* **Эмбеддинги (`src/embeddings.rs`)**:
  * `embed_text` делает прямой HTTP-запрос через `reqwest::Client` к эндпоинту `/embeddings` вместо использования абстракции провайдера.
* **Снапшоты (`src/snapshot.rs`)**:
  * Использование `SystemTime::now()` нарушает детерминизм времени в снапшоте.

---

## 3. Выводы по интеграции и план устранения

1. **Интеграция CodeFix на уровне ExecSpec**: Пайплайн CodeFix должен полностью описываться через примитивы (`PrimitiveKind::Read`, `PrimitiveKind::Write`, `PrimitiveKind::Compute`, `PrimitiveKind::Route` и др.) в ExecSpec, а не через хардкод StepKind.
2. **Абстракция моделей**: Убедиться, что `LlmProvider` поддерживает различные модели, а метаданные о выбранной модели сохраняются в логе событий.
