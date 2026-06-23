-- Pure configuration builders for the Verter Neovim LSP client.
--
-- Every function here is a pure function over a merged `opts` table and returns
-- a plain value (an argument list, a function, or a table). None of them call
-- `vim.lsp.config` / `vim.lsp.enable` or otherwise mutate global editor state,
-- so the whole decision surface is unit-testable in headless Neovim without a
-- running `verter-lsp` binary.
--
-- The Lua layer does ZERO per-request work: these builders run once, at attach.
-- All request-time work happens inside Neovim's native (C/Lua) LSP client and
-- the native `verter-lsp` process.

local M = {}

--- Server-owned flags `server_args` may never override.
---
--- `--type-provider` is owned by `opts.type_provider` (a second one would make
--- the server's last-wins parse silently ignore the validated value); `--tsdk` /
--- `--plugin-path` are tsserver-only knobs the self-contained `tsgo` provider
--- does not use and that the config intentionally never emits. Injecting any of
--- these through `server_args` would either break provider selection or smuggle
--- in a contract-breaking flag, so they are rejected loudly at build time.
local FORBIDDEN_SERVER_ARG_PATTERNS = {
  "^%-%-type%-provider",
  "^%-%-tsdk",
  "^%-%-plugin%-path",
}

--- Build the resolved argument list `cmd` will spawn for a given root.
---
--- Shape: `{ <binary>, "--type-provider=<tp>" [, <server_args...>] [, <root_dir>] }`.
--- The workspace root is passed as the STRICTLY LAST POSITIONAL argument (the
--- server's `CliArgs::parse` treats any non-`--` arg as the root, falling back to
--- the process cwd when absent). It is appended only when non-empty so the cwd
--- fallback stays intact for single-file / unresolved-root buffers. `server_args`
--- are inserted BEFORE the root so a stray positional in `server_args` cannot
--- displace it and the root stays parseable as the trailing positional.
---
--- NOTE: the type provider is `tsgo` (the server accepts auto|tsgo|tsserver|off;
--- the literal `tgo` silently falls through to `auto`). `--tsdk` / `--plugin-path`
--- are tsserver-only and the tsgo provider discovers its own binary, so they are
--- never emitted here — and `server_args` may not re-introduce them
--- (see FORBIDDEN_SERVER_ARG_PATTERNS).
---@param opts table merged options
---@param root_dir string|nil resolved workspace root (config.root_dir)
---@return string[] args
function M.build_cmd_args(opts, root_dir)
  local args = { opts.cmd_path, "--type-provider=" .. opts.type_provider }
  for _, extra in ipairs(opts.server_args or {}) do
    for _, pattern in ipairs(FORBIDDEN_SERVER_ARG_PATTERNS) do
      if type(extra) == "string" and extra:match(pattern) then
        error(
          "[verter] server_args may not override --type-provider/--tsdk/--plugin-path"
            .. " (got " .. tostring(extra) .. "); set type_provider instead."
        )
      end
    end
    args[#args + 1] = extra
  end
  -- Root stays STRICTLY last so the server parses it as the trailing positional.
  if type(root_dir) == "string" and root_dir ~= "" then
    args[#args + 1] = root_dir
  end
  return args
end

--- Build the root-aware `cmd` function for `vim.lsp.Config.cmd`.
---
--- `cmd` may be a string list OR a function `(dispatchers, config)` returning the
--- RPC client. The function form is used so the resolved `config.root_dir`
--- (populated by `vim.lsp.enable`'s root resolution) can be appended positionally
--- — mirroring the VS Code launch, which always passes the precise root. Failing
--- closed: an unresolved root still launches the server (cwd fallback).
---
--- The function form is also why `VERTER_LOG` must be threaded through the
--- `rpc.start` spawn params here: the top-level `cmd_env` config field is only
--- auto-applied by Neovim when `cmd` is a STRING LIST. With a function `cmd`,
--- nvim does not see `cmd_env`, so the env is passed explicitly via the spawn
--- params' `env` table (merged onto the inherited environment by `rpc.start`).
---@param opts table merged options
---@return fun(dispatchers: table, config: table): table
function M.build_cmd(opts)
  return function(dispatchers, config)
    local args = M.build_cmd_args(opts, config and config.root_dir or nil)
    return vim.lsp.rpc.start(args, dispatchers, {
      cwd = config and config.cmd_cwd or nil,
      -- Forward the log level the function-form `cmd` would otherwise drop.
      env = { VERTER_LOG = opts.log_level },
    })
  end
end

--- Build the `init_options` table sent on `initialize`.
---
--- Emits exactly the six camelCase wire keys the server reads
--- (`lifecycle.rs::handle_initialize`, `config.rs`) and nothing else:
--- `lint`, `inlayHints`, `viteConfig`, `experimental`, `hover`, `statistics`.
--- This is the canonical init-options parity set shared with every other Verter
--- editor client (`verter_editor_client::build_initialization_options`); the Rust
--- drift-guard `crates/verter-editor-client/tests/nvim_config_contract.rs` binds
--- this builder to that SSoT. VS-Code-UI-only surfaces (`configuration`, `mcp`,
--- `decorations`) are intentionally omitted: the server never reads them and a
--- plain LSP client has its own language-service settings. `frameworks` is NOT
--- emitted — the server ignores it, so it was dead protocol surface.
---@param opts table merged options
---@return table init_options
function M.build_init_options(opts)
  return {
    lint = {
      enabled = opts.lint.enabled,
      preset = opts.lint.preset,
    },
    inlayHints = {
      enabled = opts.inlay_hints.enabled,
    },
    viteConfig = {
      enabled = opts.vite_config.enabled,
      trustedFiles = opts.vite_config.trusted_files or {},
    },
    experimental = {
      conditionalRootNarrowing = opts.experimental.conditional_root_narrowing,
      strictSlots = opts.experimental.strict_slots,
    },
    hover = {
      provenance = opts.hover.provenance,
    },
    -- The server reads `initializationOptions.statistics` (`statistics.set_enabled`)
    -- and defaults it OFF when absent; emit it explicitly so a user opt-in is honored.
    statistics = {
      enabled = opts.statistics.enabled,
    },
  }
end

--- Build the client capabilities table.
---
--- Starts from the built-in `make_client_capabilities()` (so nvim's defaults —
--- UTF-8-first position encodings, `completionItem.resolveSupport`, etc. — are
--- always present even when the user passes only a PARTIAL caps table), then
--- deep-merges a COPY of the user's capabilities (e.g.
--- `blink.cmp.get_lsp_capabilities()` or `cmp_nvim_lsp.default_capabilities()`)
--- on top, then FORCES `workspace.didChangeWatchedFiles.dynamicRegistration` to
--- `opts.watch_files` (default false). The user's table is `deepcopy`'d before
--- the merge so this builder never mutates the caller's capabilities table.
--- Defaulting watchers OFF avoids the documented node_modules recursive-watch CPU
--- sink; the save-notify autocmd (see `on_attach`) is the cheap cross-file
--- freshness signal when watchers are off.
---@param opts table merged options
---@return table capabilities
function M.build_capabilities(opts)
  local base = vim.lsp.protocol.make_client_capabilities()
  local user = vim.deepcopy(opts.capabilities or {})
  local caps = vim.tbl_deep_extend("force", base, user)
  caps.workspace = caps.workspace or {}
  caps.workspace.didChangeWatchedFiles = caps.workspace.didChangeWatchedFiles or {}
  caps.workspace.didChangeWatchedFiles.dynamicRegistration = (opts.watch_files == true)
  return caps
end

--- Build the `on_attach(client, bufnr)` callback.
---
--- Two behaviors, both driven by `opts`:
---  * When `watch_files` is false, register a `BufWritePost` autocmd for
---    `*.js` / `*.ts` that notifies the server via `$/onFileChanged`
---    (`{ uri, type = "update" }`). That handler re-reads the file from the
---    workspace VFS, which is exactly the external-freshness semantic a save
---    needs. (`$/onDidChangeTsOrJsFile` is the in-editor-DELTA method — its
---    params require a `changes` array of edits, which a `BufWritePost` save does
---    not carry — so it is NOT used here.) This restores a low-cost external
---    freshness signal without broad recursive watchers. When `watch_files` is
---    true the dynamic watcher already covers this, so the autocmd is skipped.
---  * When `semantic_tokens` is false, clear
---    `client.server_capabilities.semanticTokensProvider` so Neovim does not
---    auto-start full-document semantic-token highlighting.
---
--- The save-notify autocmd lives in a PER-CLIENT augroup keyed by `client.id`
--- (`verter_save_notify_<id>`) and is torn down only once that client has no
--- remaining attached buffers (or is stopped) — NOT on every per-buffer
--- `LspDetach`, since one client attaches to many buffers and a single buffer
--- close must not kill the client's notify registration for its other open
--- buffers. Attaching multiple verter clients (one per workspace root) does not
--- let a later attach clobber an earlier client's notify registration — each
--- client keeps notifying for the lifetime of its own attachment. Re-creating
--- the per-client augroup with `clear = true` on a same-id re-attach drops the
--- prior registration before adding the new one, so duplicates never stack.
---@param opts table merged options
---@return fun(client: table, bufnr: integer)
function M.on_attach(opts)
  return function(client, bufnr)
    if not opts.semantic_tokens then
      if client.server_capabilities then
        client.server_capabilities.semanticTokensProvider = nil
      end
    end

    if not opts.watch_files then
      local group_name = "verter_save_notify_" .. tostring(client.id)
      local group = vim.api.nvim_create_augroup(group_name, { clear = true })
      vim.api.nvim_create_autocmd("BufWritePost", {
        group = group,
        pattern = { "*.js", "*.ts" },
        desc = "verter: notify server of external TS/JS file change ($/onFileChanged)",
        callback = function(args)
          -- Guard: the client may have stopped between attach and save.
          if client.is_stopped and client:is_stopped() then
            return
          end
          -- `$/onFileChanged` { uri, type } re-reads from the workspace VFS —
          -- the external-freshness signal. `"update"` maps server-side to
          -- WorkspaceChange::FileChanged { source = None } (VFS re-read).
          client:notify("$/onFileChanged", {
            uri = vim.uri_from_fname(args.file),
            type = "update",
          })
        end,
      })
      -- Tear the per-client augroup down only when THIS client is fully gone, so
      -- a stopped client's autocmd does not linger and fire for an unrelated
      -- save. `LspDetach` fires ONCE PER BUFFER, and a single verter client (one
      -- workspace root) attaches to many `.vue`/`.svelte` buffers; deleting the
      -- augroup on the first per-buffer detach would drop the shared
      -- `BufWritePost` notify (and this very `LspDetach` handler, since both live
      -- in the same group) while OTHER buffers of the client are still open. So
      -- only delete once no attached buffers remain for the client. Defer with
      -- `vim.schedule` because `LspDetach` can fire BEFORE the buffer is removed
      -- from the client's tracking, so the remaining-buffer count is only
      -- accurate after the event settles.
      vim.api.nvim_create_autocmd("LspDetach", {
        group = group,
        callback = function(detach_args)
          if not (detach_args.data and detach_args.data.client_id == client.id) then
            return
          end
          vim.schedule(function()
            local stopped = client.is_stopped and client:is_stopped()
            -- `vim.lsp.get_buffers_by_client_id(id)` -> list of bufnrs attached
            -- to the client (nvim 0.11). Guard for older runtimes lacking it:
            -- without a buffer count we can only safely tear down once the
            -- client is stopped, so fall back to the `is_stopped` check alone.
            local get_bufs = vim.lsp.get_buffers_by_client_id
            if get_bufs then
              local remaining = get_bufs(client.id) or {}
              if stopped or #remaining == 0 then
                pcall(vim.api.nvim_del_augroup_by_name, group_name)
              end
            elseif stopped then
              pcall(vim.api.nvim_del_augroup_by_name, group_name)
            end
          end)
        end,
      })
    end
  end
end

return M
