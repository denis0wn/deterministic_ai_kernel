#!/bin/bash
# Deterministic AI Kernel — production launcher (roadmap §5).
# Usage:
#   bash ops/run_production.sh pipeline-run --payload "..." --seed 42
#   DAK_BIN=./target/release/deterministic_ai_kernel bash ops/run_production.sh ...
#   KERNEL_DB_PATH=/tmp/x.db bash ops/run_production.sh pipeline-run ...
set -u
DAK_OPS_ROOT="$(cd "$(dirname "$0")" && pwd)"
export DAK_OPS_ROOT
# shellcheck source=ops/production.env
source "$DAK_OPS_ROOT/production.env"

BIN="${DAK_BIN:-$DAK_OPS_ROOT/../target/release/deterministic_ai_kernel}"
if [ ! -x "$BIN" ]; then
  BIN="$DAK_OPS_ROOT/../target/debug/deterministic_ai_kernel"
fi
if [ ! -x "$BIN" ]; then
  echo "ERROR: kernel binary not found (build first: cargo build --release)" >&2
  exit 2
fi

# Preflight: model endpoint must be reachable OR lifecycle auto-load enabled.
if ! curl -s -m 3 "${OPENAI_BASE_URL%/v1}/v1/models" >/dev/null 2>&1; then
  if [ "$MLX_LIFECYCLE" = "on" ]; then
    echo "[preflight] endpoint down — MLX lifecycle auto-load will start it" >&2
  else
    echo "ERROR: endpoint $OPENAI_BASE_URL unreachable and MLX_LIFECYCLE=off" >&2
    exit 3
  fi
fi

exec "$BIN" "$@"
