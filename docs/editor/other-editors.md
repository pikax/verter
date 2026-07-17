# Other Editors (Lapce / Zed / Helix / Neovim)

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Beyond the [VS Code extension](/editor/vscode), Verter ships integrations for
four more editors. Each one runs the **same** native `verter-lsp` server over
stdio; the integration tells the editor how to launch it and adds no proxy to
the per-message path. The server exposes its standard Vue/Svelte LSP features to
all four clients, but the feature a user sees still depends on that editor's LSP
capabilities. Current automated evidence is intentionally narrower than a claim
of full UI parity: the exact tested boundaries are listed below.

## The four editors

| Editor     | What it is                                                   | Per-editor guide                                                                                     |
| ---------- | ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| **Lapce**  | A compiled WASM volt (plugin) that launches `verter-lsp`.    | [`extensions/lapce/README.md`](https://github.com/pikax/verter/blob/main/extensions/lapce/README.md) |
| **Zed**    | A compiled WASM extension that launches `verter-lsp`.        | [`extensions/zed/README.md`](https://github.com/pikax/verter/blob/main/extensions/zed/README.md)     |
| **Helix**  | A pure `languages.toml` config (built-in native LSP client). | [`editors/helix/README.md`](https://github.com/pikax/verter/blob/main/editors/helix/README.md)       |
| **Neovim** | A pure Lua config (built-in LSP client).                     | [`editors/nvim/README.md`](https://github.com/pikax/verter/blob/main/editors/nvim/README.md)         |

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

For full type features, install TypeScript 7 (`typescript@7`) as a dev dependency
in the project you open. Verter launches with `--type-provider=tsgo` and
discovers the native `@typescript/typescript-<platform>-<arch>` binary installed
by that package from the project's `node_modules`.

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

## Automated test coverage

All four integrations have non-vacuous automated contracts, with deliberately
different evidence boundaries:

- **Lapce and Zed:** their CI jobs build the real WASM artifacts, export the
  exact production launch plans, and drive those plans through the shared stdio
  LSP client. Real Vue and Svelte diagnostics plus concrete typed hover are
  required. This validates artifact build, production launch policy, and that
  compact semantic slice; it does not prove untested features or GUI-host
  behavior because neither editor currently provides a headless GUI extension
  harness.
- **Helix:** the parsed TOML contract runs on every OS. A separate Linux lane
  installs checksum-verified Helix 25.07.1, proves both `hx --health` routes use
  only Verter, then drives the parsed shipping plan through the same real-server
  Vue/Svelte diagnostic-and-hover smoke. The semantic requests use the shared
  stdio driver, not Helix UI automation.
- **Neovim:** the shipped Lua setup runs inside a real headless Neovim client on
  the supported-version × OS matrix. It covers Vue/Svelte, JS/TS, readiness,
  UTF-8 negotiation, diagnostics, hover, authored definition, completion,
  template rename, and clean shutdown. Missing prerequisites and zero executed
  tests are hard failures.

These gates are regression evidence, not a statement that every server feature
has been exercised in every editor. The editor-neutral LSP suite owns broad
protocol behavior; the contracts above prove the integration-specific launch
and a compact critical slice through each available real-client boundary.
