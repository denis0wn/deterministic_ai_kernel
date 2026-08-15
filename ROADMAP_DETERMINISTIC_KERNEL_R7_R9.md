# Roadmap — Deterministic Local LLM Kernel (R7–R9)

## 0. Current status

- Kernel: deterministic orchestration, PatchV1, ApplyPatch, RunTests, validation gate.
- Closed phases: R3 (router, HD-1), R4 (stall diagnostics, HD-3), HD-2 (grounding, частично), SOAK, R5 (prose grounding), OPS-1 (TUI), R6 (streaming+idle timeout).
- Status: PRODUCTION CANDIDATE (not PRODUCTION-READY) — модель всё ещё может галлюцинировать вне покрытых классов, reasoning-hang остаётся свойством модели/сервера.
- Known debt, влияющий на статус (см. раздел 4): подтверждённый HIGH в `shell_tools.rs` (`sh -c` с обходимой readonly-эвристикой), мёртвый код исполнения команд в `execution_engine.rs` (finding C5).
- Known volatility: вся доказательная база (capability-матрикс, probe-батареи, soak-данные) живёт в `/tmp/dek_ai_matrix` и переживает только текущую машину/сессию — перенос в репо обязателен до сертификации (см. R9).

## 1. R7 — Server wedge / connection pooling

### Goal
Reduce risk of single-flight wedge: один зависший запрос не должен блокировать все остальные.

### Architecture decision first (обязательно до реализации)
mlx_lm.server однопоточен по обработке запросов: несколько соединений к ОДНОМУ инстансу всё равно сериализуются — pooling без этого решения закрывает задачу формально, не фактически. Зафиксировать выбор с логами:
- (а) несколько инстансов сервера на разных портах (kernel-side endpoint pool), и/или
- (б) очередь с отменой: kernel должен уметь ABORT-нуть зависший in-flight запрос (сегодня он просто бросается) и перезапустить его на свежем соединении, и/или
- (в) backpressure-политика при занятых соединениях (детерминированная очередь vs детерминированный отказ).

### Tasks
- Implement connection pooling / multiple MLX client connections:
  - минимум два независимых соединения (или worker-пул / несколько инстансов сервера — по решению выше).
- Kernel policy:
  - on STALL_DETECTED / HARD_TIMEOUT_EXCEEDED: переключиться на свежий connection, сохранить детерминизм (решение принимает kernel, не ретрай на том же соединении).
  - явная отмена (abort) зависшего запроса до переключения.
- Tests:
  - injection hang (искусственный блок), убедиться, что другие задачи продолжают обслуживаться параллельно.
  - full 70-task acceptance, проверить B6/C12 и concurrency-кейсы.
- Artifacts:
  - /tmp/dek_ai_matrix/R7_IMPLEMENTATION_RECORD.md
  - r7_regression.log (5/5 команд exit 0).

## 2. R8 — RAG layer for AnswerQuestion

### Goal
Maximize truthfulness / reduce hallucinations by grounding in local knowledge instead of pure model memory.

### Invariants RAG обязан сохранить
- **Детерминизм retrieval:** стабильный индекс, детерминированный ranking, версия индекса (content hash) фиксируется в event log — иначе ломается главный инвариант ядра (недетерминированный контекст → недетерминированный ответ).
- **Prompt injection defense:** retrieved-документы — новый канал untrusted input. Документы подаются только как данные (маркированный контекст), никакие инструкции из retrieved-текста не исполняются и не меняют план.
- **Provenance grounding:** существующий grounding-гейт расширяется — претензии проверяются не только против task payload, но и против retrieved-контекста (claim должен буквально присутствовать в источнике).

### Tasks
- Build local document index (docs, standards, notes, code snippets).
- For AnswerQuestion tasks:
  - deterministic retrieval step (context compiler + search),
  - only retrieved context is given to the model.
- Kernel policy:
  - if no relevant documents: force refusal / "insufficient information" instead of free-form answer.
- Refusal-hardening (дешёвая промежуточная мера, измерить в R9): системный промпт «если факт не предоставлен — явно откажи» — до и после RAG, на текущем дырявом покрытии (исторические/персональные/организационные факты: P05/P19-формы, слабый отказ C8).
- Tests:
  - adversarial anti-hallucination cases (serials, RPM, technical facts, prose),
  - ensure RAG + existing grounding gates catch invented values,
  - инъекция вредоносного retrieved-документа (prompt injection) — план не меняется, эффекты не создаются.
- Artifacts:
  - /tmp/dek_ai_matrix/R8_IMPLEMENTATION_RECORD.md
  - r8_regression.log, updated capability matrix.

## 3. R9 — Formal benchmark suite (hallucination & determinism)

### Goal
Measure progress and compare configurations (models, decoding profiles, RAG on/off).

