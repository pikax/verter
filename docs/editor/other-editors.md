# Other Editors (Lapce / Zed / Helix / Neovim)

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Beyond the [VS Code extension](/editor/vscode), Verter ships thin LSP clients for
four more editors. Each one runs the **same** native `verter-lsp` server over
stdio — the client just tells the editor how to launch it and adds **zero**
per-message latency. All four give you the same type-aware IDE features for Vue
and Svelte (diagnostics, hover, completion, go-to-definition, find-references,
rename, code actions, semantic tokens, inlay hints, signature help) over the
shared LSP path.

## The four editors

| Editor   | What it is                                  | Per-editor guide |
| -------- | ------------------------------------------- | ---------------- |
| **Lapce**  | A compiled WASM volt (plugin) that launches `verter-lsp`. | [`extensions/lapce/README.md`](https://github.com/pikax/verter/blob/main/extensions/lapce/README.md) |
| **Zed**    | A compiled WASM extension that launches `verter-lsp`.     | [`extensions/zed/README.md`](https://github.com/pikax/verter/blob/main/extensions/zed/README.md) |
| **Helix**  | A pure `languages.toml` config (built-in native LSP client). | [`editors/helix/README.md`](https://github.com/pikax/verter/blob/main/editors/helix/README.md) |
| **Neovim** | A pure Lua config (built-in LSP client).                  | [`editors/nvim/README.md`](https://github.com/pikax/verter/blob/main/editors/nvim/README.md) |

For each editor, its README covers the **prerequisites**, the **install** steps,
and the **config override** (how to point it at `verter-lsp`):

- **Lapce** — prerequisites (Rust + `wasm32-wasip1` target + a built
  `verter-lsp`), install (the unpacked-volt layout or the `install:lapce-local`
  helper), and the `lsp.serverPath` / `lsp.serverSource` config keys. See
  [`extensions/lapce/README.md`](https://github.com/pikax/verter/blob/main/extensions/lapce/README.md).
- **Zed** — prerequisites (the official Vue/Svelte Zed extension + a built
  `verter-lsp`), install (`zed: install dev extension`), and the
  `lsp.verter.binary.path` / `serverSource` settings. See
  [`extensions/zed/README.md`](https://github.com/pikax/verter/blob/main/extensions/zed/README.md).
- **Helix** — prerequisites (`verter-lsp` on `PATH` or an absolute `command`),
  install (merge `languages.toml`), and the `command` override. See
  [`editors/helix/README.md`](https://github.com/pikax/verter/blob/main/editors/helix/README.md).
- **Neovim** — prerequisites (Neovim ≥ 0.11 + `verter-lsp` on `PATH` or an
  absolute `cmd_path`), install (`require("verter").setup({})` or lazy.nvim), and
  the `cmd_path` override. See
  [`editors/nvim/README.md`](https://github.com/pikax/verter/blob/main/editors/nvim/README.md).

## Install `verter-lsp`

All four clients need the native `verter-lsp` binary. v0 does **not** auto-download
it (managed download is a roadmap item), so build it from source. From a checkout
of the repository:

```bash
git clone https://github.com/pikax/verter.git
cd verter
cargo build -p verter_lsp --release
```

This produces the server binary at:

- **Windows:** `target\release\verter-lsp.exe`
- **macOS / Linux:** `target/release/verter-lsp`

(The `.exe` suffix is Windows-only.) Use the absolute path to that binary when you
configure your editor below, or place it on your `PATH`.

For full type features, install the `tsgo` type provider
(`@typescript/native-preview`) as a dev dependency in the project you open —
Verter launches with `--type-provider=tsgo` and discovers it from the project's
`node_modules`.

## Point the editor at `verter-lsp`

Every client uses the **same** discovery model (the same concept VS Code's
[`verter.lspBinaryPath`](/editor/settings#general) setting covers): an explicit
binary path takes precedence; otherwise you can opt into `PATH` discovery.

1. **Explicit path (simplest, recommended).** Set the editor's binary-path key to
   the absolute path of the `verter-lsp` you built:
   - Lapce: `lsp.serverPath`
   - Zed: `lsp.verter.binary.path`
   - Helix: `command` in `[language-server.verter]`
   - Neovim: `cmd_path` in `require("verter").setup({ ... })`
2. **`PATH` opt-in.** Put `verter-lsp` on your `PATH` and opt in. `PATH` discovery
   is **opt-in** (Lapce/Zed: `serverSource = "path"`) so a stale binary on `PATH`
   can't silently break version coupling; Helix/Neovim launch the bare
   `verter-lsp` name directly when no absolute path is set.

If **neither** is configured, the Lapce and Zed clients **fail loudly** with setup
guidance (naming both remedies) instead of silently launching nothing — this is
intended behavior, not a bug. The loud message looks like:

> could not resolve a verter-lsp server source: … Set `lsp.serverPath` … or … opt
> into PATH discovery …

The remedy is exactly the two options above.

## Automated test coverage (honest infeasibility note)

**Lapce and Zed have no headless extension-test harness**, so a real GUI launch
cannot be exercised in CI. The **authoritative automated check of the shared
launch path** — that `verter-lsp` actually launches over stdio and completes an
LSP `initialize` handshake returning real capabilities — is the Rust
stdio-launch smoke at
[`crates/verter_lsp/tests/stdio_launch_smoke.rs`](https://github.com/pikax/verter/blob/main/crates/verter_lsp/tests/stdio_launch_smoke.rs).
Both clients' launch contracts are additionally pinned by host-target unit tests
(the launch-contract tests in each extension's `src/lib.rs`, built in their own
CI), and the contract logic itself lives in one shared crate
(`verter-editor-client`) so the clients cannot diverge.

The two config-only clients are guarded differently:

- **Helix** — a hermetic Rust config-contract test parses the shipped
  `editors/helix/languages.toml` and asserts it against the shared launch contract
  (no Helix binary required).
- **Neovim** — a headless plenary suite runs the Lua config builders and a **real
  `verter-lsp` attach smoke** (the CI job builds the binary and exports
  `$VERTER_LSP_BIN`), across a Neovim-version × OS matrix.
