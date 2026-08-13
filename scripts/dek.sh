#!/usr/bin/env bash
# dek.sh — Deterministic AI Kernel launcher
# Usage: dek [optional task text]
# Requires: jq, curl, cargo, mlx_lm.server (for MLX provider)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
ENV_FILE="$PROJECT_DIR/.env"
MANIFEST="$PROJECT_DIR/config/model_manifest.json"
DEK_PID_FILE=""

# ── cleanup on exit / SIGINT / SIGTERM ────────────────────────────────────────
cleanup() {
    if [[ -n "$DEK_PID_FILE" ]] && [[ -f "$DEK_PID_FILE" ]]; then
        local pid
        pid="$(cat "$DEK_PID_FILE" 2>/dev/null || true)"
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            echo "[dek] shutting down mlx_lm.server (pid=$pid)..."
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            echo "[dek] server stopped."
        fi
        rm -f "$DEK_PID_FILE"
    fi
}
trap cleanup EXIT
trap cleanup INT TERM

# ── helpers ───────────────────────────────────────────────────────────────────
die() { echo "[dek] ERROR: $*" >&2; exit 1; }

load_env() {
    if [[ -f "$ENV_FILE" ]]; then
        set -a; source "$ENV_FILE"; set +a
    else
        die ".env not found at $ENV_FILE"
    fi
}

# Read model path for a role from config/model_manifest.json
model_path_for_role() {
    local role="$1"
    jq -r --arg role "$role" \
        '.models[] | select(.role == $role and .enabled == true) | .id' \
        "$MANIFEST" | head -1
}

# ── step 2: interactive provider selection ────────────────────────────────────
select_provider() {
    echo ""
    echo "Select LLM provider:"
    echo "  1) MLX server (mlx_lm.server) — supported, model from manifest"
    echo "  2) LM Studio — requires provider restoration (not supported by kernel)"
    echo "  3) Ollama — requires new LlmProvider implementation (not supported by kernel)"
    echo ""
    local choice
    read -rp "Choice [1]: " choice || true
    choice="${choice:-1}"

    case "$choice" in
        1) PROVIDER="mlx" ;;
        2)
            echo ""
            echo "[dek] LM Studio integration was removed from the kernel (see src/lm_control.rs:334)."
            echo "      To use LM Studio, restore the LlmProvider implementation in src/providers/mod.rs."
            echo "      Aborting."
            exit 1
            ;;
        3)
            echo ""
            echo "[dek] Ollama integration does not exist in this kernel."
            echo "      To use Ollama, implement the LlmProvider trait in src/providers/mod.rs."
            echo "      Aborting."
            exit 1
            ;;
        *) die "invalid choice: $choice" ;;
    esac
}

# ── step 3a: probe MLX runtime ────────────────────────────────────────────────
probe_mlx() {
    local base_url="${OPENAI_BASE_URL:-http://127.0.0.1:8080/v1}"
    local url="${base_url%/}/models"
    curl -s --max-time 3 "$url" 2>/dev/null || true
}

# ── step 3b: start mlx_lm.server ──────────────────────────────────────────────
start_mlx_server() {
    local model_path="$1"
    local port
    port="$(echo "$OPENAI_BASE_URL" | sed -E 's|.*:([0-9]+)/.*|\1|')"
    port="${port:-8080}"

    echo "[dek] starting mlx_lm.server on port $port..."
    DEK_PID_FILE="$(mktemp /tmp/dek_mlx_pid.XXXXXX)"

    nohup mlx_lm.server \
        --model "$model_path" \
        --port "$port" \
        --decode-concurrency 1 --prompt-concurrency 1 \
        > /tmp/dek_mlx_server.log 2>&1 &
    echo $! > "$DEK_PID_FILE"
    echo "[dek] mlx_lm.server started (pid=$(cat "$DEK_PID_FILE"))"
}

