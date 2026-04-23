#!/usr/bin/env bash
# tools/check-four-mode-terminology.sh
#
# Verifies the workspace is free of retired two-mode terminology.
# Introduced by the generic-navigation track (.claude/plans/generic-navigation-prep-plan.md, A1b).
# Wired into CI as a mandatory step.
#
# Patterns rejected (per plan §5):
#   1. \btwo[-_ ]?modes?\b
#   2. \b(Type|Expanded)\s+modes?\b
#   3. "(Type|Expanded) (mode|MODE)"   (string literal form)
#   4. \bTypeMode\b
#   5. ///.*\b(Type|Expanded)\s+mode\b (subset of 2; covered)
#
# Backticked spans (`...`) are stripped from each line before the regex applies,
# so prose written as `ResolverMode::Type` does NOT fire.
#
# Allowlist (whole-line scan; if any of these appear on the line, the line is exempt):
#   - ProjectionMode::{Identity,Navigate,Shallow,Expanded}
# E1 retired the A1b→E1 transitional set (ResolverMode::Type, ResolverMode::Expanded)
# by deleting the ResolverMode enum from the workspace (§4 item 20).
#
# B1a retired the A1b→B1a transitional set (ExpandMode::, SemanticQueryKey::Expand,
# build_expand) by deleting those identifiers from the workspace, so this
# allowlist no longer carries them.
#
# Adding a bare prose phrase like "Type mode" to the allowlist is forbidden by
# the plan — rewrite the prose to backticked code form instead.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Prefer python3, but fall through to python if python3 is the Windows
# Microsoft Store stub (which exits with an install prompt on invocation
# rather than running anything).
find_python() {
  local candidate
  for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1; then
      if "$candidate" -c 'import sys; sys.exit(0)' >/dev/null 2>&1; then
        echo "$candidate"
        return 0
      fi
    fi
  done
  return 1
}

PYTHON="$(find_python || true)"
if [ -z "$PYTHON" ]; then
  echo "ERROR: working python3 or python required to run check-four-mode-terminology.sh." >&2
  exit 2
fi

"$PYTHON" - "$@" <<'PYEOF'
import os
import re
import subprocess
import sys

ALLOWED_RX = re.compile(
    r'ProjectionMode::(?:Identity|Navigate|Shallow|Expanded)'
)

RETIRED_RXES = [
    re.compile(r'\btwo[-_ ]?modes?\b', re.IGNORECASE),
    re.compile(r'\b(?:Type|Expanded)\s+modes?\b'),
    re.compile(r'"(?:Type|Expanded) (?:mode|MODE)"'),
    re.compile(r'\bTypeMode\b'),
]

INCLUDE_EXT = {
    '.rs', '.ts', '.tsx', '.js', '.mjs', '.cjs',
    '.md', '.toml', '.yml', '.yaml', '.json',
}

EXCLUDE_FULL_PATHS = {
    'tools/check-four-mode-terminology.sh',
    'tmp-plan.md',
}

EXCLUDE_PATH_PARTS = (
    'test-results/',
    'playwright-report/',
    '/snapshots/',
    'test-output/',
)

EXCLUDE_SUFFIXES = (
    '.pb.ts',
    '_pb.ts',
    '.snap',
    '.lock',
    'pnpm-lock.yaml',
)

BACKTICK_SPAN_RX = re.compile(r'`[^`]*`')


def strip_backticks(line: str) -> str:
    return BACKTICK_SPAN_RX.sub('', line)


def main() -> None:
    files = subprocess.check_output(['git', 'ls-files'], text=True).splitlines()
    bad_lines = []
    for f in files:
        if f in EXCLUDE_FULL_PATHS:
            continue
        if f.endswith(EXCLUDE_SUFFIXES):
            continue
        if any(part in f for part in EXCLUDE_PATH_PARTS):
            continue
        ext = os.path.splitext(f)[1].lower()
        if ext not in INCLUDE_EXT:
            continue
        try:
            with open(f, encoding='utf-8') as fh:
                for n, raw in enumerate(fh, start=1):
                    raw = raw.rstrip('\n').rstrip('\r')
                    if ALLOWED_RX.search(raw):
                        continue
                    stripped = strip_backticks(raw)
                    for rx in RETIRED_RXES:
                        if rx.search(stripped):
                            bad_lines.append((f, n, raw))
                            break
        except (UnicodeDecodeError, FileNotFoundError, PermissionError):
            continue
    for f, n, raw in bad_lines:
        print(f"{f}:{n}: {raw}")
    if bad_lines:
        print("", file=sys.stderr)
        print(
            f"Retired terminology found in {len(bad_lines)} location(s).",
            file=sys.stderr,
        )
        print(
            "Allowlist (always): ProjectionMode::{Identity,Navigate,Shallow,Expanded}.",
            file=sys.stderr,
        )
        print(
            "No transitional allowlist entries remain (E1 retired ResolverMode).",
            file=sys.stderr,
        )
        print(
            "Backticked spans (`...`) are stripped before regex application; "
            "rewrite prose to code-form instead of allowlisting prose.",
            file=sys.stderr,
        )
        sys.exit(1)


main()
PYEOF
