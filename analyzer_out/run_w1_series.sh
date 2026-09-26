#!/bin/bash
# W1 measurement: two NEW defect classes (dateflow business-days, scorer
# null-safety) on the sanctioned local Ministral model. For each class:
# 8 seeds without hints, 8 seeds with the class's Layer-1 hint recipe.
# Loop OFF for clean attribution.
set -uo pipefail

REPO="$HOME/projects/deterministic_ai_kernel_clean_2"
BIN="$REPO/target/debug/deterministic_ai_kernel"
WORK=/tmp/dek_w1

cd "$REPO" || exit 1   # dotenvy loads .env from cwd (OPENAI_BASE_URL etc.)

run_class() {
    local class="$1" fixture_rel="$2" target_rel="$3" task="$4" hints="$5" tag="$6"
    local OUT_DIR="$REPO/analyzer_out/mq_${class}_${tag}_2026-09-26"
    mkdir -p "$OUT_DIR"
    : > "$OUT_DIR/series.log"
    for seed in 42 100 101 102 103 104 105 106; do
        local ws="$WORK/${class}_${tag}/seed$seed"
        rm -rf "$ws"; mkdir -p "$(dirname "$ws")"
        cp -R "$REPO/analyzer_examples/$fixture_rel" "$ws"
        local db="$WORK/db/${class}_${tag}_$seed.db"
        mkdir -p "$WORK/db"; rm -f "$db"
        local payload="Step 1 read repository $ws/$target_rel
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
$task$hints"
        DAK_FEEDBACK_LOOP=off DAK_CODEFIX_WORKSPACE="$ws" KERNEL_DB_PATH="$db" \
            "$BIN" pipeline-run --payload "$payload" --seed "$seed" \
            > "$OUT_DIR/seed$seed.out" 2>&1
        local rc=$?
        local state=$(grep -oE 'TASK_STATE: [A-Za-z_]+' "$OUT_DIR/seed$seed.out" | tail -1)
        echo "seed=$seed exit=$rc ${state:-?}" | tee -a "$OUT_DIR/series.log"
        [ -f "$ws/$target_rel" ] && diff "$REPO/analyzer_examples/$fixture_rel/$target_rel" "$ws/$target_rel" > "$OUT_DIR/seed$seed.diff" 2>&1
    done
}

DATEFLOW_TASK="Fix add_business_days in billing/schedule.py: it adds calendar days, ignoring weekends. Contract: business days are Monday-Friday; Friday 2026-09-25 + 1 business day = Monday 2026-09-28."
DATEFLOW_HINTS="
Hints:
- The defect counts calendar days. Business days are Monday-Friday only — advance day-by-day with timedelta(days=1), checking .weekday() < 5.
- Preserve the function's existing return type: datetime.date in, datetime.date out.
- Stdlib only. Do not import holiday calendars or numpy."

SCORER_TASK="Fix average_score in reports/scores.py: it computes sum(scores) / len(scores), which crashes on None entries and would count them in the denominator. Contract: skip Nones; empty or all-None input returns 0.0; the function returns float."
SCORER_HINTS="
Hints:
- The defect counts None entries: sum() crashes on None and len() counts them. Filter first: valid = [s for s in scores if s is not None]; return 0.0 when the filtered list is empty.
- Preserve the function's existing return type: float out.
- Stdlib only."

run_class dateflow client_dateflow billing/schedule.py "$DATEFLOW_TASK" "" plain
run_class dateflow client_dateflow billing/schedule.py "$DATEFLOW_TASK" "$DATEFLOW_HINTS" hints
run_class scorer client_scorer reports/scores.py "$SCORER_TASK" "" plain
run_class scorer client_scorer reports/scores.py "$SCORER_TASK" "$SCORER_HINTS" hints

"$BIN" lm-lifecycle stop >/dev/null 2>&1 || true
echo ALL-DONE
