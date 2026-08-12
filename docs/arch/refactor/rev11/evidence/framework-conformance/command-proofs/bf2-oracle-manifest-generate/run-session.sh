#!/bin/bash
# BF1 perf-lock measurement session for BF2_VUE_ORACLE_MANIFEST_GENERATE /
# BF2_SVELTE_ORACLE_MANIFEST_GENERATE. Runs the frozen, unmodified
# generate-official-case-manifests.mjs under a network-denying sandbox,
# N times, capturing wall time (real, from /usr/bin/time -l) and peak RSS
# ("maximum resident set size", bytes on macOS) per run, and verifying the
# deterministic correctness oracle (exact row counts + byte-identical TSV
# vs the committed manifests) on every run.
set -euo pipefail

REPO=/Users/carlosrodrigues/Documents/dev/verter-bf1-perf
SCRATCH=/tmp/bf1-perf-measure
SANDBOX_PROFILE="$SCRATCH/deny-network.sb"
VUE_SOURCE=/private/tmp/verter-rescope.G7sDua/vue-core
SVELTE_SOURCE=/private/tmp/verter-rescope.G7sDua/svelte
VUE_MODULES="$SCRATCH/vue-oracle"
SCRIPT="$REPO/docs/arch/refactor/rev11/evidence/framework-conformance/generate-official-case-manifests.mjs"
COMMITTED_VUE_TSV="$REPO/docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv"
COMMITTED_SVELTE_TSV="$REPO/docs/arch/refactor/rev11/evidence/framework-conformance/svelte-official-cases.tsv"

N=${1:-10}
OUT_LOG="$SCRATCH/session-raw.log"
: > "$OUT_LOG"

echo "session_start_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "$OUT_LOG"
echo "node_version=$(node --version)" | tee -a "$OUT_LOG"
echo "runs_requested=$N" | tee -a "$OUT_LOG"
echo "vue_source_head=$(git -C "$VUE_SOURCE" rev-parse HEAD)" | tee -a "$OUT_LOG"
echo "svelte_source_head=$(git -C "$SVELTE_SOURCE" rev-parse HEAD)" | tee -a "$OUT_LOG"
echo "script_blob=$(git -C "$REPO" rev-parse HEAD:docs/arch/refactor/rev11/evidence/framework-conformance/generate-official-case-manifests.mjs)" | tee -a "$OUT_LOG"

for i in $(seq 1 "$N"); do
  RUN_OUT="$SCRATCH/run-$i"
  rm -rf "$RUN_OUT"
  mkdir -p "$RUN_OUT"
  TIME_FILE="$SCRATCH/time-$i.txt"
  STDOUT_FILE="$SCRATCH/stdout-$i.txt"

  # /usr/bin/time -l wraps the sandboxed node invocation; captures real time
  # and maximum resident set size. Network is denied for the whole child tree.
  set +e
  /usr/bin/time -l sandbox-exec -f "$SANDBOX_PROFILE" node "$SCRIPT" \
    --vue-source "$VUE_SOURCE" \
    --svelte-source "$SVELTE_SOURCE" \
    --vue-modules "$VUE_MODULES" \
    --out-dir "$RUN_OUT" \
    > "$STDOUT_FILE" 2> "$TIME_FILE"
  STATUS=$?
  set -e

  if [ "$STATUS" -ne 0 ]; then
    echo "run_$i status=FAIL exit=$STATUS" | tee -a "$OUT_LOG"
    cat "$TIME_FILE" | tee -a "$OUT_LOG"
    exit 1
  fi

  # Correctness oracle: exact stdout counts AND byte-identical TSVs vs committed.
  COUNTS=$(cat "$STDOUT_FILE")
  if ! diff -q "$RUN_OUT/vue-official-cases.tsv" "$COMMITTED_VUE_TSV" > /dev/null; then
    echo "run_$i status=FAIL reason=vue_tsv_mismatch" | tee -a "$OUT_LOG"
    exit 1
  fi
  if ! diff -q "$RUN_OUT/svelte-official-cases.tsv" "$COMMITTED_SVELTE_TSV" > /dev/null; then
    echo "run_$i status=FAIL reason=svelte_tsv_mismatch" | tee -a "$OUT_LOG"
    exit 1
  fi

  REAL=$(grep "real" "$TIME_FILE" | awk '{print $1}')
  MAXRSS=$(grep "maximum resident set size" "$TIME_FILE" | awk '{print $1}')

  echo "run_$i status=OK counts=$COUNTS real_s=$REAL maxrss_bytes=$MAXRSS" | tee -a "$OUT_LOG"

  rm -rf "$RUN_OUT"
done

echo "session_end_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee -a "$OUT_LOG"
