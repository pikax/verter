# Verter Lapce Editor Extension (Volt) — Design & Implementation Plan

> Status: **LANDED (v0 interim).** The thin Lapce volt ships at `extensions/lapce/` (a `wasm32-wasip1` cdylib) consuming the shared `crates/verter-editor-client` launch contract. The launch-contract details below reflect the landed state: the type-provider value is **`tsgo`** (not the `tgo` typo an earlier draft carried), and the crate lives under **`extensions/`** (compiled editor clients), not `editors/`. The landed discovery is the **v0 interim** — `lsp.serverPath` override → PATH (opt-in) → loud failure — with NO managed download yet (the full Strategy D in §4.2 remains the documented roadmap; its `managed`/handshake/release-pipeline pieces are deferred follow-ups, and the `verter-editor-client` discovery vocabulary keeps an additive seam for them).
> Date: 2026-06-22. Author: research+design block manager; finalized by the Lapce-client block manager.

## 1. Context

Verter already ships a native stdio LSP server, the `verter-lsp` binary (Rust crate `crates/verter_lsp`, `[[bin]] name = "verter-lsp"`, built via `cargo build -p verter_lsp --release` → `target/{debug,release}/verter-lsp[.exe]`). The reference editor client is the VS Code extension at `packages/vue-vscode`. This document designs a **second, thin LSP-client** for the **Lapce** editor — a Lapce plugin ("volt") — so Lapce users get Verter's Vue/Svelte features (diagnostics, hover, completion, definition, references, rename, code actions, semantic tokens, inlay hints, signature help). The server already exists; **the volt is a thin client** whose entire job is to tell Lapce to spawn `verter-lsp` over stdio with the right args + initialization options, mirroring the VS Code client's launch behavior.

This is a **new, non-overlapping package**. There are zero existing `lapce` references in the repo, and the volt touches **no existing crate** (the server is consumed as a built binary, not a library dependency). It does not modify `verter_session` or any shared substrate.

## 2. The Lapce plugin model (researched, with citations)

Lapce plugins are Rust crates compiled to **`wasm32-wasip1`** (the target formerly named `wasm32-wasi`, renamed March 2024 / Rust ≥1.78 [[rustc book: wasm32-wasip1]](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip1.html)). They run in a **WASI sandbox** and act as thin LSP clients: a plugin tells the native Lapce host to spawn a native language-server process over stdio. Verter's `verter-lsp` is exactly such a native server, so the model fits directly.

### 2.1 The `lapce-plugin` crate API

