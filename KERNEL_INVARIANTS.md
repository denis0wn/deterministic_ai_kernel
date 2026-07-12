# Kernel Invariants — Неизменяемые правила ядра

Данный документ фиксирует правила, которые **НИКОГДА** не должны нарушаться в Execution Kernel.

---

## 1. Source of Truth

> [!CAUTION]
> Нарушение Source of Truth правил приводит к невозможности Reconstruction и потере детерминизма.

| Слой | Source of Truth | Описание |
|:-----|:----------------|:---------|
| Спецификация | `ExecSpec` | Единственный источник правды о графе выполнения, зависимостях, политиках |
| Идентификация | `ExecSpec.spec_id` | BLAKE3 хэш spec → неизменяем после создания |
| Событийная история | `event_log` таблица | Append-only лог, определяющий состояние через реконструкцию |
| Эффекты | `effect_ledger` таблица | Детерминистическое отслеживание побочных эффектов |
| Артефакты | `artifact_registry` | Неизменяемый реестр с версионированием |

**Правило:** Runtime **НИКОГДА** не является Source of Truth. Состояние всегда восстанавливается из Event Log + ExecSpec.

---

## 2. Immutable Data

Следующие данные **ЗАПРЕЩЕНО** мутировать после создания:

- **ExecSpec** — после вычисления `spec_id`, спецификация заморожена
- **Event Log записи** — append-only, ни одна запись не может быть удалена или изменена
- **Artifact Registry записи** — append-only, версии неизменяемы
- **Replay Capsule** — snapshot состояния на момент фиксации, неизменяем
- **ExecutionId** — детерминистически производный от входных данных
- **PrimitiveId** — детерминистически производный от спецификации примитива

---

## 3. Canonical Events

Каноничные типы событий ядра:

### Execution Primitive Events (новая модель)
| Событие | Семантика |
|:--------|:----------|
| `PrimitiveScheduled` | Примитив помещен в очередь выполнения |
| `PrimitiveStarted` | Воркер начал выполнение примитива |
| `PrimitiveCompleted` | Примитив завершен успешно |
| `PrimitiveFailed` | Примитив завершен с ошибкой |
| `ArtifactProduced` | Побочный эффект зарезервирован |

### Legacy Events (сохранены для совместимости)
| Событие | Маппинг |
|:--------|:--------|
| `STEP_DISPATCHED` | → `PrimitiveScheduled` |
| `STEP_STARTED` | → `PrimitiveStarted` |
| `STEP_COMPLETED` | → `PrimitiveCompleted` |
| `STEP_FAILED` | → `PrimitiveFailed` |
| `EFFECT_RESERVED` | → `ArtifactProduced` |

### Инфраструктурные события
| Событие | Семантика |
|:--------|:----------|
| `LEASE_ACQUIRED` | Лизинг на примитив получен |
| `LEASE_EXPIRED` | Лизинг истек |
| `WORKER_CLAIMED` | Воркер заявил владение |

---

## 4. Reconstruction Guarantees

Следующее **ДОЛЖНО** быть восстановимо из Event Log + ExecSpec:

- Полное состояние каждого шага/примитива (pending/dispatched/committed/rejected)
- Порядок завершения зависимостей (parent completed before child)
- Целостность хэша спецификации (spec_id == calculated_hash)
- Соответствие возможностей воркера и требований примитива
- Баланс эффектов (reserved → committed | rejected, ни одного orphaned)

**Правило:** Если состояние нельзя восстановить из Event Log — это баг ядра, а не runtime.

---

## 5. Forbidden Dependencies

> [!WARNING]
> Нарушение dependency boundaries вводит недетерминизм и делает reconstruction невозможной.

### Ядро НЕ ДОЛЖНО зависеть от:
- **Git** — реализация контроля версий принадлежит Provider слою
- **Filesystem implementation** — реализация FS принадлежит Provider слою (`FilesystemProvider`)
- **LLM implementation** — реализация LLM принадлежит Provider слою (`LlmProvider`)
- **HTTP/Network** — сетевые вызовы принадлежат Provider слою
- **System clock** — в ядре запрещены вызовы `SystemTime::now()`, `Instant::now()`, `chrono::Utc::now()`
- **Random без seed** — в ядре запрещены `thread_rng()`, `Uuid::new_v4()`

### Запрещенные циклические зависимости:
```
scheduler.rs ✗→ worker.rs
worker.rs ✗→ scheduler.rs
exec_spec.rs ✗→ scheduler.rs | worker.rs
event_bus.rs ✗→ scheduler.rs | worker.rs
reconstruction/ ✗→ scheduler.rs | worker.rs
execution_abi/ ✗→ scheduler.rs | worker.rs | workflow/
```

### Разрешенный граф зависимостей:
```
execution_abi → (standalone)
exec_spec → execution_abi
execution_identity → (standalone)
event_bus → (standalone, SQLite only)
providers → (standalone, trait definitions)
reconstruction → exec_spec, event_bus, replay
scheduler → exec_spec, workflow::contract
worker → exec_spec, workflow::contract, providers
```

---

## 6. Determinism Contract

> [!IMPORTANT]
> Ядро является детерминистическим. Одни и те же входные данные ВСЕГДА должны порождать одну и ту же Evidence.

- `ExecSpec::calculate_hash()` — стабилен для одинаковых входных данных
- `ExecutionId::stable_hash()` — стабилен для одинаковых строк
- `serde_json::to_vec()` + BTreeMap → стабильный порядок ключей
- BLAKE3 хэширование → детерминистично
- Порядок шагов в ExecSpec → определяется топологически + по приоритету
