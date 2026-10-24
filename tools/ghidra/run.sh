#!/bin/bash
# usage: run.sh <work dir> <Script.java> [args...]   runs a script read-only on the project analyze.sh built
set -euo pipefail
source "$(dirname "$0")/env.sh"
work=$1 script=$2; shift 2
"$G/support/analyzeHeadless" "$work/proj" "${PROJ:-app}" -process "${PROG:-app.bin}" -noanalysis -readOnly \
  -scriptPath "$HERE/scripts" -postScript "$script" "$@" 2>&1 \
  | grep -E '\.java>|ERROR|Exception|error:' | sed -E 's/^INFO  //; s/ \(GhidraScript\) *$//'