### Tasks
- Define fixed benchmark suite:
  - arithmetic, logic, scheduling, codefix, anti-hallucination, domain-specific technical questions.
- **Корпус в репозитории (обязательно):** перенести capability-матрикс и probe-батареи из `/tmp/dek_ai_matrix` в `tests/acceptance/` (версионируемо, воспроизводимо после перезагрузки); `/tmp` остаётся рабочей областью прогонов.
- Metrics:
  - correct answers,
  - honest refusals,
  - hallucinations (с трендом: baseline acceptance 3/12 fabrications → 0 записанных после HD-2/R5 — эти данные зафиксировать как отправную точку).
- Rigor:
  - pass/fail пороги как regression gate (бенч блокирует merge при деградации, а не просто отчитывается),
  - N прогонов на кейс с фиксацией вариативности (модель недетерминирована вне temp=0/seed),
  - фиксированные seeds и версия модели/сервера на каждый прогон,
  - явный источник ground truth для технических вопросов (иначе метрика невалидна).
- Configurations:
  - current gemma4-reasoning + streaming,
  - alternative models / decoding parameters (if added later).
- Schedule:
  - run before major changes and periodically (e.g. weekly).
- Artifacts:
  - /tmp/dek_ai_matrix/R9_BENCHMARK_RECORD.md
  - benchmark CSV + plots (optional).

## 4. Security & technical debt — до PRODUCTION-READY

Отдельный трек; рекомендуется до/параллельно с R7–R9, обязателен для снятия статуса PRODUCTION CANDIDATE.
- Починить подтверждённый HIGH: `shell_tools.rs` (`sh -c` с обходимой readonly-эвристикой) — убрать shell-исполнение из достижимых путей или сделать allowlist kernel-owned (аналог run_tests_v1).
- Карантин/удаление мёртвого кода исполнения команд (`execution_engine.rs`, finding C5): недостижимые из production-бинарей пути не должны компилироваться в достижимые артефакты.
- Security-гейт в критериях PRODUCTION-READY: 0 открытых подтверждённых HIGH/CRITICAL + повторный forensic-проход по новым поверхностям (R7 pooling, R8 RAG/prompt injection).
- Artifacts: SECURITY_HARDENING_RECORD.md + regression log.

## 5. Performance & latency control

B6/C12 после R6 завершаются честно, но ~120s на короткий вопрос — раздутая reasoning-цепочка модели. Направления (измерять через R9):
- per-purpose лимиты бюджета генерации (короче для AnswerQuestion, длиннее для PatchCode),
- промпт-шаблоны, подавляющие verbose-reasoning на простых вопросах,
- TTFT/throughput-метрики стриминга (уже есть основа: chunks/секунду в R6-инструментации),
- сравнение с альтернативными профилями декодирования.

## 6. Models & runtimes — future work

- Compare local runtimes on Apple Silicon (MLX vs MLC-LLM vs others):
  - TTFT, throughput, long-context behavior, streaming, batching.[*]
- Compare alternative local models:
  - quality vs speed vs memory.
- Medium-term: fine-tune on internal corpus (truthfulness + refusal).
- **Версионирование рантайма (обязательно для воспроизводимости):** пин версии mlx_lm.server (поведение keepalive/wedge зависит от версии — установлено экспериментально на 0.31.3) + BLAKE3-фиксация снапшота модели и индексов в каждом bench/acceptance-прогоне.

## 7. Ops & UX

- Simplified "production" TUI/CLI profile:
  - fixed timeouts, streaming, grounding, RAG on/off flags.
- User-centric tasks (photography/videography):
  - add domain-specific acceptance cases and RAG sources.
- **Ops-реестр (отсутствует):** единый документ всех env-переменных (`DAK_LLM_REQUEST_TIMEOUT_SECS`, `DAK_LLM_IDLE_TIMEOUT_SECS`, `DAK_LLM_STREAMING`, `DAK_CODEFIX_WORKSPACE`, `OPENAI_*`, `MLX_IDLE_TIMEOUT_SECS`) с дефолтами и семантикой; health-check эндпоинта; прогрев модели при старте; runbook для NEW-1-типа hang'ов; процедура смены модели.
- **Наблюдаемость:** latency-перцентили по задачам, token usage (база: llm_calls уже считается), экспорт метрик из event store.

## 8. Cross-cutting: CI & continuous verification

Сегодня 5-командный regression (test/fmt/clippy/check/release) и acceptance — ручные. До сертификации:
- автоматический regression на каждый change (локальный hook или CI),
- ночной R9-бенчмарк с автоблокировкой при деградации порогов,
- soak-профили (2h+) как периодический gate, а не разовое событие.

[*] Use external research for runtime comparison; do not assume results without logs/scripts.
