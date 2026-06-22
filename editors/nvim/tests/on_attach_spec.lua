-- Pure-Lua unit tests for `verter.config.on_attach` behaviors.
--
-- No running server: the returned `on_attach(client, bufnr)` is exercised
-- against a FAKE client table and a scratch buffer, so the save-notify autocmd
-- gating and the semantic-tokens opt-out are asserted directly.

local config = require("verter.config")

local function merged(overrides)
  local base = {
    cmd_path = "verter-lsp",
    type_provider = "tsgo",
    server_args = {},
    filetypes = { "vue", "svelte" },
    root_markers = { "package.json", ".git" },
    watch_files = false,
    semantic_tokens = true,
    log_level = "info",
    lint = { enabled = false, preset = "recommended" },
    inlay_hints = { enabled = true },
    vite_config = { enabled = true, trusted_files = {} },
    experimental = { conditional_root_narrowing = false, strict_slots = false },
    hover = { provenance = false },
    frameworks = { "vue", "svelte" },
    capabilities = nil,
  }
  return vim.tbl_deep_extend("force", base, overrides or {})
end

-- A minimal stand-in for a real `vim.lsp.Client`: it RECORDS every notify call
-- (method + params) and carries a `server_capabilities` table the opt-out can
-- mutate. `id` is parameterized so multi-client tests get distinct per-client
-- augroups.
local function fake_client(id)
  local c = {
    id = id or 4242,
    name = "verter",
    notifications = {},
    server_capabilities = {
      semanticTokensProvider = { full = true },
    },
  }
  function c:notify(method, params)
    table.insert(self.notifications, { method = method, params = params })
    return true
  end
  function c:is_stopped()
    return false
  end
  return c
end

-- Collect BufWritePost autocmds registered against a freshly created augroup.
local function buf_write_post_autocmds()
  return vim.api.nvim_get_autocmds({ event = "BufWritePost" })
end

local function has_verter_save_notify_autocmd()
  for _, au in ipairs(buf_write_post_autocmds()) do
    local group = au.group_name or ""
    if group:match("[Vv]erter") then
      return true
    end
  end
  return false
end

-- Delete every per-client verter save-notify augroup so tests do not leak
-- autocmds into one another. Per-client groups are named
-- `verter_save_notify_<id>`.
local function clear_verter_save_notify_groups()
  for _, au in ipairs(buf_write_post_autocmds()) do
    local group = au.group_name or ""
    if group:match("^verter_save_notify") then
      pcall(vim.api.nvim_del_augroup_by_name, group)
    end
  end
  -- Also clear the legacy shared name if some impl created it.
  pcall(vim.api.nvim_del_augroup_by_name, "verter_save_notify")
end

describe("verter.config.on_attach save-notify autocmd", function()
  before_each(clear_verter_save_notify_groups)
  after_each(clear_verter_save_notify_groups)

  it("registers a BufWritePost *.js,*.ts autocmd when watch_files is off", function()
    local bufnr = vim.api.nvim_create_buf(false, true)
    local on_attach = config.on_attach(merged({ watch_files = false }))
    on_attach(fake_client(), bufnr)

    assert.is_true(
      has_verter_save_notify_autocmd(),
      "expected a verter BufWritePost autocmd when watchers are off"
    )

    -- The autocmd must target *.js / *.ts patterns.
    local patterns = {}
    for _, au in ipairs(buf_write_post_autocmds()) do
      if (au.group_name or ""):match("[Vv]erter") then
        patterns[au.pattern] = true
      end
    end
    assert.is_true(patterns["*.js"] == true or patterns["*.ts"] == true)
  end)

  it("does NOT register the save-notify autocmd when watch_files is on", function()
    local bufnr = vim.api.nvim_create_buf(false, true)
    local on_attach = config.on_attach(merged({ watch_files = true }))
    on_attach(fake_client(), bufnr)

    assert.is_false(
      has_verter_save_notify_autocmd(),
      "watch_files=true must rely on dynamic watchers, not the save-notify autocmd"
    )
  end)

  it("notifies $/onFileChanged {type='update'} for a saved .ts file", function()
    local bufnr = vim.api.nvim_create_buf(false, true)
    local client = fake_client()
    local on_attach = config.on_attach(merged({ watch_files = false }))
    on_attach(client, bufnr)

    -- Fire a BufWritePost for a .ts path; the *.ts pattern matches and the
    -- callback runs with args.file = the path. (Use a path with no real file;
    -- the callback only reads args.file.)
    local file = "/tmp/verter_save_notify_probe.ts"
    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = file })

    assert.are.equal(1, #client.notifications, "exactly one notify on a single .ts save")
    local n = client.notifications[1]
    -- These assertions FAIL against the old `$/onDidChangeTsOrJsFile`+`{uri}`
    -- impl (wrong method, no `type`) and against an empty-callback impl (no
    -- notify at all).
    assert.are.equal("$/onFileChanged", n.method)
    assert.are.equal("update", n.params.type)
    assert.are.equal(vim.uri_from_fname(file), n.params.uri)
    -- Negative: the undeserializable in-editor-delta method must NOT be used.
    assert.are_not.equal("$/onDidChangeTsOrJsFile", n.method)
  end)

  it("notifies for a .js save too (pattern covers *.js)", function()
    local bufnr = vim.api.nvim_create_buf(false, true)
    local client = fake_client()
    config.on_attach(merged({ watch_files = false }))(client, bufnr)

    local file = "/tmp/verter_save_notify_probe.js"
    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = file })

    assert.are.equal(1, #client.notifications)
    assert.are.equal("$/onFileChanged", client.notifications[1].method)
    assert.are.equal(vim.uri_from_fname(file), client.notifications[1].params.uri)
  end)

  it("does NOT notify for a saved .vue or .css file (pattern is *.js/*.ts only)", function()
    local bufnr = vim.api.nvim_create_buf(false, true)
    local client = fake_client()
    config.on_attach(merged({ watch_files = false }))(client, bufnr)

    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = "/tmp/Comp.vue" })
    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = "/tmp/styles.css" })

    -- A too-broad pattern (e.g. "*") would notify here. Discriminates that bug.
    assert.are.equal(
      0,
      #client.notifications,
      "save-notify must fire only for *.js/*.ts, not .vue/.css"
    )
  end)
