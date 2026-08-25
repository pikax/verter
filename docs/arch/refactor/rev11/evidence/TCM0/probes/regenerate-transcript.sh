#!/usr/bin/env bash
# Regenerates transcript.md from a real run of every probe.
#
# Absolute paths are REDACTED deterministically: per-run OS temp fixture roots become <FIXTURE>, and any
# other absolute path becomes <ABS>. Committed evidence must not carry machine-specific absolute paths
# (`verter_source_policy_gate`'s tracked_files_contain_no_machine_specific_path_markers), and a path that
# differs on every run is not evidence anyway. Nothing else in the output is altered.
#
# usage: ./regenerate-transcript.sh <dir-containing-node_modules/typescript>
set -uo pipefail
TS_DIR="${1:?usage: regenerate-transcript.sh <ts-candidate-dir>}"
cd "$(dirname "$0")"
PROBES=(probe1-init-timing probe2-stale-snapshot probe3-stale-sourcefile-confirm
        probe4-filechanges-correct probe5-bulk-semantic-api probe6-out-of-range-completion-panic
        probe7-mapper-wire-capture probe8-lsp-session-attach)
redact() {
  sed -E -e 's#(/private)?/var/folders/[^ "]*/(tcm0-[a-zA-Z0-9-]+)[^ ",)]*#<FIXTURE>#g' \
         -e 's#(/private)?/tmp/(tcm0-[a-zA-Z0-9-]+)[^ ",)]*#<FIXTURE>#g' \
         -e 's#/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+){3,}#<ABS>#g'
}
{
  echo "# TCM0 probe transcript"; echo
  echo "Produced by \`regenerate-transcript.sh\`, which runs every probe in this directory against"
  echo "\`typescript@7.1.0-dev.20260822.1\` and redacts absolute paths (\`<FIXTURE>\` for the per-run OS temp"
  echo "fixture root, \`<ABS>\` for any other absolute path). Nothing else is altered."; echo
  echo "| field | value |"; echo "|---|---|"
  echo "| date (UTC) | $(date -u '+%Y-%m-%d %H:%M:%S') |"
  echo "| host platform | $(uname -srm) |"
  echo "| host state | CONTENDED — other builds and agent processes were running concurrently |"
  echo "| node | $(node --version) |"
  echo "| package version | $(cd "$TS_DIR" && node -e "console.log(require('typescript/package.json').version)") |"
  echo "| package gitHead | $(cd "$TS_DIR" && node -e "console.log(require('typescript/package.json').gitHead)") |"
  echo
  echo "Probe 1 is measurement only and asserts nothing beyond \"the cold path completes\"; probes 2-8 exit"
  echo "non-zero if any assertion fails. No figure in probe 1 is an acceptance threshold — see"
  echo "\`../performance-baselines.md\`."; echo
} > transcript.md
rc=0
for p in "${PROBES[@]}"; do
  echo "## \`$p.mjs\`" >> transcript.md; echo '' >> transcript.md; echo '```' >> transcript.md
  node "$p.mjs" --ts "$TS_DIR" 2>&1 \
    | grep -vE "^(goroutine|runtime|github\.com|	|main\.(runAPI|runMain|main)|os/signal)" \
    | redact >> transcript.md
  # PIPESTATUS[0] is the probe's (node's) own exit status. `pipestatus` (lowercase) is a zsh builtin,
  # not a bash one — this script is #!/usr/bin/env bash, so that reference was always unset, and the
  # `:-` fallback always took PIPESTATUS[1], which is `grep`'s exit status (0 unless grep itself
  # errors), not the probe's. Capture PIPESTATUS[0] immediately, before anything else runs and
  # clobbers the array.
  s=${PIPESTATUS[0]}
  echo "exit=$s" >> transcript.md; echo '```' >> transcript.md; echo '' >> transcript.md
  [ "$s" = "0" ] || rc=1
done
echo "regenerated; overall rc=$rc"
exit $rc
