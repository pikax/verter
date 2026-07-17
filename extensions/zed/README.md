# Verter for Zed (Vue / Svelte)

A thin [Zed](https://zed.dev) extension that runs Verter's native `verter-lsp`
language server for `.vue` and `.svelte` files, giving you diagnostics, hover,
completion, go-to-definition, find-references, rename, code actions, semantic
tokens, inlay hints, and signature help.

The extension owns no language logic. It compiles to WebAssembly and runs in Zed's
extension host **only to return a launch command**; Zed's native LSP client then
spawns `verter-lsp` over stdio and talks to it directly. The WASM extension is
**out of the per-message hot path** — it adds zero latency to LSP requests.

## Prerequisites

Verter is an **opt-in alternative** to the official Vue / Svelte language servers
(Volar / svelte-language-server). It ships **no grammar of its own** and attaches
to the languages those official extensions define, so you need:

1. **The official Vue and/or Svelte Zed extension** — for the `Vue.js` / `Svelte`
   language definition and tree-sitter grammar (syntax highlighting, outline).
   Install them from Zed's extension list. Verter contributes only the language
   server; the grammar and language are theirs.

2. **`verter-lsp`** — the native server binary. v0 does **not** auto-download it
   (managed download is a roadmap item). You provide it via one of:
   - an explicit path in your settings (`lsp.verter.binary.path`), or
   - a `verter-lsp` on your `PATH` plus the explicit PATH opt-in (below).

   If neither is configured, the extension **fails loudly** with setup guidance
   instead of silently launching nothing.

3. **TypeScript 7 in your project** — Verter runs with
   `--type-provider=tsgo`, which discovers the native
   `@typescript/typescript-<platform>-<arch>` binary installed by `typescript@7`
   from your project's `node_modules` (the normal case for a TS/Vue/Svelte
   project). Install it as a dev dependency in the workspace you open. (No
   TypeScript SDK is bundled with the extension.)

## Install (dev extension)

1. Clone this repository.
2. In Zed, open the command palette and run **`zed: install dev extension`**.
3. Point it at `extensions/zed/` in the clone. Zed compiles the extension with its
   bundled Rust/wasm toolchain.

(Registry publication to `zed-industries/extensions` is a roadmap item; until then
the dev-extension flow above is the supported install path.)

## Enable Verter for Vue / Svelte

By default Zed uses the official Vue/Svelte language servers. To make Verter the
active server, list it **first** in `languages.<lang>.language_servers` and disable
the default with a `!`-prefixed entry. Without the `!`-disable you get **duplicate
diagnostics and completions** from both servers.

Add this to your Zed `settings.json`:

```jsonc
{
  "languages": {
    "Vue.js": {
      // Verter first; disable the default Vue server; "..." keeps Zed's other defaults.
      "language_servers": ["verter", "!vue-language-server", "..."],
    },
    "Svelte": {
      "language_servers": ["verter", "!svelte-language-server", "..."],
    },
  },
  "lsp": {
    "verter": {
      // --- Binary discovery (v0: override > PATH opt-in > loud fail) ---
      "binary": {
        // Highest precedence: an explicit verter-lsp path (covers dev builds in
        // target/{debug,release} and power users). Optional arguments/env are
        // forwarded to the launch.
        "path": "/absolute/path/to/verter-lsp",
        "arguments": [],
        "env": {},
      },

      // --- Verter server settings (forwarded as LSP initializationOptions) ---
      "settings": {
        // Opt into PATH discovery (only consulted if `binary.path` is unset).
        // Without this, a verter-lsp found on PATH is NOT launched — PATH is
        // opt-in so a stale binary can't silently break version coupling.
        "serverSource": "path",

        "lint": { "enabled": false, "preset": "recommended" },
        "inlayHints": { "enabled": true },
        "viteConfig": { "enabled": true, "trustedFiles": [] },
        "experimental": { "conditionalRootNarrowing": false, "strictSlots": false },
        "hover": { "provenance": false },
        "statistics": { "enabled": false },
      },
    },
  },
}
```

Notes:

- **One server id (`verter`) serves both Vue and Svelte.** One `verter-lsp` binary,
  one `lsp.verter` settings block.
- **`binary.path` wins over PATH discovery.** Set it for a dev build or a pinned
  binary. Leave it unset and set `settings.serverSource = "path"` to use a
  `verter-lsp` on your `PATH`.
- **`lint.preset`** accepts `essential | recommended | all | performance | a11y |
strict`; an unknown value clamps to `recommended`.
- The type provider is always `tsgo` (the SDK-free native provider); the extension
  never requests `tsserver`.

## What this extension does (and doesn't)

- **Does:** resolve the `verter-lsp` binary, build its argv
  (`--type-provider=tsgo <workspace-root>` plus any `binary.arguments`), and build
  the `initializationOptions` — then hand Zed a launch command.
- **Doesn't:** parse, type-check, transform LSP messages, or ship a grammar. All
  semantic work runs in the native `verter-lsp`; syntax highlighting comes from the
  official Vue/Svelte extension.

## Troubleshooting

- **No diagnostics / the server never starts.** Check that `verter-lsp` is
  resolvable (set `lsp.verter.binary.path`, or install it on `PATH` and set
  `settings.serverSource = "path"`). The extension surfaces a loud error naming
  these keys when it can't resolve a binary.
- **Duplicate diagnostics/completions.** You omitted the `!`-disable of the default
  server (`!vue-language-server` / `!svelte-language-server`).
- **Type information is missing or wrong.** Ensure `typescript@7` is installed
  in the opened workspace's `node_modules` (the `tsgo` provider needs its native
  platform binary).
- **The extension fails to load in Zed.** This extension targets `wasm32-wasip2`
  (required by `zed_extension_api` ≥ 0.5). A wasip1/wasip2 mismatch produces a
  `__wasi_init_tp` load error; rebuild with the wasip2 target.

## Automated contract

Zed has no headless GUI-extension harness. CI builds the real `wasm32-wasip2`
extension, exports the exact command and arguments produced by its production
`plan_launch` function, combines them with the production initialization-option
builder, and drives that plan through the shared stdio LSP client. The fail-closed
smoke requires real Vue and Svelte diagnostics and concrete typed hover results;
the host unit suite separately covers discovery and refusal branches.
