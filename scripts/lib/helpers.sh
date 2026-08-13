#!/usr/bin/env bash
# Replay OS — shared helper functions
# Source this file, don't execute directly.

REPLAY_OS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_DIR="$(dirname "$REPLAY_OS_DIR")"

# Ensure ~/bin is in PATH for gum
export PATH="$HOME/bin:$PATH"
ENV_FILE="$PROJECT_DIR/.env"
MANIFEST="$PROJECT_DIR/config/model_manifest.json"
DAK_BIN="cargo run --bin deterministic_ai_kernel --"
MLX_PID_FILE="/tmp/replay_os_mlx.pid"
MLX_LOG="/tmp/replay_os_mlx.log"
HISTORY_DIR="$PROJECT_DIR/.replay_os/history"

# ── colors ────────────────────────────────────────────────────────────────────
C_RESET='\033[0m'
C_BOLD='\033[1m'
C_DIM='\033[2m'
C_RED='\033[31m'
C_GREEN='\033[32m'
C_YELLOW='\033[33m'
C_BLUE='\033[34m'
C_MAGENTA='\033[35m'
C_CYAN='\033[36m'
C_WHITE='\033[37m'

# ── env ───────────────────────────────────────────────────────────────────────
load_env() {
    if [[ -f "$ENV_FILE" ]]; then
        set -a; source "$ENV_FILE"; set +a
    fi
}

model_id_for_role() {
    local role="$1"
    jq -r --arg role "$role" \
        '.models[] | select(.role == $role and .enabled == true) | .id' \
        "$MANIFEST" 2>/dev/null | head -1
}

