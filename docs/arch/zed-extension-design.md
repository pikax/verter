# Verter Zed Editor Extension — Design (as built)

> Status: **AS BUILT.** The extension ships at `extensions/zed/` (crate `verter-zed`,
> `wasm32-wasip2` cdylib). v0 binary acquisition is `override > opt-in PATH > loud fail`
> with **no auto-download** and **zero server change**; managed download and registry
> publication are roadmap follow-ups (§8).
> Sibling: `docs/arch/lapce-extension-design.md` (same thin-LSP-client pattern; the two
> clients share the pure `verter-editor-client` launch contract).

## 1. Context

Verter ships a native stdio LSP server, the `verter-lsp` binary (Rust crate
`crates/verter_lsp`, `[[bin]] name = "verter-lsp"`, built via `cargo build -p verter_lsp
--release` → `target/{debug,release}/verter-lsp[.exe]`). The reference editor client is the
VS Code extension at `extensions/vscode`. This **Zed** extension is a **third thin
LSP-client** so Zed users get Verter's Vue/Svelte features (diagnostics, hover, completion,
definition, references, rename, code actions, semantic tokens, inlay hints, signature
help). The server already exists; **the extension is a thin launcher** whose entire job is
to tell Zed to spawn `verter-lsp` over stdio with the right args + initialization options.

It is a **standalone, non-overlapping package** under `extensions/zed/`. It touches **no
existing crate** (the server is consumed as a built binary, not a library dependency) and
modifies `verter_session` / no shared substrate. The Verter VS Code client and the Lapce
extension are the parity references; all launch-contract policy is shared via
`crates/verter-editor-client`.

**Performance is a first-class requirement.** §5 is the authoritative mechanism comparison;
the short version: Zed's `language_server_command` model puts the WASM extension OUT of the
per-LSP-message hot path — Zed's native in-editor LSP client talks **directly** to the
spawned native `verter-lsp` over stdio, and the extension is invoked only once at launch.

## 2. The Zed extension model (researched, with citations)

Zed extensions are Rust crates compiled to **WebAssembly** that run in Zed's WASM host. An
extension that provides a language server implements the `Extension` trait from the
`zed_extension_api` crate and returns a launch `Command`; Zed's **native** LSP client then
spawns and drives the process. Verter's `verter-lsp` is exactly such a native stdio server,
so the model fits directly.

### 2.1 The `zed_extension_api` crate & `Extension` trait

