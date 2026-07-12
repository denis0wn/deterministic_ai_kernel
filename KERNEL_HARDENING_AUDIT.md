# Kernel Hardening Audit

Полный аудит архитектурных нарушений в ядре Execution Kernel.

---

## 1. Прямые вызовы БД из runtime модулей

> [!CAUTION]
> Это наиболее серьезная категория нарушений. Все runtime-модули напрямую работают с SQLite.

| Модуль | Количество SQL-операций | Критичность |
|:-------|:-----------------------:|:-----------:|
| `scheduler.rs` | ~35 | 🔴 CRITICAL |
| `worker.rs` | ~30 | 🔴 CRITICAL |
| `effects.rs` | ~12 | 🔴 CRITICAL |
| `snapshot.rs` | ~8 | 🟡 MODERATE |

**Каждый модуль самостоятельно открывает `Connection::open(db)` и выполняет raw SQL.**

Ключевые нарушения:
- `scheduler.rs`: 7 вызовов `Connection::open()`, ~35 прямых SQL-операций (INSERT, UPDATE, SELECT, JOIN)
- `worker.rs`: 6 вызовов `Connection::open()`, ~30 SQL-операций включая транзакции
- `effects.rs`: прямое чтение event_log, INSERT INTO effect_ledger, INSERT INTO external_effects
- `snapshot.rs`: чтение/запись state_snapshots напрямую

**Рекомендация:** Создать `StorageProvider` trait с методами для всех операций с данными. Runtime модули должны работать через абстракцию, а не через raw SQL.

---

## 2. Прямые вызовы файловой системы

| Файл | Строка | Вызов | Критичность |
|:-----|:------:|:------|:-----------:|
| `main.rs` | L33, L82 | `std::fs`, `Path::new(db).exists()` | 🟢 ACCEPTABLE (entry point) |
| `api.rs` | L13-18 | `std::fs`, `fs::remove_file(db)` | 🟡 MODERATE |
| `model_manifest.rs` | L31, L70, L94, L113 | `fs::read_to_string`, `fs::write` | 🟡 MODERATE |
| `reconstruction/mod.rs` | L20 | `std::path::Path::new(db).exists()` | 🟢 ACCEPTABLE (DB existence check) |
| `replay/engine.rs` | L5 | `std::path::Path::new(db).exists()` | 🟢 ACCEPTABLE (DB existence check) |
| `lm_control.rs` | L132, L231-238 | `Path::new().exists()` | 🟢 ACCEPTABLE (model path check) |

**Позитивное:** `effects.rs` корректно работает через `crate::providers::get_filesystem()` — паттерн правильный.

**Рекомендация:** Перевести `model_manifest.rs` и `api.rs` на `FilesystemProvider`.

---

## 3. Прямые вызовы LLM

| Файл | Строка | Вызов | Критичность |
|:-----|:------:|:------|:-----------:|
| `effects.rs` | L108 | `crate::providers::get_llm().coding_assistant()` | 🟢 OK (через Provider) |
| `workflow/compiler.rs` | L3 | `use crate::llm;` — прямой импорт LLM | 🟡 MODERATE |
| `execution/runtime.rs` | L4, L20 | `embed_text()` — прямой вызов embedding | 🔴 CRITICAL |

**Рекомендация:** `execution/runtime.rs` должен получать embedding через Provider trait. `workflow/compiler.rs` допустим пока workflow не удален.

---

## 4. Циклические зависимости

> [!WARNING]
> Обнаружен подтвержденный двунаправленный цикл.

### Подтвержденный цикл: `exec_spec.rs` ↔ `workflow/contract.rs`

```
exec_spec.rs:106  → crate::workflow::contract::TaskClass   (parse_task_class)
workflow/contract.rs:82+ → crate::exec_spec::{ExecSpec, StepSpec, Constraint, ...}
```

**Это нарушает архитектурный инвариант.** ExecSpec — это ядро, workflow — это legacy.

### Нарушения boundary (не циклы, но coupling):

