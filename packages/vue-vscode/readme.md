# verter-vscode

The official VS Code extension for Verter, providing enhanced Vue SFC support with TypeScript integration.

## Overview

`verter-vscode` is the VS Code extension that bundles:

- `@verter/language-server` - LSP server for IDE features
- `@verter/typescript-plugin` - TypeScript plugin for `.vue` imports
- `@verter/oxc-bindings` - Platform-specific OXC parser binaries

## Features

- **Intelligent Completions**: Context-aware code completion for Vue templates and scripts
- **Type Checking**: Full TypeScript type checking in `.vue` files
- **Go to Definition**: Navigate to component definitions, props, and methods
- **Hover Information**: See type information on hover
- **Diagnostics**: Real-time error and warning reporting
- **Compiled Code View**: View the generated TSX for debugging

## Installation

### From Marketplace

Search for "Verter VSCode" in the VS Code extensions marketplace (coming soon).

### Manual Installation

```bash
# Build the extension
cd packages/vue-vscode
pnpm build
pnpm package

# Install the .vsix file
code --install-extension verter-vscode-*.vsix
```

## Architecture

```
src/
├── extension.ts              # Extension activation
└── CompiledCodeContentProvider.ts # Compiled code view
```

### Activation

On activation, the extension:

1. Downloads platform-specific OXC bindings
2. Starts the language server
3. Configures the TypeScript plugin
4. Sets up notification handlers

```typescript
export async function activate(context: ExtensionContext) {
  // Download OXC bindings for the platform
  await resolveAndDownloadBinding(context.extensionPath);
  
  // Start language server
  const server = activateVueLanguageServer(context);
  
  // Configure TypeScript plugin for .vue imports
  commands.executeCommand(
    "_typescript.configurePlugin",
    "@verter/typescript-plugin",
    { enable: true }
  );
}
```

### Language Server Communication

The extension communicates with the language server via LSP:

```typescript
const serverModule = require.resolve("@verter/language-server/dist/server.js");

const serverOptions: ServerOptions = {
  run: {
    module: serverModule,
    transport: TransportKind.ipc
  },
  debug: {
    module: serverModule,
    transport: TransportKind.ipc,
    options: { execArgv: ["--inspect=6009"] }
  }
};

const client = new LanguageClient(
  "verter",
  "Verter Language Server",
  serverOptions,
  clientOptions
);
```

## Configuration

### Extension Settings

```json
{
  "verter.language-server.port": -1,
  "verter.trace.server": "off"
}
```

| Setting | Description | Default |
|---------|-------------|---------|
| `verter.language-server.port` | Debug port for language server (-1 for auto) | -1 |
| `verter.trace.server` | Trace level for LSP communication | "off" |

## Commands

| Command | Description |
|---------|-------------|
| `verter.showCompiledCode` | Show the compiled TSX for the current file |
| `verter.restartServer` | Restart the language server |

## Compiled Code View

View the generated TSX for any `.vue` file:

1. Open a `.vue` file
2. Run command: "Verter: Show Compiled Code"
3. A new panel opens showing the TSX output

This is useful for:
- Debugging type issues
- Understanding how Verter transforms your code
- Learning TSX equivalents of Vue templates

## Development

### Building

```bash
# Build the extension
pnpm build

# Watch mode
pnpm watch
```

### Debugging

1. Open the monorepo in VS Code
2. Run "Launch Extension" from the debug panel
3. A new VS Code window opens with the extension loaded

### Packaging

```bash
# Package as .vsix
pnpm package
```

## Dependencies

| Package | Purpose |
|---------|---------|
| `@verter/language-server` | LSP server |
| `@verter/language-shared` | Protocol types |
| `@verter/typescript-plugin` | TS plugin for imports |
| `@verter/oxc-bindings` | OXC parser binaries |
| `vscode-languageclient` | LSP client |

## Requirements

- VS Code 1.90.0 or higher
- Node.js 18+

## Troubleshooting

### Extension Not Activating

1. Check the Output panel for "Verter Language Server"
2. Ensure you have a `.vue` file open
3. Try reloading the window

### Type Errors Not Showing

1. Ensure TypeScript is configured in your project
2. Check that `@verter/typescript-plugin` is in tsconfig.json plugins
3. Restart the TypeScript server: "TypeScript: Restart TS Server"

### Performance Issues

1. Check for large `.vue` files
2. Ensure `node_modules` is excluded from file watching
3. Try increasing the language server memory limit

## License

MIT
