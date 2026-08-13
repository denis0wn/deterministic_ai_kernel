# dek — Deterministic AI Kernel Launcher

Shell-лаунчер для запуска задач через deterministic_ai_kernel CLI.

## Установка

Добавьте в `~/.zshrc`:

```bash
dek() {
    source /Users/denissmoliakov/Projects/deterministic_ai_kernel_clean/scripts/dek.sh "$@"
}
```

Или выполните `source ~/.zshrc` после добавления.

## Использование

```bash
# С аргументом — задача передаётся сразу
dek "Объясни что такое event sourcing"

# Без аргумента — интерактивный ввод
dek
# Enter task: напиши hello world на Rust
```

## Поток работы

1. **Выбор провайдера** — интерактивное меню:
   - MLX server (работает) — модель берётся из `config/model_manifest.json`
   - LM Studio — требует восстановления интеграции (сейчас не поддерживается)
   - Ollama — требует реализации LlmProvider (сейчас не поддерживается)

2. **Проверка MLX runtime** — если mlx_lm.server уже запущен на порту из `.env`, используется существующий. Если нет — запускается автоматически.

3. **Запуск pipeline-run** — задача отправляется в ядро через `cargo run --bin deterministic_ai_kernel -- pipeline-run --payload "..." --json`.

4. **Вывод результата** — парсит cli-json-v1 envelope, выводит читаемо (Plan ID, Seed, Steps, Final answer).

5. **Очистка** — если сервер был запущен скриптом, он останавливается при завершении (включая Ctrl+C).

## Пример вывода

```
Select LLM provider:
  1) MLX server (mlx_lm.server) — supported, model from manifest
  2) LM Studio — requires provider restoration (not supported by kernel)
  3) Ollama — requires new LlmProvider implementation (not supported by kernel)

[dek] model: /Users/denissmoliakov/Models/gemma4-reasoning
[dek] MLX runtime already running.
[dek] running pipeline-run (seed=1784808615)...

=== Result ===
Plan ID:   plan-abc123
Seed:      1784808615
Steps:     3
Elapsed:   1234 ms

Final answer:
Event sourcing — это паттерн...
```

## Зависимости

- `jq` — парсинг JSON
- `curl` — проверка MLX runtime
- `cargo` — сборка и запуск ядра
- `mlx_lm.server` — MLX runtime (для провайдера MLX)

## Файлы

- `scripts/dek.sh` — основной скрипт
- `.env` — переменные окружения (OPENAI_BASE_URL, OPENAI_MODEL)
- `config/model_manifest.json` — манифест моделей по ролям
