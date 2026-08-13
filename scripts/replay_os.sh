#!/usr/bin/env bash
# Replay OS — Interactive TUI for deterministic_ai_kernel
# Usage: replay-os [command] or source from zshrc
# Requires: gum, jq, curl, cargo

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/helpers.sh"

# ── main menu ─────────────────────────────────────────────────────────────────
main_menu() {
    while true; do
        header

        # Status badges
        local runtime_badge model_display hcount
        if runtime_is_running; then
            runtime_badge="${C_GREEN}● ONLINE${C_RESET}"
            model_display="$(runtime_model_id)"
        else
            runtime_badge="${C_RED}○ OFFLINE${C_RESET}"
            model_display="$(model_id_for_role coding_assistant)"
        fi
        hcount="$(history_count)"

        printf "  Runtime: ${runtime_badge}  Model: ${C_CYAN}${model_display}${C_RESET}\n"
        if [[ "$hcount" -gt 0 ]]; then
            printf "  History: ${C_DIM}${hcount} runs${C_RESET}\n"
        fi

        separator

        local choice
        choice=$(gum choose \
            --cursor "→ " \
            --selected "1" \
            "🚀  Run Task" \
            "⚙️   Runtime / Model" \
            "🩺  Diagnostics" \
            "🔄  Replay / Capsules" \
            "🔧  Workflow Ops" \
            "🧪  Tests" \
            "📋  Config / Paths" \
            "❌  Exit" \
            2>/dev/null) || true

        case "$choice" in
            *"Run Task"*)       menu_run_task ;;
            *"Runtime"*)        menu_runtime ;;
            *"Diagnostics"*)    menu_diagnostics ;;
            *"Replay"*)         menu_replay ;;
            *"Workflow"*)       menu_workflow ;;
            *"Tests"*)          menu_tests ;;
            *"Config"*)         menu_config ;;
            *"Exit"*)           clear; exit 0 ;;
        esac
    done
}

# ── Run Task ──────────────────────────────────────────────────────────────────
menu_run_task() {
    header
    printf "  ${C_CYAN}${C_BOLD}Run Task${C_RESET}\n"
    echo ""
    footer
    echo ""

    local task_text
    task_text=$(gum input --placeholder "Describe your task..." 2>/dev/null) || return
    [[ -z "$task_text" ]] && return

    # Ensure runtime
    if ! runtime_is_running; then
        echo ""
        printf "  ${C_YELLOW}Runtime is offline. Starting MLX server...${C_RESET}\n"
        start_mlx_server
        if ! wait_for_runtime; then
            printf "  ${C_RED}Failed to start MLX server.${C_RESET}\n"
            gum input --placeholder "Press Enter to continue..." 2>/dev/null || true
            return
        fi
        printf "  ${C_GREEN}Runtime online.${C_RESET}\n"
    fi

    # Summary card before execution
    local seed
    seed="$(date +%s)"
    echo ""
    info_box "📋 Task Summary" \
        "Task:       ${task_text:0:50}" \
        "Model:      $(model_id_for_role coding_assistant)" \
        "Seed:       $seed" \
        "Runtime:    ONLINE"

    echo ""
    printf "  ${C_DIM}Running pipeline-run...${C_RESET}\n"

    local tmp_output
    tmp_output="$(mktemp /tmp/replay_os_pipeline.XXXXXX)"

    cd "$PROJECT_DIR"
    $DAK_BIN pipeline-run --payload "$task_text" --seed "$seed" --json \
        > "$tmp_output" 2>&1 || true
    cd - > /dev/null

    local pipeline_json
    pipeline_json="$(grep '^{' "$tmp_output" 2>/dev/null | tail -1)"
    rm -f "$tmp_output"

    if [[ -z "$pipeline_json" ]]; then
        printf "  ${C_RED}No JSON output from pipeline-run.${C_RESET}\n"
        gum input --placeholder "Press Enter to continue..." 2>/dev/null || true
        return
    fi

    # Save to history
    save_run "$task_text" "$seed" "$pipeline_json" > /dev/null 2>&1 || true

    # Render result
    local ok plan_id answer elapsed
    ok="$(echo "$pipeline_json" | jq -r '.ok // false')"
    plan_id="$(echo "$pipeline_json" | jq -r '.report.plan_id // "n/a"')"
    answer="$(echo "$pipeline_json" | jq -r '.report.final_answer // "n/a"')"
    elapsed="$(echo "$pipeline_json" | jq -r '.report.elapsed_ms // "n/a"')"

    echo ""
    if [[ "$ok" == "true" ]]; then
        status_box "✅ Task Complete" "SUCCESS" "$C_GREEN" \
            "Plan ID:   $plan_id" \
            "Seed:      $seed" \
            "Elapsed:   ${elapsed} ms" \
            "" \
            "Answer:" \
            "${answer:0:200}"
    else
        local error
        error="$(echo "$pipeline_json" | jq -r '.report.error // "unknown error"')"
        status_box "❌ Task Failed" "ERROR" "$C_RED" \
            "Error: ${error:0:200}"
    fi

    # Post-action menu
    echo ""
    local action
    action=$(gum choose \
        --cursor "→ " \
        "🔄  Run again" \
        "📋  View raw JSON" \
        "←   Back to menu" \
        2>/dev/null) || return

    case "$action" in
        *"Run again"*)
            menu_run_task
            ;;
        *"View raw"*)
            echo ""
            echo "$pipeline_json" | jq . 2>/dev/null || echo "$pipeline_json"
            gum input --placeholder "Press Enter..." 2>/dev/null || true
            ;;
    esac
}

