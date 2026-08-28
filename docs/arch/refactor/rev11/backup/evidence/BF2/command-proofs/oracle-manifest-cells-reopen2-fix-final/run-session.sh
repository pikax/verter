#!/bin/bash
# Final-candidate measurement session for the two locked cells
# BF2_VUE_ORACLE_MANIFEST_GENERATE / BF2_SVELTE_ORACLE_MANIFEST_GENERATE.
# Runs the frozen, unmodified generate-official-case-manifests.mjs (git blob
# b61404de48e8ba86767a09414195b67a06ac56be) under the committed deny-network
# sandbox profile (git blob 5d41a32d8ba2ac7bfe905d87b406ea8f234de519), N
# times, capturing wall time (real, /usr/bin/time -l) and peak RSS per run,
# and verifying the deterministic correctness oracle (exact row counts +
# byte-identical TSVs vs the committed manifests) on every run.
set -euo pipefail

REPO=<worktree>/verter-bf2-fix2
SCRATCH=/tmp/bf2-perf-pass4-final
SANDBOX_PROFILE="$SCRATCH/deny-network.sb"
VUE_SOURCE="$REPO/packages/framework-conformance-harness/.oracle-checkouts/vue"
SVELTE_SOURCE="$REPO/packages/framework-conformance-harness/.oracle-checkouts/svelte"
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
echo "candidate_head=$(git -C "$REPO" rev-parse HEAD)" | tee -a "$OUT_LOG"
echo "vue_source_head=$(git -C "$VUE_SOURCE" rev-parse HEAD)" | tee -a "$OUT_LOG"
echo "svelte_source_head=$(git -C "$SVELTE_SOURCE" rev-parse HEAD)" | tee -a "$OUT_LOG"
echo "script_blob=$(git -C "$REPO" rev-parse HEAD:docs/arch/refactor/rev11/evidence/framework-conformance/generate-official-case-manifests.mjs)" | tee -a "$OUT_LOG"
echo "sandbox_profile_sha=$(shasum -a 1 "$SANDBOX_PROFILE" | awk '{print $1}')" | tee -a "$OUT_LOG"

# Zero-network control: curl must FAIL under the identical profile.
set +e
sandbox-exec -f "$SANDBOX_PROFILE" curl -sS -m 3 https://example.com > /dev/null 2>&1
CURL_STATUS=$?
set -e
echo "curl_denial_control_exit=$CURL_STATUS (nonzero=denied, required)" | tee -a "$OUT_LOG"
if [ "$CURL_STATUS" -eq 0 ]; then
  echo "SANDBOX NOT DENYING NETWORK — aborting" | tee -a "$OUT_LOG"
  exit 1
fi

for i in $(seq 1 "$N"); do
  RUN_OUT="$SCRATCH/run-$i"
  rm -rf "$RUN_OUT"
  mkdir -p "$RUN_OUT"
  TIME_FILE="$SCRATCH/time-$i.txt"
  STDOUT_FILE="$SCRATCH/stdout-$i.txt"

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
