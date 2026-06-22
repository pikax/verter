-- Plugin entry point for the Verter Neovim LSP client.
--
-- This file is loaded automatically when the module is on the runtimepath, but
-- it deliberately performs NO side effects: it does NOT call
-- `require('verter').setup()`, register a config, or attach a client. That keeps
-- the module friendly to lazy.nvim / packer (no surprise attach on load) and
-- leaves activation entirely in the user's control.
--
-- To activate, call (e.g. in your init.lua or a lazy.nvim `config` function):
--
--     require('verter').setup({})
--
-- See editors/nvim/README.md for full options and copy-paste recipes.

if vim.g.loaded_verter then
  return
end
vim.g.loaded_verter = true
