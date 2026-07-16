#!/usr/bin/env bash
# Architecture Guardrails for Deterministic AI Kernel
# Verifies architectural invariants defined in GOVERNANCE.md and KERNEL_SPECIFICATION.md
# Exit code 0 = all checks pass, non-zero = violations found

set -euo pipefail

SRC_DIR="src"
VIOLATIONS=0

echo "=== DAK Architecture Guardrails ==="
echo ""

# ──────────────────────────────────────────────
# Guardrail #1: Kernel core must not import runtime/domain modules
# GOVERNANCE.md §8, §21; SPEC §2
# ──────────────────────────────────────────────
echo "[1/7] Checking kernel core isolation..."

CORE_FILES=(
    "$SRC_DIR/exec_spec.rs"
    "$SRC_DIR/execution_abi.rs"
    "$SRC_DIR/kernel_error.rs"
    "$SRC_DIR/kernel_types.rs"
    "$SRC_DIR/execution_identity.rs"
)

FORBIDDEN_IMPORTS=("scheduler" "worker" "llm" "workflow::planner" "embeddings" "runtime_manager")

for file in "${CORE_FILES[@]}"; do
    if [ ! -f "$file" ]; then
        continue
    fi
    for forbidden in "${FORBIDDEN_IMPORTS[@]}"; do
        if grep -qn "use crate::${forbidden}" "$file" 2>/dev/null || \
           grep -qn "crate::${forbidden}::" "$file" 2>/dev/null; then
            echo "  ❌ VIOLATION: $file imports forbidden module '$forbidden'"
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done
done

if [ $VIOLATIONS -eq 0 ]; then
    echo "  ✅ Kernel core is isolated"
fi

# ──────────────────────────────────────────────
# Guardrail #2: No hidden non-determinism in kernel core
# GOVERNANCE.md §23; SPEC §5.3
# ──────────────────────────────────────────────
echo ""
echo "[2/7] Checking for hidden non-determinism..."

NONDET_PATTERNS=(
    "Uuid::new_v4"
    "Uuid::new_v1"
    "SystemTime::now"
    "thread_rng"
    "OsRng"
    "rand::random"
)

NONDET_VIOLATIONS=0
while IFS= read -r file; do
    case "$file" in
        */providers/*|*/bin/*|*/main.rs|*/metrics/*) continue ;;
    esac
    for pattern in "${NONDET_PATTERNS[@]}"; do
        if grep -qn "$pattern" "$file" 2>/dev/null; then
            echo "  ❌ VIOLATION: $file contains non-deterministic source '$pattern'"
            NONDET_VIOLATIONS=$((NONDET_VIOLATIONS + 1))
        fi
    done
done < <(find "$SRC_DIR" -name "*.rs" -type f ! -path "*/tests/*" ! -name "*test*")

if [ $NONDET_VIOLATIONS -eq 0 ]; then
    echo "  ✅ No hidden non-determinism detected"
else
    VIOLATIONS=$((VIOLATIONS + NONDET_VIOLATIONS))
fi

# ──────────────────────────────────────────────
# Guardrail #3: Providers must not import scheduler or worker
# GOVERNANCE.md §12, §14; SPEC §7
# ──────────────────────────────────────────────
echo ""
echo "[3/7] Checking provider isolation..."

PROVIDER_VIOLATIONS=0
while IFS= read -r file; do
    if grep -qn "use crate::scheduler" "$file" 2>/dev/null || \
       grep -qn "use crate::worker" "$file" 2>/dev/null || \
       grep -qn "crate::scheduler::" "$file" 2>/dev/null || \
       grep -qn "crate::worker::" "$file" 2>/dev/null; then
        echo "  ❌ VIOLATION: $file (provider) imports scheduler or worker"
        PROVIDER_VIOLATIONS=$((PROVIDER_VIOLATIONS + 1))
    fi
done < <(find "$SRC_DIR/providers" -name "*.rs" -type f 2>/dev/null)

if [ $PROVIDER_VIOLATIONS -eq 0 ]; then
    echo "  ✅ Providers are isolated from scheduler/worker"
else
    VIOLATIONS=$((VIOLATIONS + PROVIDER_VIOLATIONS))
fi

# ──────────────────────────────────────────────
# Guardrail #4: No direct filesystem access in kernel core
# GOVERNANCE.md §14; SPEC §5.3
# ──────────────────────────────────────────────
echo ""
echo "[4/7] Checking for direct filesystem access..."

FS_VIOLATIONS=0
FS_PATTERNS=("std::fs::read" "std::fs::write" "std::fs::create_dir" "File::open" "File::create" "std::fs::remove_file")

while IFS= read -r file; do
    case "$file" in
        */providers/*|*/bin/*|*/main.rs|*/snapshot.rs|*/api.rs) continue ;;
    esac
    for pattern in "${FS_PATTERNS[@]}"; do
        if grep -qn "$pattern" "$file" 2>/dev/null; then
            echo "  ❌ VIOLATION: $file uses direct filesystem ('$pattern')"
            FS_VIOLATIONS=$((FS_VIOLATIONS + 1))
        fi
    done
