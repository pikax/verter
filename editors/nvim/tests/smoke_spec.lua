-- @ai-generated - Real Neovim client contract for Verter's shipped Lua setup.
--
-- This is deliberately one fail-closed scenario: it requires the provisioned
-- server and fixture toolchain, loads `require("verter").setup`, and exercises
-- Neovim's built-in LSP client against Vue/Svelte in both TS and JS modes.

local init = require("verter")

local diagnostic_publications = dofile(
  vim.fs.joinpath(
    vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h"),
    "support",
    "diagnostic_publications.lua"
  )
)

local function tests_dir()
  return vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
end

local function fixture_path(name)
  return vim.fs.joinpath(tests_dir(), "fixtures", "real-client", name)
end

local function read_lines(bufnr)
  return vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
end

local function position_of(bufnr, needle, occurrence)
  occurrence = occurrence or 1
  local seen = 0
  for line_index, line in ipairs(read_lines(bufnr)) do
    local from = 1
    while true do
      local start = line:find(needle, from, true)
      if not start then break end
      seen = seen + 1
      if seen == occurrence then
        return { line = line_index - 1, character = start - 1 }
      end
      from = start + #needle
    end
  end
  error("fixture token not found: " .. needle .. " occurrence " .. occurrence)
end

local function request(client, bufnr, method, params)
  local response, err = client:request_sync(method, params, 30000, bufnr)
  assert.is_nil(err, method .. " transport error: " .. vim.inspect(err))
  assert.is_not_nil(response, method .. " returned no response")
  assert.is_nil(response.err, method .. " LSP error: " .. vim.inspect(response.err))
  return response.result
end

