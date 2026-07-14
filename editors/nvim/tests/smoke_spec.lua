-- Headless real-server LSP-attach smoke test.
--
-- GATED on the BINARY: this spec drives a REAL `verter-lsp` over stdio, so it can
-- only run where the binary is installed. When the binary is absent it SKIPS
-- VACUOUSLY via plenary's `pending` with a logged reason — mirroring the Rust e2e
-- tsgo/tsserver suites that skip without `node_modules`. It is never an empty test
-- body and never asserts-true; an absent binary yields a logged skip, a present
-- one yields a real attach assertion. This is an ATTACH smoke: it asserts the
-- client attaches and a request dispatches; full type results depend on a
-- `tsgo` / `node_modules` toolchain that may be absent even when the binary
-- exists, so they are not asserted here.

local config = require("verter.config")
local init = require("verter")

-- The binary the config would spawn. Honour $VERTER_LSP_BIN so CI / users can
-- point at a built debug/release binary without it being on PATH.
local function resolve_binary()
  local explicit = vim.env.VERTER_LSP_BIN
  if explicit ~= nil and explicit ~= "" then
    return explicit
  end
  return "verter-lsp"
end

local function binary_available(bin)
  return vim.fn.executable(bin) == 1
end

-- Absolute path to the fixture .vue next to this spec.
local function fixture_path()
  local this_file = debug.getinfo(1, "S").source:sub(2)
  local tests_dir = vim.fn.fnamemodify(this_file, ":p:h")
  return vim.fs.joinpath(tests_dir, "fixtures", "App.vue")
end

describe("verter real-server attach smoke", function()
  it("attaches a verter client to an opened .vue buffer", function()
    local bin = resolve_binary()
    if not binary_available(bin) then
      -- Vacuous, LOGGED skip — not an empty body.
      pending(
        "skipping real-server smoke: '"
          .. bin
          .. "' is not executable (install verter-lsp or set $VERTER_LSP_BIN)"
      )
      return
    end

    -- Configure against the resolved binary; the probe is redundant here since
    -- we already checked, so disable it to avoid a double notify.
    init.setup({ cmd_path = bin, check_binary = false, log_level = "error" })

    local file = fixture_path()
    vim.cmd.edit(vim.fn.fnameescape(file))
    local bufnr = vim.api.nvim_get_current_buf()
    assert.are.equal("vue", vim.bo[bufnr].filetype)

    -- Wait up to 20s for a verter client to attach to this buffer.
    local attached = vim.wait(20000, function()
      local clients = vim.lsp.get_clients({ name = "verter", bufnr = bufnr })
      return #clients > 0
    end, 100)

    assert.is_true(
      attached,
      "expected a 'verter' LSP client to attach to the opened .vue buffer"
    )

    local clients = vim.lsp.get_clients({ name = "verter", bufnr = bufnr })
    assert.is_true(#clients >= 1)

    -- Best-effort dispatch probe: fire a hover at the `count` ref and assert
    -- only that the request is DISPATCHABLE without error (no await, no result
    -- assertion). Full type results depend on a `tsgo` / `node_modules`
    -- toolchain that may be absent even when the binary exists, so asserting a
    -- hover payload here would be flaky; the attach assertion above is the real
    -- smoke signal.
    local client = clients[1]
    local params = {
      textDocument = { uri = vim.uri_from_bufnr(bufnr) },
      position = { line = 3, character = 6 },
    }
    local ok = pcall(function()
      client:request("textDocument/hover", params, function() end, bufnr)
    end)
    assert.is_true(ok, "hover request should be dispatchable without error")

    -- Clean up the spawned server so the suite does not leak a process.
    vim.lsp.stop_client(vim.lsp.get_clients({ name = "verter" }), true)
  end)
end)