# ── step 3c: wait for server readiness ────────────────────────────────────────
wait_for_mlx() {
    local timeout_secs=30
    local elapsed=0
    local base_url="${OPENAI_BASE_URL:-http://127.0.0.1:8080/v1}"
    local url="${base_url%/}/models"

    echo -n "[dek] waiting for MLX runtime"
    while (( elapsed < timeout_secs )); do
        if curl -s --max-time 2 "$url" 2>/dev/null | grep -q '"data"'; then
            echo " ready (${elapsed}s)"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
        echo -n "."
    done
    echo " TIMEOUT"
    return 1
}

# ── step 5: parse pipeline-run JSON output ────────────────────────────────────
parse_pipeline_output() {
    local json="$1"
    local ok
    ok="$(echo "$json" | jq -r '.ok // false')"
    if [[ "$ok" != "true" ]]; then
        echo "[dek] pipeline-run failed:"
        echo "$json" | jq . 2>/dev/null || echo "$json"
        return 1
    fi
    echo ""
    echo "=== Result ==="
    echo "$json" | jq -r '
        "Plan ID:   \(.report.plan_id // "n/a")",
        "Seed:      \(.report.seed // "n/a")",
        "Steps:     \(.report.step_count // (.report.steps | length) // "n/a")",
        "Elapsed:   \(.report.elapsed_ms // "n/a") ms",
        "",
        "Final answer:",
        .report.final_answer // "n/a"
    ' 2>/dev/null || {
        echo "[dek] could not parse structured output, showing raw:"
        echo "$json" | jq . 2>/dev/null || echo "$json"
    }
}

# ── main ──────────────────────────────────────────────────────────────────────
main() {
    load_env

    # Step 1: get task text
    local task_text="${1:-}"
    if [[ -z "$task_text" ]]; then
        echo -n "Enter task: "
        read -r task_text
    fi
    [[ -z "$task_text" ]] && die "no task provided"

    # Step 2: provider selection
    select_provider

    if [[ "$PROVIDER" != "mlx" ]]; then
        die "only MLX server is currently supported"
    fi

    # Step 3a: read model path from manifest
    local model_path
    model_path="$(model_path_for_role "coding_assistant")"
    [[ -z "$model_path" ]] && die "no enabled coding_assistant model in manifest"
    echo "[dek] model: $model_path"

    # Step 3b: probe runtime
    local probe_result
    probe_result="$(probe_mlx)"
    local server_already_running=false
    if echo "$probe_result" | grep -q '"data"'; then
        echo "[dek] MLX runtime already running."
        server_already_running=true
    else
        # Step 3b: start server
        start_mlx_server "$model_path"
        # Step 3c: wait for readiness
        if ! wait_for_mlx; then
            die "MLX server failed to start within 30s. Check /tmp/dek_mlx_server.log"
        fi
    fi

    # Step 4: run pipeline
    local seed
    seed="$(date +%s)"
    echo "[dek] running pipeline-run (seed=$seed)..."
    local tmp_output
    tmp_output="$(mktemp /tmp/dek_pipeline.XXXXXX)"
    local exit_code=0
    cd "$PROJECT_DIR" && cargo run --bin deterministic_ai_kernel -- \
        pipeline-run --payload "$task_text" --seed "$seed" --json \
        > "$tmp_output" 2>&1 || exit_code=$?
    cd "$PROJECT_DIR"

    if [[ $exit_code -ne 0 ]]; then
        echo "[dek] pipeline-run exited with code $exit_code"
        head -5 "$tmp_output" >&2
        rm -f "$tmp_output"
        exit 1
    fi

    # Extract JSON envelope (last line starting with {)
    local pipeline_json
    pipeline_json="$(grep '^{' "$tmp_output" | tail -1)"
    rm -f "$tmp_output"

    if [[ -z "$pipeline_json" ]]; then
        die "no JSON output from pipeline-run"
    fi

    # Step 5: parse and display result
    parse_pipeline_output "$pipeline_json"

    # Step 6: shutdown server (only if we started it)
    if [[ "$server_already_running" == "false" ]]; then
        cleanup
    else
        echo "[dek] MLX server was already running — leaving it up."
    fi
}

main "$@"
