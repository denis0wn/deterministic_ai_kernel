#!/bin/bash
# Layer-2 unblock series: seed frequency of the semantic-failure class
# (valid patch -> applies -> real tests fail), NorthPay fixture, live
# gemma4-reasoning via kernel-managed mlx_lm.server.
#
# Repro legs: seeds 42, 102 (yesterday both gave tests_failed).
# Fresh legs: seeds 103..112.
# Evidence lands next to this script (mq_northpay_2026-09-25/).
set -uo pipefail

REPO="$HOME/projects/deterministic_ai_kernel_clean_2"
BIN="$REPO/target/debug/deterministic_ai_kernel"
FIXTURE="$REPO/analyzer_examples/client_northpay"
OUT_DIR="$REPO/analyzer_out/mq_northpay_2026-09-25"
WORK=/tmp/dek_mq25
mkdir -p "$OUT_DIR" "$WORK"

cd "$REPO" || exit 1   # dotenvy loads .env from cwd (OPENAI_BASE_URL etc.)

echo "== llm-smoke ==" | tee "$OUT_DIR/series.log"
"$BIN" llm-smoke > "$OUT_DIR/llm_smoke.out" 2>&1
smoke_rc=$?
echo "llm-smoke exit=$smoke_rc" | tee -a "$OUT_DIR/series.log"
[ $smoke_rc -ne 0 ] && { echo "ABORT: llm-smoke failed"; exit 1; }

for seed in 42 102 103 104 105 106 107 108 109 110 111 112; do
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
    echo "seed=$seed exit=$rc ${state:-TASK_STATE:?missing} ${class:-classification:none}" | tee -a "$OUT_DIR/series.log"
    # capture the applied patch for diffing
    if [ -f "$ws/clearing/fees.py" ]; then
        diff "$FIXTURE/clearing/fees.py" "$ws/clearing/fees.py" > "$OUT_DIR/seed$seed.diff" 2>&1
    fi
done

echo "== stopping model server ==" | tee -a "$OUT_DIR/series.log"
"$BIN" lm-lifecycle stop >> "$OUT_DIR/series.log" 2>&1
echo "== done $(date '+%H:%M:%S') ==" | tee -a "$OUT_DIR/series.log"
