#!/usr/bin/env bash
# Build the Verter Lapce volt and install it into Lapce's local plugins
# directory (macOS / Linux).
#
# Builds bin/verter-lapce.wasm (cargo, wasm32-wasip1, release), detects the
# per-channel Lapce plugins directory for the current OS, copies volt.toml + the
# wasm into <plugins>/verter/, and prints the exact `lsp.serverPath` config
# snippet with the absolute built verter-lsp path filled in. Idempotent (re-run
# to refresh). Fails loudly if the wasm or the verter-lsp binary is missing.
#
# Usage: install-local.sh [--channel Lapce-Stable|Lapce-Nightly|Lapce-Debug]
set -euo pipefail

CHANNEL="Lapce-Stable"
while [ $# -gt 0 ]; do
  case "$1" in
    --channel)
      CHANNEL="${2:-}"
      shift 2
      ;;
    --channel=*)
      CHANNEL="${1#*=}"
      shift
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      echo "usage: install-local.sh [--channel Lapce-Stable|Lapce-Nightly|Lapce-Debug]" >&2
      exit 1
      ;;
  esac
done

case "$CHANNEL" in
  Lapce-Stable | Lapce-Nightly | Lapce-Debug) ;;
  *)
    echo "error: --channel must be one of Lapce-Stable, Lapce-Nightly, Lapce-Debug (got: $CHANNEL)" >&2
    exit 1
    ;;
esac

fail() {
  echo "error: $1" >&2
  exit 1
}

# Repo root is three levels up from this script (extensions/lapce/scripts/).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
LAPCE_DIR="$REPO_ROOT/extensions/lapce"
WASM_PATH="$LAPCE_DIR/bin/verter-lapce.wasm"
VOLT_TOML="$LAPCE_DIR/volt.toml"

# 1. Build the volt (cargo wasm32-wasip1 release) and copy to bin/verter-lapce.wasm.
echo "==> Building the Verter Lapce volt (wasm32-wasip1, release)..."
rustup target add wasm32-wasip1 >/dev/null 2>&1 || true
cargo build --manifest-path "$LAPCE_DIR/Cargo.toml" --target wasm32-wasip1 --release \
  || fail "cargo build failed; cannot install the volt."

BUILT_WASM="$LAPCE_DIR/target/wasm32-wasip1/release/verter_lapce.wasm"
[ -f "$BUILT_WASM" ] || fail "built wasm not found at $BUILT_WASM — build first (pnpm run build:lapce)."
mkdir -p "$(dirname "$WASM_PATH")"
cp -f "$BUILT_WASM" "$WASM_PATH"

[ -f "$WASM_PATH" ] || fail "volt wasm missing at $WASM_PATH — build first (pnpm run build:lapce)."
[ -f "$VOLT_TOML" ] || fail "volt.toml missing at $VOLT_TOML."

# 2. Detect the per-channel Lapce plugins directory for the current OS.
uname_s="$(uname -s)"
case "$uname_s" in
  Darwin)
    # macOS: ~/Library/Application Support/dev.lapce.<Channel>/plugins/
    PLUGINS_DIR="$HOME/Library/Application Support/dev.lapce.$CHANNEL/plugins"
    SERVER_BIN="$REPO_ROOT/target/release/verter-lsp"
    ;;
  Linux | *)
    # Linux: ~/.local/share/lapce-<channel-lowercase>/plugins/
    channel_lower="$(printf '%s' "$CHANNEL" | tr '[:upper:]' '[:lower:]')"
    data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
    PLUGINS_DIR="$data_home/$channel_lower/plugins"
    SERVER_BIN="$REPO_ROOT/target/release/verter-lsp"
    ;;
esac

# 3. Create <plugins>/verter/ and copy volt.toml + bin/verter-lapce.wasm into it.
VOLT_DIR="$PLUGINS_DIR/verter"
mkdir -p "$VOLT_DIR/bin"
cp -f "$VOLT_TOML" "$VOLT_DIR/volt.toml"
cp -f "$WASM_PATH" "$VOLT_DIR/bin/verter-lapce.wasm"
echo "==> Installed volt to $VOLT_DIR"

# 4. Print the exact lsp.serverPath snippet with the absolute verter-lsp path.
if [ -f "$SERVER_BIN" ]; then
  BIN_NOTE="found at $SERVER_BIN"
else
  BIN_NOTE="NOT built yet — run 'cargo build -p verter_lsp --release' first"
fi

echo ""
echo "==> verter-lsp binary: $BIN_NOTE"
echo "==> Add this to your Lapce settings (settings.toml):"
echo ""
echo "[volt.verter]"
echo "\"lsp.serverPath\" = \"$SERVER_BIN\""
echo "\"typeProvider\" = \"tsgo\""
echo ""
echo "==> Now restart Lapce (or reload plugins) to pick up the volt."
