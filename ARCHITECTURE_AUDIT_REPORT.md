# Architecture Audit Report

**Date:** 2026-07-16
**Commit:** 59dc612 (backup/pre-orchestrator-layer-2026-07-11)
**Auditor:** Automated + Manual Review

---

## Documents Created

| Document | Purpose | Status |
|----------|---------|--------|
| `GOVERNANCE.md` | Rules of architectural evolution | ✅ Created |
| `KERNEL_SPECIFICATION.md` | Formal system specification | ✅ Created |
| `scripts/verify_architecture.sh` | 7 automated guardrails | ✅ Created |
| `scripts/cleanup_backups.sh` | Remove leftover temp files | ✅ Created |

---

## Guardrail Results

Run: `bash scripts/verify_architecture.sh`

| # | Check | Result |
|---|-------|--------|
| 1 | Kernel core isolation (no forbidden imports) | ✅ PASS |
| 2 | No hidden non-determinism | ✅ PASS |
| 3 | Provider isolation (no scheduler/worker) | ✅ PASS |
| 4 | No direct filesystem in core | ✅ PASS |
| 5 | No direct network in core | ✅ PASS |
| 6 | Execution module isolation | ✅ PASS |
| 7 | No leftover backup/temp files | ❌ FAIL (8 files) |

### Backup Files Found (Guardrail #7)
- `src/llm.rs.backup-before-runtime-model-router`
- `src/execution/primitive_executor.rs.before_cache_final`
- `src/execution/primitive_executor.rs.before_cache_integration_v2`
- `src/execution/primitive_executor.rs.before_primitive_cache`
- `src/execution/primitive_executor.rs.pre_clean_cache_patch`
- `src/workflow/compiler.rs.bak`
- `src/workflow/compiler.rs.tmp`
- `src/providers/storage.rs.bak`
- `src/bin/bias_explain.rs.disabled`

**Fix:** Run `bash scripts/cleanup_backups.sh` or manually delete.

---

## Code Fixes Applied

### 1. `src/worker.rs` — Panic-safe DB override guard
**Problem:** Manual `set_override_path(Some/None)` pattern without panic safety. If operation panics, override remains set, corrupting state for subsequent calls.
**Fix:** Replaced with RAII `_DbOverrideGuard` (same pattern already used in `scheduler.rs`).
**Impact:** 5 functions updated. No API change. No behavior change on happy path.

---

## Test Results

```
cargo test --lib -- --test-threads=1
test result: ok. 142 passed; 0 failed; 0 ignored
```

All 142 tests pass after changes.

---

## Remaining Risks & Tech Debt

| Priority | Issue | Location | Notes |
|----------|-------|----------|-------|
| HIGH | Backup files in src/ | 9 files listed above | Run cleanup script |
| MEDIUM | `std::env::set_var` race in parallel tests | Multiple test files | Use `--test-threads=1` or injectable provider |
| MEDIUM | `api.rs` uses `rusqlite::Connection` directly | `integrity_json_report()` | Should go through StorageProvider |
| LOW | `event_bus.rs::EventBus::new()` opens raw Connection | Constructor only | Schema init; acceptable but not ideal |
| LOW | `storage.rs` contains domain logic (`classify_failure_outcome`, `capability_for_worker_id`) | `DefaultStorage` impl | Should be in worker/scheduler layer |

---

## Verification Commands

```bash
# Run architecture guardrails
bash scripts/verify_architecture.sh

# Clean up backup files
bash scripts/cleanup_backups.sh

# Run all tests (sequential for determinism)
cargo test --lib -- --test-threads=1

# Run specific test suites
cargo test --test replay_snapshot -- --test-threads=1
cargo test --test scheduler_integration -- --test-threads=1
```
