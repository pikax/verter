#!/usr/bin/env bash
# tools/check-four-mode-terminology.sh
#
# Thin wrapper kept for the stable CI entry point (`bash
# tools/check-four-mode-terminology.sh`) and local callers. The checker
# itself is the pure-Rust xtask bin `check-four-mode-terminology` — see
# xtask/src/bin/check_four_mode_terminology.rs for the pattern set,
# allowlist, and exclude lists.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

exec cargo run -q --release -p xtask --bin check-four-mode-terminology -- "$@"
