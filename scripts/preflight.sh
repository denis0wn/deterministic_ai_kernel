#!/usr/bin/env bash
set -euo pipefail
set -x

start_ts=$(date +%s)

cargo run -- integrity-json
cargo test --test snapshot_version_matrix -- --nocapture
cargo test --test golden_replay_corpus -- --nocapture
cargo test --test scheduler_integration -- --nocapture
cargo test --test event_ordering_fuzz -- --nocapture

end_ts=$(date +%s)
echo "preflight_seconds=$((end_ts - start_ts))"
