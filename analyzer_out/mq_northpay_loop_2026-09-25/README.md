# Layer-2 POC — live validation (Arm A) — 2026-09-25

Петля verifier-driven feedback (`src/execution/feedback.rs`) на живой
gemma4-reasoning, фикстура NorthPay, сиды 42/100/101/102/103, payload
идентичен `mq_northpay_2026-09-25` (контрольное плечо: та же серия без
петли = 0/14).

## Результат: конверсия 0/5 — петля работает, модель не использует сигнал

| Seed | Попыток | Исход | Динамика патчей |
|---|---|---|---|
| 42 | 2 | budget_exhausted | round+0.5 → Decimal.quantize → Decimal.quantize v2 (все неверны) |
| 100 | 2 | **identical_patch** | Decimal → тот же Decimal (temp-0 futility stop сработал вживую) |
| 101 | 2 | budget_exhausted | round+0.5 → патч от def-строки → Decimal.quantize |
| 102 | 2 | patch_error | Decimal → попытка 2: malformed patch (EOF in string), model-repair не спас |
| 103 | 1 | patch_error | попытка 1: malformed patch (unsupported escape), model-repair не спас |

## Что доказано о петле (механика — полностью зелёная)

- Цикл открывается ровно на `tests_failed` с именами (C0), имена
  обновляются между попытками из свежего отчёта (seed 42: attempt 2
  получил новый набор имён от attempt 1).
- Rollback к pre-image read-артефакта работает (каждая попытка патчит
  исходный файл).
- Дедупликация по blake3 патча останавливает futile-ретрай (seed 100).
- Честность не расширена: все 5 задач — терминальный
  `classification=tests_failed; no fabricated success`.
- События FEEDBACK_* и per-attempt артефакты (feedback_attempt)
  персистятся; llm_calls (C2) записываются на каждой попытке.

## Что доказано о модели (главный результат)

- С located-rung feedback (только имена тестов) модель **консистентно
  разворачивается в Decimal** — и продолжает падать на return-type
  контракте. Сигнал доходит (патчи меняются), способности исправить
  класс нет.
- Feedback-промпт **деградирует JSON-compliance**: 2/5 прогонов дали
  malformed patch на feedback-попытках (вне петли — 0/19 за два дня).
  Маленькая выборка, но сигнал для Layer 1: feedback-блок повышает
  нагрузку на формат.

## Вердикт POC

Инфраструктура Layer 2 корректна и остаётся в ядре (kill switch
`DAK_FEEDBACK_LOOP=off`). Конверсия над контролем = 0. **Блокер пилота —
качество модели, не петля.** По дисциплине work-selection (Analyzer
Roadmap): дальнейшая работа над петлёй не оправдана до появления модели,
способной использовать verifier signal; переход к Layer 3 оправдан ровно
настолько же — петля дала честное «нет» на вопрос «достаточно ли
located-rung feedback для этой модели».

## Баги, найденные и пофикшенные по ходу POC (в этом же коммите)

1. **Stale __pycache__** маскировал свежие патчи: CPython валидирует pyc
   по (mtime-секунды, размер); re-patch в ту же секунду с той же длиной
   давал прогон тестов против СТАРОГО кода. Харнесс теперь чистит
   __pycache__ под workspace и ставит dont_write_bytecode.
   (Регрессионный тест `stale_pycache_does_not_shadow_repatched_source`.)
2. **Латентный stale-read в `find_latest_*`** (effects.rs): helpers
   итерировали `.rev()` по DESC-списку = возвращали САМЫЙ СТАРЫЙ
   артефакт. До петли (один отчёт на задачу) не проявлялось; с петлёй
   validate_patch читал исходный падающий отчёт после конверсии.
   Флипнуто на newest-first; полный набор 922/0.

## Файлы

- `run_series_loop.sh` — harness Arm A (5 сидов, loop on)
- `seed{42,100..103}.out` / `.diff` — логи и финальные патчи
- `series.log`, `llm_smoke.out`
- Unit/integration POC: `tests/feedback_loop_poc.rs` (4 теста:
  conversion, honest exhaustion, identical-patch stop, kill switch)
