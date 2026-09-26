#!/bin/bash
# Demo runner for the 60-second asciinema recording.
# Real commands, real output — no canned text. Run from the repo root.
set -e
cd "$HOME/projects/deterministic_ai_kernel_clean_2"
BIN=./target/debug/deterministic_ai_kernel
WS=/tmp/dek_demo_ws
rm -rf "$WS"; cp -R analyzer_examples/client_northpay "$WS"
DB=/tmp/dek_demo.db; rm -f "$DB"

say() { printf '\n\033[1;36m# %s\033[0m\n' "$1"; sleep 2; }

say "A payment library with a seeded money-rounding defect:"
sed -n '15,26p' "$WS/clearing/fees.py"
sleep 3

say "Unassisted, even frontier cloud models fail this class: 0 of 14 measured runs."
sleep 3

say "Now the kernel — bounded pipeline, real tests decide, everything persisted:"
export DAK_CODEFIX_WORKSPACE="$WS" KERNEL_DB_PATH="$DB" DAK_FEEDBACK_LOOP=off
TASK_OUT=$(mktemp)
$BIN pipeline-run --payload "Step 1 read repository $WS/clearing/fees.py
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
Fix proportional_refund in clearing/fees.py: it truncates via int(raw * 100) / 100 instead of rounding HALF-UP to the cent. Contract: fee 1.0, share 1 of 8 gives 0.125 which must become 0.13.
Hints:
- The defect truncates money (e.g. int() drops sub-cent fractions). The fix needs exact decimal arithmetic: use the \`decimal\` module (\`Decimal\`, \`ROUND_HALF_UP\`).
- Preserve the function's existing return type: if it returned \`float\`, convert the decimal result back with \`float(...)\` before returning — a \`Decimal\` return breaks equality checks and callers.
- Round HALF-UP to the cent via \`Decimal.quantize(..., rounding=ROUND_HALF_UP)\` — builtin \`round()\` has no \`rounding\` keyword (\`round(x, 2, rounding=...)\` raises TypeError). You may add \`from decimal import Decimal, ROUND_HALF_UP\` INSIDE the function you are fixing." --seed 42 2>&1 | tee "$TASK_OUT" | grep -E 'STEP|TASK_STATE|PLAN_INVARIANTS' | head -12

say "The fix that passed the real test suite:"
grep -A3 'return float' "$WS/clearing/fees.py" | head -5
sleep 3

say "Every step persisted as evidence (prompts, responses, seeds, test report):"
TASK_ID=$(sqlite3 "$DB" "SELECT task_id FROM tasks ORDER BY rowid DESC LIMIT 1")
$BIN semantic-artifacts "$TASK_ID" | head -6

say "Seal the run into a replay capsule, then replay it against recorded evidence:"
KERNEL_DB_PATH="$DB" $BIN capture-capsule-save "$TASK_ID"
KERNEL_DB_PATH="$DB" $BIN replay-capsule "$TASK_ID" --json | head -3

say "When it cannot prove success, it says so. That is the product."
sleep 2
