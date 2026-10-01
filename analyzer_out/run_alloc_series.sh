#!/bin/bash
# allocation-remainder measurement: NEW defect class (money allocation,
# sum-exactness + one-cent spread + whole-cent granularity) on the
# sanctioned local Ministral model.
# 8 seeds without hints, 8 seeds with the invariant-form hint recipe.
# Loop OFF for clean attribution (W3: the loop does not convert at temp 0).
#
# Round 2 (this script's current form): the contract pins whole-cent
# granularity. Round 1 (analyzer_out/mq_alloc_{plain,hints}_weak_2026-10-01)
# ran before the granularity assertion existed; a plain-arm patch passed it
# with sub-cent shares, which exposed the suite hole (dateflow precedent).
set -uo pipefail

REPO="$HOME/projects/deterministic_ai_kernel_clean_2"
BIN="$REPO/target/debug/deterministic_ai_kernel"
WORK=/tmp/dek_alloc
DATE=2026-10-01

cd "$REPO" || exit 1   # dotenvy loads .env from cwd (OPENAI_BASE_URL etc.)

run_arm() {
    local tag="$1" hints="$2"
    local OUT_DIR="$REPO/analyzer_out/mq_alloc_${tag}_${DATE}"
    mkdir -p "$OUT_DIR"
    : > "$OUT_DIR/series.log"
    for seed in 42 100 101 102 103 104 105 106; do
        local ws="$WORK/${tag}/seed$seed"
        rm -rf "$ws"; mkdir -p "$(dirname "$ws")"
        cp -R "$REPO/analyzer_examples/client_alloc" "$ws"
        local db="$WORK/db/alloc_${tag}_$seed.db"
        mkdir -p "$WORK/db"; rm -f "$db"
        local payload="Step 1 read repository $ws/ledger/allocate.py
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
Fix split_amount in ledger/allocate.py: it rounds every share independently (round(total / parts, 2) repeated), so the shares do not sum back to the total. Contract: exactly parts float shares, every share a whole number of cents, summing exactly to total, with no share differing from any other by more than 0.01.$hints"
        DAK_FEEDBACK_LOOP=off DAK_CODEFIX_WORKSPACE="$ws" KERNEL_DB_PATH="$db" \
            "$BIN" pipeline-run --payload "$payload" --seed "$seed" \
            > "$OUT_DIR/seed$seed.out" 2>&1
        local rc=$?
        local state=$(grep -oE 'TASK_STATE: [A-Za-z_]+' "$OUT_DIR/seed$seed.out" | tail -1)
        echo "seed=$seed exit=$rc ${state:-?}" | tee -a "$OUT_DIR/series.log"
        [ -f "$ws/ledger/allocate.py" ] && diff "$REPO/analyzer_examples/client_alloc/ledger/allocate.py" "$ws/ledger/allocate.py" > "$OUT_DIR/seed$seed.diff" 2>&1
    done
}

ALLOC_HINTS="
Hints:
- The defect rounds each share independently, so the shares do not sum back to the total. Correct algorithm: work in integer cents: cents = round(total * 100); base = cents // parts; remainder r = cents % parts; exactly r shares get base + 1 cents and the rest get base cents. The shares must differ by at most one cent and sum exactly to the total.
- Preserve the function's existing return type: a list of floats (cents divided by 100).
- Stdlib only. round(total * 100) gives exact integer cents for two-decimal inputs; do not import decimal or numpy."

run_arm plain ""
run_arm hints "$ALLOC_HINTS"

"$BIN" lm-lifecycle stop >/dev/null 2>&1 || true
echo ALL-DONE
