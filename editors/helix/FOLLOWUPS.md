# Verter Helix — follow-ups

Tracked enhancements intentionally **not** implemented in v0. Each is gated on
release engineering (published per-platform `verter-lsp` assets) and/or is a
server-side change out of scope for the `languages.toml` config layer.

## Distribution

- **No managed binary download.** Helix convention (like Neovim's, unlike
  Lapce's volt) is that the editor does not fetch language-server binaries; the
  user installs `verter-lsp` themselves and points `command` at it (PATH name or
  absolute path). Revisit only if Helix gains a managed-binary mechanism.
- **Upstreaming a `[language-server.verter]` default into Helix's built-in
  `languages.toml` is not pursued.** Helix's built-in defaults point vue/svelte
  at Volar/svelteserver, and adding a third-party server to upstream defaults is
  not their model. The in-repo snippet is the supported channel.

## CI

- **Gated `hx --health` UI smoke.** A CI job that installs a pinned Helix, drops
  `editors/helix/languages.toml` into the config dir, puts a built `verter-lsp`
  on PATH, and asserts `hx --health vue` / `hx --health svelte` show
  `verter-lsp`. Lower value than the TOML-parse contract test (which already
  guards the shipped contract); it only re-confirms Helix accepts the snippet, so
  it is deferred — mirroring how the zed / neovim / lapce jobs defer their
  real-server smoke.

## Server-side (out of scope here)

- **Incremental semantic tokens (range/delta).** `verter-lsp` currently
  advertises full-document tokens only; range/delta is a server-side enhancement
  shared with the Neovim design.
