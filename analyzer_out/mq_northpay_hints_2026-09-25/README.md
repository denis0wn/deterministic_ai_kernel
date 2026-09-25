# E0 gate — Layer-1 hints vs the semantic-failure class — 2026-09-25

Гейт-эксперимент из `LAYER2_VERIFIER_FEEDBACK_SPEC.md` §3: та же 14-сидная серия
NorthPay, что и `mq_northpay_2026-09-25`, но с блоком Layer-1 хинтов
`money-truncation` (дословно из `src/analyzer/hint_engine.rs`) в конце payload.

## Результат: hints конвертируют 0/14

14/14 `TASK_STATE: failed, classification=tests_failed` — как и без хинтов.
`patch_shape_validation=ok` 14/14, `repair_report` absent 14/14 (SQLite,
все 14 db). Инвариант честности держался на всех прогонах.

**Вердикт гейта: Layer 1 класс НЕ устраняет → условие возобновления Layer 2
выполнено полностью. Спека переходит к пререквизитам C0–C3.**

## Что сделали хинты (важная находка по Layer 1)

Хинты не устранили класс, но **изменили его форму**: все 14 сидов дали
структурно одинаковый патч (вместо 3 вариантов без хинтов):

```python
from decimal import Decimal, ROUND_HALF_UP
ratio = Decimal(str(refunded_share)) / Decimal(str(total_share))
raw = Decimal(str(fee)) * ratio
return round(raw, 2, rounding=ROUND_HALF_UP)
```

Это падение **хуже**, чем без хинтов, и по двум независимым причинам:

1. **Галлюцинация API**: builtin `round()` не принимает `rounding=`
   (`TypeError: round() takes at most 2 arguments (3 given)`). Модель склеила
   `Decimal.quantize(rounding=...)` с `round(x, ndigits)`. Воспроизведено
   прогоном реального workspace (`/tmp/dek_mq25_hints/seed42`): тесты падают
   с TypeError, даже не доходя до assert'ов.
2. **Даже без TypeError** (`raw.quantize(...)`) патч возвращал бы `Decimal`,
   а контракт фикстуры требует float-совместимое сравнение — тот же класс,
   что и у Decimal-вариантов без-хинтовой серии.

Хинт №1 («use the `decimal` module») направил модель в ловушку: контракт
фикстуры требует float, а хинты не упоминают return-type контракт.
**Layer-1 импрувмент-айтем (не блокер Layer 2):** в money-truncation хинты
стоит добавить «сохрани тип возвращаемого значения исходной функции» и
запрет builtin `round(..., rounding=)` — легальный `rounding=` живёт только
в `Decimal.quantize`.

## Воспроизводимость внутри дня

14/14 структурно одинаковый патч при разных сидах — ещё одно наблюдение, что
при temp 0 исход определяется промптом, а сид влияет косвенно. (Сравнение
между днями по-прежнему конфаундировано реконструкцией payload — см.
ограничения `mq_northpay_2026-09-25/README.md`.)

## Файлы

- `run_series_hints.sh` — harness (payload с блоком Hints, дословные тексты хинтов)
- `seed{42,100..112}.out` / `.diff` — логи и патчи 14 прогонов
- `series.log`, `llm_smoke.out`
