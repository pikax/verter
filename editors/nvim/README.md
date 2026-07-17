# Verter for Neovim

LSP-client configuration for the [Verter](../../README.md) language server
(`verter-lsp`) — type-aware IDE features for Vue and Svelte single-file
components in Neovim.

Neovim has a **built-in LSP client**, so "Neovim support" is a pure Lua config
layer. There is no compiled extension and no server-side change: the native
client talks stdio directly to the native `verter-lsp` process. The Lua layer
does **zero per-request work** — it assembles the config once at attach.

Requires **Neovim ≥ 0.11** (for `vim.lsp.config` / `vim.lsp.enable`).

## Prerequisites

- `verter-lsp` on your `PATH` (or pass an absolute `cmd_path`). There is **no
  managed download** — install it through your package manager, or build it with
  `cargo build -p verter_lsp --release`. A mason.nvim registry entry is a planned follow-up
  (see the [Roadmap in the Neovim support design](../../docs/arch/neovim-support-design.md#91-roadmap-out-of-v0-scope)).
- For full type features, the project should have **TypeScript 7 (`typescript@7`)**
  installed (the normal case for a typed Vue/Svelte project). The default type
  provider is `tsgo`, which discovers the native
  `@typescript/typescript-<platform>-<arch>` binary installed by that package
  (`VERTER_TSGO_BIN` → project `node_modules` → `PATH` → npm/npx cache).

## Quick start

### 1. Modern built-in API (recommended)

Using the in-repo module:

```lua
require("verter").setup({})
```

Or fully inline, without the module (simplest copy-paste; relies on the cwd
fallback for the workspace root):

```lua
vim.lsp.config("verter", {
  cmd = { "verter-lsp", "--type-provider=tsgo" },
  filetypes = { "vue", "svelte" },
  root_markers = { "package.json", "tsconfig.json", ".git" },
})
vim.lsp.enable("verter")
```

The in-repo module additionally passes the resolved workspace root positionally
(precise-root parity with the VS Code client), forces the file-watcher
capability off by default, installs the save-notify autocmd, and mirrors the
server-read `init_options` — see [Options](#options).

### 2. lazy.nvim

Point lazy.nvim at this directory (or a published mirror) and call `setup` in
`config`:

```lua
{
  -- From a checkout of this monorepo:
  dir = "/abs/path/to/verter/editors/nvim",
  -- (or, once a standalone mirror exists: "verter-dev/verter.nvim")
  config = function()
    require("verter").setup({
      -- Recommended for completion speed (optional):
      capabilities = require("blink.cmp").get_lsp_capabilities(),
    })
  end,
}
```

### 3. nvim-lspconfig users

The in-repo module supersedes nvim-lspconfig for Verter today. An upstream
`lsp/verter.lua` for `neovim/nvim-lspconfig` is a tracked follow-up, gated on
published per-platform `verter-lsp` release assets
(see the [Roadmap in the Neovim support design](../../docs/arch/neovim-support-design.md#91-roadmap-out-of-v0-scope)). Until then use recipe 1 or 2.

## Completion engine

Auto-import **works without any completion engine**: Neovim 0.11's built-in
client already advertises `completionItem.resolveSupport` (including
`additionalTextEdits`) and `snippetSupport`, and `verter-lsp` returns auto-import
edits as `additionalTextEdits` applied during `completionItem/resolve`.

For faster completion, [**blink.cmp**](https://github.com/saghen/blink.cmp) is
recommended (Rust fuzzy matcher, low per-keystroke latency). Pass its
capabilities into `setup`:

```lua
require("verter").setup({
  capabilities = require("blink.cmp").get_lsp_capabilities(),
})
-- nvim-cmp alternative:
-- capabilities = require("cmp_nvim_lsp").default_capabilities()
```

Either engine's capabilities are merged into the verter config; the built-in
capabilities are used when neither is installed.

## Position encoding

Neovim 0.11 advertises UTF-8 first and `verter-lsp` prefers UTF-8, so the two
negotiate the **zero-conversion** encoding automatically. No action required.

## Options

`setup(opts)` accepts (defaults shown):

```lua
require("verter").setup({
  cmd_path = "verter-lsp",        -- binary name on PATH, or an absolute path
  check_binary = true,            -- probe the binary before registering; set
                                  --   false if it is provided by a wrapper
  type_provider = "tsgo",         -- auto | tsgo | tsserver | off
  server_args = {},               -- extra args, inserted before the trailing root
                                  --   (may not override --type-provider/--tsdk/--plugin-path)
  filetypes = { "vue", "svelte" },
  root_markers = {
    "tsconfig.json", "jsconfig.json", "vite.config.ts", "vite.config.js",
    "nuxt.config.ts", "svelte.config.js", "package.json", ".git",
  },
  watch_files = false,            -- see "File watching" below
  semantic_tokens = true,         -- see "Semantic tokens" below
  log_level = "info",             -- forwarded as VERTER_LOG
  -- init-option parity (server-read subset only):
  lint = { enabled = false, preset = "recommended" },
  inlay_hints = { enabled = true },
  vite_config = { enabled = true, trusted_files = {} },
  experimental = { conditional_root_narrowing = false, strict_slots = false },
  hover = { provenance = false },
  statistics = { enabled = false },
  capabilities = nil,             -- merged in (blink.cmp / cmp_nvim_lsp)
})
```

Only the canonical six server-read init options are forwarded — `lint`,
`inlayHints`, `viteConfig`, `experimental`, `hover`, `statistics` (the same
parity set every Verter editor client ships). VS-Code-UI-only surfaces
(`configuration`, `mcp`, `decorations`) are intentionally omitted, and
`frameworks` is dropped because the server ignores it.

### File watching (`watch_files`, default OFF)

`verter-lsp` can dynamically register `workspace/didChangeWatchedFiles`. The
config **forces this capability off by default** because Neovim's recursive
watcher has no `node_modules` ignore and is a documented CPU sink on large
trees. The cheap replacement is a `BufWritePost` autocmd for `*.js` / `*.ts`
that notifies the server with `$/onFileChanged { uri, type = "update" }`, whose
handler **re-reads the file from the workspace VFS** — exactly the external-edit
freshness signal a save needs. (The related `$/onDidChangeTsOrJsFile` method is
for _in-editor_ TS/JS edits and carries a `changes` array of deltas, which a save
does not have, so the save autocmd does not use it.)

Set `watch_files = true` to enable dynamic watchers (and skip the autocmd) if
you prioritize external-edit freshness over watcher CPU.

> **Caveat:** with watchers off, edits made **outside Neovim** (e.g. a `git`
> checkout, a codegen step) may leave cross-file type state stale until another
> trigger — a buffer edit, a `*.js`/`*.ts` save, or `:e`. Re-open or re-save to
> refresh, or enable `watch_files`.

### Semantic tokens (`semantic_tokens`, default ON)

`verter-lsp` advertises **full-document** semantic tokens (no range/delta), so
each refresh recomputes the whole projected surface. They are valuable for
Vue/Svelte and on by default; set `semantic_tokens = false` for a one-line
opt-out (clears the provider in `on_attach`). Incremental (range/delta) token
support is a server-side enhancement tracked separately.

## v0 limitations

- **No managed binary download.** Install `verter-lsp` yourself; a mason.nvim
  entry is a follow-up.
- **Semantic tokens are full-document only.**
- **External-edit staleness** when `watch_files` is off (see the caveat above).

## Running the tests

The config and attach specs are pure Lua. The fail-closed smoke additionally
launches a real `verter-lsp` and requires the pinned fixture dependencies. All
specs run under [plenary.nvim](https://github.com/nvim-lua/plenary.nvim)'s busted
harness in headless Neovim. Plenary must be on the runtimepath — either install
it under the standard packpath or point `$PLENARY_PATH` at a checkout.

From the repo root:

```bash
npm ci --ignore-scripts --prefix editors/nvim/tests/fixtures/real-client
cargo build -p verter_lsp
VERTER_TSGO_BIN=$(node --input-type=module -e \
  "import getExePath from './editors/nvim/tests/fixtures/real-client/node_modules/typescript/lib/getExePath.js'; console.log(getExePath())")

PLENARY_PATH=/path/to/plenary.nvim \
VERTER_LSP_BIN="$PWD/target/debug/verter-lsp" \
VERTER_TSGO_BIN="$VERTER_TSGO_BIN" \
  nvim --headless --noplugin \
    -u editors/nvim/tests/minimal_init.lua \
    -c "PlenaryBustedDirectory editors/nvim/tests/ {minimal_init='editors/nvim/tests/minimal_init.lua'}" \
    -c "qa!"
```

- `config_spec.lua` and `on_attach_spec.lua` are the always-on gate (no binary
  required).
- `smoke_spec.lua` is fail-closed: `$VERTER_LSP_BIN`, `$VERTER_TSGO_BIN`, and the
  pinned fixture dependencies are required. It loads the shipped
  `require("verter").setup`, opens Vue and Svelte in both TypeScript and
  JavaScript modes, and hard-asserts the fixture root, one UTF-8 client, matched
  readiness/sync, no TS7026, concrete hover, authored definition, exact
  completion, markup rename, and clean shutdown.

CI explicitly provisions every prerequisite and runs the same contract across a
Neovim version × OS matrix. The workflow also rejects output that does not prove
at least one assertion-bearing test ran.
