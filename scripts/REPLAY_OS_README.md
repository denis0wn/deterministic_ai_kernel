# Replay OS

Interactive TUI for deterministic_ai_kernel — a polished terminal product for iTerm.

## Versions

### v4 (Rust TUI) — NEW
Native Rust TUI using ratatui + crossterm for a product-grade experience.

**Features:**
- Full-screen native TUI
- Keyboard-first UX (j/k, Enter, Esc, q)
- Split panes (nav + content)
- Live status updates
- History browser with rerun
- Runtime details screen
- Tests and Diagnostics screens

**Launch:**
```bash
# Build and run
cargo build --bin replay_os
./target/debug/replay_os

# Or use wrapper script
scripts/replay_os_tui.sh
```

### v2/v3 (Bash + gum) — Stable
Shell-based TUI using gum for interactive menus.

**Features:**
- Interactive menus with gum
- Run Task with summary card
- Recent Runs browser
- Runtime management
- Tests with parsed summaries
- Config dashboard

**Launch:**
```bash
replay-os
# or
dek
```

## Requirements

### v4 (Rust TUI)
- Rust toolchain (for building)
- Terminal with 256 color support

### v2/v3 (Bash + gum)
- `gum` — https://github.com/charmbracelet/gum
- `jq`
- `curl`
- `cargo`

## Install gum (for bash version)

```bash
# macOS (arm64)
cd /tmp && curl -sL -o gum.tar.gz "https://github.com/charmbracelet/gum/releases/download/v0.14.5/gum_0.14.5_Darwin_arm64.tar.gz"
tar xzf gum.tar.gz && cp gum_0.14.5_Darwin_arm64/gum ~/bin/gum && chmod +x ~/bin/gum
```

## Setup

Add to `~/.zshrc`:

```bash
export PATH="$HOME/bin:$PATH"

# Rust TUI (v4)
replay-os() {
    local bin="/Users/denissmoliakov/projects/deterministic_ai_kernel_clean/target/debug/replay_os"
    if [[ -x "$bin" ]]; then
        "$bin" "$@"
    else
        source /Users/denissmoliakov/projects/deterministic_ai_kernel_clean/scripts/replay_os.sh "$@"
    fi
}
```

Then `source ~/.zshrc`.

## Keyboard Shortcuts

### v4 (Rust TUI)
| Key | Action |
|-----|--------|
| ↑↓ / j/k | Navigate |
| Enter | Select |
| Esc | Go back |
| q | Quit |
| 1, 2 | Quick actions (Tests/Diagnostics) |
| r | Refresh (Runtime Details) |
| r | Rerun (Recent Runs) |

### v2/v3 (Bash + gum)
| Key | Action |
|-----|--------|
| ↑↓ | Navigate menu |
| Enter | Select item |
| Esc | Go back |
| q | Quit (from main menu) |
| Ctrl+C | Force quit |

## Menu Structure

```
Replay OS
├── Run Task              → pipeline-run with full UX
├── Runtime Details       → MLX server status, probe, restart
├── Diagnostics           → doctor, integrity
├── Recent Runs           → history browser, rerun
├── Tests                 → lib, full, preflight with summary
├── Workflow Ops          → schedule, reconcile, effects, leases
├── Config / Paths        → configuration dashboard
└── Exit
```

## History

All task runs are saved to `.replay_os/history/` as JSON files:
- Timestamp, task text, seed, plan ID, final answer, raw JSON output
- Browse via "Recent runs" in menu
- Re-run any previous task directly from history
- Compatible between bash and Rust versions

## Log Files

- **Runtime log**: `/tmp/replay_os_mlx.log` — MLX server output
- **Test logs**: `/tmp/replay_os_test.XXXXXX` — temporary, cleaned after viewing
- **History**: `.replay_os/history/` — persistent task run records

## Architecture

```
scripts/
├── replay_os.sh          # bash TUI launcher (v2/v3)
├── replay_os_tui.sh      # Rust TUI launcher (v4)
├── lib/
│   └── helpers.sh        # shared functions for bash version
├── dek.sh                # simple CLI launcher (legacy)
└── REPLAY_OS_README.md   # this file

src/bin/
├── replay_os.rs          # Rust TUI entry point
└── tui/
    ├── app.rs            # application state
    ├── ui.rs             # rendering
    ├── history.rs        # history management
    └── runtime.rs        # runtime probe/commands
```

## Troubleshooting

**Rust TUI won't build:**
```bash
cargo build --bin replay_os
```

**gum not found (bash version):**
```bash
cd /tmp && curl -sL -o gum.tar.gz "https://github.com/charmbracelet/gum/releases/download/v0.14.5/gum_0.14.5_Darwin_arm64.tar.gz"
tar xzf gum.tar.gz && cp gum_0.14.5_Darwin_arm64/gum ~/bin/gum && chmod +x ~/bin/gum
```

**MLX server won't start:**
- Check if port 8080 is in use: `lsof -i :8080`
- Check logs: Runtime Details → Tail runtime log
- Kill stale processes: `pkill -f mlx_lm.server`

**Pipeline-run fails:**
- Check runtime is online: Runtime Details
- Check model config: Config / Paths
- Run diagnostics: Diagnostics → Doctor
