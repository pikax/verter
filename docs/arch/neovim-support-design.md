# Verter Neovim Support — Design & Implementation Plan

> Status: **DESIGN — UNCOMMITTED draft for CTO review.** Research + design block. No production code landed.
> Date: 2026-06-22. Author: research+design block manager.
> Sibling design: [`docs/arch/lapce-extension-design.md`](./lapce-extension-design.md) (same server, different editor). The server-launch contract (`--type-provider=tsgo`, no `--tsdk`, positional root, init-options parity) is shared; the **distribution and binary-discovery strategy differs deliberately** (see §5, §8) because Neovim's conventions differ from Lapce's plugin model.

## 1. Context

Verter ships a native stdio LSP server, the `verter-lsp` binary (Rust crate `crates/verter_lsp`, `[[bin]] name = "verter-lsp"`, built via `cargo build -p verter_lsp --release`). The reference editor client is the VS Code extension at `packages/vue-vscode`. This document designs **Neovim support**.

**Neovim has a BUILT-IN LSP client (C/Lua, in-process). There is NO compiled extension to write.** Unlike VS Code (a `vscode-languageclient` extension) or Lapce (a `wasm32-wasip1` "volt"), Neovim ships its own native LSP client. "Neovim support" therefore means an **LSP-client CONFIGURATION**:

- (a) a server definition for Neovim's modern built-in `vim.lsp.config('verter', {...})` + `vim.lsp.enable('verter')` API (Neovim 0.11+), and/or an `nvim-lspconfig` `lsp/verter.lua` definition;
- (b) `cmd` (the `verter-lsp` invocation), `filetypes` (`vue`, `svelte`), `root_markers` / `root_dir`, and `init_options` reaching parity with the VS Code client's launch + init;
- (c) filetype detection for `.vue` / `.svelte` (Neovim core already supplies this — see §3.3).

This is a **new, non-overlapping artifact** (`editors/nvim/`). It touches **no existing crate** — the server is consumed as a built binary, not a library — and requires **no server-side change** (§8). Zero existing `nvim`/`neovim` references exist in the repo.

The **Lua config layer does ZERO per-request work.** It sets `cmd`/`filetypes`/`root`/`init_options`/`capabilities` exactly once at attach. All request-time work happens inside Neovim's native (C/Lua) LSP client and the native `verter-lsp` process — see §2 (Performance).

## 2. Performance (FIRST-CLASS REQUIREMENT)

Performance is a non-negotiable design driver: where Neovim offers multiple integration paths, the **most direct (least indirection)** one is chosen and justified here. This section is the architectural verdict; the rest of the doc implements it. An unprimed codex-architect consult (full verdict: `.feedback/_nvim_arch_consult2.out`) ruled on every choice below.

### 2.1 Integration mechanism — built-in native client, no proxy (chosen)

Three candidate mechanisms, ranked by request-time indirection:

| Mechanism | Request-time path | Verdict |
|---|---|---|
| **(a) Built-in `vim.lsp.config` + `vim.lsp.enable`** (0.11+) | Neovim's in-process **C/Lua native LSP client** ⇄ stdio ⇄ native `verter-lsp`. Lua touched once at attach. | **CHOSEN — fastest, most direct.** |
| (b) `nvim-lspconfig` `lsp/verter.lua` | **Identical** request-time path — `lsp/*.lua` is just a config-table source that feeds the **same** built-in client. | Equivalent at runtime; a distribution helper only (§5). |
| (c) RPC-bridge / wrapper plugin | Adds an extra Lua/process hop in front of every request. | **Rejected — strictly slower indirection, no benefit.** |

Architect verdict (verbatim sense): *"(a) and (b) have no meaningful per-request overhead difference … `nvim-lspconfig/lsp/verter.lua` is just a config-table source … Reject RPC bridges/wrapper plugins."* nvim-lspconfig's modern `lsp/*.lua` files **are** consumed by the built-in `vim.lsp.config` ([nvim-lspconfig migration #3494](https://github.com/neovim/nvim-lspconfig/issues/3494)); choosing (a) directly and offering (b) as a distribution helper costs nothing at request time.

**The fastest path is therefore: Neovim's built-in native LSP client talking stdio to `verter-lsp`, with a one-time-at-attach Lua config.** There is no faster mechanism available in Neovim — the native client is the transport; nothing sits between it and the server.

### 2.2 Position encoding — UTF-8 negotiated (zero-conversion, perf win)

Neovim 0.11's `vim.lsp.protocol.make_client_capabilities()` advertises `general.positionEncodings = ['utf-8', 'utf-16', 'utf-32']` — **UTF-8 first** ([protocol.lua @ release-0.11](https://github.com/neovim/neovim/blob/release-0.11/runtime/lua/vim/lsp/protocol.lua)). `verter-lsp` negotiates `UTF-8 > UTF-32 > UTF-16` (`crates/verter_lsp/src/server/lifecycle.rs` `handle_initialize`, preferring UTF-8 because it is "native Rust encoding — no conversion needed"). **Neovim + verter-lsp therefore negotiate UTF-8**, the zero-conversion encoding on both ends — a genuine performance advantage over the VS Code default (UTF-16). No config action required; this happens automatically.

### 2.3 Completion engine — recommend blink.cmp for SPEED (not a correctness prerequisite)

`verter-lsp` returns auto-import completions as `additionalTextEdits` applied during `completionItem/resolve`, and advertises completion-resolve **server-side** only when the active type provider supports it (`lifecycle.rs`: `resolve_provider = … supports_completion_resolve()`). For those edits to apply, the **client** must advertise `completionItem.resolveSupport` including `additionalTextEdits`.

