#!/bin/bash
# Layer-2 POC live validation (Arm A on Ministral): NorthPay seeds with the feedback
# loop ENABLED (default). Control arm = mq_northpay_2026-09-25 (same
# payload, loop absent): 0/14 converted, all tests_failed.
# Conversion = TASK_STATE completed after FEEDBACK_CONVERTED.
set -uo pipefail

REPO="$HOME/projects/deterministic_ai_kernel_clean_2"
BIN="$REPO/target/debug/deterministic_ai_kernel"
FIXTURE="$REPO/analyzer_examples/client_northpay"
OUT_DIR="$REPO/analyzer_out/mq_northpay_loop_ministral_2026-09-26"
WORK=/tmp/dek_mq26_loop_ministral
mkdir -p "$OUT_DIR" "$WORK"

cd "$REPO" || exit 1

echo "== llm-smoke ==" | tee "$OUT_DIR/series.log"
"$BIN" llm-smoke > "$OUT_DIR/llm_smoke.out" 2>&1
smoke_rc=$?
echo "llm-smoke exit=$smoke_rc" | tee -a "$OUT_DIR/series.log"
[ $smoke_rc -ne 0 ] && { echo "ABORT: llm-smoke failed"; exit 1; }

for seed in 42 100 101 102 103 104 105 106; do
    ws="$WORK/seed$seed"
    rm -rf "$ws"
    cp -R "$FIXTURE" "$ws"
    db="$WORK/db/seed$seed.db"
    mkdir -p "$WORK/db"
    rm -f "$db"
    payload="Step 1 read repository $ws/clearing/fees.py
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
Fix proportional_refund in clearing/fees.py: it truncates via int(raw * 100) / 100 instead of rounding HALF-UP to the cent. Contract: fee 1.0, share 1 of 8 gives 0.125 which must become 0.13."
    echo "== seed $seed == $(date '+%H:%M:%S')" | tee -a "$OUT_DIR/series.log"
    DAK_CODEFIX_WORKSPACE="$ws" KERNEL_DB_PATH="$db" \
        "$BIN" pipeline-run --payload "$payload" --seed "$seed" \
        > "$OUT_DIR/seed$seed.out" 2>&1
    rc=$?
    state=$(grep -oE 'TASK_STATE: [A-Za-z_]+' "$OUT_DIR/seed$seed.out" | tail -1)
    class=$(grep -oE 'classification=[a-z_]+' "$OUT_DIR/seed$seed.out" | tail -1)
    fb=$(sqlite3 "$db" "SELECT group_concat(event_type || ':' || json_extract(payload,'$.attempt'), ' ') FROM event_log WHERE event_type LIKE 'FEEDBACK%' ORDER BY event_id;" 2>/dev/null)
    echo "seed=$seed exit=$rc ${state:-TASK_STATE:?missing} ${class:-classification:none} fb=[${fb}]" | tee -a "$OUT_DIR/series.log"
    if [ -f "$ws/clearing/fees.py" ]; then
        diff "$FIXTURE/clearing/fees.py" "$ws/clearing/fees.py" > "$OUT_DIR/seed$seed.diff" 2>&1
    fi
done

echo "== stopping model server ==" | tee -a "$OUT_DIR/series.log"
"$BIN" lm-lifecycle stop >> "$OUT_DIR/series.log" 2>&1
echo "== done $(date '+%H:%M:%S') ==" | tee -a "$OUT_DIR/series.log"
