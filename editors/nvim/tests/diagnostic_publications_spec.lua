local spec_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
local publications = dofile(vim.fs.joinpath(spec_dir, "support", "diagnostic_publications.lua"))

describe("real-client diagnostic publication tracker", function()
  it("stays unsatisfied until the exact fixture URI publishes diagnostics", function()
    local tracker = publications.new()
    local target = "file:///workspace/VueTs.vue"

    assert.is_false(tracker:has(target))
    tracker:observe({ uri = "file:///workspace/Other.vue", diagnostics = {} })
    assert.is_false(tracker:has(target))
    tracker:observe({ uri = target, diagnostics = {} })
    assert.is_true(tracker:has(target))
  end)

  it("records publication only after the wrapped Neovim handler completes", function()
    local tracker = publications.new()
    local target = "file:///workspace/SvelteTs.svelte"
    local observed_during_delegate = nil
    local wrapped = tracker:wrap(function()
      observed_during_delegate = tracker:has(target)
      return "delegated"
    end)

    assert.are.equal("delegated", wrapped(nil, { uri = target, diagnostics = {} }, {}, {}))
    assert.is_false(observed_during_delegate)
    assert.is_true(tracker:has(target))
  end)
end)
