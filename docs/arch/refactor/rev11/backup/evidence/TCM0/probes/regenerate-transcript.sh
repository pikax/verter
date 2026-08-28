#!/usr/bin/env bash
# Regenerates transcript.md from a real run of all ten numbered TCM0 probes.
#
# TWO transformations are applied to each probe's output, and both are stated here because a transcript
# that understates what its generator removes is not evidence of what the probe printed.
#
# 1. Absolute paths are REDACTED deterministically: per-run OS temp fixture roots become <FIXTURE>, and
#    any other absolute path becomes <ABS>. Committed evidence must not carry machine-specific absolute
#    paths (`verter_source_policy_gate`'s tracked_files_contain_no_machine_specific_path_markers), and a
#    path that differs on every run is not evidence anyway.
# 2. Go RUNTIME STACK-FRAME lines are FILTERED OUT by the `grep -vE` below: `goroutine`/`runtime`/
#    `github.com`/tab-indented frames/`main.runAPI|runMain|main`/`os/signal`. Probe 6 deliberately
#    provokes a Go panic, whose stack is hundreds of machine- and build-specific frames. The panic's
#    HEADER line survives — it is the finding — and probe 6 asserts on the panic itself, so the transcript
#    still shows what was observed. What is removed is the frame list, not the evidence.
#
# Nothing else in the output is altered.
#
# usage: ./regenerate-transcript.sh <dir-containing-node_modules/typescript>
set -euo pipefail
TS_DIR="${1:?usage: regenerate-transcript.sh <ts-candidate-dir>}"
cd "$(dirname "$0")"
PROBES=(probe1-init-timing probe2-stale-snapshot probe3-stale-sourcefile-confirm
        probe4-filechanges-correct probe5-bulk-semantic-api probe6-out-of-range-completion-panic
        probe7-mapper-wire-capture probe8-lsp-session-attach
        probe9-transform-response-contract probe10-external-source-unit)
redact() {
  sed -E -e 's#(/private)?/var/folders/[^ "]*/(tcm0-[a-zA-Z0-9-]+)[^ ",)]*#<FIXTURE>#g' \
         -e 's#(/private)?/tmp/(tcm0-[a-zA-Z0-9-]+)[^ ",)]*#<FIXTURE>#g' \
         -e 's#/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+){3,}#<ABS>#g'
}
transcript_tmp="$(mktemp .transcript.md.tmp.XXXXXX)"
cleanup() { rm -f "$transcript_tmp"; }
trap cleanup EXIT
{
  echo "# TCM0 probe transcript"; echo
  echo "Produced by \`regenerate-transcript.sh\`, which runs all ten numbered TCM0 probes against"
  echo "\`typescript@7.1.0-dev.20260822.1\` and applies TWO transformations to each probe's output:"
  echo "absolute paths are redacted (\`<FIXTURE>\` for the per-run OS temp fixture root, \`<ABS>\` for any"
  echo "other absolute path), and Go runtime STACK-FRAME lines are filtered out (probe 6 provokes a Go"
  echo "panic deliberately; its header line survives, its hundreds of build-specific frames do not)."
  echo "Nothing else is altered."; echo
  echo "| field | value |"; echo "|---|---|"
  echo "| date (UTC) | $(date -u '+%Y-%m-%d %H:%M:%S') |"
  echo "| host platform | $(uname -srm) |"
  echo "| host state | CONTENDED — other builds and agent processes were running concurrently |"
  echo "| node | $(node --version) |"
  echo "| package version | $(cd "$TS_DIR" && node -e "console.log(require('typescript/package.json').version)") |"
  echo "| package gitHead | $(cd "$TS_DIR" && node -e "console.log(require('typescript/package.json').gitHead)") |"
  echo
  echo "Probe 1 asserts no TIMING; its one assertion is that the cold path completes, so it too exits"
  echo "non-zero on a hang. Probes 2-10 exit non-zero if any assertion fails. No figure in probe 1 is an"
  echo "acceptance threshold — see \`../performance-baselines.md\`."; echo
} > "$transcript_tmp"
for p in "${PROBES[@]}"; do
  echo "## \`$p.mjs\`" >> "$transcript_tmp"; echo '' >> "$transcript_tmp"; echo '```' >> "$transcript_tmp"
  set +e
  node "$p.mjs" --ts "$TS_DIR" 2>&1 \
    | grep -vE "^(goroutine|runtime|github\.com|	|main\.(runAPI|runMain|main)|os/signal)" \
    | redact >> "$transcript_tmp"
  statuses=("${PIPESTATUS[@]}")
  set -e
  if [ "${statuses[0]}" -ne 0 ]; then
    echo "FAILED: $p.mjs exited ${statuses[0]}; transcript.md was not replaced" >&2
    exit "${statuses[0]}"
  fi
  if [ "${statuses[1]}" -ne 0 ] || [ "${statuses[2]}" -ne 0 ]; then
    echo "FAILED: transcript write/filter for $p.mjs (pipeline ${statuses[*]}); transcript.md was not replaced" >&2
    exit 1
  fi
  echo "exit=0" >> "$transcript_tmp"; echo '```' >> "$transcript_tmp"; echo '' >> "$transcript_tmp"
done
mv "$transcript_tmp" transcript.md
trap - EXIT
echo "regenerated; overall rc=0"