Confirmed against the published API ([docs.rs/lapce-plugin](https://docs.rs/lapce-plugin)) and three real LSP-client volts: the official `lapce/lapce-rust` ([main.rs](https://github.com/lapce/lapce-rust/blob/master/src/main.rs)), `VarLad/lapce-julia` ([main.rs](https://github.com/VarLad/lapce-julia/blob/master/src/main.rs)), and the `lapce-community/lapce-yaml` template ([repo](https://github.com/lapce-community/lapce-yaml)).

```rust
use lapce_plugin::{
    psp_types::{
        lsp_types::{request::Initialize, DocumentFilter, DocumentSelector, InitializeParams, MessageType, Url},
        Request,
    },
    register_plugin, LapcePlugin, VoltEnvironment, Http, PLUGIN_RPC,
};
use serde_json::Value;

#[derive(Default)]
struct State {}
register_plugin!(State);

impl LapcePlugin for State {
    fn handle_request(&mut self, _id: u64, method: String, params: Value) {
        match method.as_str() {
            Initialize::METHOD => {
                let params: InitializeParams = serde_json::from_value(params).unwrap();
                // ... call PLUGIN_RPC.start_lsp(...)
            }
            _ => {}
        }
    }
}
```

Key API (verbatim signatures from [docs.rs `PluginServerRpcHandler`](https://docs.rs/lapce-plugin), the type of the `PLUGIN_RPC` static):

- `pub fn start_lsp(&self, server_uri: Url, server_args: Vec<String>, document_selector: DocumentSelector, options: Option<Value>)` — **4 args.** Tells Lapce to spawn the native server and proxy LSP over stdio. `server_uri` is either `urn:<exe-name>` (Lapce resolves it on the host **PATH**) or a `file://` URI inside the volt directory (a bundled/downloaded binary).
- `pub fn window_show_message(&self, kind: MessageType, message: String)` — user-facing message (use for setup errors).
- `pub fn window_log_message(&self, kind: MessageType, message: String)`, `pub fn stderr(&self, msg: &str)` — diagnostics.
- `DocumentSelector = Vec<DocumentFilter>`; `DocumentFilter { language: Option<String>, pattern: Option<String> /* glob */, scheme: Option<String> /* "file" */ }`.
- `VoltEnvironment::architecture() -> Result<String>` (`"x86_64"` / `"aarch64"`), `::operating_system() -> Result<String>` (`"macos"` / `"linux"` / `"windows"`), `::uri() -> Result<String>` (the volt working-directory URI). Equivalent raw env vars: `VOLT_ARCH`, `VOLT_OS`, `VOLT_URI`.
- `Http::get(url: &str) -> Result<Response>`; `Response::body_read_all() -> Result<Vec<u8>>` — the download-on-activation primitive (backed by `lapce-wasi-experimental-http`).
- `psp_types` re-exports `lsp_types` (so `InitializeParams`, `Url`, etc. come from there) and `psp_types::Request` for `Initialize::METHOD`.

### 2.2 The `volt.toml` manifest

```toml
name = "verter"
version = "0.1.0"
author = "Verter authors"
display-name = "Verter (Vue / Svelte)"
description = "Vue & Svelte language support powered by the Verter LSP"
icon = "assets/icon.png"
repository = "https://github.com/<org>/verter"
wasm = "bin/verter-lapce.wasm"

[activation]
language = ["vue", "svelte"]
workspace-contains = ["**/*.vue", "**/*.svelte"]

# [config."key"] entries surface as initializationOptions the editor passes to the plugin.
[config."lsp.serverPath"]
default = ""
description = "Absolute path to a custom verter-lsp binary (overrides managed/PATH discovery)."

[config."typeProvider"]
default = "tsgo"
description = "TypeScript type provider. This SDK-less client supplies only tsgo (default) or off; tsserver/auto clamp to tsgo."
```

The `[activation]` `language` / `workspace-contains` fields gate when the plugin loads. The `[config."..."]` entries become `initializationOptions` the host passes the plugin on `initialize` (the user-override pattern in §2.1). Verified against `lapce-julia/volt.toml` and the `lapce-yaml` template.

### 2.3 Cargo.toml & build target

```toml
[package]
edition = "2021"
name = "verter-lapce"
version = "0.1.0"
resolver = "2"

[lib]
# `cdylib` is the volt artifact; the as-built adds `rlib` so the host `cargo test`
# can link the lib target and unit-test the pure launch surface.
crate-type = ["cdylib", "rlib"]

[target.'cfg(target_os = "wasi")'.dependencies]
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
lapce-plugin = { git = "https://github.com/lapce/lapce-plugin-rust.git" }
# optional, only if download artifacts are compressed:
# flate2 = "1.0"   # tar.gz
# zip = { version = "0.6", default-features = false, features = ["deflate"] }

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
```

Build: `rustup target add wasm32-wasip1` then `cargo build --target wasm32-wasip1 --release`; copy `target/wasm32-wasip1/release/verter_lapce.wasm` → `bin/verter-lapce.wasm` (the path in `wasm =`).

> **BUILD GOTCHA — must commit a `Cargo.lock` (validated in §9).** Upstream `psp-types` declares `lsp-types = "0"` (unpinned). A fresh resolve floats `lsp-types` to ≥0.95, where the `Url` type was renamed to `Uri`, and `psp-types` fails to compile (`unresolved import lsp_types::Url`). The fix real volts use is to **commit a `Cargo.lock` that pins `lsp-types` to a pre-0.95 version** — the official `lapce/lapce-rust` volt locks `lsp-types = 0.93.0`. The committed `Cargo.lock` here pins **`lsp-types = 0.94.1`** (the highest non-yanked pre-0.95 release; `0.94.2` is yanked), reproduced with `cargo update -p lsp-types --precise 0.94.1`. A top-level `lsp-types` dependency does **not** fix it (the float still resolves `0.97` independently through `psp-types`), and `[patch.crates-io] lsp-types` is rejected (patch must point to a different source). **The committed lockfile is the canonical mechanism.**

### 2.4 Publishing

Install the registry CLI (`cargo install volts`) and run `volts publish` in the volt directory; authentication is a GitHub-login token from the Lapce plugin registry at [plugins.lapce.dev](https://plugins.lapce.dev/) (a.k.a. volts.lapce.dev) [[Lapce plugin docs]](https://docs.lapce.dev/development/plugin-development).

## 3. What the volt must mirror from the VS Code client

Studied in `packages/vue-vscode/src/extension.ts`, `packages/vue-vscode/package.json`, `crates/verter_lsp/src/main.rs`, `crates/verter_lsp/src/server/lifecycle.rs`.

### 3.1 Activation & document selector

VS Code activates on `onLanguage:vue`, `onLanguage:svelte` (plus the TS/JS variants) with a document selector of `{ scheme: "file", language: "vue" }`, `{ ... "svelte" }`, and the TS/JS languages. For Lapce, the volt's `[activation]` `language = ["vue", "svelte"]` and a `DocumentSelector` of two `DocumentFilter`s (`vue`/`**/*.vue`, `svelte`/`**/*.svelte`, scheme `file`) is the equivalent and sufficient surface (Vue/Svelte are the carriers; the server projects them to TS internally).

### 3.2 Server launch — CLI args

The `verter-lsp` CLI (`crates/verter_lsp/src/main.rs`, hand-rolled `CliArgs::parse`, not clap) accepts: `--type-provider={auto|tsgo|tsserver|extension|off}`, `--tsdk=<path>`, `--plugin-path=<path>`, `--mcp-port=<n>` (parsed but **ignored** — LSP no longer embeds MCP), `--mcp-lint-preset=<preset>` (ignored), and a **positional workspace root**.

**Decisive simplification for Lapce: use `--type-provider=tsgo`.** In `main.rs`, `--tsdk` is consumed **only** by the tsserver path (`find_tsserver(args.tsdk…)`). `try_spawn_tsgo` ignores `--tsdk` entirely and discovers the tsgo binary via `find_tsgo_binary_canonical(workspace_root)` (order: `VERTER_TSGO_BIN` env → workspace `node_modules` → PATH → npm/npx cache). So the tsgo provider is **self-contained** — the volt does **not** need to ship a TypeScript SDK (which it has no `node_modules` to provide anyway). The user installs the tsgo binary (`@typescript/native-preview`) per-project in `node_modules` (the normal case for a TS/Vue/Svelte project), exactly as the VS Code path expects.

`--plugin-path` (tsserver-only) and the `--mcp-*` flags are **omitted** for the Lapce volt.

The **workspace root** positional arg: it is optional (the server falls back to `std::env::current_dir()`), but a WASI plugin's cwd is the volt directory, not the workspace. So the volt **must** pass the workspace root positionally, derived from the LSP `initialize` request's `root_uri` (`params.root_uri.to_file_path()` → push the plain path string; `CliArgs` treats any non-`--` arg as the root).

Resulting `server_args` for the default (tsgo) case:
```
["--type-provider=tsgo", "<workspace-root-path>"]
```
(plus any user-supplied extra args via `initializationOptions.lsp.serverArgs`).

### 3.3 Initialization options — parity mapping

VS Code passes a rich `initializationOptions` object (extension.ts ~484-520). The server reads only a subset (`crates/verter_lsp/src/server/lifecycle.rs` ~106-194; `crates/verter_lsp/src/config.rs`). The Lapce volt mirrors the **server-relevant** subset and drops VS-Code-UI-only fields.

| Init option | VS Code source | Lapce mapping |
|---|---|---|
| `lint: { enabled, preset }` | `verter.lint.*` | `[config."lint.enabled"]`, `[config."lint.preset"]` |
| `inlayHints: { enabled }` | `verter.inlayHints.enabled` | `[config."inlayHints.enabled"]` |
| `viteConfig: { enabled, trustedFiles }` | `verter.viteConfig.*` | `[config."viteConfig.enabled"]`, `[config."viteConfig.trustedFiles"]` |
| `experimental: { conditionalRootNarrowing, strictSlots }` | `verter.experimental.*` | `[config."experimental.*"]` |
| `hover: { provenance }` | `verter.hover.provenance` | `[config."hover.provenance"]` |
| `statistics: { … }` | `verter.statistics.*` | optional; drop in v0 |
| `frameworks: ["vue","svelte"]` | descriptor manifest | hardcode `["vue","svelte"]` |
| `configuration: { vue, typescript, css, … }` | VS Code per-language config | **drop** — these are VS-Code language-service settings (emmet/css/html) the Verter server reads opportunistically; Lapce has its own. v0 omits; can add a minimal `{ typescript: {…} }` later if a feature needs it. |

VS-Code-only surfaces **excluded** entirely: `decorations.*` (editor UI decorations), `analysis.enabled` / the `$/verter/*` custom requests (panels), `mcp.*`, `trace.server`, the `@verter/typescript-plugin` wiring (tsserver-in-tsserver integration — N/A to Lapce). These power VS Code UI and are not part of the standard LSP feature set Lapce consumes.

### 3.4 Client capabilities — the "tsgo completion" question (RESOLVED)

A recent commit ("advertise tsgo completion client-capabilities so handlers stop silently degrading") raised the concern that a Lapce client must advertise specific capabilities. **Investigation verdict: the relevant capabilities are STATIC server-side — Lapce needs nothing tsgo-specific.**

- `build_client_capabilities()` in `crates/verter_type_runtime/src/tsgo/ipc.rs` (~1581-1645) is a **pure static `serde_json::json!` literal with zero parameters**, called at `ipc.rs` ~1307 with no args. It is what `verter-lsp` tells the **tsgo subprocess** the client supports (completion `contextSupport`, `completionItemKind` 1..25, `completionItem.resolveSupport` for `additionalTextEdits`/auto-import, diagnostic `tagSupport`, codeAction `codeActionLiteralSupport`). It does **not** depend on what the editor (VS Code / Lapce) advertised. So the server→tsgo channel is always fully capable regardless of editor.
- The `verter-lsp` server reads the **editor's** `params.capabilities` in exactly one place: `lifecycle.rs` ~32-69 negotiates **position encoding** (`params.capabilities.general.positionEncodings`, preferring UTF-8 > UTF-32 > UTF-16, defaulting UTF-16). No handler branches on any other editor capability; `VerterLanguageServer` stores no `ClientCapabilities`.

**Practical Lapce requirement:** the volt does **not** hand-author client capabilities — Lapce's own native LSP client builds and sends the standard `textDocument` capabilities (completion `resolveSupport`, `codeActionLiteralSupport`, diagnostic `tagSupport`) on `initialize`. The only quality dependency is whether Lapce's client advertises `completionItem.resolveSupport` (needed so the final `additionalTextEdits`/auto-import edits Verter returns are applied). This is **Lapce-core behavior, not something the volt controls**; it is a **known risk to validate in E2E** (§7), not a blocker. Position encoding defaults to UTF-16 if Lapce omits `positionEncodings`, which is correct.

### 3.5 Custom requests / middleware

VS Code uses many `$/verter/*` custom requests and `$/onFileChanged` notifications to drive UI panels. Lapce (a plain LSP client) will simply never send these — the server only responds when asked, so this degrades gracefully: **all standard LSP features flow unchanged**; only the VS-Code-exclusive panels are absent.

## 4. Chosen design

### 4.1 Package layout & workspace isolation

A new standalone crate **`extensions/lapce/`** (crate name `verter-lapce`), **excluded from the root Cargo workspace**.

Rationale: the root workspace is `members = ["crates/*", "xtask"]`. A `wasm32-wasip1` `cdylib` crate placed under `crates/*` would be pulled into the default host-target `cargo build`/`cargo nextest run --workspace` and break it — its dependencies live under `[target.'cfg(target_os = "wasi")']`, so on the host target it has no `lapce-plugin` and no `lib` body. It also needs its **own committed `Cargo.lock`** (the `lsp-types` pin, §2.3) independent of the workspace lock. Therefore the volt crate must be **outside the workspace member glob**. Two equivalent ways to guarantee that: (a) place it at `extensions/lapce/` (outside `crates/`) — preferred, signals "compiled editor client, not a library crate"; or (b) add it under `crates/` and list it in `[workspace] exclude`. **Decision: `extensions/lapce/`** (cleaner; `packages/` is the TS/JS home, `crates/` is workspace-member Rust, `editors/` is reserved for config-only editor integrations, and `extensions/` is the home for compiled editor clients — joining the existing `extensions/vscode`, `extensions/vue-vscode`, `extensions/typescript-plugin`; alternative `packages/vue-lapce` is rejected because it is Rust, not a pnpm package). The volt detaches from the root workspace via an empty `[workspace]` table in its own `Cargo.toml`, which also lets it keep its own committed `Cargo.lock` for the `lsp-types` pin.

The landed crate layout (the v0 interim — no managed download yet, so no `discovery.rs`/`manifest.rs` download machinery; the shared `crates/verter-editor-client` owns the discovery decision the volt consumes):

```
extensions/lapce/
  Cargo.toml           # standalone (empty [workspace]); deps under cfg(target_os="wasi")
  Cargo.lock           # COMMITTED, pins lsp-types 0.94.1
  volt.toml            # manifest (§2.2)
  .gitignore           # ignores bin/*.wasm + /target
  src/
    lib.rs             # pure launch contract (plan_launch / handle_initialize / LspLauncher seam)
                       #   + wasi glue behind #[cfg(target_os = "wasi")] (register_plugin! + PLUGIN_RPC)
                       #   + host-target unit / manifest / lockfile tests
  bin/verter-lapce.wasm  # build output (gitignored; produced by build:lapce)
```

The decision surface in `lib.rs` (`plan_launch`, `handle_initialize`, the `LspLauncher` seam) is written as **pure functions over plain inputs** (workspace-root string, config `Value`, OS/arch strings) so it is unit-testable on the **host target** without the WASI runtime (see §7). The `init`/`args`/`discovery`/`platform` logic itself lives in the shared `crates/verter-editor-client` crate, so the Lapce and Zed clients cannot diverge.

### 4.2 Binary discovery / acquisition — architect-approved strategy (Strategy D)

An unprimed codex architect consult (full output: `.feedback/_lapce_binfork.out`) evaluated PATH-only (A), download-on-activation (B), bundled-all-platform (C), and hybrids (D) against cross-platform correctness, first-run friction, client/server version coupling, maintenance, security, and the WASI sandbox. **Verdict: Strategy D, a managed pinned download as the default, with an explicit override above it and a loud failure below it — PATH is opt-in only, never a silent fallback.**

**Precedence (highest first):**
1. **`lsp.serverPath` override** (explicit user/dev config; also covers the dev `target/{debug,release}` and E2E cases) → launch `urn:<that path>`.
2. **Verified pinned managed binary already present** in the volt dir at `servers/<serverVersion>/<target>/verter-lsp[.exe]` (hash-verified) → launch its `file://`.
3. **Download the pinned release asset** for the current OS/arch from `releases/download/v<serverVersion>/verter-lsp-v<serverVersion>-<target>.{tar.gz|zip}`, **verify SHA-256** (embedded in the volt's `manifest.rs`), extract the single expected binary (reject zip-slip/tar-traversal), atomically rename into place under `servers/<serverVersion>/<target>/`, `chmod +x` on Unix → launch its `file://`.
4. **On failure: fail loudly** via `window_show_message(ERROR, …)` with actionable guidance (set `lsp.serverPath`, or install + opt into PATH) — **do not** silently run `urn:verter-lsp` from PATH.

PATH (`urn:`) is reachable only when the user explicitly opts in (e.g. `lsp.serverSource = "path"`), to avoid a stale/unrelated PATH binary shadowing the managed, version-matched one.

Supporting rules from the architect:
- **Pin to the volt's exact server version, not `latest`** — Verter has client/server protocol/init-options coupling; `latest` would silently pair an old client with a new server. (The official `lapce-rust` volt uses `latest`; Verter deliberately does not.)
- **Compatibility handshake:** the volt passes `initializationOptions.verterClient = { name: "lapce-volt", version, protocolVersion, expectedServerVersion }`; `verter-lsp` validates at `initialize`, returns `serverInfo`, and rejects/warns on mismatch. Use a **protocol epoch/range**, not raw package semver, so a patched server stays valid when the protocol is unchanged. (This is a small **server-side addition** — see scope, §8.)
- **Cross-platform (the CRITICAL portability rule):** ship a **portable Linux artifact (static/musl)** for x64 + arm64 — the WASI plugin gets OS/arch but **no glibc/musl signal**, so libc guessing is a real hole. Use a **total platform matrix**: map only known `(os, arch)` tuples; **unknown ⇒ fail loudly, never guess**. Asset/cache names use safe target triples (`verter-lsp-v0.1.0-x86_64-apple-darwin.tar.gz`, `…-x86_64-pc-windows-msvc.zip`, `…-x86_64-unknown-linux-musl.tar.gz`, `…-aarch64-*`) — all NTFS-safe (no `:` etc.).
- **Cache versioned + immutable** in the volt dir; re-download on a volt update is correct when the server version changes; never overwrite a running `.exe` (Windows file lock) — write to a temp name and atomically rename.

**Rejected:** (A) PATH-only — high first-run friction, no version guarantee, silent mismatch; (C) bundled-all-platform — one volt artifact would carry every native binary (bloat; Lapce has no per-platform volt artifacts); (D-variant) `override > PATH > download` — a stale PATH binary shadows the managed one. **Secondary manual channels** (cargo install / Homebrew / Scoop / Winget) are documented install paths, not a replacement for release assets.

**v0 graduation note (product decision, §8):** Strategy D requires Verter to publish per-platform `verter-lsp` release assets + a release pipeline, which do not exist yet. That deferral is the shipped state: **v0 is the interim `lsp.serverPath` override > PATH (opt-in) > loud fail** (effectively Strategy A), with the discovery decision delegated to the shared `crates/verter-editor-client` crate. Graduating to full D is an **additive** change — the shared crate's `DiscoveryInputs` keeps a `managed_present` seam (always `None` in v0) so adding the managed download (future `discovery.rs` / `manifest.rs`) does not require a rewrite of the v0 precedence.

### 4.3 Config schema (`volt.toml [config]`)

`lsp.serverPath` (string, default ""), `lsp.serverArgs` (string list), `lsp.serverSource` (`managed`|`path`, default `managed`), `typeProvider` (default `tsgo`), `lint.enabled`, `lint.preset`, `inlayHints.enabled`, `viteConfig.enabled`, `viteConfig.trustedFiles`, `experimental.conditionalRootNarrowing`, `experimental.strictSlots`, `hover.provenance`. These surface as `initializationOptions` the volt reads and forwards to the server through the shared `verter-editor-client` launch contract. Note (v0 interim): the `serverSource` default `managed` is benign — no managed download exists yet, so the volt never reports a present managed binary; with neither an `lsp.serverPath` override nor a `serverSource = "path"` opt-in, discovery fails loud (it never silently runs a PATH binary).

## 5. Decomposition into implementation blocks

This section is the original decomposition plan. **Landed today (v0 interim):** the crate skeleton, manifest, LSP registration, build wiring, the launch-contract logic (housed in the shared `crates/verter-editor-client` crate rather than per-volt `init.rs`/`discovery.rs`/`manifest.rs` files), the host-target unit/manifest/lockfile tests, and the `wasm32-wasip1` build-smoke — i.e. the unit of work described below minus the managed-download machinery. **Documented roadmap (not yet landed):** the managed pinned download + SHA-256 verification + atomic install, the `verterClient` compatibility handshake (and its small server-side counterpart), and the release/publish CI. Each landed change carries discriminating tests (a test must FAIL pre-change and PASS post-change; no stubs/always-true asserts, per the project Stub-Prevention rule).

### Block L1 — Crate skeleton, manifest, LSP registration, build integration (LANDED)
- The crate `extensions/lapce/` (standalone, empty `[workspace]`; `crate-type = ["cdylib", "rlib"]`, wasi-cfg `lapce-plugin` dep) ships with a committed `Cargo.lock` pinned to `lsp-types 0.94.1` and the `volt.toml` manifest. The as-built is a **single `src/lib.rs`** (there is no `main.rs`): its `mod wasi_volt` — gated `#[cfg(target_os = "wasi")]` — holds `register_plugin!` + the `LapcePlugin::handle_request` that dispatches `Initialize::METHOD` to the pure `handle_initialize`. There is no `init::start`.
- The launch-contract logic lives in the shared `crates/verter-editor-client` crate — `pub fn build_server_args(root: Option<&str>, settings: &Value) -> Vec<String>` and `pub fn build_initialization_options(settings: &Value) -> Value` (signatures take `Option<&str>`, not `Option<&Path>`). The volt's `lib.rs` only carries the thin surface that consumes them: `plan_launch` / `handle_initialize` / `document_selector` and the `LspLauncher` test seam.
- Build wiring: a `pnpm` script `build:lapce` (`rustup target add wasm32-wasip1` guard + `cargo build --manifest-path extensions/lapce/Cargo.toml --target wasm32-wasip1 --release` + copy `.wasm` to `bin/`). `bin/*.wasm` is in the volt's `.gitignore`.
- **Discriminating tests** (host-target unit tests in `extensions/lapce/`, run with the host toolchain — the pure-fn modules compile on host):
  - `build_server_args` with a default config + a root path → asserts `["--type-provider=tsgo", "<root>"]` **and asserts the `tgo` typo, `--tsdk`, `--plugin-path`, and `--mcp*` are ABSENT** (negative assertions — `--tsdk`/`--plugin-path` are tsserver-only and would break the tsgo path; `tgo` would silently fall through to `auto` server-side).
  - `build_server_args` with `typeProvider=tsserver` + `tsdk` config → includes `--tsdk=…` (proves the branch).
  - `build_initialization_options` maps `lint.enabled=true` → `{lint:{enabled:true,…}}` and **omits** `decorations`/`mcp`/`configuration` (negative: VS-Code-only keys not forwarded).
  - A manifest test parsing `volt.toml` asserting `wasm = "bin/verter-lapce.wasm"`, `[activation] language` contains both `vue` and `svelte`, and the `[config]` keys exist.
  - A **build-smoke** (CI job, §6): `cargo build --target wasm32-wasip1 --release` succeeds and emits a non-empty `.wasm` (this is the test that would have caught the `lsp-types` float; it FAILS without the committed lock).

### Block L2 — Binary discovery / acquisition — ROADMAP (NOT landed)
**As-built (v0):** the volt has **no** `discovery.rs` / `manifest.rs` and ships **no** managed download. Server discovery is delegated to the shared `crates/verter-editor-client` crate (`resolve_server` over `DiscoveryInputs`), and the v0 precedence is **`lsp.serverPath` override → opted-in PATH (`lsp.serverSource = "path"`) → loud failure** — no download, no PATH fallback unless the user opts in. The override/PATH/fail decision and its host-target tests live on the volt's launch surface (`plan_launch`) and the shared crate.

**Roadmap (the managed pinned download — not yet landed):** the items below describe the Strategy-D managed-download machinery, which is future work and ships with its own `discovery.rs` / `manifest.rs` + release assets. None of it is part of the landed v0.
- A `resolve_server(os, arch, cfg, volt_dir, manifest, fs, http) -> Result<ServerLaunch>` extension where `fs`/`http` are trait seams so tests inject a fake filesystem + fake HTTP. `ServerLaunch = { uri: Url, /* urn or file */ }`.
- An embedded `{ (os,arch) -> { asset_url, sha256 } }` table (the known matrix; values are placeholders until release assets exist — gated behind the v0 decision, §8).
- SHA-256 verify, atomic install, zip-slip/tar-traversal rejection, versioned-immutable cache path `servers/<ver>/<target>/`.
- **Discriminating tests** (host-target, with fakes), all roadmap:
  - `lsp.serverPath` set → returns `urn:<that path>` and **never calls** the fake HTTP (assert the fake's call-count is 0 — discriminates override precedence).
  - Managed binary already present + correct hash → returns its `file://`, **no download** (call-count 0).
  - Managed binary absent → calls fake HTTP for the **pinned** version URL (assert the URL contains `v<serverVersion>` and the correct target triple, and **not** `latest` — discriminates the no-`latest` rule), verifies hash, installs, returns `file://`.
  - **Bad SHA-256** from fake HTTP → returns `Err` and the binary is **not** installed (negative: no file written; discriminates integrity check).
  - Unknown `(os, arch)` → `Err` "unsupported platform", **no PATH fallback** (discriminates fail-loud rule).
  - tar/zip with a traversal entry (`../evil`) → rejected (discriminates zip-slip guard).
  - Target-triple/asset-name builder → asserts names are NTFS-safe (no `:` `<` `>` etc.) for every matrix entry (discriminates the cross-platform rule).

### Block L3 — Init-options & capabilities parity + compatibility handshake
**As-built (v0):** the full `initializationOptions` parity mapping (§3.3) is landed in the shared `crates/verter-editor-client` crate (`build_initialization_options`), exercised by the volt's host-target tests. The `verterClient` compatibility handshake and its server-side counterpart are **NOT** part of v0 — the volt makes **zero** server-side changes (no `crates/verter_lsp` edit). The handshake items below are roadmap.

**Roadmap (the compatibility handshake — not yet landed):**
- Extend the parity mapping with `verterClient` handshake fields (`protocolVersion`, `expectedServerVersion`).
- **Server-side counterpart (a small, separate future change in `crates/verter_lsp`):** read `initializationOptions.verterClient`, validate protocol epoch/range, populate `serverInfo`, log/warn on mismatch. (When landed it touches `verter_lsp`, **not** `verter_session`.)
- **Discriminating tests (roadmap):**
  - Volt-side: `build_initialization_options` includes `verterClient.protocolVersion` and `expectedServerVersion` matching the volt's pinned version (FAILS if omitted).
  - Server-side (a new `crates/verter_lsp/tests/` integration test driving `initialize`): a request with a **compatible** `verterClient` → normal init + `serverInfo` present; a request with an **incompatible** `protocolVersion` → the defined mismatch behavior (warn/reject) fires. Discriminates the handshake (pre-change: no handshake → both behave identically; post-change: they differ). These join the existing real-LSP gatekeeper suite in `crates/verter_lsp/tests/`.

### Block L4 — Tests, CI, docs
**As-built (v0):** the `lapce` CI workflow (§6) — a **3-OS matrix** (ubuntu/macos/windows, `shell: bash`) running the `wasm32-wasip1` build-smoke, the host-target `cargo test`, and `cargo clippy --target wasm32-wasip1 -- -D warnings` + `cargo fmt --check` on all three. The committed-`Cargo.lock` `lsp-types < 0.95` guard test (FAILS if the lock drifts to a `Url`→`Uri` version — guards the §2.3 gotcha permanently) and the manifest tests are part of that host suite.

**Roadmap (not in v0):**
- A release-pipeline job (gated on the §8 decision) that builds per-platform `verter-lsp` assets + checksums and a `volts publish` step (needs a registry-token secret).
- `README.md` for the volt + a docs page (`docs/` guide) on installing the Verter Lapce plugin (incl. the tsgo / `node_modules` prerequisite and the binary-source config).

## 6. Build / CI integration

- **Local build:** new root `package.json` script `build:lapce` → ensures the `wasm32-wasip1` target, runs `cargo build --manifest-path extensions/lapce/Cargo.toml --target wasm32-wasip1 --release`, copies the `.wasm` into `extensions/lapce/bin/`. It is **not** added to the default `pnpm build` chain (it is an independent editor artifact, not a dependency of the core build), but is available and CI-gated.
- **CI (`.github/workflows/lapce.yml`, LANDED):** a `lapce` workflow with a **3-OS matrix** (`ubuntu-latest`, `macos-latest`, `windows-latest`) per the cross-platform CRITICAL rule, with `shell: bash` on every run step so the Windows runner uses Git-bash. On **all three** OSes it: installs the `wasm32-wasip1` target, builds the volt (`cargo build --manifest-path extensions/lapce/Cargo.toml --target wasm32-wasip1 --release`), runs the host-target `cargo test`, and runs `cargo clippy --target wasm32-wasip1 -- -D warnings` + `cargo fmt --check`. The portability rule mandates the build + host launch surface pass on macOS, Windows, AND Linux, so the build-smoke runs on all three host OSes (not a single Linux runner).
- **Release (ROADMAP, gated on §8 — not landed):** a future matrix job builds `verter-lsp` for `{x86_64,aarch64} × {apple-darwin, pc-windows-msvc, unknown-linux-musl}`, emits checksums, uploads as GitHub Release assets; a follow-up job stamps the future managed-download manifest (`extensions/lapce/src/manifest.rs`, which does not exist in v0) with URLs+hashes, rebuilds the volt, and runs `volts publish`.
- The existing workspace gate (`cargo nextest run --workspace` + `cargo test -p verter_session --tests`) is **unaffected** because the volt crate is excluded from the workspace (§4.1). v0 makes **no** server-side change, so there is nothing for the workspace gate to additionally cover; the volt's own gate is the `lapce` CI workflow. (The roadmap L3 handshake, when it lands, would be exercised by the existing `crates/verter_lsp/tests/` suite the workspace gate already covers.)

## 7. Test strategy (mandatory-rule compliant)

The project mandates automated tests for LSP/extension changes (no manual-only verification). Realistic split:

**Automated (the bulk):**
- **Pure-logic unit tests** (host target, no WASI runtime) — as-built, all in `extensions/lapce/src/lib.rs`: the launch tuple (`handle_initialize` → exact `(uri, args, selector, options)`, plus the fail-loud-on-`None`-root and the §5 negative assertions), the type-provider clamp, and the discovery precedence (override > opted-in PATH > loud fail) over the shared `verter-editor-client` crate. These fully cover the v0 decision logic. (ROADMAP: the fake-fs/fake-http tests for the managed download — hash verify, fail-loud, zip-slip, NTFS-safe names, platform-matrix totality — ship with the `discovery.rs` / `manifest.rs` managed-download work, not v0.)
- **Manifest tests:** parse `volt.toml`, assert activation languages, `wasm` path, config keys.
- **Build-smoke:** the `wasm32-wasip1` crate compiles to a non-empty `.wasm` (CI). This is a genuine discriminating gate — it fails without the committed `lsp-types` lock pin.
- **Lockfile-pin guard:** assert `Cargo.lock` pins `lsp-types < 0.95` (guards §2.3 permanently).
- **Server-side handshake** (L3 — ROADMAP, not in v0): when the handshake lands, new `crates/verter_lsp/tests/` integration tests would drive `initialize` with compatible/incompatible `verterClient`, reusing the repo's existing real-LSP gatekeeper harness (`crates/verter_lsp/tests/*.rs`, which drives the server in-process; tsgo/tsserver-dependent assertions skip vacuously without `node_modules`, per the established pattern). v0 ships no such tests because it makes no server-side change.

**Headless LSP-handshake (feasible, medium effort):** because the volt only *launches* a native server, the **server's** LSP behavior over stdio is already fully testable via `crates/verter_lsp/tests/` (independent of any editor). A Lapce-specific headless test would mean driving the WASI plugin's `handle_request(Initialize)` and asserting the `start_lsp` call shape — but `PLUGIN_RPC` talks to the Lapce host over WASI stdio, so this needs either (a) a thin abstraction over `PLUGIN_RPC.start_lsp` injected in tests (so the host-target unit test asserts the exact `(uri, args, selector, options)` the plugin would send — **recommended**, low cost, and it directly tests the launch contract), or (b) a full WASI host harness (high effort, low marginal value over (a)). **Plan: do (a)** — wrap `start_lsp` behind a trait, assert the launch tuple in unit tests. This makes the "what does the volt tell Lapce to spawn" contract a discriminating automated test.

**Manual (irreducible):** full Lapce **UI** E2E — installing the volt in a real Lapce build and exercising hover/completion/rename in a `.vue`/`.svelte` file, and confirming Lapce's own client advertises `completionItem.resolveSupport` so auto-import edits apply (§3.4 known risk). Lapce has no documented headless-editor test harness comparable to VS Code's `@vscode/test-electron` (`packages/vue-vscode/.vscode-test.mjs` + the `e2e/` Mocha suite), so editor-level UI E2E is a manual smoke checklist in the volt README, not an automated gate. The automated layers above (especially the launch-contract test and the server's own `crates/verter_lsp/tests/`) cover the volt's actual logic; the manual step only validates Lapce-host integration, which is outside Verter's control.

## 8. Scope, dependencies, and decisions for the CTO/user

**Scope landed (v0):**
- A **new, non-overlapping** crate (`extensions/lapce/`), **excluded from the root Cargo workspace** (§4.1). Touches **no existing crate** — and makes **no server-side change at all** (zero `crates/verter_lsp` edits). The volt is a pure client of the built `verter-lsp` binary and the shared `crates/verter-editor-client` launch contract.
- The `build:lapce` `package.json` script, the 3-OS `lapce` CI workflow (§6), and the volt's **own committed `Cargo.lock`** (the `lsp-types 0.94.1` pin).

**Scope deferred (ROADMAP, not in v0):**
- The `verterClient` compatibility handshake **and** its server-side counterpart (Block L3): a future change where `verter-lsp` reads + validates the handshake at `initialize` and populates `serverInfo`. When landed it would touch `crates/verter_lsp` (the LSP server), **not** `verter_session`, and be additive + exercised by the existing LSP test suite. It is explicitly **not** part of v0.
- The managed pinned download + SHA-256 + atomic install (Block L2) and the release/publish pipeline (below).

**Decisions outstanding for the roadmap (escalate to CTO/user before the next stage):**
1. **Release-engineering commitment (the big one).** Strategy D (the architect's recommended default) requires Verter to **publish per-platform `verter-lsp` binaries as GitHub Release assets** (incl. static/musl Linux x64+arm64) with checksums, plus a CI release pipeline and an embedded version/hash manifest in the volt. Verter has **no published `verter-lsp` binaries today.** v0 took the **interim** path (`lsp.serverPath` override > PATH opt-in > loud fail, loud setup docs); the open decision is **when** to build the full release pipeline and graduate to Strategy D. Graduation is additive: the shared `crates/verter-editor-client` discovery already exposes a `managed_present` seam (always `None` in v0), so adding the managed download does not require rewriting the v0 precedence.
2. **Publishing to the Lapce registry** (`plugins.lapce.dev`) requires a **registry token secret** and a `volts publish` CI step — confirm we want to publish (vs. distribute the `.wasm` via GitHub Releases for manual install).
3. **Supply-chain policy:** auto-download-and-execute of a native binary (Strategy D step 3) is a security/policy surface. The architect's mitigation is pinned URLs + SHA-256 + explicit override + clear docs; confirm this is acceptable, or require an explicit user opt-in to the managed-download mode.

**Toolchain prerequisites (documented build deps, not blockers):**
- `rustup target add wasm32-wasip1` (only `wasm32-unknown-unknown` is installed locally today; `wasm32-wasip1` is available to add — validated in §9).
- `cargo install volts` for publishing.
- These are recorded as build prerequisites in the volt README and the `build:lapce` script guards the target.

## 9. Build prerequisites & toolchain

Building the volt requires:
- The `wasm32-wasip1` Rust target (`rustup target add wasm32-wasip1`; the `build:lapce` script adds it idempotently). The volt is a `cdylib` whose `lapce-plugin` glue compiles only under `cfg(target_os = "wasi")`, so the wasm build is the surface that exercises that glue.
- The committed `Cargo.lock`, which pins `lsp-types = 0.94.1` (the highest non-yanked pre-0.95 release) so `psp-types`' unpinned `lsp-types = "0"` does not float past the `Url`→`Uri` rename (§2.3). The lockfile-pin guard test enforces `< 0.95` permanently.
- `cargo install volts` for publishing to the Lapce plugin registry (§2.4).

The host-target unit suite (the pure launch surface, the manifest checks, the lockfile-pin guard) runs under the default host toolchain with no WASI runtime; the `wasm32-wasip1` release build is the integration smoke that a dependency float would break first.

## 10. Citations

- Lapce plugin docs: https://docs.lapce.dev/development/plugin-development
- `lapce-plugin` crate API: https://docs.rs/lapce-plugin
- Official `lapce-rust` volt (download-on-activation reference): https://github.com/lapce/lapce-rust/blob/master/src/main.rs , https://github.com/lapce/lapce-rust/blob/master/volt.toml
- `lapce-julia` volt (4-arg `start_lsp`, document selector, config): https://github.com/VarLad/lapce-julia/blob/master/src/main.rs
- `lapce-yaml` / plugin template (volt.toml, Cargo.toml): https://github.com/lapce-community/lapce-yaml
- `wasm32-wasip1` target (rename from `wasm32-wasi`): https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip1.html
- Lapce plugin registry: https://plugins.lapce.dev/
- Verter sources studied: `packages/vue-vscode/src/extension.ts`, `packages/vue-vscode/package.json`, `crates/verter_lsp/src/main.rs`, `crates/verter_lsp/src/server/lifecycle.rs`, `crates/verter_lsp/src/config.rs`, `crates/verter_type_runtime/src/tsgo/ipc.rs`, root `Cargo.toml`, root `package.json`.
- Architect consult (binary discovery): `.feedback/_lapce_binfork.out`.
