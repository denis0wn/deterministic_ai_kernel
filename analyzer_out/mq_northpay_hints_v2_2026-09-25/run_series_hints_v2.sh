#!/bin/bash
# Layer-1 hints v2 measurement: same 14-seed NorthPay series, NEW
# money-truncation hints (return-type contract + quantize-only), after
# E0 showed hints v1 converted 0/14 (all Decimal return-type failures).
# Question: do the v2 hints convert the semantic-failure class?
set -uo pipefail

REPO="$HOME/projects/deterministic_ai_kernel_clean_2"
BIN="$REPO/target/debug/deterministic_ai_kernel"
FIXTURE="$REPO/analyzer_examples/client_northpay"
OUT_DIR="$REPO/analyzer_out/mq_northpay_hints_v2_2026-09-25"
WORK=/tmp/dek_mq25_hints2
mkdir -p "$OUT_DIR" "$WORK"

cd "$REPO" || exit 1

HINTS="Hints:
- The defect truncates money (e.g. int() drops sub-cent fractions). The fix needs exact decimal arithmetic: use the \`decimal\` module (\`Decimal\`, \`ROUND_HALF_UP\`).
- Preserve the function's existing return type: if it returned \`float\`, convert the decimal result back with \`float(...)\` before returning — a \`Decimal\` return breaks equality checks and callers.
- Round HALF-UP to the cent via \`Decimal.quantize(..., rounding=ROUND_HALF_UP)\` — builtin \`round()\` has no \`rounding\` keyword (\`round(x, 2, rounding=...)\` raises TypeError). You may add \`from decimal import Decimal, ROUND_HALF_UP\` INSIDE the function you are fixing."

echo "== llm-smoke ==" | tee "$OUT_DIR/series.log"
"$BIN" llm-smoke > "$OUT_DIR/llm_smoke.out" 2>&1
smoke_rc=$?
echo "llm-smoke exit=$smoke_rc" | tee -a "$OUT_DIR/series.log"
[ $smoke_rc -ne 0 ] && { echo "ABORT: llm-smoke failed"; exit 1; }

for seed in 42 100 101 102 103 104 105 106 107 108 109 110 111 112; do
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
Fix proportional_refund in clearing/fees.py: it truncates via int(raw * 100) / 100 instead of rounding HALF-UP to the cent. Contract: fee 1.0, share 1 of 8 gives 0.125 which must become 0.13.
$HINTS"
    echo "== seed $seed == $(date '+%H:%M:%S')" | tee -a "$OUT_DIR/series.log"
    # Loop OFF: clean A/B against the v1 hints series (which predates the
    # loop) — any conversion is attributable to the hints alone.
    DAK_FEEDBACK_LOOP=off DAK_CODEFIX_WORKSPACE="$ws" KERNEL_DB_PATH="$db" \
        "$BIN" pipeline-run --payload "$payload" --seed "$seed" \
        > "$OUT_DIR/seed$seed.out" 2>&1
    rc=$?
    state=$(grep -oE 'TASK_STATE: [A-Za-z_]+' "$OUT_DIR/seed$seed.out" | tail -1)
    class=$(grep -oE 'classification=[a-z_]+' "$OUT_DIR/seed$seed.out" | tail -1)
    echo "seed=$seed exit=$rc ${state:-TASK_STATE:?missing} ${class:-classification:none}" | tee -a "$OUT_DIR/series.log"
    if [ -f "$ws/clearing/fees.py" ]; then
        diff "$FIXTURE/clearing/fees.py" "$ws/clearing/fees.py" > "$OUT_DIR/seed$seed.diff" 2>&1
    fi
done

echo "== stopping model server ==" | tee -a "$OUT_DIR/series.log"
"$BIN" lm-lifecycle stop >> "$OUT_DIR/series.log" 2>&1
echo "== done $(date '+%H:%M:%S') ==" | tee -a "$OUT_DIR/series.log"
