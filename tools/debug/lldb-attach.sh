#!/usr/bin/env bash
# Attach LLDB to a running process by name (or PID) and capture
# backtraces of all threads. Designed for the verter bench
# (`audit_real_component_meta.exe`) hangs on Windows.
#
# Usage:
#   tools/debug/lldb-attach.sh <process-name-or-pid> [output-file]
#
# Example:
#   tools/debug/lldb-attach.sh audit_real_component_meta.exe /tmp/stack.txt
#
# Requires:
#   - LLDB installed at "/c/Program Files/LLVM/bin/lldb.exe"
#   - Python 3.11 at /c/Users/david/AppData/Local/Programs/Python/Python311
#     (lldb.exe links against python311.dll)
#   - Target binary built with line-tables-only debug info (set
#     `RUSTFLAGS="-C debuginfo=line-tables-only"` for the build).

set -euo pipefail

LLDB="/c/Program Files/LLVM/bin/lldb.exe"
PYDIR="/c/Users/david/AppData/Local/Programs/Python/Python311"

if [[ -z "${1:-}" ]]; then
  echo "usage: $0 <process-name-or-pid> [output-file]" >&2
  exit 2
fi

TARGET="$1"
OUT="${2:-/tmp/lldb-attach-output.txt}"

# Detect numeric (PID) vs string (process name)
if [[ "$TARGET" =~ ^[0-9]+$ ]]; then
  ATTACH_CMD="process attach --pid $TARGET"
else
  ATTACH_CMD="process attach --name $TARGET"
fi

# All-thread backtrace + immediate detach. The `--continue` on
# `process detach` resumes the process so the bench can keep running.
PATH="$PYDIR:$PATH" "$LLDB" \
  --batch \
  -o "$ATTACH_CMD" \
  -o "thread backtrace all" \
  -o "thread list" \
  -o "process detach" \
  -o "quit" \
  2>&1 | tee "$OUT"
