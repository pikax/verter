# Verter Neovim — follow-ups

Tracked enhancements intentionally **not** implemented in v0. Each is gated on
release engineering (published per-platform `verter-lsp` assets) and/or is a
server-side change out of scope for the Lua config layer.

## Distribution

- **Upstream `lsp/verter.lua` in `neovim/nvim-lspconfig`.** Widest reach for
  Neovim users; couples to nvim-lspconfig's release cadence. Pursue once
  `verter-lsp` has stable public install instructions. The runtime path is
  unchanged — `lsp/*.lua` is just a config-table source for the same built-in
  client.
- **mason.nvim registry entry.** Enables `:MasonInstall verter-lsp` auto-install.
  Requires published per-platform release assets + checksums (mason owns the
  download/SHA logic; the Lua module deliberately does not).
- **Standalone `verter.nvim` mirror.** A separate installable repo so lazy.nvim
  users can `{ "verter-dev/verter.nvim" }` without the monorepo. Mirror of
  `editors/nvim/`.

## Defaults / behavior

- **`watch_files` stays OFF by default.** The opt-in is documented
  ([README → File watching](./README.md#file-watching-watch_files-default-off)).
  Revisit only if a `node_modules`-ignoring watcher path lands.

## Server-side (out of scope here)

- **Incremental semantic tokens (range/delta).** `verter-lsp` currently
  advertises full-document tokens only; range/delta is a server-side enhancement
  that would let the Neovim default stay on with lower cost.
