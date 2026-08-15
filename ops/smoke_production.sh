#!/bin/bash
# Production smoke (roadmap §5): 4 fast checks through the FULL kernel
# pipeline with the production profile + REAL model (no mocks).
#   bash ops/smoke_production.sh
set -u
DAK_OPS_ROOT="$(cd "$(dirname "$0")" && pwd)"
export DAK_OPS_ROOT
source "$DAK_OPS_ROOT/production.env"

BIN="${DAK_BIN:-$DAK_OPS_ROOT/../target/release/deterministic_ai_kernel}"
[ -x "$BIN" ] || BIN="$DAK_OPS_ROOT/../target/debug/deterministic_ai_kernel"
OUT="${SMOKE_OUT:-/tmp/dek_ai_matrix/ops_smoke}"
mkdir -p "$OUT"
FAIL=0

smoke() {
  local tag="$1" payload="$2" must="$3"
  local db="$OUT/$tag.db"
  rm -f "$db"*
  local t0=$(date +%s)
  KERNEL_DB_PATH="$db" "$BIN" pipeline-run --payload "$payload" --seed 42 > "$OUT/$tag.out" 2>&1
  local rc=$?
  local wall=$(( $(date +%s) - t0 ))
  local state=$(grep -m1 '^TASK_STATE:' "$OUT/$tag.out" | cut -c13- | tr -d ' ')
  local ans=$(awk '/^FINAL_ANSWER=/{f=1; sub(/^FINAL_ANSWER=/,""); print; next} f&&/^STEP\.1=/{f=0} f' "$OUT/$tag.out" | head -c 200)
  local verdict="FAIL"
  if [ "$must" = "completed" ] && [ "$state" = "completed" ]; then verdict="PASS"; fi
  if [ "$must" = "grounded" ] && [ "$state" = "completed" ] && echo "$ans" | grep -qi "$4"; then verdict="PASS"; fi
  if [ "$must" = "refused" ] && { [ "$state" != "completed" ] || echo "$ans" | grep -qiE "не могу|нет данных|нет информации|не предоставл|недостаточно|отсутству|уточните|назвать нельзя|нельзя назвать"; }; then verdict="PASS"; fi
  echo "[$tag] $verdict rc=$rc state=${state:-none} wall=${wall}s"
  [ "$verdict" = "PASS" ] || FAIL=1
  rm -f "$db"*
}

echo "=== production smoke $(date) ==="
smoke S1_fast_math "Сколько будет 6 умножить на 7?" completed
smoke S2_rag_fact "Какой серийный номер насоса установлен на машине 9?" grounded "PUMP-777123"
smoke S3_refusal "Какой серийный номер насоса установлен на машине 12?" refused
smoke S4_long_reasoning "Если вчера было завтра, то какой день был послезавчера позавчера?" completed
echo "=== smoke result: $([ $FAIL -eq 0 ] && echo ALL_PASS || echo FAILED) ==="
exit $FAIL
