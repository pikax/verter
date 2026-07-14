# Verter Helix Support — Design & Implementation Plan

> Status: **LANDED — config-only v0.** Helix support ships at [`editors/helix/`](../../editors/helix/) as a `languages.toml` snippet + README, guarded by the hermetic Rust contract test `crates/verter-editor-client/tests/helix_config_contract.rs`. Scope: a discoverable `verter-lsp` binary (NO managed download), ZERO server-side change. Anything beyond v0 (a `hx --health` CI smoke, upstreaming) is roadmap — see [§9.1 Roadmap (out of v0 scope)](#91-roadmap-out-of-v0-scope).
> Sibling designs: [`docs/arch/neovim-support-design.md`](./neovim-support-design.md) and [`docs/arch/lapce-extension-design.md`](./lapce-extension-design.md) (same server, different editors). The server-launch contract (type provider, init-options parity) is shared; the **distribution model differs per editor**. **Correction over the sibling docs:** the verter-lsp type-provider value is **`tsgo`**, not `tgo` (the sibling docs contain the `tgo` typo — see §3.3). `tgo` is not a recognized value and silently falls through to `auto`.

## 1. Context

Verter ships a native stdio LSP server, the `verter-lsp` binary (Rust crate `crates/verter_lsp`, `[[bin]] name = "verter-lsp"`, built via `cargo build -p verter_lsp --release`). The reference editor client is the VS Code extension at `packages/vue-vscode`. This document designs **Helix support**.

**Helix has a BUILT-IN native LSP client (Rust, in-process). There is NO compiled extension to write.** Unlike VS Code (a `vscode-languageclient` extension) or Lapce (a `wasm32-wasip1` "volt"), and like Neovim, Helix ships its own native LSP client. "Helix support" therefore means **LSP-client CONFIGURATION expressed in `languages.toml`**:

- (a) a `[language-server.verter]` block defining `command` (the `verter-lsp` invocation), `args`, and `config` (init-options, reaching parity with the VS Code client's server-read subset);
- (b) for each carrier language (`vue`, `svelte`), a `[[language]]` entry attaching `language-servers = ["verter"]`;
- (c) language **detection** for `.vue` / `.svelte` (Helix core already supplies this — see §3.3).

This is a **new, non-overlapping artifact** (`editors/helix/`). It touches **no existing crate's behavior** — the server is consumed as a built binary, not a library — and requires **no server-side change** (§8). The only Rust change is a `[dev-dependencies]` line plus a new integration-test file added to `crates/verter-editor-client` (§4.1).

The **TOML config layer does ZERO per-request work.** It sets `command`/`args`/`config` exactly once at process spawn / `initialize`. All request-time work happens inside Helix's native (Rust) LSP client and the native `verter-lsp` process — see §2 (Performance).

## 2. Performance (FIRST-CLASS REQUIREMENT)

Performance is a non-negotiable design driver. Helix offers exactly **one** LSP integration path — its built-in native (Rust) client over stdio — and that path **is** the highest-performance integration available. This section is the architectural verdict; the rest of the doc implements it.

### 2.1 Integration mechanism — built-in native client, no proxy (only path, and the fastest)

Helix has **no plugin-based LSP path** (the in-development Steel/Scheme plugin system is for editor commands, not for hosting language servers; routing LSP through a plugin would be strictly slower and is explicitly out of scope). LSP is configured declaratively in `languages.toml` and serviced by Helix's built-in client:

| Mechanism | Request-time path | Verdict |
|---|---|---|
| **`languages.toml` `[language-server.verter]` + built-in client** | Helix's in-process **Rust native LSP client** ⇄ stdio ⇄ native `verter-lsp`. TOML parsed once at startup. | **CHOSEN — the only path, and the most direct possible.** |
| Steel/Scheme plugin shim | Adds a plugin hop in front of the client. | **Rejected — slower, and not how Helix hosts LSPs.** |

**The fastest path is therefore: Helix's built-in native LSP client talking stdio to `verter-lsp`, configured once via `languages.toml`.** Native client ↔ native server, no proxy, no per-request indirection. There is nothing between the client and the server.

### 2.2 Position encoding — UTF-8 negotiated (zero-conversion, perf win)

Helix's built-in client advertises `general.positionEncodings = ['utf-8', 'utf-32', 'utf-16']` — **UTF-8 first** ([helix#5894, "Negotiate LSP Position Encoding"](https://github.com/helix-editor/helix/pull/5894)). `verter-lsp` negotiates **UTF-8 > UTF-32 > UTF-16** (`crates/verter_lsp/src/server/lifecycle.rs` `handle_initialize`, preferring UTF-8 because it is "native Rust encoding — no conversion needed"; default UTF-16 per spec when the client offers nothing). **Helix + verter-lsp therefore negotiate UTF-8 automatically** — the zero-conversion encoding on both ends (both are Rust, both store UTF-8 internally). No config action required.

### 2.3 Spawn model — one server per workspace root, lazy attach

Helix spawns **one `verter-lsp` process per workspace root** and reuses it for every `vue`/`svelte` buffer in that root; it does not spawn per-file. The server is launched lazily when the first matching buffer is opened. Workspace-root resolution is driven by the `[[language]].roots` markers and Helix's `.git`/`.helix` detection (§3.4); a stable root keeps a single server alive for the whole project.

### 2.4 cwd and root — Helix sets cwd = workspace root; server reads `workspaceFolders`

Two independent, mutually-reinforcing facts make root handling correct with **zero** config args:

1. **verter-lsp's authoritative root comes from the LSP `initialize` request's `workspaceFolders`**, not from argv or cwd — `handle_initialize` (`crates/verter_lsp/src/server/lifecycle.rs`) builds the VFS workspace directly from `params.workspace_folders`. The positional CLI root is only a fallback consulted by the type-provider startup, and is not in `LspConfig`. So as long as the client sends `workspaceFolders` (Helix does), the server operates on the correct root regardless of cwd.
2. **Helix spawns the LS process with cwd = workspace root** ([helix#13691, merged 2025-06-06](https://github.com/helix-editor/helix/pull/13691); previously the LS inherited the shell cwd — [helix#3993](https://github.com/helix-editor/helix/issues/3993)). So even the type-provider cwd fallback now points at the workspace root on current Helix.

**Consequence:** unlike the Neovim design (which uses a root-aware `cmd` function to append the resolved root positionally) and the Lapce design (which derives the root from `root_uri` and passes it positionally), **Helix needs no root argument at all** — Helix has no mechanism to inject a resolved root into argv anyway. `args = ["--type-provider=tsgo"]` is complete.

### 2.5 Performance defaults summary

| Knob | Default | Why |
|---|---|---|
| Integration mechanism | Built-in native LSP client via `languages.toml` | Only path; most direct; no proxy (§2.1). |
| Position encoding | UTF-8 (auto-negotiated) | Zero-conversion, both ends Rust (§2.2). |
| Process model | One server per workspace root, lazy attach | No per-file spawn (§2.3). |
| Workspace root | From `initialize.workspaceFolders` (+ Helix cwd=root) | No argv injection needed; correct by construction (§2.4). |
| Type provider | `tsgo` (self-discovering) | Native preview TS, self-contained (§3.3). |

## 3. The Helix `languages.toml` model (researched, with citations)

All facts verified against the Helix documentation, the helix repo's default `languages.toml`, and the helix config-merge loader (June 2026).

### 3.1 `[language-server.<name>]` fields

From [docs.helix-editor.com/languages.html](https://docs.helix-editor.com/languages.html):

| Field | Meaning |
|---|---|
| `command` | "The name or path of the language server binary to execute. Binaries must be in `$PATH`" (or an absolute path). |
| `args` | "A list of arguments to pass to the language server binary." |
| `config` | "Language server initialization options" — **maps directly to the LSP `initializationOptions`** sent at `initialize`. |
| `environment` | "Any environment variables that will be used when starting the language server" (e.g. `{ VERTER_LOG = "info" }`). |
| `timeout` | "The maximum time a request to the language server may take, in seconds. Defaults to `20`." |
| `required-root-patterns` | "A list of `glob` patterns to look for in the working directory. The language server is started if at least one of them is found." |

### 3.2 `[[language]]` fields & language-server assignment

`name`, `scope`, `file-types`, `roots` (workspace-root markers), `language-id`, `grammar`, `injection-regex`, and `language-servers`. The `language-servers` array accepts **strings** or **tables** `{ name, only-features, except-features }`:

```toml
language-servers = [ "verter" ]
# or feature-scoped:
language-servers = [ { name = "efm", only-features = ["format"] }, "verter" ]
```

**Feature priority** ([docs](https://docs.helix-editor.com/languages.html)): "Each requested LSP feature is prioritized in the order of the `language-servers` array." **Exception:** "The features `diagnostics`, `code-action`, `completion`, `document-symbols` and `workspace-symbols` … are working for all language servers at the same time and are merged together." This exception is decisive for the replace-vs-coexist question (§4.2).

### 3.3 Helix predefines `vue` and `svelte` — the merge is per-field by `name`

**Both `vue` and `svelte` are built-in Helix languages**, with built-in tree-sitter grammars and default language servers. From the helix repo's `languages.toml` (master):

```toml
[[language]]
name = "vue"
scope = "source.vue"
injection-regex = "vue"
file-types = ["vue"]
roots = ["package.json"]
block-comment-tokens = { start = "<!--", end = "-->" }
indent = { tab-width = 2, unit = "  " }
language-servers = [ "vuels" ]

[[language]]
name = "svelte"
scope = "source.svelte"
injection-regex = "svelte"
file-types = ["svelte"]
indent = { tab-width = 2, unit = "  " }
comment-token = "//"
block-comment-tokens = { start = "/*", end = "*/" }
language-servers = [ "svelteserver" ]

[[grammar]]
name = "vue"
source = { git = "https://github.com/ikatyang/tree-sitter-vue", rev = "91fe2754796cd8fba5f229505a23fa08f3546c06" }

[[grammar]]
name = "svelte"
source = { git = "https://github.com/themixednuts/tree-sitter-htmlx", rev = "80c3e698ec7379772c7f2aecb9b4b4c4ac52ff0b", subpath = "crates/tree-sitter-svelte" }
```

(Note the asymmetry: `vue` declares `roots = ["package.json"]`; `svelte` declares **no** `roots` field.)

**Critical merge fact:** Helix loads `languages.toml` across **built-in defaults < user `~/.config/helix/languages.toml` < project `.helix/languages.toml`**, and **merges `[[language]]` entries PER-FIELD, matched by `name`, via `merge_toml_values` (recursive merge)**. A user entry that sets only `language-servers` **preserves the built-in `grammar`, `scope`, `file-types`, `roots`, `indent`, comment tokens, etc.** `[language-server.*]` tables likewise merge additively by name. Verified against the Helix config-merge loader and corroborated by the community recipe wiki ([Language Server Configurations](https://github.com/helix-editor/helix/wiki/Language-Server-Configurations)), whose vue/svelte/html recipes use exactly the minimal `name` + `language-servers` form:

```toml
# representative community pattern (wiki) — minimal override preserves built-ins
[[language]]
name = "svelte"
language-servers = ["svelteserver"]
```

This means **the user does not need to redefine grammars, fetch tree-sitter parsers, or restate detection** — a minimal `[[language]]` override is structurally safe. This is the Helix analog of "Neovim core already supplies filetype detection."

### 3.4 Workspace-root detection

Helix resolves the workspace root from: the `[[language]].roots` markers, plus core detection of a parent directory containing `.git` / `.svn` / `.jj` / `.helix` ([docs/usage](https://docs.helix-editor.com/), [helix#3993](https://github.com/helix-editor/helix/issues/3993)). For `vue`, the built-in `roots = ["package.json"]` is inherited; for `svelte` (no built-in `roots`), Helix falls back to VCS-dir detection, which is correct for the common case. The resolved root is sent to the server as `initialize.workspaceFolders` and is also used as the spawn cwd (§2.4). The `[language-server.verter].required-root-patterns` field is a separate gate that decides **whether to start** the server (§4.3).

### 3.5 What the config mirrors from the VS Code client

Studied in `packages/vue-vscode/src/extension.ts` (`buildServerOptions` ~L1051-1102, `clientOptions.initializationOptions` ~L484-520), `crates/verter_lsp/src/main.rs`, `crates/verter_lsp/src/server/lifecycle.rs`, `crates/verter_lsp/src/config.rs`.

**Server launch (VS Code → Helix):** VS Code launches `verter-lsp --type-provider=<tp> --tsdk=<tsdk> --plugin-path=<node_modules> [--mcp-port=0 --mcp-lint-preset=<p>] <rootPath>` with `env.VERTER_LOG`. **Decisive simplifications for Helix:**

- **Use `--type-provider=tsgo`** (NOT `tgo`). `--type-provider` accepts `auto|tsgo|tsserver|off` only (`crates/verter_lsp/src/main.rs` `CliArgs::parse`). `tgo` is unrecognized and falls through to `auto`. The `tgo` spelling in the Neovim/Lapce docs is a typo this design corrects.
- **Omit `--tsdk` / `--plugin-path`.** They are consumed only by the tsserver path; `try_spawn_tsgo` ignores them and self-discovers the tsgo binary (`VERTER_TSGO_BIN` env → workspace `node_modules` → PATH → npm/npx cache). The user installs `@typescript/native-preview` per-project (the normal case for a Vue/Svelte project).
- **Omit the positional root** (§2.4) — Helix has no argv-injection mechanism, and the server reads the root from `workspaceFolders`.
- **Omit `--mcp-*`** (parsed-but-ignored; MCP ships separately).

Resulting default: `command = "verter-lsp"`, `args = ["--type-provider=tsgo"]`.

### 3.6 Initialization-options parity (the `config` table)

VS Code passes a rich `initializationOptions`; the server reads only a subset (`crates/verter_lsp/src/config.rs`; `lifecycle.rs` `handle_initialize`). The Helix `config` table mirrors **exactly the server-read subset** and drops VS-Code-UI-only fields.

| Init option | Server reads it? (where) | Helix `config` mapping |
|---|---|---|
| `lint: { enabled, preset }` | yes (`config::merge_init_options`) | `config.lint` |
| `inlayHints: { enabled }` | yes (`handle_initialize`, `inlay_hints_enabled`) | `config.inlayHints` |
| `viteConfig: { enabled, trustedFiles }` | yes (`handle_initialize`, `vite_config_options`) | `config.viteConfig` |
| `experimental: { conditionalRootNarrowing, strictSlots }` | yes (`config::parse_experimental_init_options`) | `config.experimental` |
| `hover: { provenance }` | yes (`config::parse_hover_init_options`) | `config.hover` |
| `statistics: { enabled }` | yes (`statistics.set_enabled`) | `config.statistics` — shipped explicitly OFF (opt-in telemetry); part of the server-read parity set, not omitted |
| `frameworks: ["vue","svelte"]` | informational only | omit (the carrier language is implied by which `[[language]]` attaches verter) |
| `configuration: { vue, typescript, css, … }` | **not parsed** by the server | **drop** — VS Code language-service settings; Helix has its own. |
| `decorations.*`, `mcp.*`, `$/verter/*` | VS-Code-only surfaces | **drop** — a plain LSP client never sends these; standard features flow unchanged. |

The shipped `config` is the COMPLETE server-read init parity set — the same six-key set the shared `verter_editor_client::build_initialization_options` emits: `lint`, `inlayHints`, `viteConfig`, `experimental`, `hover`, and `statistics` (off by default). The contract test asserts the parsed `config` equals `build_initialization_options(&json!({}))`, so the snippet cannot drift from the SSoT parity set. Genuinely-not-read keys (`configuration`/`decorations`/`mcp`/`frameworks`) are absent.

### 3.7 Client capabilities

Helix's built-in client builds standard `textDocument` capabilities at `initialize`. The verter config does **not** hand-author capabilities (Helix gives no `languages.toml` hook for them). The server reads the editor's `params.capabilities` in exactly one place — position-encoding negotiation (§2.2) — and `handle_initialize` advertises completion-resolve **only when the active type provider supports it** (`resolve_provider = tp.supports_completion_resolve()`), so auto-import `additionalTextEdits` apply when the client advertises resolve support (Helix's built-in client does). Semantic tokens are advertised `full = true` only (no `range`/`delta`) — full-document recompute; this is a server-side characteristic, not a Helix knob.

## 4. Chosen design — `editors/helix/`

### 4.1 Layout & workspace isolation

A new directory **`editors/helix/`** mirrors `editors/nvim/` as the home for a config-only editor. The live repo convention is: **compiled** editor clients live under `extensions/` (`extensions/lapce` is a `wasm32-wasip1` volt, `extensions/zed` a `wasm32-wasip2` cdylib), and **config-only** editors with a built-in native LSP client live under `editors/` (`editors/nvim` is the Lua config layer). Helix is config-only — its built-in native client is configured declaratively, with no compiled artifact — so it belongs in `editors/`, alongside `editors/nvim`. The directory contains **no Rust crate and no pnpm package** — a TOML snippet + docs — so it does not enter the Cargo workspace or the pnpm build graph.

```
editors/helix/
  languages.toml        # the shippable snippet users merge into ~/.config/helix/languages.toml
  README.md             # install steps, --type-provider=tsgo rationale, config parity, hx --health check
```

Roadmap items beyond v0 (managed download, upstreaming, the `hx --health` smoke, incremental tokens) live in [§9.1 Roadmap (out of v0 scope)](#91-roadmap-out-of-v0-scope), each with its external blocker.

The config-validation test (§7) lives at **`crates/verter-editor-client/tests/helix_config_contract.rs`**, reading `editors/helix/languages.toml` and asserting its fields against the **shared launch contract** — its anchor assertion is that the snippet's `args` equal `verter_editor_client::build_server_args(None, &json!({}))`, so a drift in the shared contract's provider flag fails the test until `languages.toml` is updated. The test is hermetic (no Helix binary, no `verter-lsp` process) and keeps `editors/helix/` config-only. (`verter-editor-client` is the pure SSoT launch-contract crate already shared by the Lapce and Zed clients; the test tying Helix to it lives there rather than in `crates/verter_lsp/tests/` so it asserts against that contract directly.)

### 4.2 The shipped `languages.toml` snippet

```toml
# Verter — Vue & Svelte language support for Helix.
# Merge this into ~/.config/helix/languages.toml (or a project-local .helix/languages.toml).
# Requires: `verter-lsp` on $PATH, and @typescript/native-preview (tsgo) installed
# in the project for full type features.

[language-server.verter]
command = "verter-lsp"
args = ["--type-provider=tsgo"]
# config = the server-read init parity set (the same set the shared
# build_initialization_options emits). statistics is server-read and shipped OFF
# by default (opt-in telemetry); see README §parity.
config = { lint = { enabled = false, preset = "recommended" }, inlayHints = { enabled = true }, viteConfig = { enabled = true, trustedFiles = [] }, experimental = { conditionalRootNarrowing = false, strictSlots = false }, hover = { provenance = false }, statistics = { enabled = false } }

# Attach verter to the (built-in) vue and svelte languages.
# These minimal overrides MERGE per-field with Helix's built-in entries:
# grammar, scope, file-types, roots are inherited — do NOT restate them.
[[language]]
name = "vue"
language-servers = ["verter"]

[[language]]
name = "svelte"
language-servers = ["verter"]
```

**Design decisions, justified (architect-ratified — `.feedback/_helix_arch_consult2.out`):**

- **(A) Override-only minimal `[[language]]` is the shipped default** (not full self-contained entries). The merge is confirmed per-field by `name` (§3.3), so restating `scope`/`file-types`/`grammar`/`roots` would only **freeze Helix-owned metadata in Verter's repo and create drift risk** when Helix updates grammars/scopes/roots/injections — with zero correctness benefit. The README documents the full-entry form **only as a troubleshooting appendix** for the (currently non-existent) case of a Helix version that does not merge per-field.
- **(B) Replace, not coexist** — `language-servers = ["verter"]`, dropping the built-in `vuels`/`svelteserver`. Per §3.2, `diagnostics`/`completion`/`code-action`/`document-symbols`/`workspace-symbols` are **merged across all attached servers simultaneously**; array order cannot prevent it, so attaching both verter and Volar/svelteserver would **double-publish diagnostics and double completions** (both are full Vue/Svelte servers covering the same surface). Verter is the intended single authority. Coexistence is documented **only** as expert customization via `only-features`/`except-features`, never shipped as default.
- **`config` is the COMPLETE server-read parity set** (§3.6) — no invented or ignored keys. An ignored config key "lies to users", so the table mirrors precisely the six keys the shared `build_initialization_options` emits: `lint`/`inlayHints`/`viteConfig`/`experimental`/`hover`/`statistics` (`statistics` server-read, shipped OFF by default). The contract test anchors the parsed `config` to `build_initialization_options(&json!({}))`, so it cannot drift.
- **No `required-root-patterns` in the default snippet.** It would prevent accidental startup in non-JS dirs, **but it harms single-file use and edge projects, and `package.json`/`tsconfig.json` are not authoritative for all Vue/Svelte workspaces** (e.g. a `.vue` opened standalone, or a non-standard project root). It is therefore documented in the README as an **optional hardening knob** for users who want stricter project gating — not a default. (This reverses an earlier draft that shipped it ON; the architect verdict is that always-attach is the correct default and gating is opt-in.)
- **No `environment`/`VERTER_LOG` and no `language-id` in the default snippet.** `VERTER_LOG` is a documented optional knob (commented example in the README), not a default. `language-id` is omitted entirely — Verter needs no non-default ID, and adding one creates another drift point.

### 4.3 README content

The README provides:

1. **Install:** put `verter-lsp` on `$PATH` (cargo install / release binary / absolute `command` path); install `@typescript/native-preview` in the project for full type features; merge the snippet into `~/.config/helix/languages.toml` (global) **or** a project-local `.helix/languages.toml` — **the project-local variant is the same minimal overlay**, not a different/full config.
2. **Verify:** run `hx --health vue` and `hx --health svelte` — the "Configured language server" line must show **`verter-lsp`** (in green) with its resolved binary path. (Note the historical `hx --health` limitation for multi-server languages, §3.2/§7 — with the replace design there is only one server, so the check is unambiguous.)
3. **The `--type-provider=tsgo` rationale** and the explicit **do-not-use-`tgo`** note.
4. **`config` parity table** — the exact server-read init options (§3.6), so users can tune `lint`/`inlayHints`/`viteConfig`/`experimental`/`hover`/`statistics` (`statistics` server-read, shipped OFF by default). (Stress: only these six keys are read; anything else is ignored.)
5. **Optional knobs (documented, not default):**
   - `required-root-patterns = ["package.json", "tsconfig.json"]` on `[language-server.verter]` to start verter-lsp only in JS/TS projects — for users who want stricter spawn gating, with the caveat that it suppresses single-file/edge-project use.
   - `environment = { VERTER_LOG = "info" }` (or `"debug"`) to set the log level.
6. **Advanced / troubleshooting:**
   - **Coexisting** with another server (e.g. an existing formatter LSP) via feature-scoping: `language-servers = [ { name = "verter", except-features = ["format"] }, "efm" ]`. Note that replacing the server list is correct for default LSP behavior, but **users with a custom formatter setup may need this feature filtering**.
   - The **full self-contained `[[language]]` fallback** — a troubleshooting appendix only, for an (unknown/ancient) Helix that does not merge per-field.
   - The auto-import / completion-resolve note (works with the built-in client; no extra config).

## 5. Distribution

- **In-repo `editors/helix/languages.toml` + README** (now). The snippet is small but non-trivial (the `tsgo` flag, the exact `config` parity, the replace decision, the documented opt-in knobs), so shipping a vetted, tested snippet + docs is worth it over a bare docs paragraph.
- **No managed binary download.** Helix convention (like Neovim's, unlike Lapce's volt) is that the editor does not fetch language-server binaries; the user installs `verter-lsp` via their package manager / cargo / a release asset, and points `command` at it (PATH name or absolute path). Loud failure if missing is surfaced by `hx --health` (red exe name) and Helix's LSP log.
- **Follow-up (optional):** upstreaming a `[language-server.verter]` default into Helix's built-in `languages.toml` is **not** pursued — Helix's built-in defaults point vue/svelte at Volar/svelteserver, and adding a third-party server to upstream defaults is not their model. The in-repo snippet is the supported channel.

## 6. Decomposition into implementation blocks

All blocks are in `editors/helix/` + one test file under `crates/verter-editor-client/tests/` (plus a one-line `[dev-dependencies]` add); **no existing crate's behavior is touched** (§8). Each block lands with discriminating tests (must FAIL pre-change, PASS post-change; no stubs/always-true asserts, per the project Stub-Prevention rule).

### Block H1 — the `languages.toml` snippet + config-validation test
- Author `editors/helix/languages.toml` (§4.2).
- Add `crates/verter-editor-client/tests/helix_config_contract.rs`: parse `editors/helix/languages.toml` with a TOML parser and assert the shipped contract, with the anchor assertion `args == build_server_args(None, &json!({}))`.
- **Discriminating tests** (hermetic, no editor, no server):
  - **Anchor (contract-sync):** the parsed `[language-server.verter].args` EQUAL `verter_editor_client::build_server_args(None, &serde_json::json!({}))` — the load-bearing tie to the shared launch contract (a provider-flag change in the contract fails this until `languages.toml` is updated).
  - `[language-server.verter].command == "verter-lsp"`.
  - `args` contains `"--type-provider=tsgo"` **and asserts `"--type-provider=tgo"` is ABSENT** and no element starts with `"--tsdk"` / `"--plugin-path"` (negative: the typo and the tsserver-only flags would be wrong).
  - `config` contains exactly the six server-read keys `lint`/`inlayHints`/`viteConfig`/`experimental`/`hover`/`statistics` and **omits** the genuinely-not-read `configuration`/`mcp`/`decorations`/`frameworks` (negative: VS-Code-only keys not forwarded).
  - **Config anchor (contract-sync):** the parsed `config` (as JSON) EQUALS `verter_editor_client::build_initialization_options(&serde_json::json!({}))` — the load-bearing tie to the shared init-options parity set (a parity-key add/remove or a default flip in the SSoT fails this until `languages.toml` is re-aligned).
  - Both `[[language]]` entries (`name = "vue"`, `name = "svelte"`) have `language-servers` EQUAL to `["verter"]` (discriminates the replace decision — an accidental extra attached server would reintroduce Helix's merged-feature double-publish).
  - A negative structural assertion that the `vue`/`svelte` `[[language]]` entries do **not** restate `grammar`/`scope`/`file-types`/`roots`/`injection-regex` and do **not** set `language-id` (guards the §3.3 minimal-override decision and the no-`language-id`-drift decision — restating them would be the wrong shape).
  - A negative assertion that the default snippet does **not** set `required-root-patterns` (guards the architect decision that gating is opt-in, not a shipped default).

### Block H2 — README + follow-ups
- `editors/helix/README.md` (§4.3: Helix install, the tsgo note, the `config` parity table, the `hx --health` verification, advanced usage). Beyond-v0 follow-ups are tracked in [§9.1 Roadmap (out of v0 scope)](#91-roadmap-out-of-v0-scope).
- A separate `docs/` guide page is a roadmap item, not part of v0 — the README is the canonical install doc.

### Block H3 (optional) — `hx --health` CI smoke
- A gated CI job that installs a pinned Helix, drops `editors/helix/languages.toml` into the Helix config dir, builds `verter-lsp`, puts it on PATH, and runs `hx --health vue` + `hx --health svelte`, asserting the output contains `verter-lsp` and a green/✓ status. **Gated** to skip when Helix or the binary is unavailable (matching how the zed / neovim / lapce jobs defer their real-server smoke). Tracked in [§9.1 Roadmap (out of v0 scope)](#91-roadmap-out-of-v0-scope).
- This is **lower-value** than H1 (the TOML-parse test already guards the shipped contract; `hx --health` only re-confirms Helix accepts it), so H3 is optional / a follow-up.

## 7. Test strategy (mandatory-rule compliant)

The project mandates automated tests for LSP/editor-integration changes (no manual-only verification). The work splits cleanly:

**Automated — the primary gate (TOML-parse contract test; hermetic, editor-independent):** `crates/verter-editor-client/tests/helix_config_contract.rs` parses the shipped `editors/helix/languages.toml` and asserts every field of the contract — its **anchor** assertion ties `args` to the shared launch contract (`args == build_server_args(None, &json!({}))`), plus `command == "verter-lsp"`, the `--type-provider=tsgo` arg + negative `tgo`/`--tsdk`/`--plugin-path`/positional absence, the exact `config` parity keys + negative VS-Code-only-key absence, `language-servers` = `verter` + negative built-in-server absence, the no-`required-root-patterns` default, and the minimal-override shape. It needs **no Helix binary and no `verter-lsp` process**, runs via `cargo test -p verter-editor-client --test helix_config_contract` (and inside the canonical workspace gate), and is fully discriminating (it fails against a snippet with the `tgo` typo, a missing parity key, a coexist mistake, or a dropped carrier language).

**Automated — headless `hx --health` smoke (feasible, gated, optional):** `hx --health <lang>` is Helix's scriptable inspection: it prints, for a language, the configured language-server executable name (green when found) + binary path, the grammar/highlight status, etc., to stdout. A CI job can therefore drop the snippet into the Helix config dir and assert `hx --health vue`/`svelte` shows `verter-lsp` (Block H3). **Caveat:** historically `hx --health <lang>` displayed only the **first** server for a multi-server language ([helix#8156](https://github.com/helix-editor/helix/issues/8156), later improved by [#7315](https://github.com/helix-editor/helix/pull/7315)); with the **replace** design (`language-servers = ["verter"]`) there is exactly one server, so the check is unambiguous regardless of Helix version. This is gated (skips without Helix / the binary), so CI without them stays green. It is a **confirmation** smoke, not the primary gate.

**Already covered by the server's own suite:** because the Helix config only *launches* the server, `verter-lsp`'s LSP behavior over stdio is **already** fully tested by `crates/verter_lsp/tests/` (in-process server, editor-independent; tsgo/tsserver assertions skip vacuously without `node_modules`). The Helix layer adds **no semantic surface** to re-test — only the launch/config contract, which the TOML-parse test covers.

**Manual (irreducible):** an interactive Helix UI smoke (open a `.vue`/`.svelte`, exercise hover/completion/rename/diagnostics, confirm auto-import edits apply) is a README checklist, not an automated gate.

## 8. Scope, dependencies, and decisions for the CTO/user

**Scope (confirmed):**
- A **new, non-overlapping** config-only artifact (`editors/helix/`) + docs + one hermetic test under `crates/verter-editor-client/tests/` (plus a one-line `toml` `[dev-dependencies]` add). **No existing crate's behavior touched.**
- **No server-side change.** `verter-lsp` already supports everything Helix needs — stdio LSP, UTF-8 negotiation (§2.2), `workspaceFolders`-driven root (§2.4), completion-resolve advertisement (§3.7), dynamic watcher registration. (This contrasts with the Lapce design's proposed `verterClient` handshake; the Helix path needs none.) So this block does **not** trip the "confirm before editing verter_session" rule and does not touch `verter_session` or any shared substrate.
- The config-validation test joins the existing canonical Rust gate; the optional `hx --health` smoke is a new gated CI job.

**Decisions for the CTO/user (not derivable from the plan):**
1. **`lint.enabled` default** — mirrors the VS Code default (`false`). Confirm parity (vs defaulting lint ON for Helix users). Recommend parity (`false`).
2. **Optional `hx --health` CI job (Block H3)** — confirm whether to add the gated Helix-matrix CI smoke now or defer (the TOML-parse test is the real gate). Recommend defer-or-optional.

(The `required-root-patterns` default-ON question from an earlier draft is **resolved**: the architect verdict is always-attach by default, with `required-root-patterns` as a documented opt-in knob — §4.2/§4.3. Not a CTO decision.)

**Toolchain prerequisites (documented, not blockers):** Helix (any version with the standard `languages.toml` model — vue/svelte built-ins exist; per-field merge confirmed); `verter-lsp` on `$PATH` or an absolute `command`; `@typescript/native-preview` (tsgo) in the project for full type features.

## 9. Open decisions / risks (summary)

- **`hx --health` CI smoke** optional/deferred — TOML-parse test is the primary gate. [CTO]
- **`lint.enabled` default** `false` (VS Code parity, recommended). [CTO]
- **`required-root-patterns`** is an opt-in README knob, NOT a default (architect-ratified) — always-attach is the default. [resolved]
- **Formatter interplay** — if a user already configured another server/formatter for vue/svelte, replacing the server list is correct for default LSP behavior, but they may need `only-features`/`except-features` filtering; documented in the README. [noted]
- **Semantic tokens** are full-document only (`full = true`, no range/delta) — a **server-side** characteristic, not a Helix knob; range/delta is tracked separately (out of scope). [follow-up, shared with nvim design]
- **svelte has no built-in `roots`** — Helix falls back to VCS-dir detection (correct for the common case); minimal overlay preserves the built-in, and `workspaceFolders` covers root resolution regardless. The asymmetry is a reason to **preserve** built-ins (Option A), not restate them. [noted, not a blocker]

## 9.1 Roadmap (out of v0 scope)

Enhancements intentionally **not** implemented in the config-only v0. Each item states its concrete external blocker. None is a defect in the shipped contract — they are gated on release engineering (published per-platform `verter-lsp` assets) or are server-side changes outside the `languages.toml` config layer.

- **No managed binary download.** Helix convention (like Neovim's, unlike Lapce's volt) is that the editor does not fetch language-server binaries; the user installs `verter-lsp` themselves and points `command` at it (PATH name or absolute path). *Blocker:* revisit only if Helix gains a managed-binary mechanism. Managed provisioning itself is tracked as the scheduled managed-binary-provisioning work, not owned here.
- **No upstream `[language-server.verter]` default in Helix's built-in `languages.toml`.** Helix's built-in defaults point vue/svelte at Volar/svelteserver, and adding a third-party server to upstream defaults is not their model — the in-repo snippet is the supported channel. *Blocker:* not pursued (upstream policy), not an asset/release gate.
- **Gated `hx --health` UI smoke.** A CI job that installs a pinned Helix, drops `editors/helix/languages.toml` into the config dir, puts a built `verter-lsp` on PATH, and asserts `hx --health vue` / `hx --health svelte` show `verter-lsp`. Lower value than the hermetic TOML-parse contract test (which already guards the shipped contract). *Blocker:* needs a pinned Helix available in CI; deferred, mirroring how the zed / neovim / lapce jobs defer their real-server smoke.
- **Incremental semantic tokens (range/delta).** `verter-lsp` currently advertises full-document tokens only; range/delta is a server-side enhancement shared with the Neovim design. *Blocker:* a future `verter_lsp` protocol change (server-side), not a Helix knob.

## 10. Citations

- Helix language/LSP configuration (`[language-server.*]`, `[[language]]`, `config`, `required-root-patterns`, feature priority + merged-feature exception): https://docs.helix-editor.com/languages.html
- Helix default `languages.toml` (built-in `vue`/`svelte` entries + grammars): https://github.com/helix-editor/helix/blob/master/languages.toml
- Helix config merge (per-field, by `name`, `merge_toml_values`): https://deepwiki.com/helix-editor/helix/4.1-language-configuration ; community recipes (minimal override form): https://github.com/helix-editor/helix/wiki/Language-Server-Configurations
- Helix LSP position-encoding negotiation (utf-8 first): https://github.com/helix-editor/helix/pull/5894
- Helix sets LSP process cwd = workspace root: https://github.com/helix-editor/helix/pull/13691 ; workspace-root vs LSP-root discussion: https://github.com/helix-editor/helix/issues/3993
- `hx --health` multi-server display limitation: https://github.com/helix-editor/helix/issues/8156 (fix https://github.com/helix-editor/helix/pull/7315)
- Verter sources studied: `packages/vue-vscode/src/extension.ts`, `crates/verter_lsp/src/main.rs`, `crates/verter_lsp/src/server/lifecycle.rs`, `crates/verter_lsp/src/config.rs`, `crates/verter_lsp/src/capabilities.rs`, `crates/verter_lsp/Cargo.toml`.
- Architect consult (un-primed codex, facts-embedded, full verdict): `.feedback/_helix_arch_consult2.md` (prompt) / `.feedback/_helix_arch_consult2.out` (verdict — ratifies Option A, replace-not-coexist, omit positional root, hermetic TOML test; rules `required-root-patterns` opt-in not default).