# ── Runtime / Model ───────────────────────────────────────────────────────────
menu_runtime() {
    while true; do
        header
        printf "  ${C_CYAN}${C_BOLD}Runtime / Model${C_RESET}\n"
        echo ""
        footer
        echo ""

        if runtime_is_running; then
            status_line "Status" "ONLINE" "$C_GREEN"
            status_line "Model ID" "$(runtime_model_id)" "$C_CYAN"
            status_line "Base URL" "${OPENAI_BASE_URL:-http://127.0.0.1:8080/v1}" "$C_DIM"
        else
            status_line "Status" "OFFLINE" "$C_RED"
            status_line "Model" "$(model_id_for_role coding_assistant)" "$C_DIM"
        fi

        separator

        local choice
        choice=$(gum choose \
            --cursor "→ " \
            "📊  Runtime details" \
            "🔍  Probe /v1/models" \
            "🚀  Start runtime" \
            "🛑  Stop runtime" \
            "🔄  Restart runtime" \
            "📋  Show doctor report" \
            "←   Back" \
            2>/dev/null) || return

        case "$choice" in
            *"details"*)
                show_runtime_details
                ;;
            *"Probe"*)
                echo ""
                probe_runtime | jq . 2>/dev/null || printf "  ${C_RED}Probe failed${C_RESET}\n"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Start"*)
                if runtime_is_running; then
                    printf "  ${C_YELLOW}Already running.${C_RESET}\n"
                else
                    start_mlx_server
                    wait_for_runtime && printf "  ${C_GREEN}Started.${C_RESET}\n" || printf "  ${C_RED}Failed.${C_RESET}\n"
                fi
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Stop"*)
                if confirm "Stop MLX server?"; then
                    stop_mlx_server
                    printf "  ${C_GREEN}Stopped.${C_RESET}\n"
                fi
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Restart"*)
                stop_mlx_server
                sleep 1
                start_mlx_server
                wait_for_runtime && printf "  ${C_GREEN}Restarted.${C_RESET}\n" || printf "  ${C_RED}Failed.${C_RESET}\n"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"doctor"*)
                echo ""
                run_with_spinner "Running doctor..." "dak doctor"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Back"*) return ;;
        esac
    done
}

show_runtime_details() {
    header
    printf "  ${C_CYAN}${C_BOLD}Runtime Details${C_RESET}\n"
    echo ""
    footer
    echo ""

    local status_text model_id base_url pid log_exists
    if runtime_is_running; then
        status_text="${C_GREEN}ONLINE${C_RESET}"
        model_id="$(runtime_model_id)"
    else
        status_text="${C_RED}OFFLINE${C_RESET}"
        model_id="$(model_id_for_role coding_assistant)"
    fi
    base_url="${OPENAI_BASE_URL:-http://127.0.0.1:8080/v1}"
    pid="$(runtime_pid 2>/dev/null || echo "n/a")"
    [[ -f "$MLX_LOG" ]] && log_exists="Yes ($MLX_LOG)" || log_exists="No"

    info_box "🖥️  Runtime Status" \
        "Status:     ${status_text}" \
        "Model ID:   ${model_id}" \
        "Base URL:   ${base_url}" \
        "PID:        ${pid}" \
        "Log file:   ${log_exists}"

    separator

    local choice
    choice=$(gum choose \
        --cursor "→ " \
        "📋  Tail runtime log" \
        "🔍  Probe /v1/models" \
        "🔄  Restart runtime" \
        "←   Back" \
        2>/dev/null) || return

    case "$choice" in
        *"Tail"*)
            if [[ -f "$MLX_LOG" ]]; then
                gum pager < "$MLX_LOG" 2>/dev/null || tail -50 "$MLX_LOG"
            else
                printf "  ${C_DIM}No log file found.${C_RESET}\n"
            fi
            gum input --placeholder "Press Enter..." 2>/dev/null || true
            ;;
        *"Probe"*)
            echo ""
            probe_runtime | jq . 2>/dev/null || printf "  ${C_RED}Probe failed${C_RESET}\n"
            gum input --placeholder "Press Enter..." 2>/dev/null || true
            ;;
        *"Restart"*)
            stop_mlx_server
            sleep 1
            start_mlx_server
            wait_for_runtime && printf "  ${C_GREEN}Restarted.${C_RESET}\n" || printf "  ${C_RED}Failed.${C_RESET}\n"
            gum input --placeholder "Press Enter..." 2>/dev/null || true
            ;;
        *"Back"*) return ;;
    esac
}

