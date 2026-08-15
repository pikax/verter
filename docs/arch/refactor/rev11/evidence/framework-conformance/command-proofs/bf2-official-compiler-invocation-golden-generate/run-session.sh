#!/bin/bash
# BF2 performance-lock measurement session for
# BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE. Runs the frozen,
# unmodified bin/generate-goldens.mjs (the FULL regenerate path — the actual
# "invoke the official compilers and produce golden output" workload) under
# a network-denying sandbox, N times, capturing wall time (real, from
# /usr/bin/time -l) and peak RSS ("maximum resident set size", bytes on
# macOS) per run, and verifying a deterministic correctness oracle (exact
# combined SHA-256 of all 48 produced golden files) on every run.
set -euo pipefail

REPO=<worktree>/verter-bf2
HARNESS="$REPO/packages/framework-conformance-harness"
SCRATCH=/tmp/bf2-perf-measure
SANDBOX_PROFILE="$SCRATCH/deny-network.sb"
SCRIPT="$HARNESS/bin/generate-goldens.mjs"

N=${1:-10}
OUT_LOG="$SCRATCH/session-raw.log"
: > "$OUT_LOG"

echo "session_start_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "$OUT_LOG"
echo "node_version=$(node --version)" | tee -a "$OUT_LOG"
echo "runs_requested=$N" | tee -a "$OUT_LOG"
echo "harness_head=$(git -C "$REPO" rev-parse HEAD)" | tee -a "$OUT_LOG"
echo "script_blob=$(git -C "$REPO" rev-parse HEAD:packages/framework-conformance-harness/bin/generate-goldens.mjs)" | tee -a "$OUT_LOG"

# Reference digest: goldens as currently committed (already verified via
# --check in the implementer's own session before this measurement began).
REFERENCE_DIGEST=$(cat "$HARNESS"/goldens/vue/*.json "$HARNESS"/goldens/svelte/*.json | shasum -a 256 | awk '{print $1}')
echo "reference_combined_digest=$REFERENCE_DIGEST" | tee -a "$OUT_LOG"

for i in $(seq 1 "$N"); do
  TIME_FILE="$SCRATCH/time-$i.txt"
  STDOUT_FILE="$SCRATCH/stdout-$i.txt"

  set +e
  /usr/bin/time -l sandbox-exec -f "$SANDBOX_PROFILE" node "$SCRIPT" \
    > "$STDOUT_FILE" 2> "$TIME_FILE"
  STATUS=$?
  set -e

  if [ "$STATUS" -ne 0 ]; then
    echo "run_$i status=FAIL exit=$STATUS" | tee -a "$OUT_LOG"
    cat "$TIME_FILE" | tee -a "$OUT_LOG"
    exit 1
  fi

  DIGEST=$(cat "$HARNESS"/goldens/vue/*.json "$HARNESS"/goldens/svelte/*.json | shasum -a 256 | awk '{print $1}')
  if [ "$DIGEST" != "$REFERENCE_DIGEST" ]; then
    echo "run_$i status=FAIL reason=digest_mismatch got=$DIGEST" | tee -a "$OUT_LOG"
    exit 1
  fi
  COUNTS=$(cat "$STDOUT_FILE")

  REAL=$(grep "real" "$TIME_FILE" | awk '{print $1}')
  MAXRSS=$(grep "maximum resident set size" "$TIME_FILE" | awk '{print $1}')

  echo "run_$i status=OK counts=$COUNTS digest_ok=true real_s=$REAL maxrss_bytes=$MAXRSS" | tee -a "$OUT_LOG"
done

echo "session_end_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "$OUT_LOG"
