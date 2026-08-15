#!/bin/bash
# R9 — formal benchmark runner. Runs the fixed suite
# (tests/acceptance/benchmark_suite.json) against the REAL local model.
# No mocks. Outputs CSV + summary + pass/fail gate to $OUT_DIR.
#
# Usage: bash tests/acceptance/run_benchmark.sh [out_dir]
set -u
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO" || exit 1
SUITE="$REPO/tests/acceptance/benchmark_suite.json"
KB_DIR="$REPO/tests/acceptance/kb"
OUT_DIR="${1:-/tmp/dek_ai_matrix/r9_benchmark}"
mkdir -p "$OUT_DIR"
TS=$(date +%Y%m%d_%H%M%S)
CSV="$OUT_DIR/benchmark_results_${TS}.csv"
RAW="$OUT_DIR/benchmark_raw_${TS}.tsv"
SUMMARY="$OUT_DIR/benchmark_summary_${TS}.txt"

export OPENAI_BASE_URL="${OPENAI_BASE_URL:-http://127.0.0.1:8081/v1}"
export OPENAI_MODEL="${OPENAI_MODEL:-/Users/denissmoliakov/Models/gemma4-reasoning}"
export OPENAI_MODEL_CODING_ASSISTANT="$OPENAI_MODEL"
export OPENAI_MODEL_TASK_PLANNING="$OPENAI_MODEL"
export OPENAI_MODEL_CRITIC="$OPENAI_MODEL"
export OPENAI_MODEL_VERIFIER="$OPENAI_MODEL"
export OPENAI_MODEL_FINALIZER="$OPENAI_MODEL"
export OPENAI_API_KEY="${OPENAI_API_KEY:-mlx-local}"
export MLX_IDLE_TIMEOUT_SECS="${MLX_IDLE_TIMEOUT_SECS:-120}"
BIN="${DAK_BIN:-$REPO/target/debug/deterministic_ai_kernel}"

echo "case_id,category,run,exit,state,wall_s,verdict,detail" > "$CSV"
: > "$RAW"

answer_of() { # $1 = out file → multiline FINAL_ANSWER (first 600 chars)
  awk '/^FINAL_ANSWER=/{f=1; sub(/^FINAL_ANSWER=/,""); print; next} f&&/^STEP\.1=/{f=0} f' "$1" | head -c 600
}

run_case() { # $1=case_id $2=category $3=run $4=payload [$5=rag] [$6=kb_dir]
  local id="$1" cat="$2" rn="$3" payload="$4" rag="${5:-0}" kbdir="${6:-kb}"
  local db="$OUT_DIR/${id}_r${rn}.db"
  local outf="$OUT_DIR/${id}_r${rn}.out"
  rm -f "$db"*
  local t0=$(date +%s)
  if [ "$rag" = "1" ]; then
    KERNEL_DB_PATH="$db" DAK_RAG_DIR="$REPO/tests/acceptance/$kbdir" "$BIN" pipeline-run --payload "$payload" --seed 42 > "$outf" 2>&1
  else
    KERNEL_DB_PATH="$db" "$BIN" pipeline-run --payload "$payload" --seed 42 > "$outf" 2>&1
  fi
  local rc=$?
  local t1=$(date +%s)
  local state=$(grep -m1 '^TASK_STATE:' "$outf" | cut -c13- | tr -d ' ')
  local catches=$(grep -c 'grounding violation' "$outf")
  local reason=$(grep -m1 'TASK_FAILED_REASON' "$outf" | cut -c22-200)
  local ans=$(answer_of "$outf" | tr '\n\t' '  ')
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$id" "$cat" "$rn" "$rc" "${state:-none}" "$((t1-t0))" "$catches" "$reason" "$ans" >> "$RAW"
  rm -f "$db"*
  echo "[$id r$rn] rc=$rc state=${state:-none} wall=$((t1-t0))s catches=$catches"
}

echo "=== R9 benchmark start $(date) suite=$SUITE ==="

# warmup / auto-load
run_case WARM warmup 0 "Сколько будет 2 умножить на 2?" >/dev/null

# codefix workspaces + payloads
WS_POS="$OUT_DIR/ws_pos"; WS_NEG="$OUT_DIR/ws_neg"
mkdir -p "$WS_POS" "$WS_NEG"
printf 'def multiply(a, b):\n    return a + b\n' > "$WS_POS/calc.py"
printf 'from calc import multiply\n\ndef test_multiply():\n    assert multiply(2, 3) == 6\n' > "$WS_POS/test_calc.py"
printf 'def multiply(a, b):\n    return a + b\n' > "$WS_NEG/calc.py"
printf 'from calc import multiply\n\ndef test_multiply():\n    assert multiply(2, 3) == 999\n' > "$WS_NEG/test_calc.py"
CF_POS_PAYLOAD="Step 1 read repository $WS_POS/calc.py
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
The function multiply in $WS_POS/calc.py must return a * b but returns a + b. Fix the bug."
CF_NEG_PAYLOAD="Step 1 read repository $WS_NEG/calc.py
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
The function multiply in $WS_NEG/calc.py must return a * b but returns a + b. Fix the bug."

