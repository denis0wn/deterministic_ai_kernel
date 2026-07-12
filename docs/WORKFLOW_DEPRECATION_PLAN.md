# Workflow Deprecation and Migration Plan

В данном документе описывается стратегия перехода от workflow-ориентированного ядра (Workflow Engine) к обобщенному ядру исполнения (Generic Execution Kernel).

---

## 1. Что заменяет Workflow?

* **ExecSpec (Execution Specification):** Декларативное описание графа выполнения, примитивов, зависимостей и политик безопасности. Заменяет жестко зашитые последовательности `TaskClass`.
* **Execution Identity:** Строгие типы идентификаторов (`ExecutionId`, `PrimitiveId`, `CapabilityId`) вместо доменных строк.
* **Compiler Layer:** Модуль компилятора (`workflow/compiler.rs`), который переводит входящие доменные требования в чистый, неизменяемый `ExecSpec`.

---

## 2. Депрекация и удаление устаревших API

Ниже приведен список устаревших API, которые подлежат удалению в следующей мажорной версии:

| Legacy API | Заменяющий API | Срок удаления |
| :--- | :--- | :--- |
| `TaskClass` | `ExecSpec` | Мажорный релиз v2.0 |
| `StepKind` | `PrimitiveKind` | Мажорный релиз v2.0 |
| `Workflow::build_steps` | `Workflow::compile` | Мажорный релиз v2.0 |
| `replay` | `reconstruction` | Мажорный релиз v2.0 |

---

## 3. Руководство по миграции для пользователей

### Шаг 1: Переход от TaskClass к ExecSpec
Вместо создания задач с указанием класса задачи:
```rust
// Раньше:
INSERT INTO tasks (task_id, task_class) VALUES ('task1', 'Generic');
```
Пользователям необходимо скомпилировать и передавать сериализованную спецификацию:
```rust
// Теперь:
let spec = Workflow::compile(&TaskInput::generic(""));
let spec_json = serde_json::to_string(&spec).unwrap();
INSERT INTO tasks (task_id, task_class, exec_spec) VALUES ('task1', 'Generic', spec_json);
```

### Шаг 2: Использование Primitive-событий в EventBus
Вместо прослушивания событий `STEP_COMPLETED` и `STEP_FAILED`, переключите обработчики на новые типы:
* `PrimitiveScheduled` (ранее `STEP_DISPATCHED`)
* `PrimitiveStarted` (ранее `STEP_STARTED`)
* `PrimitiveCompleted` (ранее `STEP_COMPLETED`)
* `PrimitiveFailed` (ранее `STEP_FAILED`)
* `ArtifactProduced` (ранее `EFFECT_RESERVED`)