| Связь | Статус |
|:------|:-------|
| `scheduler.rs` → `workflow::contract` | 🟡 legacy coupling |
| `worker.rs` → `workflow::contract` | 🟡 legacy coupling |
| `effects.rs` → `scheduler`, `worker` | 🔴 runtime coupling |
| `reconstruction` → `worker` | 🟡 acceptable (capability check) |
| `reconstruction` → `replay` | 🟢 acceptable (one-directional) |

### Чистые границы:
- ✅ `scheduler` ↛ `worker`
- ✅ `worker` ↛ `scheduler`
- ✅ `event_bus` ↛ `scheduler` | `worker`
- ✅ `replay` ↛ `reconstruction`

---

## 5. Доменная логика в ядре

> [!CAUTION]
> Хардкод доменных строк в ядре делает его непригодным для использования как generic execution engine.

### `worker.rs` — `parse_step_kind_from_step_id()` (13 хардкодов)
- `"tighten_planner_prompt"`, `"normalize_planner_output"`, `"add_llm_fallback_handling"`, `"add_planner_test_coverage"`, `"validate_planner_output"`, `"analyze_task"`, `"plan_execution"`, `"execute_changes"`, `"read_repository"`, `"locate_bug"`, `"patch_code"`, `"run_tests"`, `"validate_patch"`

### `effects.rs` — `step_from_step_id()` (13 хардкодов)
**ДУБЛИКАТ** маппинга из `worker.rs` — та же самая доменная логика скопирована.

### `effects.rs` — `default_worker_for_step()` (хардкод)
- `"analyze_task" | "plan_execution"` → `"worker-planner"`
- `"execute_changes" | "patch_code"` → `"worker-executor"`
- `"validate_patch"` → `"worker-verifier"`

### `exec_spec.rs` — `parse_task_class()` (хардкод)
- `"Generic"`, `"PlannerHardening"`, `"CodeFix"`

### `execution/runtime.rs` — прямая проверка `StepKind::AnalyzeTask`

---

## 6. Скрытый недетерминизм

| Файл | Строка | Источник | Критичность |
|:-----|:------:|:---------|:-----------:|
| `snapshot.rs` | L5, L133-136 | `SystemTime::now()` → wall-clock time в snapshot | 🔴 CRITICAL |
| `replay/capsule.rs` | L17 | `"now".to_string()` — заглушка timestamps | 🟡 PLACEHOLDER |

**Другие источники случайности НЕ обнаружены:**
- ✅ Нет `Uuid::new_v4`
- ✅ Нет `rand` / `thread_rng` / `OsRng`
- ✅ Нет `chrono::Utc::now`
- ✅ Lease IDs генерируются детерминистически

---

## Сводная таблица

| Категория | Критичность | Кол-во | Действие |
|:----------|:----------:|:------:|:---------|
| Direct SQL в runtime | 🔴 CRITICAL | ~85 вызовов | Создать StorageProvider trait |
| Direct filesystem | 🟡 MODERATE | ~12 мест | Перевести на FilesystemProvider |
| Direct LLM coupling | 🟡 MODERATE | 3 нарушения | Перевести на Provider trait |
| Циклическая зависимость | 🔴 CRITICAL | 1 цикл | Вынести parse_task_class из exec_spec |
| Доменная логика в ядре | 🔴 CRITICAL | 6+ модулей | Вынести в Compatibility Adapter |
| Скрытый недетерминизм | 🔴 CRITICAL | 1 место | Заменить SystemTime на generation-based timestamp |

---

## Защитные меры (реализованы в этой фазе)

1. **KernelError** — единый тип ошибок ядра с 5 категориями
2. **ExecSpec::validate()** — структурная валидация спецификации
3. **ExecSpec::validate_hash()** — проверка целостности хэша
4. **Architecture tests** — тесты изоляции ядра от runtime
5. **Determinism tests** — тесты стабильности хэширования и сериализации
6. **KERNEL_INVARIANTS.md** — формальная фиксация правил