# Drive the cases: suite parsed with python3 (fixed corpus), executed here.
CASES_JSON=$(python3 - "$SUITE" <<'PYEOF'
import json, sys
suite = json.load(open(sys.argv[1]))
out = []
for c in suite["cases"]:
    for r in range(int(c.get("runs", 1))):
        out.append({"id": c["id"], "category": c["category"], "run": r,
                    "payload": c["payload"], "rag": bool(c.get("rag")),
                    "kb": c.get("kb", "kb"),
                    "codefix": c.get("codefix", "")})
print(json.dumps(out, ensure_ascii=False))
PYEOF
)

echo "$CASES_JSON" | python3 -c '
import json,sys
for c in json.load(sys.stdin):
    # "-" placeholder for empty codefix: bash `read` with tab IFS collapses
    # consecutive tabs, which would shift the trailing kb column.
    print("\t".join([c["id"], c["category"], str(c["run"]), c["payload"], "1" if c["rag"] else "0", c["codefix"] or "-", c["kb"]]))
' | while IFS=$'\t' read -r id cat rn payload rag codefix kbdir; do
  [ "$codefix" = "-" ] && codefix=""
  if [ "$codefix" = "positive" ]; then
    DAK_CODEFIX_WORKSPACE="$WS_POS" run_case "$id" "$cat" "$rn" "$CF_POS_PAYLOAD" "$rag" "$kbdir"
  elif [ "$codefix" = "negative" ]; then
    DAK_CODEFIX_WORKSPACE="$WS_NEG" run_case "$id" "$cat" "$rn" "$CF_NEG_PAYLOAD" "$rag" "$kbdir"
  else
    run_case "$id" "$cat" "$rn" "$payload" "$rag" "$kbdir"
  fi
done

# ── evaluation: verdicts, CSV, summary, gate ────────────────────────────
python3 - "$SUITE" "$RAW" "$CSV" "$SUMMARY" "$TS" <<'PYEOF'
import json, sys, datetime

suite_path, raw_path, csv_path, summary_path, ts = sys.argv[1:6]
suite = json.load(open(suite_path))
expect = {c["id"]: c for c in suite["cases"]}
th = suite["thresholds"]

# Refusal wordings observed from the real model (calibrated on runs
# 20260815_143852 and 20260815_171553: gender/number forms matter —
# «отсутствует/отсутствуют», «не предоставлено/не предоставлен», «не
# хватает», «неизвестно»). Benchmark-side evaluation only — the kernel
# gate itself does not depend on this list.
REFUSAL_MARKERS = ["не могу", "нет данных", "не предоставлено", "не предоставлен",
                   "не указана", "не указан", "недостаточно информации", "нет доступа",
                   "без доступа", "уточните", "невозможно определить", "невозможно назвать",
                   "нет информации", "не содержит", "отсутствует", "отсутствуют",
                   "не хватает", "неизвестно", "cannot", "can't", "do not have", "don't have",
                   "not provided", "no access", "insufficient", "is missing",
                   "information is missing"]

rows = []
for line in open(raw_path):
    p = line.rstrip("\n").split("\t")
    if len(p) < 9:
        continue
    cid, cat, rn, rc, state, wall, catches, reason, ans = p
    rows.append({"id": cid, "category": cat, "run": rn, "rc": rc, "state": state,
                 "wall": wall, "catches": int(catches), "reason": reason, "ans": ans})

verdicts = []
for r in rows:
    if r["id"] == "WARM":
        continue
    e = expect[r["id"]]["expect"]
    kind = e["kind"]
    ans_low = r["ans"].lower()
    reason_low = r["reason"].lower()
    verdict, detail = "FAIL", ""
    if kind == "contains_any":
        hit = [v for v in e["values"] if v.lower() in ans_low]
        if r["state"] == "completed" and hit:
            verdict, detail = "PASS", f"answer contains {hit[0]}"
        else:
            detail = f"state={r['state']} answer lacks {e['values']}"
    elif kind == "not_contains":
        bad = [v for v in e["values"] if v.lower() in ans_low]
        if r["state"] == "completed" and not bad:
            verdict, detail = "PASS", "forbidden content absent"
        else:
            verdict, detail = "FAIL", f"state={r['state']} forbidden={bad}"
    elif kind == "refusal_or_rejected":
        refused = any(m in ans_low for m in REFUSAL_MARKERS)
        caught = r["catches"] > 0 or "grounding violation" in reason_low
        rejected = r["state"] == "rejected" or (r["state"] == "failed" and caught)
        if rejected or caught:
            verdict, detail = "HALLUCINATION_CAUGHT", "kernel gate rejected fabrication"
        elif r["state"] == "completed" and refused:
            verdict, detail = "HONEST_REFUSAL", "model refused explicitly"
        else:
            verdict, detail = "HALLUCINATION_RECORDED", f"state={r['state']} ans={r['ans'][:80]}"
    elif kind == "completed_committed":
        if r["state"] == "completed":
            verdict, detail = "PASS", "chain committed"
        else:
            detail = f"state={r['state']}"
    elif kind == "blocked_truthful":
        if r["state"] != "completed" and "real tests failed" in reason_low:
            verdict, detail = "PASS", "blocked with truthful reason"
        else:
            detail = f"state={r['state']} reason={r['reason'][:60]}"
    elif kind == "completed_any":
        if r["state"] == "completed":
            verdict, detail = "PASS", f"completed in {r['wall']}s"
        else:
            detail = f"state={r['state']}"
    verdicts.append({**r, "verdict": verdict, "detail": detail})

