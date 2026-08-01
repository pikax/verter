# verter-lsp

The [Verter](https://verterjs.dev/) Language Server Protocol server for Vue and
Svelte single-file components.

This package is a **launcher**. The native server ships in one per-platform
optional dependency (`@verter/lsp-<platform>`), and your package manager
installs only the one matching your OS, architecture and libc.

```bash
pnpm add -D verter-lsp
```

Supported platforms: macOS x64/arm64, Linux x64/arm64 (glibc and musl),
Windows x64.

## Using it

Installing as a project dev dependency pins the server version alongside the
rest of your Verter tooling — the same reason `typescript` belongs in a project
rather than on the machine.

**As an editor launch command.** Point your editor's LSP client at
`node_modules/.bin/verter-lsp`. The shim hands the process stdio straight to the
native server, so it is not on the per-message path.

**As a path.** For editor settings that want an explicit binary path:

```bash
npx verter-lsp --print-server-path
```

**Programmatically.** Skip the shim and spawn the native binary directly:

```js
const { serverBinaryPath, resolveServerBinary } = require("verter-lsp");

const server = spawn(serverBinaryPath(), ["--type-provider=tsgo"], {
  stdio: ["pipe", "pipe", "inherit"],
});

// `resolveServerBinary()` also reports where the binary came from:
// "platform-package" | "dev-build" | "path"
const { path, source } = resolveServerBinary();
```

`resolveServerBinary` throws on a platform no package covers, rather than
spawning something that is not the server.

## Editor setup

Per-editor configuration for Lapce, Zed, Helix and Neovim is documented in
[Other Editors](https://verterjs.dev/editor/other-editors). VS Code users do not
need this package — the
[Verter extension](https://marketplace.visualstudio.com/items?itemName=verter.verter-vscode)
bundles the server.

## License

MIT
