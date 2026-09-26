#!/bin/bash
# Cloud-model capability series: same 14-seed NorthPay run on DashScope qwen-plus WITH Layer-1 hints v2
# qwen-plus (cloud), WITH hints v2, loop OFF. Question: does a stronger model
# clear the semantic-failure class that gemma4-reasoning failed 14/14?
# Key comes from macOS Keychain (service qwen-dashscope-api-key) and is
# exported only into the child env; it is never printed or written to disk.
set -uo pipefail

REPO="$HOME/projects/deterministic_ai_kernel_clean_2"
BIN="$REPO/target/debug/deterministic_ai_kernel"
FIXTURE="$REPO/analyzer_examples/client_northpay"
OUT_DIR="$REPO/analyzer_out/mq_northpay_cloud_hints_2026-09-26"
WORK=/tmp/dek_mq26_cloud_hints
mkdir -p "$OUT_DIR" "$WORK"

cd "$REPO" || exit 1

KEY="$(security find-generic-password -s qwen-dashscope-api-key -w 2>/dev/null)"
[ -n "$KEY" ] || { echo "ABORT: no keychain key"; exit 1; }

export OPENAI_BASE_URL="https://dashscope.aliyuncs.com/compatible-mode/v1"
export OPENAI_API_KEY="$KEY"
export OPENAI_MODEL="qwen-plus"
export OPENAI_MODEL_TASK_PLANNING="qwen-plus"
export OPENAI_MODEL_CODING_ASSISTANT="qwen-plus"
export OPENAI_MODEL_CODE_REVIEW="qwen-plus"
export OPENAI_MODEL_REASONING="qwen-plus"
export OPENAI_MODEL_VERIFICATION="qwen-plus"
export OPENAI_MODEL_CRITIC="qwen-plus"
export OPENAI_MODEL_EMBEDDINGS="qwen-plus"
export DAK_FEEDBACK_LOOP=off

echo "== llm-smoke (cloud) ==" | tee "$OUT_DIR/series.log"
"$BIN" llm-smoke > "$OUT_DIR/llm_smoke.out" 2>&1
smoke_rc=$?
echo "llm-smoke exit=$smoke_rc" | tee -a "$OUT_DIR/series.log"
[ $smoke_rc -ne 0 ] && { echo "ABORT: llm-smoke failed"; cat "$OUT_DIR/llm_smoke.out"; exit 1; }

for seed in 42 100 101 102 103 104 105 106 107 108 109 110 111 112; do
    ws="$WORK/seed$seed"
    rm -rf "$ws"
    mkdir -p "$WORK"
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
Hints:
- The defect truncates money (e.g. int() drops sub-cent fractions). The fix needs exact decimal arithmetic: use the \`decimal\` module (\`Decimal\`, \`ROUND_HALF_UP\`).
- Preserve the function's existing return type: if it returned \`float\`, convert the decimal result back with \`float(...)\` before returning — a \`Decimal\` return breaks equality checks and callers.
- Round HALF-UP to the cent via \`Decimal.quantize(..., rounding=ROUND_HALF_UP)\` — builtin \`round()\` has no \`rounding\` keyword (\`round(x, 2, rounding=...)\` raises TypeError). You may add \`from decimal import Decimal, ROUND_HALF_UP\` INSIDE the function you are fixing."
    echo "== seed $seed == $(date '+%H:%M:%S')" | tee -a "$OUT_DIR/series.log"
    DAK_CODEFIX_WORKSPACE="$ws" KERNEL_DB_PATH="$db" \
        "$BIN" pipeline-run --payload "$payload" --seed "$seed" \
        > "$OUT_DIR/seed$seed.out" 2>&1
    rc=$?
    state=$(grep -oE 'TASK_STATE: [A-Za-z_]+' "$OUT_DIR/seed$seed.out" | tail -1)
    echo "seed=$seed exit=$rc ${state:-TASK_STATE:?missing}" | tee -a "$OUT_DIR/series.log"
    if [ -f "$ws/clearing/fees.py" ]; then
        diff "$FIXTURE/clearing/fees.py" "$ws/clearing/fees.py" > "$OUT_DIR/seed$seed.diff" 2>&1
    fi
done
echo "== done $(date '+%H:%M:%S') ==" | tee -a "$OUT_DIR/series.log"