# ── dependency check ──────────────────────────────────────────────────────────
check_deps() {
    local missing=()
    command -v gum &>/dev/null || missing+=("gum")
    command -v jq &>/dev/null || missing+=("jq")
    command -v curl &>/dev/null || missing+=("curl")
    command -v cargo &>/dev/null || missing+=("cargo")

    if [[ ${#missing[@]} -gt 0 ]]; then
        printf "${C_RED}Missing dependencies: ${missing[*]}${C_RESET}\n"
        if [[ " ${missing[*]} " =~ " gum " ]]; then
            printf "${C_DIM}Install gum: https://github.com/charmbracelet/gum#installation${C_RESET}\n"
        fi
        return 1
    fi
    return 0
}

# ── runtime probe ─────────────────────────────────────────────────────────────
probe_runtime() {
    local base_url="${OPENAI_BASE_URL:-http://127.0.0.1:8080/v1}"
    curl -s --max-time 3 "${base_url%/}/models" 2>/dev/null || true
}

runtime_is_running() {
    probe_runtime | grep -q '"data"' 2>/dev/null
}

runtime_model_id() {
    probe_runtime | jq -r '.data[0].id // "unknown"' 2>/dev/null || echo "unknown"
}

runtime_pid() {
    if [[ -f "$MLX_PID_FILE" ]]; then
        cat "$MLX_PID_FILE" 2>/dev/null
    fi
}

# ── server management ─────────────────────────────────────────────────────────
start_mlx_server() {
    local model_path="${1:-$(model_id_for_role coding_assistant)}"
    local port
    port="$(echo "${OPENAI_BASE_URL:-http://127.0.0.1:8080/v1}" | sed -E 's|.*:([0-9]+)/.*|\1|')"
    port="${port:-8080}"

    nohup mlx_lm.server --model "$model_path" --port "$port" \
        --decode-concurrency 1 --prompt-concurrency 1 \
        > "$MLX_LOG" 2>&1 &
    echo $! > "$MLX_PID_FILE"
}

stop_mlx_server() {
    if [[ -f "$MLX_PID_FILE" ]]; then
        local pid
        pid="$(cat "$MLX_PID_FILE" 2>/dev/null)"
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null
            wait "$pid" 2>/dev/null
        fi
        rm -f "$MLX_PID_FILE"
    fi
}

wait_for_runtime() {
    local timeout=30 elapsed=0
    while (( elapsed < timeout )); do
        if runtime_is_running; then
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    return 1
}

# ── dak wrapper ───────────────────────────────────────────────────────────────
dak() {
    cd "$PROJECT_DIR" && $DAK_BIN "$@" 2>/dev/null
}

dak_json() {
    cd "$PROJECT_DIR" && $DAK_BIN "$@" 2>/dev/null | grep '^{' | tail -1
}

# ── history ───────────────────────────────────────────────────────────────────
init_history() {
    mkdir -p "$HISTORY_DIR"
}

save_run() {
    local task_text="$1" seed="$2" json="$3"
    init_history
    local ts
    ts="$(date +%Y%m%d_%H%M%S)"
    local file="$HISTORY_DIR/${ts}_seed${seed}.json"
    local ok plan_id answer elapsed
    ok="$(echo "$json" | jq -r '.ok // false')"
    plan_id="$(echo "$json" | jq -r '.report.plan_id // "n/a"')"
    answer="$(echo "$json" | jq -r '.report.final_answer // "n/a"')"
    elapsed="$(echo "$json" | jq -r '.report.elapsed_ms // "n/a"')"

    jq -n \
        --arg ts "$ts" \
        --arg task "$task_text" \
        --arg seed "$seed" \
        --arg ok "$ok" \
        --arg plan_id "$plan_id" \
        --arg answer "$answer" \
        --arg elapsed "$elapsed" \
        --argjson raw "$json" \
        '{timestamp: $ts, task: $task, seed: $seed, ok: $ok, plan_id: $plan_id, answer: $answer, elapsed_ms: $elapsed, raw: $raw}' \
        > "$file"
    echo "$file"
}

list_history() {
    init_history
    find "$HISTORY_DIR" -name "*.json" -type f 2>/dev/null | sort -r | head -20
}

history_count() {
    init_history
    find "$HISTORY_DIR" -name "*.json" -type f 2>/dev/null | wc -l | tr -d ' '
}

# ── test output parsing ───────────────────────────────────────────────────────
extract_test_summary() {
    local log_file="$1"
    # Try to find "test result: ok" or "test result: FAILED" line
    local summary
    summary="$(grep -E "^test result:" "$log_file" 2>/dev/null | tail -1)"
    if [[ -n "$summary" ]]; then
        echo "$summary"
        return
    fi
    # Fallback: last 3 non-empty lines
    grep -v '^$' "$log_file" 2>/dev/null | tail -3
}

test_passed() {
    local log_file="$1"
    grep -q "test result: ok" "$log_file" 2>/dev/null
}

# ── UI helpers ────────────────────────────────────────────────────────────────
header() {
    clear
    printf "${C_CYAN}${C_BOLD}"
    cat << 'EOF'
  ╔══════════════════════════════════════════════════════════╗
  ║                    R E P L A Y   O S                    ║
  ║           Deterministic AI Kernel · v0.1.0               ║
  ╚══════════════════════════════════════════════════════════╝
EOF
    printf "${C_RESET}"
    echo ""
}

footer() {
    printf "  ${C_DIM}↑↓ navigate · Enter select · Esc back · q quit${C_RESET}\n"
}

status_line() {
    local label="$1" value="$2" color="$3"
    printf "  ${C_DIM}${label}:${C_RESET} ${color}${C_BOLD}${value}${C_RESET}\n"
}

separator() {
    printf "  ${C_DIM}──────────────────────────────────────────────────────${C_RESET}\n"
}

info_box() {
    local title="$1"
    shift
    echo ""
    printf "  ${C_CYAN}┌─ ${title} ─────────────────────────────────────┐${C_RESET}\n"
    for line in "$@"; do
        printf "  ${C_CYAN}│${C_RESET}  %-50s${C_CYAN}│${C_RESET}\n" "$line"
    done
    printf "  ${C_CYAN}└─────────────────────────────────────────────────────┘${C_RESET}\n"
}

status_box() {
    local title="$1" status="$2" color="$3"
    shift 3
    echo ""
    printf "  ${color}┌─ ${title} ─────────────────────────────────────┐${C_RESET}\n"
    printf "  ${color}│${C_RESET}  Status: ${color}${C_BOLD}${status}${C_RESET}\n"
    for line in "$@"; do
        printf "  ${color}│${C_RESET}  %-50s${color}│${C_RESET}\n" "$line"
    done
    printf "  ${color}└─────────────────────────────────────────────────────┘${C_RESET}\n"
}

empty_state() {
    local icon="$1" msg="$2"
    echo ""
    printf "  ${C_DIM}${icon}  ${msg}${C_RESET}\n"
    echo ""
}

confirm() {
    local msg="${1:-Continue?}"
    gum confirm --default=false "$msg" 2>/dev/null
}

spinner() {
    local msg="$1"
    gum spin --spinner dot --title "$msg" -- bash -c "sleep 0.5" 2>/dev/null
}

run_with_spinner() {
    local msg="$1"
    shift
    local tmp
    tmp="$(mktemp /tmp/replay_os_out.XXXXXX)"
    gum spin --spinner dot --title "$msg" -- bash -c "$* > '$tmp' 2>&1" || true
    cat "$tmp"
    rm -f "$tmp"
}

run_test_with_summary() {
    local cmd="$1" label="$2"
    header
    printf "  ${C_CYAN}${C_BOLD}${label}${C_RESET}\n"
    echo ""

    if confirm "Run ${cmd}?"; then
        echo ""
        local tmp_log
        tmp_log="$(mktemp /tmp/replay_os_test.XXXXXX)"
        gum spin --spinner dot --title "Running ${label}..." -- \
            bash -c "cd $PROJECT_DIR && $cmd > $tmp_log 2>&1" || true

        # Extract summary
        local summary passed
        summary="$(extract_test_summary "$tmp_log")"
        test_passed "$tmp_log" && passed=true || passed=false

        echo ""
        if [[ "$passed" == "true" ]]; then
            status_box "✅ ${label}" "PASSED" "$C_GREEN" \
                "$summary" \
                "" \
                "Log: $tmp_log"
        else
            # Show last 20 lines for failures
            local fail_tail
            fail_tail="$(tail -20 "$tmp_log" | head -10)"
            status_box "❌ ${label}" "FAILED" "$C_RED" \
                "$summary" \
                "" \
                "Last output:" \
                "$fail_tail"
        fi

        # Option to view full log
        echo ""
        if gum confirm --default=false "View full log?"; then
            gum pager < "$tmp_log" 2>/dev/null || less "$tmp_log"
        fi
        rm -f "$tmp_log"
    fi

    gum input --placeholder "Press Enter..." 2>/dev/null || true
}
