#!/usr/bin/env bash
set -euo pipefail

PLAN_SRC="artifacts/verification_plan.json"
PLAN_FAKE="artifacts/verification_plan.invalid_env.json"
VERDICT_OUT="artifacts/verification_verdict.invalid_reuse.json"

./scripts/run_verification_graph.sh --pipeline fast >/dev/null

python3 - <<'PY'
import json
from pathlib import Path

src = Path("artifacts/verification_plan.json")
dst = Path("artifacts/verification_plan.invalid_env.json")

plan = json.loads(src.read_text())
plan["environment"]["environment_fingerprint"] = "0000000000000000000000000000000000000000000000000000000000000000"
dst.write_text(json.dumps(plan, indent=2) + "\n")
PY

set +e
./scripts/run_verification_graph.sh --pipeline fast --reuse-plan "$PLAN_FAKE" --out "$VERDICT_OUT" >/tmp/invalid_reuse_stdout.txt 2>&1
rc=$?
set -e

python3 - <<'PY'
import json
from pathlib import Path

verdict = json.loads(Path("artifacts/verification_verdict.invalid_reuse.json").read_text())

assert verdict["status"] == "invalid_reuse", verdict
assert verdict["ok"] is False, verdict
assert verdict["reason"] == "plan_hash matched but environment_fingerprint differed", verdict

print("invalid_reuse_status=ok")
PY

if [ "$rc" -ne 2 ]; then
  echo "expected exit code 2, got $rc"
  cat /tmp/invalid_reuse_stdout.txt
  exit 1
fi

echo "invalid_reuse_exit_code=2"
echo "invalid_reuse_regression=passed"