local function hover_text(hover, fixture)
  assert.is_not_nil(hover, fixture .. " hover must return a payload")
  local contents = hover.contents
  if type(contents) == "string" then return contents end
  if type(contents) == "table" and type(contents.value) == "string" then
    return contents.value
  end
  local chunks = {}
  for _, item in ipairs(contents or {}) do
    chunks[#chunks + 1] = type(item) == "table" and (item.value or "") or tostring(item)
  end
  return table.concat(chunks, "\n")
end

local function completion_items(result)
  if result == nil then return {} end
  return result.items or result
end

local function definition_locations(result)
  if result == nil then return {} end
  if result.uri or result.targetUri then return { result } end
  return result
end

local function range_text(bufnr, range)
  local line = read_lines(bufnr)[range.start.line + 1]
  return line:sub(range.start.character + 1, range["end"].character)
end

local function diagnostic_code(diag)
  if type(diag.code) == "table" then return diag.code.value end
  return diag.code
end

local live_clients = {}

local function stop_clients()
  if #live_clients == 0 then return end
  vim.lsp.stop_client(live_clients, false)
  local stopped = vim.wait(10000, function()
    for _, id in ipairs(live_clients) do
      local client = vim.lsp.get_client_by_id(id)
      if client ~= nil and not client:is_stopped() then return false end
    end
    return true
  end, 50)
  assert.is_true(stopped, "Verter client must complete a clean LSP shutdown")
  live_clients = {}
end

describe("verter real Neovim client contract", function()
  after_each(stop_clients)

  it("serves Vue/Svelte JS+TS semantics through exactly one UTF-8 client", function()
    local bin = vim.env.VERTER_LSP_BIN
    assert.is_not_nil(bin, "VERTER_LSP_BIN must be provisioned by the real-client gate")
    assert.are_not.equal("", bin)
    assert.are.equal(1, vim.fn.executable(bin), "VERTER_LSP_BIN must be executable")
    local tsgo_bin = vim.env.VERTER_TSGO_BIN
    assert.is_not_nil(tsgo_bin, "VERTER_TSGO_BIN must pin the fixture's TypeScript 7 engine")
    assert.are_not.equal("", tsgo_bin)
    assert.are.equal(1, vim.fn.executable(tsgo_bin), "VERTER_TSGO_BIN must be executable")
    local expected_root = vim.fs.dirname(fixture_path("package.json"))

    local ready_generations = {}
    local sync_generations = {}
    local started_providers = {}
    local shown_messages = {}
    local old_ready = vim.lsp.handlers["$/verter/ready"]
    local old_sync = vim.lsp.handlers["$/verter/typeProviderSyncComplete"]
    local old_provider_started = vim.lsp.handlers["$/verter/typeProviderStarted"]
    local old_publish_diagnostics = vim.lsp.handlers["textDocument/publishDiagnostics"]
    local old_show_message = vim.lsp.handlers["window/showMessage"]
    local published_diagnostics = diagnostic_publications.new()
    -- Record every server-shown message for the crash-absence contract below.
    vim.lsp.handlers["window/showMessage"] = function(err, params, ctx, config)
      if params and type(params.message) == "string" then
        shown_messages[#shown_messages + 1] = params.message
      end
      if old_show_message then return old_show_message(err, params, ctx, config) end
    end
    vim.lsp.handlers["$/verter/ready"] = function(_, params)
      if params and params.gen ~= nil then ready_generations[params.gen] = true end
    end
    vim.lsp.handlers["$/verter/typeProviderSyncComplete"] = function(_, params)
      if params and params.gen ~= nil then sync_generations[params.gen] = true end
    end
    vim.lsp.handlers["$/verter/typeProviderStarted"] = function(_, params)
      if params and params.kind ~= nil then started_providers[params.kind] = true end
    end
    vim.lsp.handlers["textDocument/publishDiagnostics"] = published_diagnostics:wrap(
      old_publish_diagnostics
    )

    local ok, failure = pcall(function()
      init.setup({ cmd_path = bin, check_binary = true, log_level = "error" })

      local cases = {
        { file = "VueTs.vue", token = "vueTsTitle", occurrence = 2 },
        { file = "VueJs.vue", token = "vueJsTitle", occurrence = 2 },
        { file = "SvelteTs.svelte", token = "svelteTsTitle", occurrence = 2 },
        { file = "SvelteJs.svelte", token = "svelteJsTitle", occurrence = 2 },
      }

      local opened = {}
      local one_client_id = nil
      for _, case in ipairs(cases) do
        vim.cmd.edit(vim.fn.fnameescape(fixture_path(case.file)))
        local bufnr = vim.api.nvim_get_current_buf()
        opened[#opened + 1] = { bufnr = bufnr, case = case }
        local attached = vim.wait(30000, function()
          return #vim.lsp.get_clients({ name = "verter", bufnr = bufnr }) == 1
        end, 50)
        assert.is_true(attached, "exactly one Verter client must attach to " .. case.file)
        local client = vim.lsp.get_clients({ name = "verter", bufnr = bufnr })[1]
        one_client_id = one_client_id or client.id
        assert.are.equal(one_client_id, client.id, "all fixture buffers must share one root client")
        assert.is_not_nil(client.root_dir, "Verter must resolve a workspace root for " .. case.file)
        assert.are.equal(
          vim.fs.normalize(expected_root):lower(),
          vim.fs.normalize(client.root_dir):lower(),
          "Verter must attach at the pinned fixture root"
        )
        assert.are.equal("utf-8", client.offset_encoding, "Neovim and Verter must negotiate UTF-8")
      end
      live_clients = { one_client_id }
      local client = assert(vim.lsp.get_client_by_id(one_client_id))

      local semantic_ready = vim.wait(60000, function()
        for gen in pairs(ready_generations) do
          if sync_generations[gen] then return true end
        end
        return false
      end, 50)
      assert.is_true(semantic_ready, "ready and type-provider sync must match one generation")
      assert.is_true(
        started_providers.tsgo == true,
        "the pinned TypeScript 7 tsgo provider must start before semantic assertions"
      )

      for _, opened_case in ipairs(opened) do
        local bufnr = opened_case.bufnr
        local case = opened_case.case
        local uri = vim.uri_from_bufnr(bufnr)
        local diagnostics_ready = vim.wait(30000, function()
          return published_diagnostics:has(uri)
        end, 50)
        assert.is_true(
          diagnostics_ready,
          case.file .. " must publish diagnostics before the clean-diagnostic assertion"
        )
        local position = position_of(bufnr, case.token, case.occurrence)
        local hover = request(client, bufnr, "textDocument/hover", {
          textDocument = { uri = uri },
          position = position,
        })
        local text = hover_text(hover, case.file)
        assert.is_true(text:find("string", 1, true) ~= nil, case.file .. " hover must be string")
        assert.is_nil(text:match("%f[%a]any%f[%A]"), case.file .. " hover must not degrade to any")
        assert.is_nil(text:match("%f[%a]unknown%f[%A]"), case.file .. " hover must not be unknown")
        assert.is_nil(text:find("__Verter", 1, true), case.file .. " hover must not expose a shim")

        for _, diag in ipairs(vim.diagnostic.get(bufnr)) do
          assert.are_not.equal(7026, tonumber(diagnostic_code(diag)), case.file .. " must have no TS7026")
        end
      end

      local primary = opened[1]
      local primary_uri = vim.uri_from_bufnr(primary.bufnr)
      local definition = request(client, primary.bufnr, "textDocument/definition", {
        textDocument = { uri = primary_uri },
        position = position_of(primary.bufnr, "vueTsTitle", 2),
      })
      local locations = definition_locations(definition)
      assert.is_true(#locations > 0, "template symbol must resolve to an authored definition")
      local authored = false
      for _, location in ipairs(locations) do
        local uri = location.uri or location.targetUri
        local range = location.range or location.targetSelectionRange
        if uri == primary_uri and range and range.start.line == 1 then authored = true end
        assert.is_nil(uri:find(".vue.tsx", 1, true), "definition must not leak generated carrier URIs")
      end
      assert.is_true(authored, "definition must land on the authored script declaration")

      local completion = request(client, primary.bufnr, "textDocument/completion", {
        textDocument = { uri = primary_uri },
        position = position_of(primary.bufnr, "exactCompletion", 1),
        context = { triggerKind = 1 },
      })
      local exact_matches = 0
      for _, item in ipairs(completion_items(completion)) do
        if item.label == "exactCompletion" then exact_matches = exact_matches + 1 end
      end
      assert.are.equal(1, exact_matches, "completion must contain exactly one exact authored label")

      local rename = request(client, primary.bufnr, "textDocument/rename", {
        textDocument = { uri = primary_uri },
        position = position_of(primary.bufnr, "vueTsTitle", 2),
        newName = "renamedTitle",
      })
      assert.is_not_nil(rename, "markup rename must return a workspace edit")
      assert.is_not_nil(rename.changes, "rename must use authored URI edits")
      local edits = rename.changes[primary_uri] or {}
      assert.are.equal(2, #edits, "rename must update script declaration and template use")
      for _, edit in ipairs(edits) do
        assert.are.equal("vueTsTitle", range_text(primary.bufnr, edit.range))
        assert.are.equal("renamedTitle", edit.newText)
      end
      for uri in pairs(rename.changes) do
        assert.is_nil(uri:find(".vue.tsx", 1, true), "rename must not edit generated carriers")
      end
    end)

    vim.lsp.handlers["$/verter/ready"] = old_ready
    vim.lsp.handlers["$/verter/typeProviderSyncComplete"] = old_sync
    vim.lsp.handlers["$/verter/typeProviderStarted"] = old_provider_started
    vim.lsp.handlers["textDocument/publishDiagnostics"] = old_publish_diagnostics
    stop_clients()
    vim.lsp.handlers["window/showMessage"] = old_show_message
    if not ok then error(failure) end

    -- CRASH-ABSENCE CONTRACT: the engine must survive the ENTIRE scenario —
    -- every hover (including the Svelte-JS carrier) AND the clean client
    -- shutdown — without the resilient monitor ever reporting a crash. A
    -- "crashed. Restarting" notification here means either the generated
    -- carrier payload killed the engine or a deliberate teardown was
    -- misreported as a crash; both are release-blocking defects.
    for _, message in ipairs(shown_messages) do
      assert.is_nil(
        message:find("crashed. Restarting", 1, true),
        "the tsgo engine must never crash or be misreported as crashed: " .. message
      )
      assert.is_nil(
        message:find("verter-only mode", 1, true),
        "the engine must never degrade to verter-only mode: " .. message
      )
    end
  end)
end)