end)

describe("verter.config.on_attach multi-client save-notify", function()
  -- Holder for any test that stubs `vim.lsp.get_buffers_by_client_id`. The
  -- detach-lifecycle test sets this to the captured original; `after_each`
  -- restores it on EVERY exit path (including a thrown assertion) so the stub
  -- never leaks into a later test. Tests that never stub leave it nil and the
  -- restore is a no-op.
  local saved_get_buffers_by_client_id = nil
  before_each(clear_verter_save_notify_groups)
  after_each(function()
    if saved_get_buffers_by_client_id ~= nil then
      vim.lsp.get_buffers_by_client_id = saved_get_buffers_by_client_id
      saved_get_buffers_by_client_id = nil
    end
    clear_verter_save_notify_groups()
  end)

  it("notifies BOTH clients (one per root) on a .ts save — no augroup clobber", function()
    local on_attach = config.on_attach(merged({ watch_files = false }))
    local c1 = fake_client(1)
    local c2 = fake_client(2)
    -- Two verter clients attach (e.g. two workspace roots). A single shared
    -- cleared augroup capturing only the latest client would leave c1 with NO
    -- registration; this test asserts BOTH still receive the save notify.
    on_attach(c1, vim.api.nvim_create_buf(false, true))
    on_attach(c2, vim.api.nvim_create_buf(false, true))

    -- Per-client augroups are distinct.
    local groups = {}
    for _, au in ipairs(buf_write_post_autocmds()) do
      local g = au.group_name or ""
      if g:match("^verter_save_notify") then
        groups[g] = true
      end
    end
    assert.is_true(groups["verter_save_notify_1"] == true, "client 1's per-client augroup must exist")
    assert.is_true(groups["verter_save_notify_2"] == true, "client 2's per-client augroup must exist")

    local file = "/tmp/verter_multi_probe.ts"
    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = file })

    -- The clobber bug would leave one of these at 0.
    assert.are.equal(1, #c1.notifications, "client 1 must still be notified after client 2 attaches")
    assert.are.equal(1, #c2.notifications, "client 2 must be notified")
    assert.are.equal("$/onFileChanged", c1.notifications[1].method)
    assert.are.equal("$/onFileChanged", c2.notifications[1].method)
  end)

  it("re-attaching the same client id does not stack duplicate notifies", function()
    local on_attach = config.on_attach(merged({ watch_files = false }))
    local c = fake_client(7)
    -- Two attaches of the SAME id (e.g. a re-attach). The per-client augroup is
    -- re-created with clear=true, so the prior autocmd is dropped and a single
    -- save fires exactly one notify (not two).
    on_attach(c, vim.api.nvim_create_buf(false, true))
    on_attach(c, vim.api.nvim_create_buf(false, true))

    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = "/tmp/verter_reattach.ts" })

    assert.are.equal(1, #c.notifications, "same-id re-attach must not stack duplicate notifies")
  end)

  it("keeps notifying after one buffer detaches; tears down only at last detach", function()
    -- One verter client (one workspace root) attached to TWO buffers. Closing
    -- the FIRST buffer fires `LspDetach` once for that buffer only — the client
    -- still has the second buffer open, so the per-client augroup (and its
    -- shared BufWritePost notify) MUST survive. The too-aggressive impl deleted
    -- the augroup on the first per-buffer detach, killing save-notify for the
    -- client's remaining buffers; step 1 below discriminates exactly that bug.
    local on_attach = config.on_attach(merged({ watch_files = false }))
    local c = fake_client(11)
    local buf1 = vim.api.nvim_create_buf(false, true)
    local buf2 = vim.api.nvim_create_buf(false, true)
    on_attach(c, buf1)
    on_attach(c, buf2)

    -- The fake client is not a real LSP client, so stub the buffer-tracking API
    -- the fix consults. `remaining` is the controllable "buffers still attached"
    -- list. Capture the original into the describe-scoped holder so the block's
    -- `after_each` restores it on EVERY path (including an assertion throwing
    -- mid-test) — the stub never leaks into a later test.
    saved_get_buffers_by_client_id = vim.lsp.get_buffers_by_client_id
    local remaining = { buf1, buf2 }
    vim.lsp.get_buffers_by_client_id = function(id)
      if id == c.id then
        return remaining
      end
      return {}
    end

    -- Step 1: buf1 detaches; buf2 still remains. The deferred teardown check
    -- must see one remaining buffer and KEEP the augroup, so a subsequent .ts
    -- save STILL notifies. (The fix defers via vim.schedule; drain it.)
    remaining = { buf2 }
    vim.api.nvim_exec_autocmds("LspDetach", {
      data = { client_id = c.id },
    })
    vim.wait(50, function()
      return false
    end) -- flush scheduled callbacks (deferred teardown check)

    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = "/tmp/verter_remaining.ts" })
    -- FAILS against the per-buffer-delete impl: it deleted the augroup on buf1's
    -- detach, so this save would record 0 notifies.
    assert.are.equal(
      1,
      #c.notifications,
      "augroup must survive a per-buffer detach while another buffer remains"
    )

    -- Step 2: buf2 (the last buffer) detaches; none remain. The deferred check
    -- now sees zero remaining buffers and tears the augroup down. Cleanup STILL
    -- happens at true client stop.
    remaining = {}
    vim.api.nvim_exec_autocmds("LspDetach", {
      data = { client_id = c.id },
    })
    vim.wait(50, function()
      return false
    end)

    assert.is_false(
      has_verter_save_notify_autocmd(),
      "augroup must be gone once the client's last buffer detaches"
    )
    -- Stronger than the BufWritePost-only check above: assert the WHOLE
    -- per-client augroup (its BufWritePost notify AND its LspDetach autocmd) is
    -- gone. A deleted augroup makes `nvim_get_autocmds{group=...}` raise
    -- ("invalid group") or yield zero autocmds; either outcome means fully torn
    -- down. A still-present LspDetach-only autocmd (BufWritePost cleared but the
    -- group left alive) would pass the weaker check yet fail this one.
    local group_name = "verter_save_notify_" .. c.id
    local ok, autocmds = pcall(vim.api.nvim_get_autocmds, { group = group_name })
    assert.is_true(
      not ok or #autocmds == 0,
      "the per-client augroup (BufWritePost + LspDetach) must be fully deleted at last detach"
    )
    -- A save after the last detach must NOT notify (count stays at 1).
    vim.api.nvim_exec_autocmds("BufWritePost", { pattern = "/tmp/verter_gone.ts" })
    assert.are.equal(
      1,
      #c.notifications,
      "no notify after the client's augroup is torn down at last detach"
    )
  end)
end)

describe("verter.config.on_attach semantic-tokens opt-out", function()
  it("clears server semanticTokensProvider when semantic_tokens is false", function()
    local bufnr = vim.api.nvim_create_buf(false, true)
    local client = fake_client()
    assert.is_not_nil(client.server_capabilities.semanticTokensProvider)

    local on_attach = config.on_attach(merged({ semantic_tokens = false }))
    on_attach(client, bufnr)

    assert.is_nil(client.server_capabilities.semanticTokensProvider)
  end)

  it("leaves a pre-set semanticTokensProvider untouched when enabled", function()
    local bufnr = vim.api.nvim_create_buf(false, true)
    local client = fake_client()

    local on_attach = config.on_attach(merged({ semantic_tokens = true }))
    on_attach(client, bufnr)

    assert.is_not_nil(client.server_capabilities.semanticTokensProvider)
    assert.are.equal(true, client.server_capabilities.semanticTokensProvider.full)
  end)
end)