# ── Diagnostics ───────────────────────────────────────────────────────────────
menu_diagnostics() {
    while true; do
        header
        printf "  ${C_CYAN}${C_BOLD}Diagnostics${C_RESET}\n"
        echo ""
        footer
        echo ""

        local choice
        choice=$(gum choose \
            --cursor "→ " \
            "🩺  Doctor (text)" \
            "📊  Doctor (JSON)" \
            "🔒  Integrity check" \
            "←   Back" \
            2>/dev/null) || return

        case "$choice" in
            *"Doctor (text)"*)
                echo ""
                run_with_spinner "Running doctor..." "dak doctor"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Doctor (JSON)"*)
                echo ""
                run_with_spinner "Running doctor-json..." "dak_json doctor-json | jq ."
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Integrity"*)
                echo ""
                run_with_spinner "Running integrity check..." "dak integrity"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Back"*) return ;;
        esac
    done
}

# ── Replay / Capsules ─────────────────────────────────────────────────────────
menu_replay() {
    while true; do
        header
        printf "  ${C_CYAN}${C_BOLD}Replay / Capsules${C_RESET}\n"
        echo ""
        footer
        echo ""

        local choice
        choice=$(gum choose \
            --cursor "→ " \
            "📜  Recent runs" \
            "📥  Capture capsule" \
            "📋  Latest capsule" \
            "🔄  Replay capsule" \
            "←   Back" \
            2>/dev/null) || return

        case "$choice" in
            *"Recent"*)
                show_history
                ;;
            *"Capture"*)
                local task_id
                task_id=$(gum input --placeholder "Task ID" 2>/dev/null) || return
                [[ -z "$task_id" ]] && return
                echo ""
                run_with_spinner "Capturing capsule..." "dak capture-capsule $task_id"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Latest"*)
                local task_id
                task_id=$(gum input --placeholder "Task ID" 2>/dev/null) || return
                [[ -z "$task_id" ]] && return
                echo ""
                run_with_spinner "Loading latest capsule..." "dak latest-capsule $task_id"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Replay"*)
                local task_id
                task_id=$(gum input --placeholder "Task ID" 2>/dev/null) || return
                [[ -z "$task_id" ]] && return
                echo ""
                run_with_spinner "Replaying capsule..." "dak replay-capsule $task_id"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Back"*) return ;;
        esac
    done
}

show_history() {
    while true; do
        header
        printf "  ${C_CYAN}${C_BOLD}Recent Runs${C_RESET}\n"
        echo ""
        footer
        echo ""

        local files
        files="$(list_history)"

        if [[ -z "$files" ]]; then
            empty_state "📭" "No history yet. Run a task first."
            gum input --placeholder "Press Enter..." 2>/dev/null || true
            return
        fi

        # Build menu items from history
        local items=()
        local file_map=()
        local idx=0
        while IFS= read -r f; do
            local ts task ok elapsed
            ts="$(basename "$f" .json | sed 's/_seed.*//')"
            task="$(jq -r '.task // "n/a"' "$f" 2>/dev/null)"
            ok="$(jq -r '.ok // false' "$f" 2>/dev/null)"
            elapsed="$(jq -r '.elapsed_ms // "n/a"' "$f" 2>/dev/null)"
            local icon="✅"
            [[ "$ok" != "true" ]] && icon="❌"
            items+=("$icon ${ts} | ${task:0:35} | ${elapsed}ms")
            file_map+=("$f")
            idx=$((idx + 1))
        done <<< "$files"

        items+=("←   Back")

        local choice
        choice=$(gum choose --cursor "→ " "${items[@]}" 2>/dev/null) || return

        [[ "$choice" == *"Back"* ]] && return

        # Find selected file by matching timestamp
        local selected_ts
        selected_ts="$(echo "$choice" | sed 's/^[✅❌] //' | cut -d'|' -f1 | xargs)"

        local selected_file=""
        for f in "${file_map[@]}"; do
            local fts
            fts="$(basename "$f" .json | sed 's/_seed.*//')"
            if [[ "$fts" == "$selected_ts" ]]; then
                selected_file="$f"
                break
            fi
        done

        if [[ -z "$selected_file" ]]; then
            printf "  ${C_RED}File not found.${C_RESET}\n"
            gum input --placeholder "Press Enter..." 2>/dev/null || true
            continue
        fi

        # Show detail view with actions
        show_history_detail "$selected_file"
    done
}

