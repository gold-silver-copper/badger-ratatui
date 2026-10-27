#!/bin/sh
# Steps the showcase through its backdrops over USB (`press down`) and prints
# each one's mean fps and render time on the badge. Each step takes 4 s,
# the first second of which is the transition from the previous backdrop.
#
# Usage: tools/bench-backdrops.sh [steps, default 22]
# Environment: PYTHON, a Python with pyserial (default python3).
set -eu
cd "$(dirname "$0")/.."
: "${PYTHON:=python3}"
for _ in $(seq 1 "${1:-22}"); do
    "$PYTHON" tools/badge.py send 'press down'
    "$PYTHON" tools/badge.py stats 4 | tail -3
done | sed -nE 's/^showcase (.*) fps ([0-9.]+) .* render ([0-9.]+) .*/\1|\2|\3/p' |
    awk -F'|' '{ n[$1]++; f[$1] += $2; r[$1] += $3 }
        END { for (k in n) printf "%-18s fps %5.1f  render %5.1f ms\n", k, f[k] / n[k], r[k] / n[k] }' |
    sort
