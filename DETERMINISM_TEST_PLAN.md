# Determinism Test Plan

Стратегия тестирования детерминизма Execution Kernel.

---

## 1. Категории тестов

### A. Serialization Stability
Проверяют, что сериализация и десериализация ключевых структур данных полностью детерминистична.

| Тест | Что проверяет | Файл |
|:-----|:-------------|:-----|
| `exec_spec_serialization_is_deterministic` | JSON вывод одинаков при повторной сериализации | `tests/determinism_guarantees.rs` |
| `exec_spec_hash_is_stable_after_roundtrip` | Хэш выживает JSON roundtrip | `tests/determinism_guarantees.rs` |
| `btreemap_key_order_is_stable` | BTreeMap гарантирует стабильный порядок ключей | `tests/determinism_guarantees.rs` |

### B. Identity Stability
Проверяют, что идентификаторы выполнения стабильны.

| Тест | Что проверяет | Файл |
|:-----|:-------------|:-----|
| `execution_id_stable_hash_is_deterministic` | ExecutionId::stable_hash() стабилен | `tests/determinism_guarantees.rs` |
| `primitive_id_deterministic_across_invocations` | PrimitiveId равенство стабильно | `tests/determinism_guarantees.rs` |
| `same_spec_produces_identical_hash_twice` | Одинаковые спецификации → одинаковый spec_id | `tests/determinism_guarantees.rs` |

### C. Tamper Detection
Проверяют, что модификация данных обнаруживается.

| Тест | Что проверяет | Файл |
|:-----|:-------------|:-----|
| `modified_spec_detected_by_hash` | Измененный spec → другой calculate_hash() | `tests/determinism_guarantees.rs` |
| `validate_hash_rejects_tampered_spec` | validate_hash() возвращает Err на tampered spec | `tests/determinism_guarantees.rs` |
| `validate_catches_missing_dependency_target` | validate() обнаруживает несуществующие зависимости | `tests/determinism_guarantees.rs` |

### D. Architecture Isolation
Проверяют, что kernel core не зависит от runtime/domain.

| Тест | Что проверяет | Файл |
|:-----|:-------------|:-----|
| `exec_spec_must_not_import_runtime` | exec_spec.rs не импортирует scheduler/worker/llm | `tests/kernel_hardening.rs` |
| `execution_abi_must_not_import_runtime` | execution_abi.rs чист от runtime imports | `tests/kernel_hardening.rs` |
| `execution_identity_must_not_import_runtime` | execution_identity.rs изолирован | `tests/kernel_hardening.rs` |
| `kernel_error_must_not_import_runtime` | kernel_error.rs не зависит от runtime | `tests/kernel_hardening.rs` |
| `providers_must_not_import_scheduler_or_worker` | providers/ не зависит от runtime | `tests/kernel_hardening.rs` |

### E. No Hidden Non-Determinism
Проверяют отсутствие скрытых источников случайности.

| Тест | Что проверяет | Файл |
|:-----|:-------------|:-----|
| `kernel_core_has_no_hidden_randomness` | Нет thread_rng/Uuid::new_v4/rand::random/OsRng | `tests/kernel_hardening.rs` |
| `kernel_core_has_no_direct_filesystem` | Нет std::fs/File::open/File::create | `tests/kernel_hardening.rs` |
| `kernel_core_has_no_direct_network` | Нет reqwest/hyper/TcpStream | `tests/kernel_hardening.rs` |

---

## 2. Принципы тестирования

### Determinism axiom:
```
∀ spec ∈ ExecSpec:
  serialize(spec) == serialize(deserialize(serialize(spec)))
  hash(spec) == hash(deserialize(serialize(spec)))
```

### Tamper detection axiom:
```
∀ spec ∈ ExecSpec, ∀ mutation M ≠ identity:
  hash(spec) ≠ hash(M(spec))
```

### Isolation axiom:
```
kernel_core ∩ runtime_imports = ∅
kernel_core ∩ domain_imports = ∅
kernel_core ∩ randomness_sources = ∅
```

---

## 3. Будущие тесты (запланированы)

| Тест | Описание | Приоритет |
|:-----|:---------|:---------:|
| `replay_equals_original_execution` | Replay пошагово повторяет original state transitions | 🔴 HIGH |
| `corrupted_event_log_detected` | Вставка поврежденного события в event_log → reconstruction fails | 🔴 HIGH |
| `concurrent_spec_execution_deterministic` | Параллельное исполнение одной спеки → одинаковые evidence | 🟡 MEDIUM |
| `snapshot_reconstruction_matches_full_replay` | Снапшот + дельта == полный replay | 🟡 MEDIUM |
| `provider_substitution_transparent` | Замена провайдера не меняет kernel behavior | 🟢 LOW |