show_history_detail() {
    local file="$1"
    while true; do
        header
        printf "  ${C_CYAN}${C_BOLD}Run Details${C_RESET}\n"
        echo ""
        footer
        echo ""

        local task ok seed plan_id answer elapsed
        task="$(jq -r '.task // "n/a"' "$file")"
        ok="$(jq -r '.ok // false' "$file")"
        seed="$(jq -r '.seed // "n/a"' "$file")"
        plan_id="$(jq -r '.plan_id // "n/a"' "$file")"
        answer="$(jq -r '.answer // "n/a"' "$file")"
        elapsed="$(jq -r '.elapsed_ms // "n/a"' "$file")"

        local status_icon="✅"
        [[ "$ok" != "true" ]] && status_icon="❌"

        info_box "${status_icon} Run Details" \
            "Task:       ${task:0:50}" \
            "Seed:       $seed" \
            "Plan ID:    $plan_id" \
            "Elapsed:    ${elapsed} ms" \
            "" \
            "Answer:" \
            "${answer:0:200}"

        separator

        local action
        action=$(gum choose \
            --cursor "→ " \
            "📋  View raw JSON" \
            "🔄  Re-run this task" \
            "←   Back" \
            2>/dev/null) || return

        case "$action" in
            *"raw"*)
                echo ""
                jq . "$file" 2>/dev/null || cat "$file"
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Re-run"*)
                # Re-run with same task text
                local re_task="$task"
                if [[ -n "$re_task" && "$re_task" != "n/a" ]]; then
                    header
                    printf "  ${C_CYAN}${C_BOLD}Re-running: ${re_task:0:40}${C_RESET}\n"
                    echo ""

                    if ! runtime_is_running; then
                        printf "  ${C_YELLOW}Starting runtime...${C_RESET}\n"
                        start_mlx_server
                        wait_for_runtime || { gum input --placeholder "Press Enter..." 2>/dev/null || true; continue; }
                    fi

                    local new_seed
                    new_seed="$(date +%s)"
                    local tmp_output
                    tmp_output="$(mktemp /tmp/replay_os_pipeline.XXXXXX)"

                    cd "$PROJECT_DIR"
                    $DAK_BIN pipeline-run --payload "$re_task" --seed "$new_seed" --json \
                        > "$tmp_output" 2>&1 || true
                    cd - > /dev/null

                    local new_json
                    new_json="$(grep '^{' "$tmp_output" 2>/dev/null | tail -1)"
                    rm -f "$tmp_output"

                    if [[ -n "$new_json" ]]; then
                        save_run "$re_task" "$new_seed" "$new_json" > /dev/null 2>&1 || true
                        local new_answer
                        new_answer="$(echo "$new_json" | jq -r '.report.final_answer // "n/a"')"
                        printf "  ${C_GREEN}Done. Answer: ${new_answer:0:100}${C_RESET}\n"
                    else
                        printf "  ${C_RED}No output.${C_RESET}\n"
                    fi
                    gum input --placeholder "Press Enter..." 2>/dev/null || true
                fi
                ;;
            *"Back"*) return ;;
        esac
    done
}

# ── Workflow Ops ──────────────────────────────────────────────────────────────
menu_workflow() {
    while true; do
        header
        printf "  ${C_CYAN}${C_BOLD}Workflow Ops${C_RESET}\n"
        echo ""
        footer
        echo ""

        local choice
        choice=$(gum choose \
            --cursor "→ " \
            "📅  Schedule" \
            "🔄  Reconcile" \
            "⚡  Execute effects" \
            "⏰  Expire leases" \
            "←   Back" \
            2>/dev/null) || return

        case "$choice" in
            *"Schedule"*)
                workflow_action "Schedule" "schedule" "Scheduling"
                ;;
            *"Reconcile"*)
                workflow_action "Reconcile" "reconcile" "Reconciling"
                ;;
            *"Execute"*)
                workflow_action "Execute effects" "execute-effects" "Executing effects"
                ;;
            *"Expire"*)
                workflow_action "Expire leases" "expire-leases" "Expiring leases"
                ;;
            *"Back"*) return ;;
        esac
    done
}

