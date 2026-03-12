# VS Code Extension

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Setup and features for the Verter VS Code extension.

## Installation

Search **"Verter"** in the VS Code marketplace, or install from the command line:

```bash
code --install-extension pikax.verter-vscode
```

Alternatively, you can build from source (see [Building from Source](#building-from-source) below).

## Features

- **Intelligent completions** -- context-aware for components, props, CSS classes, with trigger characters `.` `@` `<` `:` `"` `'` and space
- **Real-time TypeScript diagnostics** -- pull-based diagnostics with inter-file dependency support
- **Go-to-definition navigation** -- bindings, imports, CSS-to-template, DOM query selectors
- **Hover information** -- type info, CSS rules on elements, elements on selectors
- **Template and script error reporting** -- diagnostics for both `<template>` and `<script>` blocks
- **Compiled code viewer** -- inspect the generated TSX output for any `.vue` file
- **Reactivity color decorations** -- faint color highlights on bindings based on their reactivity type (ref, computed, reactive, prop, composable, mutable, function)
- **Vue API call annotations** -- inline annotations for lifecycle hooks, watchers, reactivity primitives, and provide/inject
- **Semantic tokens** -- 23 token types and 10 modifiers for fine-grained syntax highlighting
- **Analysis sidebar** -- virtual files, component tree, and analysis views (opt-in)
- **MCP server** -- built-in MCP endpoint sharing LSP data, providing 36+ Vue analysis tools to AI agents

## Commands

| Command | ID | Description |
|---------|----|----|
| Restart Language Server | `verter.restartLanguageServer` | Restart the Verter LSP server |
| Show Compiled Code to Side | `verter.showCompiledCodeToSide` | View generated TSX in a side panel |
| Write Virtual Files | `verter.writeVirtualFiles` | Write virtual files to disk for debugging |
| Show Statistics | `verter.showStatistics` | Display processing statistics |
| Open Virtual File | `verter.openVirtualFile` | Open a virtual file in the editor |
| Show Source Map | `verter.showSourceMapForFile` | Show source map for the current file |
| Refresh Virtual Files | `verter.refreshVirtualFiles` | Refresh the virtual files view |
| Refresh Analysis | `verter.refreshAnalysis` | Refresh the analysis view |
| Show Source Map Visualization | `verter.showSourceMapVisualization` | Show source map visualization side-by-side |
| Go to Component Definition | `verter.goToComponent` | Navigate to a component's definition file |
| Go to Parent File | `verter.goToParentFile` | Navigate to the parent file |
| Setup MCP for Claude Code | `verter.setupMcpForClaudeCode` | Configure `.mcp.json` for Claude Code MCP integration |

## Analysis Sidebar

When `verter.analysis.enabled` is set to `true`, the extension adds an **Analysis** activity bar icon with three views:

- **Virtual Files & Source Map** -- browse generated virtual files and source map for the active `.vue` file
- **Component Tree** -- visualize the component hierarchy
- **Analysis** -- static analysis information

These views are visible only when a `.vue` file is active.

## Language Support

The extension registers language support for:

- `vue` -- `.vue` single file components
- `vue-html` -- Vue HTML templates
- `vue-postcss` -- PostCSS in Vue styles
- `vue-sugarss` -- SugarSS in Vue styles

Embedded language grammars are included for HTML, CSS, SCSS, Less, Sass, Stylus, PostCSS, JavaScript, TypeScript, Pug, CoffeeScript, Markdown, YAML, JSON, PHP, and GraphQL.

## TypeScript Plugin

The extension bundles `@verter/typescript-plugin`, which resolves `.vue` imports in TypeScript and JavaScript files. This is registered automatically for both bundled and workspace TypeScript versions.

## Building from Source

```bash
git clone https://github.com/pikax/verter.git
cd verter
pnpm install
pnpm build
```

Then press **F5** in VS Code to launch the Extension Development Host. This starts a new VS Code window with the locally built extension loaded.

### Build Steps Explained

`pnpm build` runs sequentially:

1. **Native bindings** (`pnpm run build:native`) -- builds the Rust NAPI-RS `.node` binary
2. **LSP binary** (`pnpm run build:lsp`) -- builds the `verter-lsp` Rust binary
3. **WASM** (`pnpm run build:wasm`) -- builds the wasm-bindgen WASM binary
4. **TypeScript packages** (`pnpm run build:ts`) -- builds all TypeScript packages including the extension

### Development Workflow

For iterative development:

```bash
pnpm dev-extension   # Build LSP binary, then watch language-shared + extension + TypeScript plugin
```

This watches for changes and rebuilds the extension automatically. Press **F5** to reload the Extension Development Host after changes.
