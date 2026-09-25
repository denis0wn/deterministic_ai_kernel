# Layer-1 hints v2 — measurement series — 2026-09-25

Те же 14 сидов NorthPay, тот же payload, **новые money-truncation хинты**
(после E0: v1 конвертировали 0/14 — Decimal return-type + галлюцинация
`round(rounding=)`). Петля ВЫКЛЮЧЕНА (`DAK_FEEDBACK_LOOP=off`) — чистый
A/B против серий без хинтов и v1. Бинарь `406bf4e`+.

## Результат: 7/14 completed (v1: 0/14, без хинтов: 0/14)

- Completed: 42, 100, 101, 107, 108, 111, 112 — все выдали корректный
  фикс по рецепту v2: `Decimal` арифметика + `quantize(ROUND_HALF_UP)` +
  `float(...)` на возврате (return-type контракт соблюдён).
- Failed: 102–106, 109, 110 — **все 7 одним классом: `malformed patch:
  invalid escape at line 1 column 290`** (форматный класс C4, не
  семантика). `patch_repair` сработал, но не покрыл форму
  («unsupported escape … no R1 pattern»), model-retry тоже не спас.

## Выводы

1. **Семантический класс (валидный патч → тесты падают) устранён хинтами
   v2 полностью: 0/14** (было 14/14 без хинтов, 14/14 с v1).
   Return-type гард + запрет builtin `round(rounding=)` работают.
2. Слоистая модель подтверждена: Layer 1 (хинты) закрывает класс там,
   где знает рецепт; петля Layer 2 нужна для классов вне рецептов.
3. Открытая работа (не Layer 2): форматный escape-класс
   (`invalid escape @ col 290`, 7/14) — разрыв покрытия `patch_repair`
   (нет R1-паттерна под эту форму) + model-retry не спасает. Это
   расширение repair-паттернов, не feedback.

## Файлы

- `run_series_hints_v2.sh` — harness (тексты хинтов = `hint_engine.rs` v2)
- `seed{42,100..112}.out` / `.diff` — 14 логов и патчей
- `series.log` — сводка