workflow_action() {
    local title="$1" cmd="$2" verb="$3"
    header
    printf "  ${C_CYAN}${C_BOLD}${title}${C_RESET}\n"
    echo ""

    local task_id
    task_id=$(gum input --placeholder "Task ID [task1]" 2>/dev/null) || return
    task_id="${task_id:-task1}"

    echo ""
    info_box "⚠️  Confirm ${title}" \
        "Command:  ${cmd}" \
        "Task ID:  ${task_id}" \
        "" \
        "This will ${verb,,} for task ${task_id}."

    echo ""
    if confirm "${title} task ${task_id}?"; then
        echo ""
        run_with_spinner "${verb}..." "dak ${cmd} ${task_id}"
        printf "  ${C_GREEN}Done.${C_RESET}\n"
    fi

    gum input --placeholder "Press Enter..." 2>/dev/null || true
}

# ── Tests ─────────────────────────────────────────────────────────────────────
menu_tests() {
    while true; do
        header
        printf "  ${C_CYAN}${C_BOLD}Tests${C_RESET}\n"
        echo ""
        footer
        echo ""

        local choice
        choice=$(gum choose \
            --cursor "→ " \
            "⚡  Quick test (lib only)" \
            "🧪  Full test suite" \
            "🔬  Preflight check" \
            "←   Back" \
            2>/dev/null) || return

        case "$choice" in
            *"Quick"*)
                run_test_with_summary "cargo test --lib" "Quick lib tests"
                ;;
            *"Full"*)
                run_test_with_summary "cargo test" "Full test suite"
                ;;
            *"Preflight"*)
                header
                printf "  ${C_CYAN}${C_BOLD}Preflight Check${C_RESET}\n"
                echo ""

                if [[ -f "$PROJECT_DIR/scripts/preflight.sh" ]]; then
                    run_with_spinner "Running preflight..." "bash $PROJECT_DIR/scripts/preflight.sh"
                    echo ""
                    if test_passed /dev/stdin 2>/dev/null; then
                        printf "  ${C_GREEN}Preflight passed.${C_RESET}\n"
                    fi
                else
                    empty_state "📄" "preflight.sh not found in scripts/"
                fi
                gum input --placeholder "Press Enter..." 2>/dev/null || true
                ;;
            *"Back"*) return ;;
        esac
    done
}

# ── Config / Paths ────────────────────────────────────────────────────────────
menu_config() {
    header
    printf "  ${C_CYAN}${C_BOLD}Config / Paths${C_RESET}\n"
    echo ""
    footer
    echo ""

    load_env

    # Check for missing/warning values
    local base_url_warn="" model_warn="" manifest_warn=""
    [[ -z "${OPENAI_BASE_URL:-}" ]] && base_url_warn=" ${C_YELLOW}⚠️ MISSING${C_RESET}"
    [[ -z "${OPENAI_MODEL:-}" ]] && model_warn=" ${C_YELLOW}⚠️ MISSING${C_RESET}"
    [[ ! -f "$MANIFEST" ]] && manifest_warn=" ${C_RED}⚠️ NOT FOUND${C_RESET}"

    info_box "Configuration" \
        "Project:     $PROJECT_DIR" \
        "Base URL:    ${OPENAI_BASE_URL:-MISSING}${base_url_warn}" \
        "Model:       ${OPENAI_MODEL:-MISSING}${model_warn}" \
        "Manifest:    $MANIFEST${manifest_warn}" \
        "Launcher:    $SCRIPT_DIR/replay_os.sh" \
        "Env file:    $ENV_FILE"

    echo ""
    info_box "Model Roles" \
        "coding_assistant:   $(model_id_for_role coding_assistant)" \
        "task_planning:      $(model_id_for_role task_planning)" \
        "code_review:        $(model_id_for_role code_review)" \
        "embeddings:         $(model_id_for_role embeddings)"

    echo ""
    info_box "History" \
        "Directory:   $HISTORY_DIR" \
        "Total runs:  $(history_count)"

    echo ""
    gum input --placeholder "Press Enter to go back..." 2>/dev/null || true
}

# ── entry point ───────────────────────────────────────────────────────────────
main() {
    # Check dependencies
    if ! check_deps; then
        exit 1
    fi

    load_env
    init_history
    main_menu
}

# Handle direct command execution
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