**Fact (first-party):** Neovim 0.11's built-in `make_client_capabilities()` **does** advertise `completionItem.resolveSupport.properties = ['additionalTextEdits', 'command']` and `snippetSupport = true` by default ([protocol.lua @ release-0.11](https://github.com/neovim/neovim/blob/release-0.11/runtime/lua/vim/lsp/protocol.lua)). So **auto-import / additionalTextEdits works with the built-in client alone** — it is NOT gated on a third-party completion engine.

**Recommendation (performance):** use **blink.cmp** ([github.com/saghen/blink.cmp](https://github.com/saghen/blink.cmp)) where the user has a choice. blink.cmp uses a Rust-based fuzzy matcher ("frizbee") with ~0.5–4 ms async per-keystroke latency and LSP prefetching, versus nvim-cmp's Lua matcher + default 60 ms debounce. blink.cmp is the **completion-menu/source layer on top of** the built-in native LSP client — it does **not** replace the transport, so it composes with §2.1. Both engines also widen the advertised resolve property set (adding `documentation`/`detail`) via their capabilities helpers:

```lua
-- blink.cmp (recommended): faster, augments capabilities
capabilities = require('blink.cmp').get_lsp_capabilities()
-- nvim-cmp alternative:
capabilities = require('cmp_nvim_lsp').default_capabilities()
```

Our config merges whichever the user provides into the verter config's `capabilities` (§4.3). With neither installed, the built-in capabilities already cover auto-import.

### 2.4 File watching — `didChangeWatchedFiles` OFF by default, opt-in (perf)

`verter-lsp` **dynamically registers** `workspace/didChangeWatchedFiles` at runtime (carrier glob `**/*.{vue,svelte}`, plus `**/*.{ts,tsx,js,jsx,…}`, `**/tsconfig*.json`, `**/vite.config.*`, `**/package.json`, `**/.verterrc.json`) — see `lifecycle.rs` `handle_initialized`, whose comment explicitly names *"non-VS Code clients (Neovim, etc.)"* as the intended beneficiary. Whether Neovim honors that registration depends on the **client** advertising `workspace.didChangeWatchedFiles.dynamicRegistration`.

**Fact:** the built-in client sets `dynamicRegistration = (sysname == 'Darwin' or 'Windows_NT')` — **TRUE on macOS/Windows, FALSE on Linux** by default ([protocol.lua @ release-0.11](https://github.com/neovim/neovim/blob/release-0.11/runtime/lua/vim/lsp/protocol.lua)). Neovim's built-in watcher recursively watches the registered globs; on large trees (node_modules) this is a documented CPU sink ([neovim/neovim#23291](https://github.com/neovim/neovim/issues/23291)), and Neovim's watcher has no built-in node_modules ignore.

**Architect verdict:** *"Default do not advertise `workspace.didChangeWatchedFiles.dynamicRegistration`. Make it explicit user opt-in."* Rationale: it is an **optional client capability, not a semantic requirement**; broad recursive watching causes unacceptable hot-state CPU; the server stays correct for open-buffer edits regardless.

**Design:** our config **forces `capabilities.workspace.didChangeWatchedFiles.dynamicRegistration = false` by default** (overriding the macOS/Windows default to a single cross-platform behavior), with a `watch_files = true` opt-in for users who prioritize external-edit freshness. This is paired with §2.5 (the save-notify autocmd) and the §8 documented freshness caveat.

### 2.5 Cheap external-freshness signal — `BufWritePost` save-notify autocmd

Because watchers are off by default (§2.4), we replicate the pattern nvim-lspconfig's own Svelte definition uses: an `on_attach` `BufWritePost` autocmd for `*.js` / `*.ts` that notifies the server. The save fires `$/onFileChanged { uri, type = "update" }` (registered in `crates/verter_lsp/src/main.rs`); its handler maps `"update"` to `WorkspaceChange::FileChanged { source = None }`, which **re-reads the file from the workspace VFS** — exactly the external-edit freshness semantic a save (which carries no in-editor edit deltas) needs. The sibling `$/onFileChanged` `"create"`/`"delete"` types cover external add/remove. This is **client-side event plumbing, not semantic work** — a single `client:notify` on save, restoring a low-cost cross-file freshness signal without broad watchers. Architect: *"Replicate the small `BufWritePost` notification … only if dynamic file watching is disabled by default … client-side event plumbing, not semantic computation."* When `watch_files = true` it is redundant but harmless (server-side dedup).

> **Why not `$/onDidChangeTsOrJsFile`?** That custom method is the **in-editor delta** channel (the analog of VS Code's `onDidChangeTextDocument`): its params are `{ uri, changes }` where `changes` is a **required, non-defaulted** array of text edits, and the handler applies the last edit's `text` directly. A `BufWritePost` save has no such delta payload, so a `{ uri }`-only notify would fail server-side deserialization and be dropped. `$/onFileChanged`'s VFS re-read is the correct save/external-edit signal; `$/onDidChangeTsOrJsFile` is reserved for live in-buffer TS/JS edits and is not used by the save autocmd.

### 2.6 Semantic tokens — on by default, easy opt-out (full-document caveat)

`verter-lsp` advertises `semantic_tokens_provider` with `full = Bool(true)` and **no `range`, no `delta`** (`crates/verter_lsp/src/capabilities.rs`) — i.e. **full-document** token computation, not incremental. Neovim auto-starts semantic-token highlighting on attach when the server advertises the provider. Architect: leave semantic tokens *"on by default only if the server's semantic-token path is incremental, bounded, and projection-aware … else expose an opt-out."* Since verter's path is full-document (each refresh recomputes the whole projected TS surface), the design keeps them **on by default** (they are valuable for Vue/Svelte) but exposes a one-line opt-out (`semantic_tokens = false` → clears `server_capabilities.semanticTokensProvider` in `on_attach`), and flags the full-only cost as a known follow-up (range/delta support is a server-side enhancement, out of scope here).

### 2.7 Startup latency & client-per-root

`vim.lsp.enable('verter')` lazily attaches via a `FileType` autocmd, so `verter-lsp` is spawned only when a `vue`/`svelte` buffer opens — never per-file eagerly. Neovim starts **one client per resolved `root_dir`** and dedups via `reuse_client`, so opening many files in one project reuses a single server. Correct `root_markers` (§4.2) keep root resolution stable so the server is not re-spawned for sibling directories. `cmd_env = { VERTER_LOG = … }` sets log level once.

### 2.8 Performance defaults summary

| Knob | Default | Why |
|---|---|---|
| Integration mechanism | Built-in `vim.lsp.config` native client | Most direct; no proxy hop (§2.1). |
| Position encoding | UTF-8 (auto-negotiated) | Zero-conversion both ends (§2.2). |
| Completion engine | blink.cmp recommended (optional) | Rust matcher, ~0.5–4 ms; auto-import works without it (§2.3). |
| `didChangeWatchedFiles` dynamicRegistration | **false** (opt-in `watch_files`) | Avoid node_modules recursive-watch CPU spikes (§2.4). |
| `BufWritePost` save-notify | on (when watchers off) | Cheap external-freshness signal (§2.5). |
| Semantic tokens | on, easy opt-out | Valuable but full-document; opt-out for perf (§2.6). |
| Attach | Lazy, FileType-triggered, one client/root | No per-file spawn (§2.7). |

## 3. The Neovim LSP-config model (researched, with citations)

All facts in this section are **first-party verified** against Neovim 0.11 `lsp.txt` and the v0.11.0 `filetype.lua`, plus the live nvim-lspconfig `lsp/*.lua` sources (June 2026).

### 3.1 Modern built-in API — `vim.lsp.config` / `vim.lsp.enable` (0.11+)

Neovim 0.11 (2025) introduced a native LSP configuration system. `vim.lsp.config('name', cfg)` defines/merges a config; `vim.lsp.enable('name')` auto-starts a client for matching buffers. The `vim.lsp.Config` table fields (from `:help lsp-config` / `lsp.txt` @ release-0.11) include:

`cmd`, `filetypes`, `root_dir`, `root_markers`, `init_options`, `settings`, `capabilities`, `cmd_cwd`, `cmd_env`, `reuse_client`, `workspace_required`, `offset_encoding`, `before_init`, `on_init`, `on_attach`, `on_exit`, `handlers`, `name`, `workspace_folders`.

Load-bearing field semantics (verbatim quotes, `lsp.txt` @ release-0.11):

- **`cmd`** — `"string[]|fun(dispatchers, config): vim.lsp.rpc.PublicClient"` — accepts a command array **OR a function** that receives `dispatchers` and the **resolved `config`** and returns the RPC client. (This corrects a common misconception that `cmd` must be a static list.)
- **`root_dir`** — `"string|fun(bufnr: integer, on_dir:fun(root_dir?:string))"`.
- **`root_markers`** — `"Filename(s) (\".git/\", \"package.json\", …) used to decide the workspace root. Unused if root_dir is defined."`
- **`cmd_cwd`** — `"(string, default: cwd) Directory to launch the cmd process. Not related to root_dir."`
- **`workspace_required`** — `"(boolean, default: false) Server requires a workspace (no 'single file' support)."`
- **`offset_encoding`** — `"('utf-8'|'utf-16'|'utf-32') … Can be modified in on_init."`
- **`vim.lsp.enable()`** — *"Auto-starts LSP when a buffer is opened, based on the |lsp-config| `filetypes`, `root_markers`, and `root_dir` fields."*
- A config named **`'*'`** provides defaults merged into all clients (increasing-priority merge: `'*'` → `lsp/<name>.lua` → explicit `vim.lsp.config('<name>', …)`).
- **There is NO `on_new_config` field** in `vim.lsp.config` — that is a legacy nvim-lspconfig framework concept and must not be used here.

Citations: [Neovim `lsp.txt` (release-0.11)](https://github.com/neovim/neovim/blob/release-0.11/runtime/doc/lsp.txt); [Neovim 0.11 `vim.lsp.config` discussion #32523](https://github.com/neovim/neovim/discussions/32523).

### 3.2 nvim-lspconfig `lsp/*.lua` convention (2026)

nvim-lspconfig migrated its server definitions to per-server `lsp/<server>.lua` files, each returning a `vim.lsp.Config` table consumed by the built-in `vim.lsp.config`. The legacy `require('lspconfig').<server>.setup{}` framework path is deprecated. Reference definitions (live, master):

- **`lsp/svelte.lua`** — the closest analog to verter's design (see §3.5): `cmd = { 'svelteserver', '--stdio' }`, `filetypes = { 'svelte' }`, a `root_dir = function(bufnr, on_dir)` using `vim.fs.root`, and an `on_attach` `BufWritePost` autocmd that sends `client:notify('$/onDidChangeTsOrJsFile', { uri = ctx.match })`. ([lsp/svelte.lua](https://github.com/neovim/nvim-lspconfig/blob/master/lsp/svelte.lua))
- **`lsp/vue_ls.lua`** — the Volar config: `cmd = { 'vue-language-server', '--stdio' }`, `filetypes = { 'vue' }`, `root_markers = { 'package.json' }`, and an `on_init` that forwards `tsserver/request` to a **separate** `ts_ls`/`vtsls`/`typescript-tools` client. **This is Volar's split-process architecture and is NOT verter's model** — verter spawns and owns its own tsgo type provider in-process, so the verter config does **not** forward to a separate TS client (a key contrast). ([lsp/vue_ls.lua](https://github.com/neovim/nvim-lspconfig/blob/master/lsp/vue_ls.lua))

Citations: [nvim-lspconfig migration #3494](https://github.com/neovim/nvim-lspconfig/issues/3494); the two `lsp/*.lua` files above.

### 3.3 Filetype detection for `.vue` / `.svelte` — already in Neovim core

**`.vue` and `.svelte` are mapped to filetypes `vue` and `svelte` in Neovim core's filetype detection** — confirmed by direct inspection of `runtime/lua/vim/filetype.lua` at the v0.11.0 release tag: the `extension` table contains `vue = 'vue'` and `svelte = 'svelte'` ([filetype.lua @ v0.11.0](https://github.com/neovim/neovim/blob/v0.11.0/runtime/lua/vim/filetype.lua)). **No `vim.filetype.add` call is required for detection.**

Separation of concerns (load-bearing): filetype **detection** (`.vue` → `vue`) is independent of LSP **attach gating** (filetype `vue` → start client). The `filetypes` field of a server config gates attach; it does **not** register an extension. Because core already supplies detection, our `setup()` registers a `vim.filetype.add({ extension = { vue = 'vue', svelte = 'svelte' } })` only as a **robustness fallback** (harmless idempotent re-assertion; protects users on unusual/older runtimes or with conflicting overrides). For users who already have a Svelte/Vue syntax plugin, that plugin typically also asserts detection; ours is additive and last-write-wins-equivalent.

### 3.4 Passing the workspace root as a positional CLI arg

`verter-lsp` takes the workspace root as a **positional** CLI argument (`crates/verter_lsp/src/main.rs`, hand-rolled `CliArgs::parse`: any non-`--` arg is the root), falling back to `std::env::current_dir()` if absent. The VS Code client always passes it explicitly (`buildServerOptions` pushes `rootPath` last).

**Architect verdict:** pass the **resolved root positionally via a root-aware `cmd` function** — *"Do not rely solely on cwd fallback if the server's CLI already accepts an explicit workspace root … `before_init` is too late/indirect for process argv construction."* The `cmd` function receives the **resolved `config`** (with `root_dir` populated by `vim.lsp.enable`'s root resolution), so it can append `config.root_dir`:

```lua
cmd = function(dispatchers, config)
  local args = { 'verter-lsp', '--type-provider=tsgo' }
  if config.root_dir and config.root_dir ~= '' then
    args[#args + 1] = config.root_dir
  end
  return vim.lsp.rpc.start(args, dispatchers, { cwd = config.cmd_cwd })
end
```

This **fails closed**: if `root_dir` is unresolved the server still launches and falls back to cwd, but the common path passes the precise root, exactly mirroring the VS Code launch. (Alternatively, when `root_dir` is set as a string/marker-resolved value, `cmd` reads `config.root_dir`; the function form is the robust general case.) We keep `cmd` as a **string list** in the docs-only minimal snippet (relying on the cwd fallback) for users who want the simplest copy-paste, and use the function form in the in-repo module for precise-root parity.

### 3.5 What the config mirrors from the VS Code client

Studied in `packages/vue-vscode/src/extension.ts` (`buildServerOptions`, `clientOptions.initializationOptions`), `crates/verter_lsp/src/main.rs`, `crates/verter_lsp/src/server/lifecycle.rs`, `crates/verter_lsp/src/config.rs`.

**Server launch (VS Code → Neovim):** VS Code launches `verter-lsp --type-provider=<tp> --tsdk=<tsdk> --plugin-path=<node_modules> [--mcp-port=0 --mcp-lint-preset=<p>] <rootPath>` with `env.VERTER_LOG`. **Decisive simplification (shared with the Lapce design): use `--type-provider=tsgo` and omit `--tsdk` / `--plugin-path`.** In `main.rs`, `--tsdk` and `--plugin-path` are consumed **only** by the tsserver path; `try_spawn_tsgo` ignores them and discovers the tsgo binary itself (`find_tsgo_binary_canonical`: `VERTER_TSGO_BIN` env → workspace `node_modules` → PATH → npm/npx cache). So the tsgo provider is self-contained — the user installs `@typescript/native-preview` per-project (the normal case for a Vue/Svelte project), exactly as the VS Code tsgo path expects. The `--mcp-*` flags are parsed-but-ignored by the server (MCP shipped separately) and are **omitted**. Resulting default `cmd`: `verter-lsp --type-provider=tsgo <root>` (plus user-overridable extra args).

**Type-provider surface — a deliberate native-client superset.** The default stays `tsgo`, but the Neovim client validates `type_provider` against the full server-accepted set `{ auto, tsgo, tsserver, off }` (`VALID_TYPE_PROVIDERS` in `init.lua`), rejecting anything else fail-closed. This is an **intentional advanced capability**, not a divergence from the SDK-less wasm clients (Lapce/Zed), which clamp to `{ tsgo, off }`. The wasm volts cannot supply a TypeScript SDK, so they refuse `tsserver`/`auto`; Neovim is a full editor where the user can put a TypeScript SDK on PATH and `tsserver` self-discovers its own install, so exposing `tsserver`/`auto` as opt-in overrides is correct here. Users who do nothing get the self-contained `tsgo` provider; the broader surface is purely an advanced override.

### 3.6 Initialization-options parity

VS Code passes a rich `initializationOptions`; the server reads only a subset (`lifecycle.rs` `handle_initialize`; `config.rs`). The Neovim config mirrors the **server-relevant** subset and drops VS-Code-UI-only fields.

| Init option | Server reads it? (where) | Neovim mapping |
|---|---|---|
| `lint: { enabled, preset }` | yes (`init_lint_options`; `config::merge_init_options`) | `init_options.lint` from module opts |
| `inlayHints: { enabled }` | yes (`inlay_hints_enabled`) | `init_options.inlayHints` |
| `viteConfig: { enabled, trustedFiles }` | yes (`vite_config_options`) | `init_options.viteConfig` |
| `experimental: { conditionalRootNarrowing, strictSlots }` | yes (`config::parse_experimental_init_options`) | `init_options.experimental` |
| `hover: { provenance }` | yes (`config::parse_hover_init_options`) | `init_options.hover` |
| `statistics: { enabled }` | yes (`statistics.set_enabled`) | `init_options.statistics`, default OFF |
| `frameworks: ["vue","svelte"]` | **no — server ignores it** | **dropped** (dead protocol surface) |
| `configuration: { vue, typescript, css, emmet, … }` | opportunistic VS-Code language-service settings | **drop** — these are VS Code's emmet/css/html/ts language-service settings; Neovim has its own. v0 omits. |

The Neovim client now emits **exactly** the canonical six server-read init-option keys — `lint`, `inlayHints`, `viteConfig`, `experimental`, `hover`, `statistics` — the same parity set every Verter editor client ships (`verter_editor_client::build_initialization_options`, the SSoT). `statistics` is emitted (default OFF, honoring a user opt-in); the previously-emitted `frameworks` key is **dropped** because the server ignores it. This builder is bound to the shared SSoT by a Rust drift-guard (`crates/verter-editor-client/tests/nvim_config_contract.rs`): it extracts `build_init_options`' top-level keys and asserts set equality against `build_initialization_options(&{})`, so a missing key (e.g. dropping `statistics`) or an extra key (e.g. re-adding `frameworks`) fails the gate. Neovim was the only editor client not written in Rust, hence the only one able to silently re-diverge; the drift-guard closes that gap.

VS-Code-only surfaces **excluded** entirely: `$/verter/*` custom requests + `decorations.*` (panels/decorations), `mcp.*`, the `@verter/typescript-plugin` wiring (tsserver-in-tsserver, N/A). A plain LSP client simply never sends the `$/verter/*` requests; the server only responds when asked, so **all standard LSP features flow unchanged** and only VS-Code-exclusive panels are absent.

### 3.7 Client capabilities — what the built-in client already provides

The verter config does **not** hand-author client capabilities (beyond the §2.4 watcher override and merging the user's completion-engine capabilities). Neovim's built-in client builds standard `textDocument` capabilities on `initialize`. The server reads the editor's `params.capabilities` in exactly one place — position-encoding negotiation (`lifecycle.rs`) — and no handler branches on any other editor capability. As established in §2.2/§2.3: the built-in client advertises `completionItem.resolveSupport` (`additionalTextEdits`, `command`), `snippetSupport`, and UTF-8-first position encodings by default, so verter's auto-import + UTF-8 path work out of the box.

## 4. Chosen design — `editors/nvim/` Lua module

### 4.1 Layout & workspace isolation

A new directory **`editors/nvim/`** (`editors/` is the natural home for config-only editor integrations, distinct from `extensions/` — the home for compiled editor clients such as the Lapce volt at `extensions/lapce/` — and from `crates/` (workspace-member Rust) and `packages/` (pnpm TS/JS)). It contains **no Rust and no pnpm package** — pure Lua + docs — so it does not enter the Cargo workspace, the `cargo nextest run --workspace` gate, or the pnpm build graph.

```
editors/nvim/
  lua/verter/
    init.lua        # M.setup(opts): builds the vim.lsp.Config, registers filetype fallback,
                    #                merges capabilities, calls vim.lsp.config + vim.lsp.enable
    config.lua      # pure builders: build_cmd(opts)/build_init_options(opts)/build_capabilities(opts)
                    #                — unit-testable without a running server
  plugin/verter.lua # optional auto-setup guard (no-op unless the user opts in; lazy.nvim/packer friendly)
  tests/
    minimal_init.lua
    config_spec.lua          # pure-Lua unit tests (plenary busted)
    smoke_spec.lua           # headless real-server LSP-attach smoke (gated; skips w/o binary+node_modules)
  README.md         # copy-paste for BOTH the modern API and an nvim-lspconfig snippet + completion-engine note
```

`config.lua`'s builders are **pure functions over plain inputs** (the merged `opts` table) returning plain tables (the `cmd` list/function, the `init_options` table, the merged `capabilities` table), so they are unit-testable in headless Neovim **without** spawning the server (§7).

### 4.2 The produced `vim.lsp.Config`

```lua
-- editors/nvim/lua/verter/init.lua (shape; final form in implementation)
local M = {}

local DEFAULTS = {
  cmd = nil,                      -- nil → derive: 'verter-lsp' on PATH (or opts.cmd_path)
  cmd_path = 'verter-lsp',        -- binary name/abs path (PATH-discovered; NO managed download — §5)
  type_provider = 'tsgo',          -- tsgo is self-contained (§3.5)
  server_args = {},               -- extra args appended after the positional root
  filetypes = { 'vue', 'svelte' },
  root_markers = { 'tsconfig.json', 'jsconfig.json', 'vite.config.ts', 'vite.config.js',
                   'nuxt.config.ts', 'svelte.config.js', 'package.json', '.git' },
  watch_files = false,            -- §2.4: didChangeWatchedFiles dynamicRegistration opt-in
  semantic_tokens = true,         -- §2.6: easy opt-out
  log_level = 'info',             -- → cmd_env.VERTER_LOG
  -- init-option parity (§3.6):
  lint = { enabled = false, preset = 'recommended' },
  inlay_hints = { enabled = true },
  vite_config = { enabled = true, trusted_files = {} },
  experimental = { conditional_root_narrowing = false, strict_slots = false },
  hover = { provenance = false },
  capabilities = nil,             -- user passes blink.cmp/cmp_nvim_lsp caps; merged in
}

function M.setup(opts)
  opts = vim.tbl_deep_extend('force', DEFAULTS, opts or {})
  -- robustness-fallback filetype detection (core already supplies it — §3.3)
  vim.filetype.add({ extension = { vue = 'vue', svelte = 'svelte' } })

  vim.lsp.config('verter', {
    cmd = require('verter.config').build_cmd(opts),                 -- root-aware fn (§3.4)
    filetypes = opts.filetypes,
    root_markers = opts.root_markers,
    cmd_env = { VERTER_LOG = opts.log_level },
    init_options = require('verter.config').build_init_options(opts),
    capabilities = require('verter.config').build_capabilities(opts), -- §2.3/§2.4 merge + watcher override
    on_attach = require('verter.config').on_attach(opts),             -- §2.5 save-notify, §2.6 sem-tokens opt-out
    workspace_required = false,                                       -- tolerate single-file (server falls back to cwd)
  })
  vim.lsp.enable('verter')
end

return M
```

`build_capabilities(opts)` starts from `vim.lsp.protocol.make_client_capabilities()`, deep-merges a copy of `opts.capabilities` on top (so nvim defaults survive a partial user table and the caller's table is never mutated), and **forces** `caps.workspace.didChangeWatchedFiles.dynamicRegistration = opts.watch_files` (default false; §2.4). `on_attach` installs the `BufWritePost` `*.js`/`*.ts` → `$/onFileChanged { uri, type = "update" }` autocmd when `not opts.watch_files` (§2.5), and clears `client.server_capabilities.semanticTokensProvider` when `not opts.semantic_tokens` (§2.6).

### 4.3 README content (both surfaces)

The README provides three copy-paste recipes:

1. **Modern built-in API (recommended, no plugin manager):**
   ```lua
   require('verter').setup({})            -- if using the in-repo module
   -- or, fully inline without the module:
   vim.lsp.config('verter', {
     cmd = { 'verter-lsp', '--type-provider=tsgo' },  -- cwd fallback supplies the root
     filetypes = { 'vue', 'svelte' },
     root_markers = { 'package.json', 'tsconfig.json', '.git' },
   })
   vim.lsp.enable('verter')
   ```
2. **lazy.nvim plugin spec** pointing at the repo's `editors/nvim/` (or a published mirror), calling `require('verter').setup{}` in `config`.
3. **nvim-lspconfig** users: a one-liner noting that the in-repo module supersedes it today, and that an upstream `lsp/verter.lua` is a planned follow-up (§5/§8).

Plus the **completion-engine note**: recommend blink.cmp for speed and show passing `capabilities = require('blink.cmp').get_lsp_capabilities()` (or `cmp_nvim_lsp.default_capabilities()`) into `setup{ capabilities = … }`; note that auto-import works without either (§2.3). Plus the **prerequisite note**: `verter-lsp` on PATH (or `cmd_path`), and `@typescript/native-preview` (tsgo) in the project for full type features (§3.5).

## 5. Distribution & binary discovery — architect-approved (Neovim-idiomatic)

**Architect verdict:** *in-repo Lua module + docs now; upstream lspconfig and mason later; PATH/absolute binary discovery only.* This **deliberately differs** from the Lapce design's "Strategy D" managed-download, because Neovim's conventions differ.

### 5.1 Distribution mix (land order)

1. **In-repo Lua module `editors/nvim/` + README** (now). Worth shipping because the config owns non-trivial options (root argv, type-provider flag, capability shaping + watcher override, save-notify autocmd, init-option parity). Architect: *"If it were only `cmd = { 'verter-lsp' }`, docs-only would be enough"* — verter's config is more than that, so the module earns its place.
2. **Docs-only minimal snippet** (now) for users who don't want the module (recipe 1 above).
3. **Upstream `lsp/verter.lua` to `neovim/nvim-lspconfig`** (follow-up) — once the server has stable public install instructions. Widest reach; couples to their cadence; pursued **without changing the chosen runtime path** (§2.1).
4. **mason.nvim registry entry** (follow-up) — once per-platform `verter-lsp` release assets exist, for auto-install.

### 5.2 Binary discovery — PATH / absolute path only; NO Lua downloader

Architect verdict (verbatim sense): *"Do not reuse Lapce Strategy D in Lua. That is architecturally wrong for Neovim. Managed server installation belongs to mason.nvim or user package managers, not a thin LSP config plugin … no downloader, no cache directory, no SHA management in Neovim module."*

**Policy:** `cmd` names `verter-lsp` on **PATH** (or `opts.cmd_path` as an absolute path). If the binary is missing, **fail loudly** with actionable guidance (`vim.notify(..., ERROR)`: install `verter-lsp` / set `cmd_path` / use mason once available). This is the Neovim convention: the editor/plugin does **not** download language-server binaries; that is mason.nvim's job (its own registry) or the user's package manager (cargo install / Homebrew / Scoop / Winget). Re-implementing Lapce's pinned-download + SHA-verify + cache in Lua would **duplicate mason** and violate the convention.

**Contrast with Lapce:** Lapce's volt has a plugin distribution/activation model with a download primitive (`Http::get`) and a `[config]` schema, and *no per-platform volt artifacts* — so a managed in-volt download (Strategy D) is the right fit there. Neovim has mason as the dedicated installer, so delegation is correct here. The two designs reach **different** binary-discovery conclusions for **principled, editor-specific** reasons — not an inconsistency.

## 6. Decomposition into implementation blocks

Each block lands with discriminating tests (must FAIL pre-change, PASS post-change; no stubs/always-true asserts, per the project Stub-Prevention rule). All blocks are in `editors/nvim/` + docs; **no existing crate is touched** (§8).

### Block N1 — Lua module: pure config builders + setup
- `lua/verter/config.lua`: `build_cmd(opts)` (root-aware `cmd` function/list per §3.4), `build_init_options(opts)` (parity map §3.6), `build_capabilities(opts)` (merge user caps + force watcher dynamicRegistration §2.4), `on_attach(opts)` (save-notify §2.5 + semantic-tokens opt-out §2.6).
- `lua/verter/init.lua`: `M.setup(opts)` (filetype fallback §3.3 + `vim.lsp.config` + `vim.lsp.enable`).
- **Discriminating tests** (plenary, no server; §7):
  - `build_cmd` with default `tsgo` → first elems `{'verter-lsp','--type-provider=tsgo'}`; **asserts `--tsdk` and `--plugin-path` ABSENT** (negative: they are tsserver-only and would break the tsgo path).
  - `build_cmd` appends the resolved root as the trailing positional arg.
  - `build_init_options` maps `lint.enabled=true`→`{lint={enabled=true,…}}` and **omits** `configuration`/`mcp`/`decorations` (negative: VS-Code-only keys not forwarded).
  - `build_capabilities` with default → `workspace.didChangeWatchedFiles.dynamicRegistration == false`; with `watch_files=true` → `true` (discriminates the override).
  - `setup{}` then `vim.filetype.match({ filename = 'App.vue' }) == 'vue'` and `'X.svelte' == 'svelte'` (detection holds).
  - `setup{}` then `vim.lsp.config('verter')` getter returns a table whose `filetypes` contains both `vue` and `svelte` and whose `cmd` is present (config registered).

### Block N2 — `on_attach` behaviors + capability merge
- Finalize the save-notify `BufWritePost` autocmd and the semantic-tokens opt-out.
- **Discriminating tests** (plenary, no server):
  - With `watch_files=false`, `on_attach(fake_client, bufnr)` registers a `BufWritePost` autocmd for `*.js,*.ts` (assert the augroup/autocmd exists); with `watch_files=true` it does **not** (discriminates §2.5 gating).
  - With `semantic_tokens=false`, `on_attach` sets `fake_client.server_capabilities.semanticTokensProvider = nil`; with `true` it leaves it untouched (discriminates §2.6).
  - `build_capabilities` merges a user-provided blink.cmp-shaped caps table without dropping `completionItem.resolveSupport` (assert it survives the merge).

### Block N3 — Headless real-server smoke + CI + docs
- `tests/smoke_spec.lua`: a headless test that `vim.lsp.enable('verter')`, opens a fixture `.vue`, `vim.wait`s for `#vim.lsp.get_clients({ name='verter' }) > 0`, and asserts the client attached (and, when a type provider is available, that a diagnostic/hover arrives). **Gated**: skips vacuously (like the Rust `crates/verter_lsp/tests/` tsgo/tsserver e2e) when `verter-lsp` or `node_modules`/tsgo is absent, so CI without those is green.
- CI workflow (`.github/workflows`): a `neovim` job using `rhysd/action-setup-vim@v1` to matrix Neovim `0.11` / `0.12` / `nightly` on Linux + macOS; runs `nvim --headless -c 'PlenaryBustedDirectory editors/nvim/tests/ {minimal_init="editors/nvim/tests/minimal_init.lua"}' -c 'qa!'`. The pure-Lua unit specs (N1/N2) are the always-on gate; the smoke spec runs where the binary is built.
- README (§4.3) + a `docs/` guide page on installing Verter for Neovim (prerequisites, the modern + lspconfig recipes, completion-engine note, the §8 freshness caveat + `watch_files` opt-in).
- **Discriminating test:** a lint/structure test asserting `README` recipes reference `--type-provider=tsgo` and **not** `--tsdk` (guards the §3.5 simplification), runnable in the same plenary suite via file read.

## 7. Test strategy (mandatory-rule compliant)

The project mandates automated tests for LSP/editor-integration changes (no manual-only verification). The work splits cleanly:

**Automated — the bulk (pure-Lua, no server; highest value):** `config.lua` builders and `setup()` behaviors, run under **plenary.nvim** busted (`PlenaryBustedDirectory`, the de-facto Neovim plugin test standard) in headless Neovim with a `minimal_init.lua` that prepends the module + plenary to `rtp`. These cover **all** of the module's decision logic: `cmd` args (+ negative `--tsdk`/`--plugin-path` absence), init-option parity (+ negative VS-Code-only-key absence), capability/watcher override, filetype registration (`vim.filetype.match`), the save-notify and semantic-tokens gating, and reading back the registered config via the `vim.lsp.config('verter')` getter. They need **no `verter-lsp` binary** and run in milliseconds. CI invocation:
```bash
nvim --headless -c 'PlenaryBustedDirectory editors/nvim/tests/ {minimal_init="editors/nvim/tests/minimal_init.lua"}' -c 'qa!'
```
Citations: [plenary.nvim TESTS_README](https://github.com/nvim-lua/plenary.nvim/blob/master/TESTS_README.md); [rhysd/action-setup-vim](https://github.com/rhysd/action-setup-vim).

**Automated — headless real-server smoke (feasible, gated):** `nvim --headless`/`nvim -l` can drive a real attach: open a `.vue`, `vim.wait(ms, predicate)` for `vim.lsp.get_clients({ name='verter' })`, assert attach + (provider-permitting) a diagnostic/hover, exit nonzero via `vim.cmd('cq N')` on failure. This is **gated** to skip vacuously without the binary/tsgo (matching the established Rust e2e skip pattern). Citations: [Neovim `lsp.txt` — `vim.lsp.enable`/`get_clients`/`vim.wait`](https://github.com/neovim/neovim/blob/release-0.11/runtime/doc/lsp.txt); [testing Neovim LSP plugins](https://zignar.net/2022/10/26/testing-neovim-lsp-plugins/).

**Already covered by the server's own suite:** because the Neovim config only *launches* the server, `verter-lsp`'s LSP behavior over stdio is **already** fully tested by the repo's existing real-LSP gatekeeper suite at `crates/verter_lsp/tests/` (in-process server, editor-independent; tsgo/tsserver assertions skip vacuously without `node_modules`). The Neovim layer adds **no semantic surface** to re-test — only the launch/config contract, which the pure-Lua specs cover.

**Manual (irreducible):** an interactive Neovim UI smoke (open a `.vue`/`.svelte` in a real Neovim, exercise hover/completion/rename/diagnostics, confirm auto-import edits apply) is a README checklist, not an automated gate — the automated layers above cover the module's actual logic and the server's behavior.

## 8. Scope, dependencies, and decisions for the CTO/user

**Scope (confirmed):**
- A **new, non-overlapping** Lua-only artifact (`editors/nvim/`) + docs. **No existing crate touched.**
- **No server-side change.** Architect: *"No server-side change is implied by this design."* `verter-lsp` already supports everything the Neovim client needs — stdio LSP, UTF-8 negotiation, completion-resolve advertisement, dynamic watcher registration, and the `$/onFileChanged` custom method (the VFS-re-read save/external-edit signal; the `$/onDidChangeTsOrJsFile` in-editor-delta method also exists but is not used by the Neovim save autocmd). (This contrasts with the Lapce design, which proposed a small `verterClient` handshake addition; the Neovim path needs none.) So this block does **not** trip the "confirm before editing verter_session" rule, and does not touch `verter_session` or any shared substrate.
- A new CI job (Neovim matrix) + the pure-Lua test suite.

**Decisions for the CTO/user (not derivable from the plan):**
1. **Where does the in-repo module live and how is it published?** `editors/nvim/` in-repo is clear; the open question is whether to also publish it as a standalone installable (a separate `verter.nvim` repo / mirror) so lazy.nvim users can `{ 'verter/verter.nvim' }` without the monorepo. Recommend: in-repo now; mirror/standalone as a follow-up alongside the upstream-lspconfig PR.
2. **Upstream lspconfig PR + mason registry** (§5.1 items 3–4) — confirm we want to pursue these follow-ups, and note both depend on the same release-engineering work the Lapce design's §8 flags (published per-platform `verter-lsp` assets). Until then, PATH/`cmd_path` discovery is the supported path.
3. **`watch_files` default** — the design defaults `didChangeWatchedFiles` OFF (perf; §2.4) with the save-notify autocmd as the cheap freshness signal (§2.5). The flagged residual risk (architect): **external edits made outside Neovim may leave cross-file type state stale until another trigger** (a buffer edit, a `*.js`/`*.ts` save, or `:e`). Confirm OFF-by-default + documented opt-in is the right product call (vs ON, accepting node_modules-watch CPU). Recommend OFF-by-default per the architect.

**Toolchain prerequisites (documented, not blockers):** Neovim ≥ 0.11 (for `vim.lsp.config`/`enable`); `verter-lsp` on PATH or `cmd_path`; `@typescript/native-preview` (tsgo) in the project for full type features; plenary.nvim for the test suite (CI-vendored).

## 9. Open decisions / risks (summary)

- **Upstream-lspconfig + mason** depend on published release assets (shared blocker with Lapce §8). [CTO]
- **Standalone `verter.nvim` mirror** vs in-repo-only distribution. [CTO]
- **`watch_files` default** OFF (recommended) — documented stale-external-edit risk + opt-in. [CTO, architect-flagged]
- **Semantic tokens** are full-document only (`full=Bool(true)`, no range/delta) — kept on with an opt-out; range/delta is a **server-side** enhancement tracked separately (out of scope). [follow-up]
- **Completion engine** — blink.cmp recommended for speed; not required for auto-import. [doc note]

## 10. Citations

- Neovim `lsp.txt` (release-0.11): https://github.com/neovim/neovim/blob/release-0.11/runtime/doc/lsp.txt
- Neovim `lsp/protocol.lua` `make_client_capabilities` (release-0.11): https://github.com/neovim/neovim/blob/release-0.11/runtime/lua/vim/lsp/protocol.lua
- Neovim core filetype detection (`vue`/`svelte` extensions, v0.11.0): https://github.com/neovim/neovim/blob/v0.11.0/runtime/lua/vim/filetype.lua
- Neovim 0.11 `vim.lsp.config` discussion: https://github.com/neovim/neovim/discussions/32523
- nvim-lspconfig `lsp/*.lua` migration: https://github.com/neovim/nvim-lspconfig/issues/3494
- nvim-lspconfig `lsp/svelte.lua` (BufWritePost `$/onDidChangeTsOrJsFile` analog): https://github.com/neovim/nvim-lspconfig/blob/master/lsp/svelte.lua
- nvim-lspconfig `lsp/vue_ls.lua` (Volar split-process — NOT verter's model): https://github.com/neovim/nvim-lspconfig/blob/master/lsp/vue_ls.lua
- blink.cmp (Rust matcher; `get_lsp_capabilities`): https://github.com/saghen/blink.cmp
- cmp-nvim-lsp (`default_capabilities`): https://github.com/hrsh7th/cmp-nvim-lsp
- didChangeWatchedFiles CPU cost on large trees: https://github.com/neovim/neovim/issues/23291
- plenary.nvim test harness: https://github.com/nvim-lua/plenary.nvim/blob/master/TESTS_README.md
- rhysd/action-setup-vim (CI matrix): https://github.com/rhysd/action-setup-vim
- Testing Neovim LSP plugins (vim.wait pattern): https://zignar.net/2022/10/26/testing-neovim-lsp-plugins/
- Verter sources studied: `packages/vue-vscode/src/extension.ts`, `crates/verter_lsp/src/main.rs`, `crates/verter_lsp/src/server/lifecycle.rs`, `crates/verter_lsp/src/config.rs`, `crates/verter_lsp/src/capabilities.rs`.
- Architect consult (both forks): `.feedback/_nvim_arch_consult.md` (prompt), `.feedback/_nvim_arch_consult2.out` (verdict).
