# Verter for Helix

LSP-client configuration for the [Verter](../../README.md) language server
(`verter-lsp`) — type-aware IDE features for Vue and Svelte single-file
components in Helix.

Helix has a **built-in native LSP client** (Rust, in-process), so "Helix
support" is a pure `languages.toml` config layer. There is no compiled extension
and no server-side change: the native client talks stdio directly to the native
`verter-lsp` process. The config does **zero per-request work** — Helix reads it
once at startup and spawns one `verter-lsp` per workspace root.

## Prerequisites

- `verter-lsp` on your `PATH` (or set `command` to an absolute path in the
  snippet). There is **no managed download** — install it via your package
  manager / `cargo build -p verter_lsp --release`. Helix does not fetch
  language-server binaries.
- For full type features, the project should have **TypeScript 7 (`typescript@7`)**
  installed (the normal case for a typed Vue/Svelte project). The default type
  provider is `tsgo`, which discovers the native
  `@typescript/typescript-<platform>-<arch>` binary installed by that package
  (`VERTER_TSGO_BIN` → project `node_modules` → `PATH` → npm/npx cache).

## Install

Merge [`languages.toml`](./languages.toml) into your Helix config. You have two
equivalent options:

- **Global:** append it to `~/.config/helix/languages.toml`.
- **Project-local:** append it to a `.helix/languages.toml` at the project root.

Both use the **same minimal overlay** — the project-local form is not a
different/fuller config. Helix loads `languages.toml` across **built-in defaults
< user `~/.config/helix/languages.toml` < project `.helix/languages.toml`** and
merges `[[language]]` entries **per-field, matched by `name`**. A minimal entry
that sets only `language-servers` therefore **preserves Helix's built-in
grammar, scope, file-types, and roots** for `vue`/`svelte` — the override is
structurally safe and never freezes Helix-owned metadata in this repo.

## Verify

Run Helix's health check for each carrier language:

```bash
hx --health vue
hx --health svelte
```

The "Configured language server" line must show **`verter-lsp`** (in green) with
its resolved binary path. Because the snippet **replaces** the server list with
a single server, there is exactly one configured server per language, so the
check is unambiguous on every Helix version.

## The `--type-provider=tsgo` flag

The snippet launches `verter-lsp --type-provider=tsgo`. `tsgo` is the native
TypeScript 7 provider; it self-discovers the platform binary installed by the
`typescript` package and needs no TypeScript SDK passed on the command line.

> **Use `tsgo`, NOT `tgo`.** The `--type-provider` flag accepts
> `auto | tsgo | tsserver | off` only. `tgo` is **not** a recognized value and
> silently falls through to `auto` — defeating native-provider selection. Always
> spell it `tsgo`.

This matches the shared launch contract: Helix sends the workspace root via the
`initialize` request's `workspaceFolders` (not via argv), so the snippet passes
**no positional root** — `args = ["--type-provider=tsgo"]` is complete.

## `config` parity

The `config` table maps directly to the LSP `initializationOptions`. It is the
**server-read init parity set** — the same keys the shared
`build_initialization_options` emits (`lint`, `inlayHints`, `viteConfig`,
`experimental`, `hover`, `statistics`) — so the config and the launch contract
cannot drift. `statistics.enabled` is **server-read** (`verter-lsp` reads
`initializationOptions.statistics.enabled`) and shipped **OFF by default**:
telemetry is opt-in — flip it to `true` to enable. The genuinely
omitted/not-read keys are the VS-Code-UI-only surfaces `configuration`, `mcp`,
`decorations`, and `frameworks`.

| `config` key                            | Meaning                                   | Default in the snippet   |
| --------------------------------------- | ----------------------------------------- | ------------------------ |
| `lint.enabled`                          | enable the Verter linter                  | `false` (VS Code parity) |
| `lint.preset`                           | lint preset when enabled                  | `"recommended"`          |
| `inlayHints.enabled`                    | inlay hints                               | `true`                   |
| `viteConfig.enabled`                    | read `vite.config.*` for resolution       | `true`                   |
| `viteConfig.trustedFiles`               | trusted Vite config files                 | `[]`                     |
| `experimental.conditionalRootNarrowing` | experimental root narrowing               | `false`                  |
| `experimental.strictSlots`              | strict slot typing                        | `false`                  |
| `hover.provenance`                      | show provenance in hovers                 | `false`                  |
| `statistics.enabled`                    | emit server statistics (opt-in telemetry) | `false`                  |

