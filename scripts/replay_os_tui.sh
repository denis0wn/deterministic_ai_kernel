#!/usr/bin/env bash
# Replay OS v4 — Rust TUI launcher
# Falls back to bash version if Rust binary not available

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RUST_BIN="$PROJECT_DIR/target/debug/replay_os"

if [[ -x "$RUST_BIN" ]]; then
    exec "$RUST_BIN" "$@"
else
    # Try to build silently
    cd "$PROJECT_DIR" && cargo build --bin replay_os 2>/dev/null 1>/dev/null
    if [[ -x "$RUST_BIN" ]]; then
        exec "$RUST_BIN" "$@"
    else
        # Fallback to bash version
        exec bash "$SCRIPT_DIR/replay_os.sh" "$@"
    fi
fi
