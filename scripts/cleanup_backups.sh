#!/usr/bin/env bash
# Remove leftover backup/temp files from src/
set -euo pipefail

echo "Removing leftover backup/temp files..."

find src/ -type f \( \
    -name "*.bak" \
    -o -name "*.tmp" \
    -o -name "*.backup*" \
    -o -name "*.before_*" \
    -o -name "*.pre_*" \
    -o -name "*.disabled" \
\) -print -delete

echo "Done."
