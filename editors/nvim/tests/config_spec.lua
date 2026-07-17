-- Pure-Lua unit tests for the verter Neovim config builders.
--
-- These run under plenary.nvim's busted harness in headless Neovim and need NO
-- running `verter-lsp` binary: every builder in `verter.config` is a pure
-- function over a merged `opts` table returning a plain table, so the whole
-- decision surface is exercised here in milliseconds.
--
-- Run:
--   nvim --headless -c "PlenaryBustedDirectory editors/nvim/tests/ \
--     {minimal_init='editors/nvim/tests/minimal_init.lua'}" -c "qa!"

local config = require("verter.config")
local init = require("verter")

-- Mirror of verter.init DEFAULTS so the builder tests run against the same
-- shape `setup()` would merge. `merged()` lets each test override a single key
-- without restating the whole table.
local function merged(overrides)
  local base = {
    cmd_path = "verter-lsp",
    type_provider = "tsgo",
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
    watch_files = false,
    semantic_tokens = true,
    log_level = "info",
    lint = { enabled = false, preset = "recommended" },
    inlay_hints = { enabled = true },
    vite_config = { enabled = true, trusted_files = {} },
    experimental = { conditional_root_narrowing = false, strict_slots = false },
    hover = { provenance = false },
    statistics = { enabled = false },
    capabilities = nil,
  }
  return vim.tbl_deep_extend("force", base, overrides or {})
end

local function index_of(list, value)
  for i, v in ipairs(list) do
    if v == value then
      return i
    end
  end
  return nil
end

local function contains(list, value)
  return index_of(list, value) ~= nil
end