Confirmed against the published API ([docs.rs/zed_extension_api](https://docs.rs/zed_extension_api/latest/zed_extension_api/trait.Extension.html))
and real, current Zed extensions: the official **`zed-extensions/tsgo`** (TypeScript-native;
a language-server-only extension, no grammar), **`zed-extensions/harper`** (a native-binary
GitHub-release download), and **`zed-extensions/vue`** (Volar; npm-installed server +
grammar).

The extension (as built) implements exactly two methods over the required `new()`:

```rust
use zed_extension_api::{self as zed, LanguageServerId, Result, settings::LspSettings};

struct VerterExtension;

impl zed::Extension for VerterExtension {
    fn new() -> Self { VerterExtension }

    fn language_server_command(
        &mut self,
        _id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> { /* discover binary, build argv via verter-editor-client */ }

    fn language_server_initialization_options(
        &mut self, _id: &LanguageServerId, worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> { /* build_initialization_options(settings) */ }
}

zed::register_extension!(VerterExtension);
```

Key API facts (verbatim shapes from docs.rs + the `tsgo`/`harper` sources), all exercised by
the shipped `wasm32-wasip2` build:

- **`Extension` trait — only `fn new() -> Self` is REQUIRED; every other method is provided
  with a default**, including `language_server_command`. ([docs.rs Extension trait](https://docs.rs/zed_extension_api/latest/zed_extension_api/trait.Extension.html))
- `language_server_command(&mut self, &LanguageServerId, &Worktree) -> Result<Command>`.
- `language_server_initialization_options(&mut self, &LanguageServerId, &Worktree) ->
Result<Option<serde_json::Value>>` — the LSP `initialize` `initializationOptions`
  (one-time).
- `Command { command: String, args: Vec<String>, env: Vec<(String, String)> }` — the launch
  spec.
- `Worktree` methods used: `root_path() -> String` (the launch root). Others available:
  `which(&self, binary: &str) -> Option<String>` (PATH lookup), `shell_env()`,
  `read_text_file(&self, path: &str)`, `id() -> u64`. ([docs.rs Worktree](https://docs.rs/zed_extension_api/latest/zed_extension_api/struct.Worktree.html))
- `zed::current_platform() -> (zed::Os, zed::Architecture)`; `Os::{Mac, Linux, Windows}`;
  `Architecture::{Aarch64, X8664, X86}`.
- Binary acquisition helpers (NOT used in v0 — roadmap): `zed::latest_github_release`,
  `zed::github_release_by_tag_name`, `zed::download_file`, `zed::make_file_executable`,
  `zed::set_language_server_installation_status`.
- User settings access: `LspSettings::for_worktree(server_name, worktree) ->
Result<LspSettings>` with `.binary: Option<CommandSettings { path, arguments, env }>`,
  `.settings: Option<Value>`, `.initialization_options: Option<Value>`. The extension reads
  the user's `lsp.verter.binary.path` override, `binary.arguments`/`binary.env`, and the
  `lsp.verter.settings` blob through this.

### 2.2 The `extension.toml` manifest — ONE server id, plural `languages`

Verter ships a **single** language-server contribution `verter` bound to BOTH the `"Vue.js"`
and `"Svelte"` languages via a **plural `languages` array** — the shape proven by the
official `zed-extensions/tsgo` extension (`schema_version = 1`, one `[language_servers.tsgo]`
binding four languages). No grammar.

```toml
id = "verter"
name = "Verter (Vue / Svelte)"
description = "Vue & Svelte language support powered by the Verter LSP"
version = "0.1.0"
schema_version = 1
authors = ["pikax"]
repository = "https://github.com/pikax/verter"

[language_servers.verter]
name = "Verter"
languages = ["Vue.js", "Svelte"]
language_ids = { "Vue.js" = "vue", "Svelte" = "svelte" }
```

`language_ids` maps each Zed language NAME to the LSP `languageId` the server expects (`vue`
/ `svelte`). There is **no `[grammars]` table and no `languages/*/config.toml`** — Verter is
language-server-only (§4.2).

### 2.3 Cargo.toml & build target

```toml
[workspace]            # empty: detaches the crate from the root workspace

[package]
name = "verter-zed"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]   # cdylib for Zed; rlib so host-target tests link

[dependencies]
serde_json = "1.0"
verter-editor-client = { path = "../../crates/verter-editor-client" }  # shared pure policy (§9)

[target.'cfg(target_os = "wasi")'.dependencies]
zed_extension_api = "0.7.0"       # wasi-only; not pulled on the host test target

[dev-dependencies]
toml = "0.8"                       # manifest + lockfile parse tests
```

**Build target — `wasm32-wasip2` (NOT `wasm32-wasip1`).** `zed_extension_api` ≥ 0.5 targets
`wasm32-wasip2` (the component-model successor of wasip1). This is the **one notable
divergence from the Lapce volt**, which builds `wasm32-wasip1` (the `lapce-plugin` crate
targets wasip1). ([rustc wasm32-wasip2](https://doc.rust-lang.org/nightly/rustc/platform-support/wasm32-wasip2.html);
[Zed issue #48724](https://github.com/zed-industries/zed/issues/48724) — a wasip1/wasip2
mismatch produces a `__wasi_init_tp` load error and no language appears.)

Build: `rustup target add wasm32-wasip2` then `cargo build --manifest-path
extensions/zed/Cargo.toml --target wasm32-wasip2 --release` (wired as the `pnpm` script
`build:zed`, which also copies the `.wasm` into `extensions/zed/bin/`). For local
development, Zed compiles the extension itself via **"zed: install dev extension"** pointed
at `extensions/zed/`; the standalone build is the CI smoke + reproducibility gate.

### 2.4 Publishing

Zed extensions are published by opening a PR against the **`zed-industries/extensions`**
registry repo (adding the extension as a git submodule + a row in `extensions.toml`), not via
a CLI. This is a **roadmap** follow-up (§8); v0 ships via the dev-extension flow.

## 3. What the Zed extension mirrors from the VS Code client

Studied in `extensions/vscode/src/extension.ts`, `crates/verter_lsp/src/main.rs`,
`crates/verter_lsp/src/server/lifecycle.rs`, `crates/verter_lsp/src/config.rs`.

### 3.1 Languages & document attachment

The VS Code client attaches a document selector of `{ scheme: "file", language: "vue" }`,
`{ … "svelte" }`. For Zed, the equivalent is the single `[language_servers.verter]` binding
with `languages = ["Vue.js", "Svelte"]` and `language_ids` mapping each Zed language name to
the LSP `languageId` (`vue` / `svelte`), activated per-language via the user's
`language_servers` setting. Vue/Svelte are the carriers; the server projects them to TS
internally. Each Zed `Worktree` is a separate launch root — `language_server_command` passes
that worktree's `root_path()` and caches no global root (multi-root correctness).

### 3.2 Server launch — CLI args

The `verter-lsp` CLI (`crates/verter_lsp/src/main.rs`, hand-rolled `CliArgs::parse`) accepts
`--type-provider={auto|tsgo|tsserver|extension|off}`, `--tsdk=<path>`,
`--plugin-path=<path>`, `--mcp-port=<n>` (ignored), `--mcp-lint-preset=<preset>` (ignored),
and a **positional workspace root**.

**Decision: emit `--type-provider=tsgo`.** The accepted literal in `main.rs` is `tsgo` (the
match arms are `"auto" | "tsgo" | "tsserver" | "extension" | "off"`; there is no `tgo` arm —
it would fall through to the `_ => "auto"` default). The shared `verter-editor-client`
crate's `build_server_args` **clamps** any non-`{tsgo, off}` value (including the `tgo` typo,
`tsserver`, and `auto`) to `tsgo`, so a SDK-less client never requests a provider it cannot
satisfy (it never passes `--tsdk`). `tsgo` is self-contained: `try_spawn_tsgo` discovers the
native provider via the `verter_tsgo_api::toolchain` 4-tier resolver (`VERTER_TSGO_BIN` → shared PATH → project-local `node_modules` → update cache → bundled sidecar, validated candidates only), workspace
`node_modules` → PATH → npm/npx cache). The extension ships no TypeScript SDK; the user
installs TypeScript 7 (`typescript@7`, which supplies the native platform binary) per-project. `--plugin-path` and the `--mcp-*` flags
are omitted.

The **workspace root** positional is forwarded explicitly (a WASM extension's cwd is not the
workspace), derived from `worktree.root_path()` and placed LAST. Resulting argv for the
default case:

```
["--type-provider=tsgo", "<worktree.root_path()>"]
```

User `lsp.verter.binary.arguments` are **merged into `settings.lsp.serverArgs`** (via
`merge_binary_arguments_into_settings`) BEFORE the argv is built, so they flow through the
same shared `build_server_args` filtering/ordering as the `settings`-blob `serverArgs`
rather than being appended raw — the shared crate filters user extras that collide with
crate-owned args (the `--type-provider`/`--tsdk` namespaces and any bare positional) and
keeps the workspace root LAST.

### 3.3 Initialization options — parity mapping

The VS Code client passes a rich `initializationOptions`; the server reads only a subset
(`lifecycle.rs`; `config.rs`). The extension delegates to the shared
`build_initialization_options`, which projects the user's `lsp.verter.settings` blob onto
**exactly** the server-read parity set and fills defaults:

| Init option (server reads)                                | Default                                     |
| --------------------------------------------------------- | ------------------------------------------- |
| `lint: { enabled, preset }`                               | `{ enabled: false, preset: "recommended" }` |
| `inlayHints: { enabled }`                                 | `{ enabled: true }`                         |
| `viteConfig: { enabled, trustedFiles }`                   | `{ enabled: true, trustedFiles: [] }`       |
| `experimental: { conditionalRootNarrowing, strictSlots }` | `{ false, false }`                          |
| `hover: { provenance }`                                   | `{ provenance: false }`                     |
| `statistics: { enabled }`                                 | `{ enabled: false }`                        |

The builder constructs the output POSITIVELY — it only ever inserts the parity keys — so
editor/UI-only settings (`configuration`, `decorations`, `mcp`, `analysis`, `trace`) cannot
leak. Two parity properties of the shipped shared crate:

- **No `frameworks` field is emitted.** The server reads no `frameworks` field, so emitting
  it would be dead protocol surface. `build_initialization_options` drops it.
- **`statistics` IS emitted, defaulting OFF.** The field is present so a user opt-in is
  honored; it defaults to `{ enabled: false }`.

These are pinned by the shared crate's
`top_level_key_set_is_exactly_the_emitted_parity_set` / `frameworks_field_is_not_emitted`
tests and by the Zed crate's `init_options_come_from_shared_builder_with_parity_negatives`
wiring test.

### 3.4 Client capabilities — nothing Zed-specific to hand-author

`verter-lsp` reads the editor's `params.capabilities` in exactly one place —
position-encoding negotiation (`lifecycle.rs`, prefers UTF-8 > UTF-32 > UTF-16, defaults
UTF-16). The tsgo client-capabilities are STATIC server-side
(`build_client_capabilities()` in `crates/verter_type_runtime/src/tsgo/ipc.rs` is a pure
literal independent of the editor). Zed's native LSP client builds and sends standard
`textDocument` capabilities; the extension hand-authors none. The only quality dependency is
whether Zed advertises `completionItem.resolveSupport` (so auto-import `additionalTextEdits`
apply) — Zed-core behavior, validated in manual E2E (§7), not a blocker.

### 3.5 Custom requests / middleware

The VS Code client uses `$/verter/*` custom requests and middleware (CSS service merge,
decorations). Zed (a plain LSP client) never sends these — the server only responds when
asked, so this degrades gracefully: all standard LSP features flow unchanged; only the
VS-Code-exclusive panels/decorations are absent. There is no per-message middleware seam in
the Zed extension model (§5 — exactly why the extension is out of the hot path).

## 4. Implementation (as built)

### 4.1 Package layout & workspace isolation

A standalone crate **`extensions/zed/`** (crate name `verter-zed`), **outside the root Cargo
workspace** (detached by an empty `[workspace]` table), so the host-target `cargo build` /
`cargo nextest run --workspace` never pulls in this `wasm32-wasip2` cdylib. It lives under
`extensions/` (the home for editor clients — `extensions/vscode`, `extensions/lapce`),
automatically outside the workspace member glob.

```
extensions/zed/
  Cargo.toml          # standalone, NOT a workspace member; cdylib+rlib; zed_extension_api + verter-editor-client
  Cargo.lock          # committed; pins zed_extension_api 0.7.x
  extension.toml      # ONE language-server id `verter`, plural languages, no grammar (§2.2/§4.2)
  src/lib.rs          # pure launch surface (host-testable) + register_extension!/Extension glue behind cfg(wasi)
  README.md
  .gitignore          # /target, bin/*.wasm
  # build output (target dir, bin/*.wasm) is gitignored
```

`src/lib.rs` is a **single file** carrying both halves of the pure/glue split (mirroring the
Lapce `lib.rs`): the pure decision surface ([`plan_launch`] + [`LaunchPlan`] /
[`LaunchError`]) compiles and unit-tests on the host target (std + `serde_json` +
`verter-editor-client`), and the `zed_extension_api` glue (the `wasi_extension` module with
`register_extension!` + `impl zed::Extension`) is isolated behind
`#[cfg(target_os = "wasi")]`. The crate is `#![forbid(unsafe_code)]`.

`language_server_command` reads `worktree.root_path()`, the `lsp.verter.settings` blob, and
the `lsp.verter.binary.path` override; under an active `serverSource = "path"` opt-in it asks
`worktree.which("verter-lsp")` for the absolute PATH hit (`None` otherwise); merges
`binary.arguments` into `settings.lsp.serverArgs` (`merge_binary_arguments_into_settings`)
so they ride the shared contract; calls `plan_launch` (which delegates discovery + argv to
the shared crate); and forwards `binary.env` to `zed::Command.env` verbatim (env is not
argv). A discovery failure maps to `Err(message)` from `language_server_command` (a loud,
actionable string naming the override key + the PATH opt-in) — it **never launches a
rootless/pathless server**.

### 4.2 Grammar + language registration — language-server-ONLY

**Verter is a language-server-only extension (no grammar); it attaches to the official
Vue.js/Svelte languages.** It ships NO `[grammars]` and NO `languages/*/config.toml`, attaches
its LSP to the official `"Vue.js"` and `"Svelte"` languages, and is selected via the user's
`languages.<lang>.language_servers` setting. Verter is an explicit **opt-in alternative** to
Volar / svelte-language-server, not a replacement language package.

**The decisive constraint, proven against real extensions:** Zed requires a tree-sitter
grammar only for a language an extension **defines**; an extension may instead contribute a
language-server-only entry bound to languages that already exist. **Proof:
`zed-extensions/tsgo` ships NO `[grammars]` and NO `languages/*/config.toml`** — only
`[language_servers.tsgo] languages = ["TypeScript","TSX","JavaScript","JSX"]`, attaching to
Zed's built-in languages. ([zed-extensions/tsgo](https://github.com/zed-extensions/tsgo).)

Vue and Svelte are NOT built into Zed — they come from the official `zed-extensions/vue`
(`"Vue.js"`) and `zed-extensions/svelte` (`"Svelte"`) extensions. So the Verter extension
**declares a documented prerequisite** on those official extensions for the grammar +
language definition, and contributes only the language server.

Why language-server-only:

1. A language-defining extension needs a grammar; a server-only extension does not. Zero
   grammar maintenance.
2. The official `vue`/`svelte` extensions already own `"Vue.js"`/`"Svelte"` with correct
   grammars — duplicating them is wasted work.
3. Two extensions claiming the same language name OR the same path ownership (`vue`/`svelte`)
   is risky; an opt-in server attaching to the existing languages avoids that.
4. Verter as an opt-in alternative (not a global default that silently displaces Volar) is
   the correct, honest product stance.

**Manifest (final, as built) — ONE server id `verter`, plural `languages`.** The official
`zed-extensions/tsgo` extension proves a single `[language_servers.<id>]` binding multiple
languages via a plural `languages = [...]` array (it binds four). Verter ships ONE
id `verter` bound to both carriers, matching that proven reference:

```toml
[language_servers.verter]
name = "Verter"
languages = ["Vue.js", "Svelte"]
language_ids = { "Vue.js" = "vue", "Svelte" = "svelte" }
```

No `[grammars]`, no `languages/*/config.toml`, no `path_suffixes`. One `verter-lsp` serves
both carriers; one settings key `lsp.verter`. The manifest test
(`single_verter_server_id_with_display_name` /
`languages_are_exactly_vue_and_svelte_via_plural_array` /
`no_grammars_table_language_server_only`) pins this shape and the absence of a `[grammars]`
table.

**User-facing `settings.json`** (the opt-in that activates Verter, disabling the default):

```jsonc
{
  "languages": {
    "Vue.js": { "language_servers": ["verter", "!vue-language-server", "..."] },
    "Svelte": { "language_servers": ["verter", "!svelte-language-server", "..."] },
  },
}
```

(Without the `!`-disable the user gets duplicate diagnostics/completions from both servers —
the README calls this out.)

### 4.3 Binary discovery / acquisition — v0 = override > opt-in PATH > loud fail

Discovery precedence is owned by the shared `verter-editor-client` crate's `resolve_server`
(the same precedence the Lapce volt uses), driven by host-gathered inputs:

**Precedence (highest first):**

1. **User `lsp.verter.binary.path` override** → launch it (covers dev `target/{debug,release}`
   - power users). `binary.arguments` / `binary.env` are also forwarded.
2. **PATH discovery — opt-in only.** When the user sets `lsp.verter.settings.serverSource =
"path"`, the extension asks `worktree.which("verter-lsp")` for the binary on `PATH`. That
   host call resolves the ABSOLUTE path itself — handling `.exe`/`PATHEXT` on Windows — and
   returns the resolved absolute path (or `None`). The extension injects that real hit into
   `plan_launch` as `path_found` and the resolved source uses it VERBATIM as the launch
   command; the extension fabricates no basename. A `verter-lsp` found on PATH **without** the
   opt-in is NOT launched — it yields the distinct `PathFoundButNotOptedIn` reason, so a stale
   PATH binary can't silently break client/server version coupling.
3. **Loud fail.** No override, and an opt-in with no `worktree.which` hit (`path_found =
None`), → `language_server_command` returns an actionable error (set `lsp.verter.binary.path`,
   or install `verter-lsp` and opt in) — the distinct `NothingResolved` reason, never a
   silent mis-launch and never a guessed binary name.

The shared crate's `DiscoveryInputs` carries a `managed_present` field that is always `None`
in v0; the host does NOT download anything. PATH discovery is delegated entirely to the host
(`worktree.which`), so there is no platform-basename derivation on the launcher path; an
opted-in worktree where `worktree.which` finds nothing fails loud rather than guessing — the
CRITICAL portability rule.

**Roadmap (managed download).** A future `override > pinned managed download > opt-in PATH`
state requires Verter to publish per-platform `verter-lsp` GitHub-release assets + a release
pipeline (none exist yet). The shared `discovery` module documents the forward-compat seam
(an additive `ServerSource`/`DiscoveryInputs` extension), so adding the managed pinned-download

- checksum-verification source is additive, not a rewrite (§8).

### 4.4 Settings schema (Zed `settings.json`)

Zed has no `extension.toml [config]` block; extension settings live under the user's
`lsp.<server>` and `languages.<lang>` keys, read via `LspSettings`. The Verter extension
reads (all under the single `verter` id):

```jsonc
{
  "lsp": {
    "verter": {
      "binary": { "path": "…", "arguments": ["…"], "env": { "…": "…" } }, // discovery override + extras
      "settings": {
        // forwarded as initializationOptions (§3.3)
        "serverSource": "path", // PATH-discovery opt-in (only if binary.path unset)
        "lint": { "enabled": false, "preset": "recommended" },
        "inlayHints": { "enabled": true },
        "viteConfig": { "enabled": true, "trustedFiles": [] },
        "experimental": { "conditionalRootNarrowing": false, "strictSlots": false },
        "hover": { "provenance": false },
        "statistics": { "enabled": false },
      },
    },
  },
  "languages": {
    "Vue.js": { "language_servers": ["verter", "!vue-language-server", "..."] },
    "Svelte": { "language_servers": ["verter", "!svelte-language-server", "..."] },
  },
}
```

The `serverSource` opt-in lives inside the `settings` blob (the neutral data the shared crate
reads); `binary.path` is a first-class Zed `CommandSettings` field. Because there is ONE
server id, all of this is keyed under `lsp.verter` — no per-carrier duplication.

## 5. Performance

**Verdict: Zed's `language_server_command` → native-stdio model is the highest-performance
integration Zed offers, and it places the WASM extension OUT of the per-LSP-message hot
path.** The heavy semantic work runs in the native `verter-lsp` at full native speed; Zed's
native (in-editor, Rust) LSP client speaks LSP directly to the spawned process.

### 5.1 The extension is out of the per-message hot path

The `Extension` trait exposes **no per-LSP-request hook** — no method receives/transforms each
`textDocument/*` request or response. The only entry points relevant to a running server are
`language_server_command` (called **once** at launch to produce `Command`) and
`language_server_initialization_options` (**one-time** config). After
`language_server_command` returns, **Zed's native LSP client spawns the process and
communicates with it directly over stdio** — the WASM extension is not an intermediary on any
subsequent message. Same architecture as `zed-extensions/tsgo` / `harper` (they only compute
a launch `Command` and never see an LSP message). **Zero per-request WASM overhead.**

### 5.2 Transport — stdio is the right (and only practical) local transport

Zed's extension-launched servers communicate over the process's **stdio** pipes (the
`Command` model launches a child process; `verter-lsp` already speaks LSP over stdio —
`Server::new(stdin, stdout, …)` in `main.rs`). TCP / socket / named-pipe add connection
setup + kernel networking overhead and a port/security surface with no latency benefit for a
co-located process; the `Command` API does not offer a socket-connect mode anyway. stdio is
the fastest transport and the idiomatic Zed mechanism.

### 5.3 Where the work runs

All semantic work — parsing, TS projection, type resolution, component-meta, diagnostics —
runs **inside the native `verter-lsp`** (native Rust + the tsgo native provider). The
extension contributes no computation. Syntax highlighting/outline is driven by the
tree-sitter grammar from the official Vue/Svelte extension (Verter ships none), which runs in
Zed's native core, independent of the LSP request path.

### 5.4 Startup & steady-state latency

- **Lazy activation.** `language_server_command` runs only when a `Vue`/`Svelte` document
  triggers the `verter` server — no eager work at editor startup.
- **One-time discovery + config.** Binary discovery and init-options run at launch, not per
  request.
- **Native warm caches.** Steady-state latency is governed entirely by `verter-lsp`'s own
  warm caches (the fact-based cache architecture), exactly as in VS Code — the extension adds
  nothing.

**Net:** the only extension-attributable cost is a one-time WASM call to build a launch
command (microseconds). The per-LSP-message hot path is 100% native.

## 6. Test strategy (mandatory-rule compliant)

**Automated, host target, no wasm runtime:** the bulk of the launch-contract logic is the
shared `crates/verter-editor-client` suite (`build_server_args` incl. the `tsgo`-not-`tgo`
clamp + negative `--tsdk`/`--plugin-path` absence; `build_initialization_options` parity +
`frameworks`-absent + `statistics`-present negatives; `discovery` precedence + the two
distinct loud-fail reasons; `platform` matrix totality + unknown-fails-loud), run under the
canonical `cargo nextest run --workspace` gate (the shared crate is a workspace member).

The Zed crate's own host-target tests (`cargo test --manifest-path extensions/zed/Cargo.toml`)
cover the wiring:

- **Launch contract:** a fake root + override settings → the EXACT plan (`command_path` ==
  the override; `args == ["--type-provider=tsgo", "<root>"]`).
- **`tsgo` emitted, `tgo`/`tsserver`/`auto`/`bogus`/`""` clamped, the literal `tgo` NEVER in
  argv** (the §1a revert-test target).
- **init-options parity negatives** (`frameworks` absent, `statistics.enabled == false`
  present, `mcp`/`configuration`/`decorations` absent) — proving the delegation to the shared
  builder.
- **Fail-loud on unresolved binary** (no override + no PATH opt-in → `Err`, naming the
  override key + the PATH opt-in; `PathFoundButNotOptedIn` vs `NothingResolved` preserved).
- **PATH opt-in uses the injected real host hit verbatim** — the absolute path the host's
  `worktree.which("verter-lsp")` resolved (already carrying `.exe` on Windows) becomes the
  launch command as-is, NOT a re-derived basename; a sibling test asserts an opt-in with NO
  host hit (`path_found = None`) fails loud as `NothingResolved` rather than fabricating a
  binary name.
- **`binary.arguments` ride the shared contract** — a test merges colliding extras
  (`merge_binary_arguments_into_settings` → `plan_launch`) and asserts a reinjected
  `--type-provider=` is dropped (the clamp stays `tsgo` first), a bare positional is dropped
  (the root stays last), and benign flags survive — proving they go THROUGH the shared
  filter rather than landing raw past the contract's argv.
- **Root is the trailing positional**, last.
- **Manifest test:** parse `extension.toml`, assert the single `verter` id, `languages`
  exactly `["Vue.js","Svelte"]`, `language_ids` → `vue`/`svelte`, `schema_version`/`id`
  present, and **NO `[grammars]` table** (negative).
- **Lockfile pin test:** `zed_extension_api` is pinned to the 0.7.x line the glue compiles
  against.

**wasm build-smoke** (CI, `wasm32-wasip2` release) — the highest-value Zed-specific gate: it
compiles the `cfg(wasi)` glue against the real `zed_extension_api` 0.7.0 API and catches a
wasip1/wasip2 mismatch or an API drift.

**Production-plan semantic smoke:** the Linux CI lane builds the real extension and
`verter-lsp`, calls the extension's production `plan_launch`, and drives the exact command,
argv, and initialization options through the shared stdio client. Valid Vue/Svelte hover
must be concrete, an authored mutation must publish its exact TS2322, restoration must return
to zero diagnostics, and startup/shutdown must complete. Missing inputs fail closed.

**Server-side LSP behavior** is already covered by the real-LSP gatekeeper suite in
`crates/verter_lsp/tests/` (drives the server in-process over stdio, independent of any
editor). The Zed extension only _launches_ that server, so **no server change is needed**.

**Manual (irreducible):** full Zed **UI** E2E — installing the dev extension in a real Zed
build and exercising hover/completion/rename in a `.vue`/`.svelte` file; confirming Zed
advertises `completionItem.resolveSupport` (§3.4). Zed has no documented headless-editor
harness comparable to `@vscode/test-electron`, so editor-level UI E2E is a manual smoke
checklist in the README, not an automated gate (mirrors the Lapce/neovim posture).

## 7. CI

`.github/workflows/zed.yml` runs a 3-OS matrix (ubuntu/macos/windows, `shell: bash`,
`dtolnay/rust-toolchain@stable` with `targets: wasm32-wasip2` + clippy/rustfmt): the
`wasm32-wasip2` release build, the host-target unit tests, `cargo clippy --target
wasm32-wasip2 -- -D warnings`, and `cargo fmt --check`. The Linux lane additionally runs the
production-plan semantic smoke. Real-Zed UI loading remains manual because Zed has no
headless extension-host harness; the neutral-client lane is documented as a shipping-plan
contract, not as GUI automation.

The `pnpm build:zed` script (`rustup target add wasm32-wasip2` + the wasip2 release build + a
`shx` copy of the `.wasm` into `extensions/zed/bin/`) is NOT in the default `pnpm build`
chain.

## 8. Scope, dependencies, and roadmap

**Scope (as built):**

- **`extensions/zed/`** — a standalone crate **outside the root Cargo workspace** (§4.1).
  Touches **no existing crate**.
- **`crates/verter-editor-client`** — consumed unchanged as the shared launch contract; the
  Zed extension required NO modification to it.
- New `package.json` script `build:zed`; a new CI job. **No server-side change** (tsgo
  client-caps + position-encoding are already static/handled).
- The canonical `cargo nextest run --workspace` + `cargo test -p verter_session --tests` gate
  is unaffected by `extensions/zed/` (out of workspace).

**Decisions (as built):**

- **Grammar/language → language-server-only, no grammar.** Verter attaches to the official
  `"Vue.js"`/`"Svelte"` languages, an opt-in alternative to Volar/svelte-language-server.
- **Manifest → ONE server id `verter` with plural `languages`** (proven by `zed-extensions/tsgo`).
- **Discovery → override > opt-in PATH > loud fail** (v0). Managed download is roadmap.

**Roadmap (follow-ups — not v0):**

1. **Managed pinned-download + checksum verification (requires published per-platform release
   assets).** Wire the managed-download source into the shared crate's `discovery` once
   per-platform `verter-lsp` release assets exist; a CI matrix builds
   `{x86_64,aarch64} × {apple-darwin, pc-windows-msvc, unknown-linux-musl}` assets +
   checksums; stamp the pinned tag/checksum into `verter-editor-client`. Use
   `github_release_by_tag_name` with a pinned server version, SHA-verify before exec, then
   `make_file_executable`; unknown OS/arch/libc fails loudly. (Shared with Lapce.)
2. **Publish to `zed-industries/extensions`.** A PR adding the extension as a submodule + an
   `extensions.toml` row — only after binary acquisition is reliable, or with the listing
   clearly marked manual-binary v0.
3. **Product positioning.** Whether to market Verter as the recommended Vue/Svelte server for
   Zed (the architecture is opt-in regardless).

**Toolchain prerequisites:** `rustup target add wasm32-wasip2` (NOT wasip1 — §2.3); Zed
itself for the dev-extension load + manual E2E.

## 9. Shared `verter-editor-client` crate

Lapce and Zed (and the VS Code client, in TS) all perform the same jobs: (a) discover the
`verter-lsp` binary, (b) build CLI args (`--type-provider=tsgo` + positional root + extras),
(c) build the `initializationOptions` parity object — over the same platform matrix and
discovery precedence. The host APIs differ (`lapce-plugin` wasip1 vs `zed_extension_api`
wasip2); the **policy must not diverge**, so it lives in one pure, host-API-free crate at
`crates/verter-editor-client` (a workspace member — host-target-compilable, so the canonical
gate builds and tests it).

**In the shared crate (pure, no host API):** CLI-arg construction (the `tsgo` clamp +
positional root + filtered extras); the `initializationOptions` builder (the server-read
parity set, with `frameworks` dropped and `statistics` emitted); the platform matrix +
asset-naming policy `(Os, Arch) → triple/asset-name`; the discovery-precedence state machine
over neutral inputs; constants. The public API the v0 Zed glue calls: `build_server_args`,
`build_initialization_options`, and `resolve_server` (over `DiscoveryInputs`/`ServerSource`/
`DiscoveryError`) — `plan_launch` injects the host's `worktree.which` PATH hit as
`DiscoveryInputs.path_found`, so the launcher path needs no platform helper. The platform /
asset-naming surface (`from_host`, `binary_file_name`, `target_triple`, `asset_name`) exists
for the roadmap managed-download source (§8) — it derives the per-platform release-asset name,
NOT the PATH launch command.

**Host-specific (in `extensions/zed/`):** `zed_extension_api` ABI; `Worktree` reads
(`root_path`); `LspSettings` reads (`binary.path`/`arguments`/`env`, the `settings` blob);
`zed::Command` construction; user-error reporting; manifest registration. (Managed
download/cache/chmod is roadmap, not present in v0.)

## 10. Citations

- Zed extensions overview / developing / languages / language-servers:
  https://zed.dev/docs/extensions , https://zed.dev/docs/extensions/developing-extensions ,
  https://zed.dev/docs/extensions/languages
- Configuring languages (per-language `language_servers` selection; `lsp.<server>.binary`
  override): https://zed.dev/docs/configuring-languages
- `zed_extension_api` trait + types:
  https://docs.rs/zed_extension_api/latest/zed_extension_api/trait.Extension.html ,
  https://docs.rs/zed_extension_api/latest/zed_extension_api/struct.Worktree.html ,
  https://crates.io/crates/zed_extension_api
- Reference extensions (real, current): `zed-extensions/tsgo` (language-server-only, no
  grammar; single server id + plural `languages`): https://github.com/zed-extensions/tsgo ;
  `zed-extensions/harper` (native-binary GitHub-release download):
  https://github.com/zed-extensions/harper ; `zed-extensions/vue` (Volar; grammar + npm
  server): https://github.com/zed-extensions/vue ; `zed-extensions/svelte`:
  https://github.com/zed-extensions/svelte
- `wasm32-wasip2` target + wasip1/wasip2 mismatch symptom:
  https://doc.rust-lang.org/nightly/rustc/platform-support/wasm32-wasip2.html ,
  https://github.com/zed-industries/zed/issues/48724
- Sibling design (parity reference): `docs/arch/lapce-extension-design.md`.
- Verter sources studied: `extensions/vscode/src/extension.ts`,
  `crates/verter_lsp/src/main.rs`, `crates/verter_lsp/src/server/lifecycle.rs`,
  `crates/verter_lsp/src/config.rs`, `crates/verter_type_runtime/src/tsgo/ipc.rs`, root
  `Cargo.toml`, root `package.json`, the shared `crates/verter-editor-client`.
