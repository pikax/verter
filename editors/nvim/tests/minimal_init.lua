-- Minimal init for running the verter Neovim test suite headlessly under
-- plenary.nvim's busted harness.
--
-- Run from the repo root:
--   nvim --headless -c "PlenaryBustedDirectory editors/nvim/tests/ \
--     {minimal_init='editors/nvim/tests/minimal_init.lua'}" -c "qa!"
--
-- plenary.nvim must be discoverable on the runtimepath. This init looks for it
-- in (in order):
--   1. $PLENARY_PATH                       (CI exports this — see the workflow)
--   2. the standard packpath location      ~/.local/share/nvim/site/pack/*/start/plenary.nvim
--   3. whatever is already on the rtp       (a user who runs the suite locally)

-- Resolve this module's own `lua/` directory so `require('verter...')` works
-- regardless of the cwd the suite is launched from. This file lives at
-- editors/nvim/tests/minimal_init.lua; the module root is two levels up.
local this_file = debug.getinfo(1, "S").source:sub(2)
local tests_dir = vim.fn.fnamemodify(this_file, ":p:h")
local module_root = vim.fn.fnamemodify(tests_dir, ":h") -- editors/nvim

-- Normalize so a path captured with native (backslash) separators on Windows
-- still resolves on the runtimepath.
vim.opt.runtimepath:prepend(vim.fs.normalize(module_root))

-- Add plenary to the runtimepath.
local function add_plenary()
  local env_path = vim.env.PLENARY_PATH
  if env_path ~= nil and env_path ~= "" then
    -- Normalize so a Windows path passed via Git-bash on CI (mixed/back slashes)
    -- resolves on the runtimepath.
    vim.opt.runtimepath:prepend(vim.fs.normalize(env_path))
    return true
  end

  -- Standard packpath fallback. Build the path with vim.fs.joinpath so the
  -- separator is correct on every OS (no hardcoded `/`).
  local pack_root = vim.fs.joinpath(vim.fn.stdpath("data"), "site", "pack")
  local candidates = vim.fn.globpath(
    pack_root,
    vim.fs.joinpath("*", "start", "plenary.nvim"),
    false,
    true
  )
  if candidates and #candidates > 0 then
    vim.opt.runtimepath:prepend(vim.fs.normalize(candidates[1]))
    return true
  end

  -- Maybe it is already on the rtp (local dev with plenary installed).
  return pcall(require, "plenary.busted")
end

if not add_plenary() then
  error(
    "[verter tests] plenary.nvim not found. Set $PLENARY_PATH to a plenary "
      .. "checkout, or install it under the standard packpath."
  )
end

-- Quiet, deterministic test environment.
vim.opt.swapfile = false
vim.opt.more = false
vim.cmd("runtime plugin/plenary.vim")