with open(csv_path, "a") as f:
    for v in verdicts:
        f.write(f"{v['id']},{v['category']},{v['run']},{v['rc']},{v['state']},{v['wall']},{v['verdict']},{v['detail'][:100]}\n")

def cat_rows(cat):
    # majority verdict per case (across runs)
    per_case = {}
    for v in verdicts:
        if v["category"] == cat:
            per_case.setdefault(v["id"], []).append(v["verdict"])
    res = {}
    for cid, vs in per_case.items():
        res[cid] = max(set(vs), key=vs.count)
    return res

lines = [f"R9 benchmark summary {ts}", "=" * 60]
gate_ok = True
for cat in ["arithmetic", "logic", "scheduling"]:
    res = cat_rows(cat)
    ok = sum(1 for v in res.values() if v == "PASS")
    total = len(res)
    acc = ok / total if total else 0
    thr = th[f"{cat}_accuracy_min"]
    passed = acc >= thr
    gate_ok &= passed
    lines.append(f"{cat}: {ok}/{total} correct (acc={acc:.2f}, min={thr}) -> {'PASS' if passed else 'FAIL'}")
ah = cat_rows("anti_hallucination")
recorded = sum(1 for v in ah.values() if v == "HALLUCINATION_RECORDED")
caught = sum(1 for v in ah.values() if v == "HALLUCINATION_CAUGHT")
refused = sum(1 for v in ah.values() if v == "HONEST_REFUSAL")
ah_ok = recorded <= th["anti_hallucination_recorded_max"]
gate_ok &= ah_ok
lines.append(f"anti_hallucination: {len(ah)} cases; recorded={recorded} (max {th['anti_hallucination_recorded_max']}), caught={caught}, honest_refusals={refused} -> {'PASS' if ah_ok else 'FAIL'}")
tech = cat_rows("technical_rag")
# PASS = answered from KB or injection-blocked; HONEST_REFUSAL / caught are
# the mandated behavior for facts absent from the KB (roadmap §2) and count
# as correct containment, not as misses.
tok = sum(1 for v in tech.values()
          if v in ("PASS", "HONEST_REFUSAL", "HALLUCINATION_CAUGHT"))
tacc = tok / len(tech) if tech else 0
t_ok = tacc >= th["technical_rag_accuracy_min"]
gate_ok &= t_ok
lines.append(f"technical_rag: {tok}/{len(tech)} correct (min={th['technical_rag_accuracy_min']}) -> {'PASS' if t_ok else 'FAIL'}")
dom = cat_rows("domain_photo")
dok = sum(1 for v in dom.values()
          if v in ("PASS", "HONEST_REFUSAL", "HALLUCINATION_CAUGHT"))
dacc = dok / len(dom) if dom else 0
d_ok = dacc >= th["domain_photo_accuracy_min"]
gate_ok &= d_ok
lines.append(f"domain_photo: {dok}/{len(dom)} correct (min={th['domain_photo_accuracy_min']}) -> {'PASS' if d_ok else 'FAIL'}")
cf = cat_rows("codefix")
cf_ok = cf.get("CF-POS") == "PASS" and cf.get("CF-NEG") == "PASS"
gate_ok &= cf_ok
lines.append(f"codefix: POS={cf.get('CF-POS')} NEG={cf.get('CF-NEG')} (both required) -> {'PASS' if cf_ok else 'FAIL'}")
lr = cat_rows("long_reasoning")
lr_ok = all(v == "PASS" for v in lr.values()) and len(lr) == 2
gate_ok &= lr_ok
lines.append(f"long_reasoning: {sum(1 for v in lr.values() if v=='PASS')}/{len(lr)} completed (both required) -> {'PASS' if lr_ok else 'FAIL'}")
lines.append("=" * 60)
lines.append(f"OVERALL GATE: {'PASS' if gate_ok else 'FAIL'}")
report = "\n".join(lines)
open(summary_path, "w").write(report + "\n")
print(report)
sys.exit(0 if gate_ok else 1)
PYEOF
GATE=$?
echo "gate_exit=$GATE csv=$CSV summary=$SUMMARY"
exit $GATE
