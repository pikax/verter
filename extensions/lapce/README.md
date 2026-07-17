# Verter for Lapce (Vue / Svelte)

A thin [Lapce](https://lapce.dev) volt (plugin) that runs Verter's native
`verter-lsp` language server for `.vue` and `.svelte` files, giving you
diagnostics, hover, completion, go-to-definition, find-references, rename, code
actions, semantic tokens, inlay hints, and signature help.

The volt owns no language logic. It compiles to WebAssembly and runs in Lapce's
plugin host **only to issue a one-time launch command** on `initialize`; Lapce's
native LSP client then spawns `verter-lsp` over stdio and talks to it directly.
The WASM volt is **out of the per-message hot path** — it adds zero latency to
LSP requests.

## Prerequisites

Verter is an **opt-in alternative** to the official Vue / Svelte language
servers. It ships **no grammar of its own** and attaches to Lapce's `vue` /
`svelte` languages, so you need:

1. **A Rust toolchain with the `wasm32-wasip1` target** — to build the volt:

   ```bash
   rustup target add wasm32-wasip1
   ```

2. **A built `verter-lsp` binary.** v0 does **not** auto-download it (managed
   download is a roadmap item — see [Troubleshooting](#troubleshooting)). Build
   it from the repo root:

   ```bash
   cargo build -p verter_lsp --release
   ```

   This produces `target/release/verter-lsp` (`target/release/verter-lsp.exe` on
   Windows — the `.exe` suffix is Windows-only). Use the absolute path to that
   binary in your config below.

3. **TypeScript 7 in your project** (for full type features). Verter runs with
   `--type-provider=tsgo`, which discovers the native
   `@typescript/typescript-<platform>-<arch>` binary installed by `typescript@7`
   from your project's `node_modules` (the normal case for a typed Vue/Svelte
   project). Install it as a dev dependency in the workspace you open. (No
   TypeScript SDK is bundled with the volt.) Set `typeProvider = "off"` to run
   Verter without TypeScript type checking.

## Build

From the repo root:

```bash
pnpm run build:lapce
```

This adds the `wasm32-wasip1` target, builds the volt in release mode, and copies
the artifact to `extensions/lapce/bin/verter-lapce.wasm` (the path the volt's
`volt.toml` declares via `wasm = "bin/verter-lapce.wasm"`).

If you don't have Node/pnpm, build the same artifact directly with cargo:

```bash
cargo build --manifest-path extensions/lapce/Cargo.toml --target wasm32-wasip1 --release
# then copy extensions/lapce/target/wasm32-wasip1/release/verter_lapce.wasm
#      to   extensions/lapce/bin/verter-lapce.wasm
```

## Install (dev / local)

Lapce has **no `volts` local-install CLI** — the supported dev path is to drop an
unpacked volt folder into Lapce's plugins directory. The folder must contain the
`volt.toml` manifest plus the built wasm at the path the manifest names:

```
<plugins>/verter/
├── volt.toml
└── bin/
    └── verter-lapce.wasm
```

The `wasm = "bin/verter-lapce.wasm"` entry in `volt.toml` resolves **relative to
the volt folder**, so this exact layout is required.

### The easy path: `install:lapce-local`

From the repo root, run the cross-platform helper — it builds the wasm, finds
your OS's Lapce plugins directory, copies `volt.toml` + the wasm into
`<plugins>/verter/`, and prints the exact `lsp.serverPath` snippet (with the
absolute `verter-lsp` path filled in) for you to paste into your Lapce settings:

```bash
pnpm run install:lapce-local
```

The helper is a small Node dispatcher (`extensions/lapce/scripts/install-local.mjs`)
that runs `install-local.ps1` on Windows and `install-local.sh` elsewhere. It is
idempotent — re-run it after every rebuild to refresh the installed wasm. It
**fails loudly** if the wasm or the `verter-lsp` binary is missing, telling you to
build first.

Override the Lapce channel (default `Lapce-Stable`) with an argument:

```bash
# Windows (PowerShell): install into the Nightly channel's plugins dir
pwsh extensions/lapce/scripts/install-local.ps1 -Channel Lapce-Nightly
# POSIX:
extensions/lapce/scripts/install-local.sh --channel Lapce-Nightly
```

### Manual install (where the plugins directory lives)

If you prefer to copy the files yourself, the per-channel plugins directory is:

| OS      | Plugins directory                                               |
| ------- | --------------------------------------------------------------- |
| Windows | `%LOCALAPPDATA%\lapce\Lapce-Stable\data\plugins\`               |
| macOS   | `~/Library/Application Support/dev.lapce.Lapce-Stable/plugins/` |
| Linux   | `~/.local/share/lapce-stable/plugins/`                          |

The channel segment varies by the Lapce build you run — `Lapce-Stable`,
`Lapce-Nightly`, or `Lapce-Debug` (and the matching `lapce-nightly` /
`lapce-debug` on Linux, `dev.lapce.Lapce-Nightly` on macOS). The reliable way to
find it on any platform: in Lapce, open the command palette and run **"Open
Plugins Directory"**.

Create `<plugins>/verter/`, then copy `extensions/lapce/volt.toml` and
`extensions/lapce/bin/verter-lapce.wasm` (preserving the `bin/` subfolder) into
it. Restart Lapce or reload plugins.

## Required config

Lapce surfaces the volt's `[config."…"]` keys as settings you set under the
`volt.verter` namespace; the volt forwards them to `verter-lsp` as nested
`initializationOptions`. The **simplest correct setup** points the volt at your
built binary with an absolute path:

```toml
# Lapce settings (settings.toml) — Verter volt configuration

[volt.verter]
# Highest-precedence discovery: an explicit, ABSOLUTE verter-lsp path. Use the
# binary you built with `cargo build -p verter_lsp --release`.
# Windows: use the .exe and forward slashes, e.g. "C:/dev/verter/target/release/verter-lsp.exe"
"lsp.serverPath" = "/absolute/path/to/verter/target/release/verter-lsp"

# TypeScript type provider: "tsgo" (default, SDK-free native provider) or "off"
# (no TypeScript type checking). Any other value clamps to "tsgo".
"typeProvider" = "tsgo"
```

### Alternative: discover `verter-lsp` on your `PATH`

Instead of an absolute path, you can opt into PATH discovery. Leave
`lsp.serverPath` unset and set `lsp.serverSource = "path"` (PATH discovery is
**opt-in** so a stale binary on `PATH` can't silently break version coupling):

```toml
[volt.verter]
"lsp.serverSource" = "path"   # look up `verter-lsp` (or verter-lsp.exe) on PATH
"typeProvider" = "tsgo"
```

### Other settings

These map directly to the server's `initializationOptions` (the same parity set
every Verter editor client ships). All are optional; defaults shown:

```toml
[volt.verter]
"lint.enabled" = false               # enable Verter lint diagnostics
"lint.preset" = "recommended"        # essential | recommended | all | performance | a11y | strict
"inlayHints.enabled" = true
"viteConfig.enabled" = true          # read vite.config.* for resolve aliases
"viteConfig.trustedFiles" = []
"experimental.conditionalRootNarrowing" = false
"experimental.strictSlots" = false
"hover.provenance" = false
"statistics.enabled" = false         # server-side resolution statistics (opt-in)
```

## How to verify it works

1. Open a `.vue` or `.svelte` file in a project that has `typescript@7`
   installed.
2. Paste this minimal `.vue` and hover over `count` — you should get hover type
   info, completion after `count.`, and diagnostics on a type error:

   ```vue
   <script setup lang="ts">
   import { ref } from "vue";
   const count = ref(0);
   </script>

   <template>
     <button @click="count++">{{ count }}</button>
   </template>
   ```

Expect: **hover** showing `count`'s type, **completion** inside `{{ }}` and in the
script, and **diagnostics** for type errors in both `<script>` and `<template>`.

## Troubleshooting

- **The volt shows: "could not resolve a verter-lsp server source …".** This is
  the intended **loud fail** when neither an explicit path nor a PATH opt-in is
  configured (it never silently launches nothing). The remedy: set
  `lsp.serverPath` to the absolute path of your built `verter-lsp`, **or** put
  `verter-lsp` on your `PATH` and set `lsp.serverSource = "path"`. Managed
  auto-download is **not available yet** (roadmap).
- **No diagnostics / the server never starts.** Check that `lsp.serverPath`
  points at an existing, executable `verter-lsp` (use the `.exe` on Windows), or
  that `verter-lsp` is on `PATH` and `lsp.serverSource = "path"` is set.
- **Type information is missing or wrong.** Ensure `typescript@7` (tsgo) is
  installed in the opened workspace's `node_modules`, or set
  `typeProvider = "off"` if you want Verter without TypeScript type checking.

## Testing this volt

Lapce has no headless GUI-extension harness. CI therefore tests the strongest
automatable boundary: it builds the real `wasm32-wasip1` volt, asks the volt's
production `plan_launch` function to export its exact command, arguments,
selector, and initialization options, then drives that plan through the shared
stdio LSP client. The smoke requires real Vue and Svelte diagnostics and concrete
typed hover results from a real `verter-lsp`; missing binaries or providers fail
the job. Host unit tests continue to cover discovery and refusal branches.

## What this volt does (and doesn't)

- **Does:** resolve the `verter-lsp` binary, build its argv
  (`--type-provider=tsgo <workspace-root>`), build the `initializationOptions`,
  and hand Lapce a launch command on `initialize`.
- **Doesn't:** parse, type-check, transform LSP messages, or ship a grammar. All
  semantic work runs in the native `verter-lsp`.
