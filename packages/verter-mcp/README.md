# verter-mcp

The [Verter](https://verterjs.dev/) [Model Context Protocol](https://modelcontextprotocol.io/)
server. It exposes Verter's Vue and Svelte analysis — parsing, diagnostics,
compilation and cross-file type resolution — to AI agents such as Claude Code
and GitHub Copilot.

This package is a **launcher**. The native server ships in one per-platform
optional dependency (`@verter/mcp-<platform>`), and your package manager
installs only the one matching your OS, architecture and libc.

```bash
pnpm add -D verter-mcp
```

Supported platforms: macOS x64/arm64, Linux x64/arm64 (glibc and musl),
Windows x64.

## Using it

Point your MCP client at the launcher over stdio. It hands the client's stdio
straight to the native server, so it is not on the per-message path:

```json
{
  "mcpServers": {
    "verter": {
      "command": "npx",
      "args": ["-y", "verter-mcp", "--project-root", "."]
    }
  }
}
```

Installing as a project dev dependency pins the server version alongside the
rest of your Verter tooling, and `node_modules/.bin/verter-mcp` is then a stable
command for clients that prefer an explicit path.

For HTTP transport instead of stdio:

```bash
npx verter-mcp --transport http --port 6772 --project-root .
```

## Resolving the binary yourself

For a host that spawns the native binary directly:

```bash
npx verter-mcp --print-server-path
```

```js
const { serverBinaryPath, resolveServerBinary } = require("verter-mcp");

// `resolveServerBinary()` also reports where the binary came from:
// "platform-package" | "dev-build" | "path"
const { path, source } = resolveServerBinary();
```

`resolveServerBinary` throws on a platform no package covers, rather than
spawning something that is not the server.

## License

MIT