Tune any of these in your own `languages.toml`; only these keys are read.

## Optional knobs (documented, not default)

These are **not** in the shipped snippet — add them yourself if you want them.

- **Stricter spawn gating** — start `verter-lsp` only in JS/TS projects by adding
  to `[language-server.verter]`:

  ```toml
  required-root-patterns = ["package.json", "tsconfig.json"]
  ```

  Caveat: this **suppresses single-file and edge-project use** (a `.vue` opened
  standalone, or a non-standard project root). It is off by default because
  always-attach is the correct default; gating is opt-in.

- **Log level** — set the server log level via the environment:

  ```toml
  [language-server.verter]
  environment = { VERTER_LOG = "info" }   # or "debug"
  ```

## Position encoding

Helix's built-in client advertises `positionEncodings = ['utf-8', 'utf-32',
'utf-16']` (UTF-8 first) and `verter-lsp` prefers UTF-8, so the two negotiate the
**zero-conversion** encoding automatically (both ends are Rust and store UTF-8
internally). No action required.

## v0 limitations

- **No managed binary download.** Install `verter-lsp` yourself; Helix does not
  fetch language-server binaries.
- **Semantic tokens are full-document only** (`full = true`, no range/delta) — a
  server-side characteristic, not a Helix knob. Range/delta is tracked
  separately.

## Advanced / troubleshooting

### Coexisting with another server (formatter, etc.)

Replacing the server list is correct for default LSP behavior. If you already run
a dedicated formatter LSP for vue/svelte, scope features per server instead of a
bare replace:

```toml
[[language]]
name = "vue"
language-servers = [ { name = "verter", except-features = ["format"] }, "efm" ]
```

Helix prioritizes each requested feature in `language-servers` order, **except**
`diagnostics` / `code-action` / `completion` / `document-symbols` /
`workspace-symbols`, which are merged across all attached servers — so attaching
two full Vue/Svelte servers would double-publish diagnostics and completions.
Use `only-features` / `except-features` to avoid that.

### Full self-contained `[[language]]` fallback (last resort)

The per-field merge preserves Helix's built-in metadata, so the minimal override
is the right shape on every current Helix. **Only** if you run a Helix that does
not merge per-field, restate the full entry:

```toml
[[language]]
name = "vue"
scope = "source.vue"
injection-regex = "vue"
file-types = ["vue"]
roots = ["package.json"]
language-servers = ["verter"]

[[language]]
name = "svelte"
scope = "source.svelte"
injection-regex = "svelte"
file-types = ["svelte"]
language-servers = ["verter"]
```

This freezes Helix-owned grammar/scope/file-types/roots in your config and can
drift when Helix updates them — hence it is a troubleshooting appendix, not the
default.

### Auto-import / completion-resolve

Auto-import works with Helix's built-in client and **no extra config**:
`verter-lsp` advertises completion-resolve when the active type provider supports
it, and returns auto-import edits as `additionalTextEdits` applied during
`completionItem/resolve`, which the built-in client requests.

## Running the tests

The shipped contract is guarded by a hermetic Rust test that parses this
directory's `languages.toml` and asserts it against the shared launch contract
(`verter_editor_client::build_server_args`) — it needs **no Helix binary and no
`verter-lsp` process**:

```bash
cargo test -p verter-editor-client --test main cases::helix_config_contract
```

CI runs the parsed config contract across an OS matrix. Its Linux real-client
lane also installs checksum-verified Helix 25.07.1, installs the shipped snippet
unchanged, and fails unless `hx --health vue` and `hx --health svelte` resolve
only the executable `verter-lsp`. It then exports the parsed command, arguments,
and initialization options and requires real Vue/Svelte diagnostics and typed
hover through the shared stdio LSP client.