done < <(find "$SRC_DIR" -name "*.rs" -type f ! -path "*/tests/*" ! -path "*/providers/*")

if [ $FS_VIOLATIONS -eq 0 ]; then
    echo "  ✅ No direct filesystem access in kernel core"
else
    VIOLATIONS=$((VIOLATIONS + FS_VIOLATIONS))
fi

# ──────────────────────────────────────────────
# Guardrail #5: No direct network access in kernel core
# GOVERNANCE.md §14; SPEC §5.3
# ──────────────────────────────────────────────
echo ""
echo "[5/7] Checking for direct network access..."

NET_VIOLATIONS=0
NET_PATTERNS=("reqwest::" "hyper::" "TcpStream" "UdpSocket")

while IFS= read -r file; do
    case "$file" in
        */providers/*|*/bin/*|*/main.rs) continue ;;
    esac
    for pattern in "${NET_PATTERNS[@]}"; do
        if grep -qn "$pattern" "$file" 2>/dev/null; then
            echo "  ❌ VIOLATION: $file uses direct network ('$pattern')"
            NET_VIOLATIONS=$((NET_VIOLATIONS + 1))
        fi
    done
done < <(find "$SRC_DIR" -name "*.rs" -type f ! -path "*/tests/*" ! -path "*/providers/*")

if [ $NET_VIOLATIONS -eq 0 ]; then
    echo "  ✅ No direct network access in kernel core"
else
    VIOLATIONS=$((VIOLATIONS + NET_VIOLATIONS))
fi

# ──────────────────────────────────────────────
# Guardrail #6: Execution module must not import scheduler or workflow planner
# GOVERNANCE.md §8; SPEC §2
# ──────────────────────────────────────────────
echo ""
echo "[6/7] Checking execution module isolation..."

EXEC_VIOLATIONS=0
while IFS= read -r file; do
    if grep -qn "use crate::scheduler" "$file" 2>/dev/null || \
       grep -qn "crate::scheduler::" "$file" 2>/dev/null || \
       grep -qn "use crate::workflow::planner" "$file" 2>/dev/null || \
       grep -qn "crate::workflow::planner::" "$file" 2>/dev/null; then
        echo "  ❌ VIOLATION: $file (execution) imports scheduler or workflow::planner"
        EXEC_VIOLATIONS=$((EXEC_VIOLATIONS + 1))
    fi
done < <(find "$SRC_DIR/execution" -name "*.rs" -type f 2>/dev/null)

if [ $EXEC_VIOLATIONS -eq 0 ]; then
    echo "  ✅ Execution module is isolated"
else
    VIOLATIONS=$((VIOLATIONS + EXEC_VIOLATIONS))
fi

# ──────────────────────────────────────────────
# Guardrail #7: No .bak/.tmp/.backup files in src/
# Hygiene check
# ──────────────────────────────────────────────
echo ""
echo "[7/7] Checking for leftover backup/temp files..."

BACKUP_VIOLATIONS=0
while IFS= read -r file; do
    echo "  ❌ VIOLATION: Leftover file found: $file"
    BACKUP_VIOLATIONS=$((BACKUP_VIOLATIONS + 1))
done < <(find "$SRC_DIR" -type f \( -name "*.bak" -o -name "*.tmp" -o -name "*.backup*" -o -name "*.before_*" -o -name "*.pre_*" -o -name "*.disabled" \) 2>/dev/null)

if [ $BACKUP_VIOLATIONS -eq 0 ]; then
    echo "  ✅ No leftover backup/temp files"
else
    VIOLATIONS=$((VIOLATIONS + BACKUP_VIOLATIONS))
fi

# ──────────────────────────────────────────────
# Summary
# ──────────────────────────────────────────────
echo ""
echo "=========================================="
if [ $VIOLATIONS -eq 0 ]; then
    echo "✅ ALL ARCHITECTURE CHECKS PASSED"
    exit 0
else
    echo "❌ $VIOLATIONS VIOLATION(S) FOUND"
    exit 1
fi
