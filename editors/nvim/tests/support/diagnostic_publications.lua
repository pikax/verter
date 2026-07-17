local M = {}

function M.new()
  local tracker = { counts = {} }

  function tracker:observe(params)
    if type(params) ~= "table" or type(params.uri) ~= "string" then return end
    self.counts[params.uri] = (self.counts[params.uri] or 0) + 1
  end

  function tracker:has(uri)
    return (self.counts[uri] or 0) > 0
  end

  function tracker:wrap(delegate)
    assert(type(delegate) == "function", "publishDiagnostics delegate must be callable")
    return function(err, params, context, config)
      local result = delegate(err, params, context, config)
      -- Mark the publication only after Neovim's real handler has installed the
      -- diagnostics, so a subsequent `vim.diagnostic.get` cannot race it.
      self:observe(params)
      return result
    end
  end

  return tracker
end

return M
