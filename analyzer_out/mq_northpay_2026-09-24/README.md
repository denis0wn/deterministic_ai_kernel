# NorthPay live-model probe — 2026-09-24

Первое машинное доказательство с живой моделью, сохранённое в репозиторий, а не в `/tmp`.
Предыдущая доказательная база (`/tmp/dek_ai_matrix/model_quality_experiment_mq{1..5}/`) утрачена;
harness mq1–mq5 в репозитории отсутствует, поэтому это **новый** зондирующий прогон, а не
воспроизведение старого эксперимента.

## Что запускалось

- Ветка `analyzer` @ `caa9684` (после мерджа с `orchestrator-rebuild` и security-фиксов).
- Бинарь `target/debug/deterministic_ai_kernel`, `pipeline-run`, вызываемый **напрямую**
  (не через cargo), поэтому `MLX_LIFECYCLE` активен и ядро само поднимает `mlx_lm.server`.
- Модель `~/Models/gemma4-reasoning` (6.3 GB), `OPENAI_BASE_URL` по умолчанию из `.env`.
- Фикстура `analyzer_examples/client_northpay` копировалась в отдельный workspace на каждый сид;
  `DAK_CODEFIX_WORKSPACE` указывал на неё.
- Payload — каноническая форма CodeFix из `tests/4h_router_forensics.rs:85-91`
  (`Step 1 read repository … Step 6 validate patch` + описание дефекта).
- Дефект: `proportional_refund` в `clearing/fees.py:25` усечкает вместо округления HALF-UP
  (`int(raw * 100) / 100`), контракт в докстроке: fee 1.0, share 1 of 8 → 0.125 → **0.13**.

Предварительно `llm-smoke` → `LLM_SMOKE_OK` / `MODEL_RESPONSE: OK`, exit 0.

## Результаты

| Сид | `patch_shape_validation` | `repair_report` | Применённое изменение | `TASK_STATE` |
|---|---|---|---|---|
| 42 (зонд) | ok | absent | `round(raw * 100 + 0.5) / 100` | failed (tests_failed) |
| 100 | ok | absent | `int(raw * 100 + 0.5) / 100` | **completed** |
| 101 | ok | absent | `int(raw * 100 + 0.5) / 100` | **completed** |
| 102 | ok | absent | `Decimal(str(raw*100)).quantize(Decimal('1'), ROUND_HALF_UP) / 100` | failed (tests_failed) |
| 102 + `DAK_PATCH_ESCAPE_REPAIR=off` | ok | n/a | то же | failed (tests_failed) |

- **`malformed_patch`: 0 случаев из 5 прогонов.** Проверено по логам и по персистированным
  артефактам `02_patch_code` в SQLite каждого прогона.
- **`patch_repair` не участвовал**: поля `repair_report`/`patch_repair`/`escape_repair` в
  артефактах нет ни на одном сиде, и A/B с `DAK_PATCH_ESCAPE_REPAIR=off` даёт идентичный
  результат. То есть «0 malformed» — не следствие молчаливого починивания.
- Все проходы `00_read_repository → 05_validate_patch` видны в `STEP_STATUS`; на сидах 100/101
  цепочка дошла до `completed`, на 102 — честный отказ на `04_run_tests`.
- Инвариант честности отработал вживую: seed 102 →
  `fatal: real tests failed with exit code 1 (classification=tests_failed; no fabricated success)`,
  `TERMINAL_ASSESSMENT: InProgress (baseline or inconclusive attempt)`. Никакого fabricated success.

## Семантика отказов

Сиды 100/101 дали `int(raw * 100 + 0.5) / 100` — корректный HALF-UP для неотрицательных
значений, тесты фикстуры прошли. Seed 102 дал `Decimal`-версию, которая **возвращает `Decimal`,
а не `float`**, поэтому сравнения в `test_fees.py` не проходят. Это содержательный
семантический отказ хорошо сформированного патча, а не дефект формата.

## Ограничения — читать перед тем, как на это ссылаться

1. **Класс дефекта из mq1–mq3 здесь НЕ проверялся.** Документированная сигнатура —
   «capitalized directory in the patch target path» (`ANALYZER_ROADMAP.md:150-152`). Все мои
   workspace-пути строчные, триггер отсутствует. Утверждать, что тот дефект исправлен,
   по этим данным **нельзя** — можно утверждать только то, что он не возникает в этом сценарии.
2. **n = 3 сида на основной серии.** Для Layer 2 этого мало: условие возобновления требует
   *воспроизводимого* исполняемого семантического отказа. Seed 102 — один экземпляр нужного
   класса (валидный патч → применился → реальные тесты упали по семантике), то есть класс
   продемонстрирован, но частота не установлена.
3. **Причина отличия от 12/12 не установлена.** Кандидаты: более строгий промпт, пришедший из
   `orchestrator-rebuild` (`"target_file":"{target}"` + «copied unchanged», `src/llm.rs`),
   другая версия модели/сервера относительно августа, форма payload. A/B по промпту не проводился.
4. **Независимая проверка тестов фикстуры не выполнена**: в системном python3.14 нет `pytest`
   (`No module named pytest`). Авторитет здесь — собственный runpy-харнесс ядра
   (`src/tools/test_runner.rs:55`), а не внешний pytest.
5. Payload свободной формы (первый зонд, без `Step 1..6`) роутится в немутирующий поток и даёт
   `TASK_STATE: completed` **без изменения файла**. Это ожидаемое поведение роутера, а не дефект,
   но операторский риск: «completed» без мутации легко принять за успех. Зафиксировано в
   `probe_freeform.out`.

## Файлы

- `seed{42,100,101,102}.out` — полные логи `pipeline-run`
- `seed102_repairoff.out` — A/B с выключенным escape-repair
- `probe_freeform.out` — зонд payload'ем свободной формы (немутирующий поток)
- `diffs.txt` — фактические изменения `clearing/fees.py` по каждому сиду