describe("verter.config.build_cmd_args", function()
  it("uses --type-provider=tsgo by default (never tgo)", function()
    local args = config.build_cmd_args(merged(), nil)
    -- The binary is first.
    assert.are.equal("verter-lsp", args[1])
    -- Discriminates a `tsgo` -> `tgo` regression: the literal `tgo` value
    -- silently falls through to `auto` on the server and breaks provider
    -- selection, so its absence is load-bearing.
    assert.is_true(contains(args, "--type-provider=tsgo"))
    assert.is_false(contains(args, "--type-provider=tgo"))
    -- `--tsdk` / `--plugin-path` are tsserver-only flags that the tgo path
    -- ignores; they must never be emitted by the default tsgo config.
    for _, a in ipairs(args) do
      assert.is_nil(a:match("^%-%-tsdk"))
      assert.is_nil(a:match("^%-%-plugin%-path"))
    end
  end)

  it("honours an explicit type_provider override", function()
    local args = config.build_cmd_args(merged({ type_provider = "tsserver" }), nil)
    assert.is_true(contains(args, "--type-provider=tsserver"))
    assert.is_false(contains(args, "--type-provider=tsgo"))
  end)

  it("respects a custom cmd_path as the binary", function()
    local args = config.build_cmd_args(merged({ cmd_path = "/opt/bin/verter-lsp" }), nil)
    assert.are.equal("/opt/bin/verter-lsp", args[1])
  end)

  it("appends the resolved root as the trailing positional when set", function()
    local args = config.build_cmd_args(merged(), "/home/u/project")
    -- The root is positional (no `--` prefix) and last.
    assert.are.equal("/home/u/project", args[#args])
    assert.is_nil(args[#args]:match("^%-%-"))
    -- It must appear AFTER the type-provider flag.
    assert.is_true(index_of(args, "/home/u/project") > index_of(args, "--type-provider=tsgo"))
  end)

  it("omits the positional root when root_dir is unset (cwd fallback)", function()
    local args = config.build_cmd_args(merged(), nil)
    -- Last arg is still the flag, not a stray positional.
    assert.are.equal("--type-provider=tsgo", args[#args])
  end)

  it("omits the positional root when root_dir is an empty string", function()
    local args = config.build_cmd_args(merged(), "")
    assert.are.equal("--type-provider=tsgo", args[#args])
  end)

  it("places server_args BEFORE the positional root (root stays last)", function()
    local args = config.build_cmd_args(merged({ server_args = { "--extra" } }), "/p")
    -- Order: binary, flag, extra args, THEN root. The root must remain the
    -- trailing positional so the server parses it as the workspace root; a
    -- server_args entry appended AFTER the root would either displace it or be
    -- mis-parsed as the root. Discriminates an append-after-root impl.
    assert.is_true(index_of(args, "--extra") < index_of(args, "/p"))
    assert.are.equal("/p", args[#args])
  end)

  it("rejects a server_args entry that overrides --type-provider", function()
    -- A second --type-provider would make the server's last-wins parse silently
    -- ignore the validated opts.type_provider. Must raise at build time.
    -- Discriminates a no-validation impl (which would happily emit two flags).
    local ok, err = pcall(function()
      config.build_cmd_args(merged({ server_args = { "--type-provider=tgo" } }), "/p")
    end)
    assert.is_false(ok, "expected build_cmd_args to error on a --type-provider override")
    assert.is_truthy(tostring(err):match("type%-provider"))
  end)

  it("rejects a server_args entry that injects --tsdk or --plugin-path", function()
    -- These tsserver-only flags break the tsgo contract; they must never be
    -- smuggled in via server_args. Discriminates a no-validation impl.
    local ok_tsdk = pcall(function()
      config.build_cmd_args(merged({ server_args = { "--tsdk=/x" } }), "/p")
    end)
    assert.is_false(ok_tsdk, "expected --tsdk in server_args to be rejected")
    local ok_plugin = pcall(function()
      config.build_cmd_args(merged({ server_args = { "--plugin-path=/x" } }), "/p")
    end)
    assert.is_false(ok_plugin, "expected --plugin-path in server_args to be rejected")
  end)

  it("accepts a benign server_args entry (negative control for the rejection)", function()
    -- Proves the rejection is targeted, not a blanket ban on server_args.
    local args = config.build_cmd_args(merged({ server_args = { "--foo" } }), "/p")
    assert.is_true(contains(args, "--foo"))
  end)
end)

describe("verter.config.build_cmd", function()
  it("returns a function (root-aware cmd), not a static list", function()
    local cmd = config.build_cmd(merged())
    assert.are.equal("function", type(cmd))
  end)

  it("spawns via rpc.start with the resolved root and forwards VERTER_LOG", function()
    -- Stub vim.lsp.rpc.start so NO real server is spawned; capture its args.
    local captured = nil
    local orig_start = vim.lsp.rpc.start
    vim.lsp.rpc.start = function(cmd_args, _dispatchers, spawn)
      captured = { cmd_args = cmd_args, spawn = spawn }
      return { is_closing = function() return false end, terminate = function() end }
    end

    local cmd = config.build_cmd(merged({ log_level = "debug" }))
    -- The function form receives (dispatchers, config); config carries the
    -- resolved root_dir nvim populates after root resolution.
    cmd({}, { root_dir = "/home/u/project", cmd_cwd = "/home/u/project" })

    vim.lsp.rpc.start = orig_start

    assert.is_not_nil(captured, "build_cmd must call vim.lsp.rpc.start")
    -- The resolved root is the trailing positional in the spawned argv.
    assert.are.equal("/home/u/project", captured.cmd_args[#captured.cmd_args])
    -- cwd is preserved.
    assert.are.equal("/home/u/project", captured.spawn.cwd)
    -- VERTER_LOG is threaded through the spawn env (the function-form cmd would
    -- otherwise drop the top-level cmd_env). Discriminates a cwd-only impl.
    assert.is_not_nil(captured.spawn.env, "spawn params must carry an env table")
    assert.are.equal("debug", captured.spawn.env.VERTER_LOG)
  end)

  it("preserves an explicit VERTER_TSGO_BIN override in the function-form spawn", function()
    local captured = nil
    local orig_start = vim.lsp.rpc.start
    local orig_tsgo = vim.env.VERTER_TSGO_BIN
    vim.env.VERTER_TSGO_BIN = "/toolchain/typescript/lib/tsc"
    vim.lsp.rpc.start = function(_cmd_args, _dispatchers, spawn)
      captured = spawn
      return { is_closing = function() return false end, terminate = function() end }
    end

    local ok, err = pcall(function()
      config.build_cmd(merged())({}, { root_dir = "/home/u/project" })
    end)
    vim.lsp.rpc.start = orig_start
    vim.env.VERTER_TSGO_BIN = orig_tsgo
    if not ok then error(err) end

    assert.is_not_nil(captured)
    assert.are.equal("/toolchain/typescript/lib/tsc", captured.env.VERTER_TSGO_BIN)
  end)
end)

describe("verter.config.build_init_options", function()
  it("maps lint.enabled through to the wire table", function()
    local io_default = config.build_init_options(merged())
    assert.are.equal(false, io_default.lint.enabled)
    assert.are.equal("recommended", io_default.lint.preset)

    local io_on = config.build_init_options(merged({ lint = { enabled = true, preset = "strict" } }))
    assert.are.equal(true, io_on.lint.enabled)
    assert.are.equal("strict", io_on.lint.preset)
  end)

  it("emits camelCase wire keys the server actually reads", function()
    local io = config.build_init_options(merged())
    -- These exact keys are read by crates/verter_lsp/src/server/lifecycle.rs
    -- and crates/verter_lsp/src/config.rs.
    assert.is_not_nil(io.inlayHints)
    assert.are.equal(true, io.inlayHints.enabled)
    assert.is_not_nil(io.viteConfig)
    assert.are.equal(true, io.viteConfig.enabled)
    assert.are.same({}, io.viteConfig.trustedFiles)
    assert.is_not_nil(io.experimental)
    assert.are.equal(false, io.experimental.conditionalRootNarrowing)
    assert.are.equal(false, io.experimental.strictSlots)
    assert.is_not_nil(io.hover)
    assert.are.equal(false, io.hover.provenance)
    -- `statistics` is server-read (statistics.set_enabled) and shipped OFF by
    -- default; it is part of the canonical parity set.
    assert.is_not_nil(io.statistics)
    assert.are.equal(false, io.statistics.enabled)
    -- `frameworks` is NOT emitted: the server ignores it (dead protocol surface).
    assert.is_nil(io.frameworks)
  end)

  it("translates snake_case opts into camelCase wire keys (no snake_case leak)", function()
    local io = config.build_init_options(merged({
      inlay_hints = { enabled = false },
      vite_config = { enabled = false, trusted_files = { "/a/vite.config.ts" } },
      experimental = { conditional_root_narrowing = true, strict_slots = true },
      hover = { provenance = true },
    }))
    -- Positive: camelCase carries the overridden values.
    assert.are.equal(false, io.inlayHints.enabled)
    assert.are.equal(false, io.viteConfig.enabled)
    assert.are.same({ "/a/vite.config.ts" }, io.viteConfig.trustedFiles)
    assert.are.equal(true, io.experimental.conditionalRootNarrowing)
    assert.are.equal(true, io.experimental.strictSlots)
    assert.are.equal(true, io.hover.provenance)
    -- Negative: the snake_case opt keys must NOT leak onto the wire table
    -- (the server only reads camelCase; a snake_case leak is a silent no-op).
    assert.is_nil(io.inlay_hints)
    assert.is_nil(io.vite_config)
    assert.is_nil(rawget(io.experimental, "conditional_root_narrowing"))
    assert.is_nil(rawget(io.experimental, "strict_slots"))
    assert.is_nil(rawget(io.viteConfig, "trusted_files"))
  end)

  it("omits VS-Code-UI-only keys even when they are present in opts", function()
    -- Inject the VS-Code-UI-only surfaces INTO the merged opts so this is a
    -- real pass-through test: a builder that blindly forwarded its input would
    -- carry these onto the wire table. (The previous version of this test
    -- passed vacuously because the input never contained these keys.)
    -- `rawget` defeats any metatable that might mask a key.
    --
    -- NOTE: `statistics` is NOT in this set — it IS a server-read parity key, so
    -- it is emitted (asserted in the closed-whitelist test below). The builder
    -- positively constructs each emitted group, so an injected non-parity key
    -- like `statistics.panel` still cannot leak: only `statistics.enabled` is
    -- read and re-emitted.
    local io = config.build_init_options(merged({
      configuration = { foo = true },
      mcp = { enabled = true },
      decorations = { inline = true },
      statistics = { enabled = false, panel = true },
    }))
    assert.is_nil(rawget(io, "configuration"))
    assert.is_nil(rawget(io, "mcp"))
    assert.is_nil(rawget(io, "decorations"))
    -- statistics IS emitted, but only its server-read `enabled` field — the
    -- injected `panel` sub-key is not forwarded (positive construction).
    assert.is_not_nil(rawget(io, "statistics"))
    assert.are.equal(false, io.statistics.enabled)
    assert.is_nil(rawget(io.statistics, "panel"))
  end)

  it("emits EXACTLY the server-read top-level keys (closed whitelist)", function()
    -- Even with extra UI-only keys injected, the result's top-level key set is
    -- exactly the canonical six server-read keys
    -- (verter_editor_client::build_initialization_options): `statistics` is
    -- server-read and PRESENT; `frameworks` is server-ignored and ABSENT. A
    -- pass-through impl would add the injected keys and fail this sorted-set
    -- compare. The Rust drift-guard
    -- (crates/verter-editor-client/tests/nvim_config_contract.rs) binds this set
    -- to the shared SSoT.
    local io = config.build_init_options(merged({
      configuration = { foo = true },
      mcp = { enabled = true },
    }))
    local keys = {}
    for k in pairs(io) do
      keys[#keys + 1] = k
    end
    table.sort(keys)
    local expected = { "experimental", "hover", "inlayHints", "lint", "statistics", "viteConfig" }
    table.sort(expected)
    assert.are.same(expected, keys)
  end)
end)

describe("verter.config.build_capabilities", function()
  it("forces didChangeWatchedFiles.dynamicRegistration false by default", function()
    local caps = config.build_capabilities(merged())
    assert.are.equal(
      false,
      caps.workspace.didChangeWatchedFiles.dynamicRegistration
    )
  end)

  it("flips dynamicRegistration to true when watch_files is opted in", function()
    local caps = config.build_capabilities(merged({ watch_files = true }))
    assert.are.equal(
      true,
      caps.workspace.didChangeWatchedFiles.dynamicRegistration
    )
  end)

  it("preserves a user-supplied blink-shaped completionItem.resolveSupport", function()
    -- Shape a minimal blink.cmp / cmp_nvim_lsp style capabilities table.
    local user_caps = {
      textDocument = {
        completion = {
          completionItem = {
            snippetSupport = true,
            resolveSupport = {
              properties = { "documentation", "detail", "additionalTextEdits" },
            },
          },
        },
      },
    }
    local caps = config.build_capabilities(merged({ capabilities = user_caps }))
    local resolve = caps.textDocument.completion.completionItem.resolveSupport
    assert.is_not_nil(resolve)
    assert.is_true(contains(resolve.properties, "additionalTextEdits"))
    assert.is_true(contains(resolve.properties, "documentation"))
    -- The watcher override is still applied on top of the user's caps.
    assert.are.equal(false, caps.workspace.didChangeWatchedFiles.dynamicRegistration)
  end)

  it("preserves nvim built-in defaults when the user passes a PARTIAL caps table", function()
    -- A partial user caps table (only a completion tweak, no general.* block)
    -- must NOT erase the built-in position-encoding default. A
    -- start-from-user-caps impl (no merge with make_client_capabilities())
    -- would drop general.positionEncodings entirely. Discriminates that impl.
    local builtin = vim.lsp.protocol.make_client_capabilities()
    -- Only meaningful if the runtime actually advertises a default here.
    if builtin.general and builtin.general.positionEncodings then
      local partial = {
        textDocument = { completion = { completionItem = { snippetSupport = true } } },
      }
      local caps = config.build_capabilities(merged({ capabilities = partial }))
      assert.is_not_nil(caps.general, "built-in general.* defaults must survive a partial user table")
      assert.is_not_nil(caps.general.positionEncodings)
      assert.is_true(#caps.general.positionEncodings > 0)
      -- And the user's partial tweak still merged in.
      assert.are.equal(true, caps.textDocument.completion.completionItem.snippetSupport)
    end
  end)

  it("does NOT mutate the caller's capabilities table", function()
    -- build_capabilities deepcopies the user table before merging, so forcing
    -- the watcher flag must not write back into the caller's table. An
    -- in-place-mutate impl would stamp dynamicRegistration onto user_caps.
    --
    -- Build opts so `opts.capabilities` is UNAMBIGUOUSLY this exact table
    -- reference (set it AFTER merged() so a deep-extend copy can't hide the
    -- mutation and make the assertion non-discriminating). Pre-seed a marker
    -- value the mutate-in-place impl would overwrite.
    local user_caps = {
      workspace = { didChangeWatchedFiles = { dynamicRegistration = true } },
    }
    local opts = merged()
    opts.capabilities = user_caps
    local result = config.build_capabilities(opts)
    -- The forced default must apply to the RESULT...
    assert.are.equal(false, result.workspace.didChangeWatchedFiles.dynamicRegistration)
    -- ...but the caller's table keeps its original marker (untouched). A
    -- mutate-in-place impl flips this to false. Discriminates that impl.
    assert.are.equal(
      true,
      user_caps.workspace.didChangeWatchedFiles.dynamicRegistration,
      "build_capabilities must not mutate the caller's capabilities table"
    )
  end)
end)

describe("verter.setup", function()
  -- `check_binary = false` skips the PATH/executable probe so these pure-config
  -- assertions run in headless CI where `verter-lsp` is not installed. It is a
  -- first-class option (a user whose binary is provided by a wrapper uses it
  -- too), not a test-only shim.
  it("registers .vue / .svelte filetype detection", function()
    init.setup({ check_binary = false })
    assert.are.equal("vue", vim.filetype.match({ filename = "App.vue" }))
    assert.are.equal("svelte", vim.filetype.match({ filename = "X.svelte" }))
  end)

  it("registers a 'verter' config whose filetypes cover vue + svelte", function()
    init.setup({ check_binary = false })
    local cfg = vim.lsp.config["verter"]
    assert.is_not_nil(cfg)
    assert.is_true(contains(cfg.filetypes, "vue"))
    assert.is_true(contains(cfg.filetypes, "svelte"))
    -- A `cmd` must be present (the root-aware function).
    assert.is_not_nil(cfg.cmd)
    assert.are.equal("function", type(cfg.cmd))
  end)

  it("registers the carrier root_markers (package.json + .git among them)", function()
    init.setup({ check_binary = false })
    local cfg = vim.lsp.config["verter"]
    assert.is_true(contains(cfg.root_markers, "package.json"))
    assert.is_true(contains(cfg.root_markers, ".git"))
    assert.is_true(contains(cfg.root_markers, "tsconfig.json"))
  end)

  it("does NOT register a broken client when the binary is missing", function()
    -- Spy on the registration + enable seams so the assertion does not depend on
    -- whether a prior test left a 'verter' config behind. A correct impl
    -- fails closed: with an unresolvable binary and the probe enabled, neither
    -- vim.lsp.config('verter', ...) nor vim.lsp.enable('verter') is called.
    local orig_config = vim.lsp.config
    local orig_enable = vim.lsp.enable
    local orig_notify = vim.notify
    local registered, enabled, notified_error = false, false, false

    -- vim.lsp.config is callable; intercept the (name, cfg) registration call.
    vim.lsp.config = setmetatable({}, {
      __call = function(_, name, _cfg)
        if name == "verter" then
          registered = true
        end
      end,
      __index = orig_config,
    })
    vim.lsp.enable = function(name)
      if name == "verter" then
        enabled = true
      end
    end
    vim.notify = function(_msg, level)
      if level == vim.log.levels.ERROR then
        notified_error = true
      end
    end

    local ok = pcall(function()
      init.setup({ cmd_path = "verter-lsp-definitely-not-on-path-xyz", check_binary = true })
    end)

    vim.lsp.config = orig_config
    vim.lsp.enable = orig_enable
    vim.notify = orig_notify

    assert.is_true(ok, "setup must not error on a missing binary")
    assert.is_false(registered, "must not register the 'verter' config when the binary is missing")
    assert.is_false(enabled, "must not enable the 'verter' client when the binary is missing")
    assert.is_true(notified_error, "must surface an ERROR notification about the missing binary")
  end)

  it("refuses to register on an invalid type_provider (fail-closed)", function()
    -- A typo like `tgo` would otherwise be emitted as `--type-provider=tgo` and
    -- silently fall back to `auto` server-side. setup must reject it BEFORE
    -- registering, even with check_binary=false. Discriminates a no-validation
    -- impl (which would register the broken config).
    local orig_config = vim.lsp.config
    local orig_enable = vim.lsp.enable
    local orig_notify = vim.notify
    local registered, enabled, notified_error = false, false, false

    vim.lsp.config = setmetatable({}, {
      __call = function(_, name, _cfg)
        if name == "verter" then
          registered = true
        end
      end,
      __index = orig_config,
    })
    vim.lsp.enable = function(name)
      if name == "verter" then
        enabled = true
      end
    end
    vim.notify = function(_msg, level)
      if level == vim.log.levels.ERROR then
        notified_error = true
      end
    end

    local ok = pcall(function()
      init.setup({ type_provider = "tgo", check_binary = false })
    end)

    vim.lsp.config = orig_config
    vim.lsp.enable = orig_enable
    vim.notify = orig_notify

    assert.is_true(ok, "setup must not throw on an invalid type_provider")
    assert.is_false(registered, "must not register with an invalid type_provider")
    assert.is_false(enabled, "must not enable with an invalid type_provider")
    assert.is_true(notified_error, "must surface an ERROR about the invalid type_provider")
  end)

  it("registers when type_provider is a valid value (positive control)", function()
    -- Negative control for the validation: a valid provider registers normally
    -- (binary probe disabled for headless CI). Proves the validation is
    -- targeted, not a blanket refusal.
    local orig_config = vim.lsp.config
    local orig_enable = vim.lsp.enable
    local registered, enabled = false, false

    vim.lsp.config = setmetatable({}, {
      __call = function(_, name, _cfg)
        if name == "verter" then
          registered = true
        end
      end,
      __index = orig_config,
    })
    vim.lsp.enable = function(name)
      if name == "verter" then
        enabled = true
      end
    end

    init.setup({ type_provider = "tsserver", check_binary = false })

    vim.lsp.config = orig_config
    vim.lsp.enable = orig_enable

    assert.is_true(registered, "a valid type_provider must register the config")
    assert.is_true(enabled, "a valid type_provider must enable the client")
  end)

  it("uses the PRODUCT default type_provider (tsgo, never tgo) end-to-end", function()
    -- Direct guard on the SHIPPED `DEFAULTS.type_provider` in verter/init.lua.
    -- Every other tsgo test in this file builds args from the test-local
    -- `merged()` mirror (which hardcodes type_provider = "tsgo"), so a revert of
    -- the PRODUCT default to `tgo` would slip past all of them. This test drives
    -- the REAL product path with NO type_provider override, so the default value
    -- itself is on trial:
    --   setup({}) -> DEFAULTS.type_provider -> build_cmd -> build_cmd_args -> argv.
    --
    -- The product validates type_provider BEFORE registering (init.lua: an
    -- invalid value notifies ERROR and returns without calling vim.lsp.config),
    -- and `tgo` is NOT in VALID_TYPE_PROVIDERS. So a `tgo` revert fails this test
    -- TWO independent ways:
    --   1. validation fails closed -> the registration spy never captures a
    --      'verter' config -> `registered_cfg` stays nil -> the cmd assertion
    --      (and the explicit nil check) fail; and
    --   2. were validation ever loosened, the captured argv would carry
    --      `--type-provider=tgo` and lack `--type-provider=tsgo`.
    -- With the shipped `tsgo` default both checks pass. PASS on tsgo, FAIL on tgo.
    --
    -- Capture the config from the ACTUAL registration call (not a stale global a
    -- prior test may have left) so the assertion depends only on THIS setup call.
    local orig_config = vim.lsp.config
    local orig_rpc_start = vim.lsp.rpc.start
    local registered_cfg = nil

    local ok, err = pcall(function()
      vim.lsp.config = setmetatable({}, {
        __call = function(_, name, cfg)
          if name == "verter" then
            registered_cfg = cfg
          end
        end,
        __index = orig_config,
      })

      -- NOTE: deliberately NO type_provider override -> the product DEFAULTS
      -- value is what ends up in the built argv.
      init.setup({ check_binary = false })

      assert.is_not_nil(
        registered_cfg,
        "the product default type_provider must be valid enough to register "
          .. "(a `tgo` default fails closed and never registers)"
      )

      -- The registered `cmd` is the root-aware function; invoke it with a stubbed
      -- rpc.start and a fake resolved root to capture the argv it builds from the
      -- product default. No real server is spawned.
      local captured = nil
      vim.lsp.rpc.start = function(args, _dispatchers, _spawn)
        captured = args
        return { is_closing = function() return false end, terminate = function() end }
      end
      assert.are.equal("function", type(registered_cfg.cmd))
      registered_cfg.cmd({}, { root_dir = "/proj" })

      assert.is_not_nil(captured, "the registered cmd must build an argv via rpc.start")
      assert.is_true(
        contains(captured, "--type-provider=tsgo"),
        "the product default must emit --type-provider=tsgo"
      )
      assert.is_false(
        contains(captured, "--type-provider=tgo"),
        "the product default must never emit the typo'd --type-provider=tgo"
      )
    end)

    -- Failure-safe restore: runs even if an assertion above raised, then the
    -- error is re-propagated so a real failure still fails the test.
    vim.lsp.config = orig_config
    vim.lsp.rpc.start = orig_rpc_start
    if not ok then
      error(err)
    end
  end)
end)
