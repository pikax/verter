-- Verter Neovim LSP-client setup.
--
-- `require('verter').setup(opts)` builds a `vim.lsp.Config` for the native
-- `verter-lsp` stdio server and enables it. Neovim's BUILT-IN LSP client is the
-- transport — there is no proxy, no wrapper plugin, and no per-request Lua work:
-- the config is assembled once here and the native client talks stdio to the
-- native server directly.

local config = require("verter.config")

local M = {}

--- The exact `--type-provider` values the server's `CliArgs::parse` accepts
--- (`extension` is VS-Code-only and irrelevant to a Neovim launch). A value
--- outside this set — a typo like `tgo` — would be emitted verbatim and the
--- server would silently fall back to `auto`, degrading provider selection with
--- no error, so `setup` refuses to register on an invalid value (fail-closed,
--- mirroring the missing-binary path).
---
--- This is a DELIBERATE native-client superset of the SDK-less wasm clients'
--- `{tsgo, off}` clamp (Lapce/Zed): Neovim is a full editor that can supply a
--- TypeScript SDK on PATH and `tsserver` self-discovers its own install, so the
--- richer `auto | tsgo | tsserver | off` surface is an intentional advanced
--- capability here, not a divergence. The default still stays `tsgo` (self-
--- contained, discovers its own binary).
local VALID_TYPE_PROVIDERS = {
  auto = true,
  tsgo = true,
  tsserver = true,
  off = true,
}

--- Default options. Override any subset by passing a table to `setup`.
local DEFAULTS = {
  -- Binary name (PATH-discovered) or an absolute path. There is NO managed
  -- download — installing `verter-lsp` is mason.nvim's job or the user's
  -- package manager. See README "Binary discovery".
  cmd_path = "verter-lsp",
  -- Probe that the binary is resolvable before registering the client. Set
  -- false if the binary is provided by a wrapper / not on PATH at setup time.
  check_binary = true,
  -- Type provider. The server accepts auto|tsgo|tsserver|off; tsgo is self-
  -- contained (discovers its own binary) so no --tsdk/--plugin-path is needed.
  type_provider = "tsgo",
  -- Extra args inserted before the trailing positional root (which stays last).
  -- May not override --type-provider/--tsdk/--plugin-path (rejected at build).
  server_args = {},
  filetypes = { "vue", "svelte" },
  root_markers = {
    "tsconfig.json",
    "jsconfig.json",
    "vite.config.ts",
    "vite.config.js",
    "nuxt.config.ts",
    "svelte.config.js",
    "package.json",
    ".git",
  },
  -- didChangeWatchedFiles dynamicRegistration is OFF by default to avoid the
  -- node_modules recursive-watch CPU sink; the BufWritePost save-notify autocmd
  -- is the cheap external-freshness signal. Opt in for full external-edit
  -- freshness.
  watch_files = false,
  -- Semantic tokens are on by default (valuable for Vue/Svelte) but full-
  -- document only; set false for a one-line opt-out.
  semantic_tokens = true,
  -- Forwarded to the server via cmd_env.VERTER_LOG.
  log_level = "info",
  -- Init-option parity (the canonical six server-read keys only; mirrors
  -- verter_editor_client::build_initialization_options). `frameworks` is NOT a
  -- key: the server ignores it.
  lint = { enabled = false, preset = "recommended" },
  inlay_hints = { enabled = true },
  vite_config = { enabled = true, trusted_files = {} },
  experimental = { conditional_root_narrowing = false, strict_slots = false },
  hover = { provenance = false },
  -- The server defaults statistics OFF; mirror that default OFF here.
  statistics = { enabled = false },
  -- User completion-engine capabilities (blink.cmp / cmp_nvim_lsp). Merged in.
  capabilities = nil,
}

--- Resolve whether the configured binary is runnable.
---
--- `vim.fn.executable` is the single oracle for EVERY form — a bare name
--- (PATH-resolved), a POSIX/Windows absolute path, or a UNC path — and it already
--- accounts for existence, the executable bit, and platform suffixes (`.exe` via
--- `$PATHEXT` on Windows). An earlier version also `fs_stat`'d an absolute path,
--- but that defeated the suffix handling: `fs_stat("C:/tools/verter-lsp")` is nil
--- when the real file is `verter-lsp.exe`, so a perfectly runnable Windows path
--- given without `.exe` was wrongly rejected. Relying on `executable()` alone is
--- both correct and simpler — no absolute-vs-relative or `.exe` logic here.
---@param cmd_path string
---@return boolean
local function binary_is_available(cmd_path)
  return cmd_path ~= nil and cmd_path ~= "" and vim.fn.executable(cmd_path) == 1
end

--- Configure and enable the Verter LSP client.
---@param opts table|nil user options (see DEFAULTS)
function M.setup(opts)
  opts = vim.tbl_deep_extend("force", DEFAULTS, opts or {})

  -- Reject an unknown type_provider BEFORE registering anything: an invalid
  -- value would otherwise be emitted as `--type-provider=<typo>` and silently
  -- fall back to `auto` on the server. Fail-closed (refuse to register) so a
  -- typo cannot silently degrade the language service.
  if not VALID_TYPE_PROVIDERS[opts.type_provider] then
    vim.notify(
      table.concat({
        "[verter] invalid type_provider '" .. tostring(opts.type_provider) .. "'.",
        "Expected one of: auto | tsgo | tsserver | off.",
        "The client was NOT registered.",
      }, " "),
      vim.log.levels.ERROR
    )
    return
  end

  -- Robustness-fallback filetype detection. Neovim core already maps
  -- .vue -> vue and .svelte -> svelte; this idempotent re-assertion protects
  -- users on unusual/older runtimes or with conflicting overrides.
  vim.filetype.add({ extension = { vue = "vue", svelte = "svelte" } })

  -- Fail loudly (no managed download) if the binary cannot be found, and do
  -- NOT register a broken client.
  if opts.check_binary and not binary_is_available(opts.cmd_path) then
    vim.notify(
      table.concat({
        "[verter] language server '" .. tostring(opts.cmd_path) .. "' not found.",
        "Install the `verter-lsp` binary and ensure it is on your PATH, or pass",
        "`require('verter').setup({ cmd_path = '/abs/path/to/verter-lsp' })`.",
        "(A mason.nvim registry entry is a planned follow-up.)",
      }, " "),
      vim.log.levels.ERROR
    )
    return
  end

  vim.lsp.config("verter", {
    cmd = config.build_cmd(opts),
    filetypes = opts.filetypes,
    root_markers = opts.root_markers,
    cmd_env = { VERTER_LOG = opts.log_level },
    init_options = config.build_init_options(opts),
    capabilities = config.build_capabilities(opts),
    on_attach = config.on_attach(opts),
    -- Tolerate single-file usage: the server falls back to cwd when no root.
    workspace_required = false,
  })

  vim.lsp.enable("verter")
end

return M
