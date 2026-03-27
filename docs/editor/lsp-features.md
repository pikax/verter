# LSP Features

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter includes a full Rust LSP server (`verter-lsp`) that communicates with editors over stdio. The server is launched automatically by the VS Code extension and implements a comprehensive set of Language Server Protocol features.

## Text Document Features

| Feature            | LSP Method                       | Description                                                                                                                    |
| ------------------ | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Completion         | `textDocument/completion`        | Context-aware completions with trigger characters `.` `@` `<` `:` `"` `'` and space. Supports resolve for additional detail.   |
| Hover              | `textDocument/hover`             | Type information, CSS rules on template elements, matched elements on CSS selectors                                            |
| Definition         | `textDocument/definition`        | Go-to-definition for bindings, imports, CSS-to-template navigation, DOM query selectors                                        |
| Type Definition    | `textDocument/typeDefinition`    | Navigate to type declarations                                                                                                  |
| Declaration        | `textDocument/declaration`       | Navigate to declarations                                                                                                       |
| References         | `textDocument/references`        | Find all references to a symbol                                                                                                |
| Rename             | `textDocument/rename`            | Symbol renaming with prepare support (`textDocument/prepareRename`) for validation before rename                               |
| Diagnostics        | `textDocument/diagnostic`        | Pull-based diagnostics with inter-file dependency support. Covers script errors, template errors, and Vue-specific lint rules. |
| Document Symbols   | `textDocument/documentSymbol`    | Document outline showing the structure of SFC blocks, script declarations, and template elements                               |
| Document Highlight | `textDocument/documentHighlight` | Highlight all occurrences of the same symbol in the current document                                                           |
| Signature Help     | `textDocument/signatureHelp`     | Function signature information with trigger characters `(` and `,`, retrigger on `,`                                           |

## Navigation

| Feature         | LSP Method                          | Description                                                                                      |
| --------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------ |
| Folding Range   | `textDocument/foldingRange`         | Folding ranges for SFC blocks (`<template>`, `<script>`, `<style>`) and nested template elements |
| Selection Range | `textDocument/selectionRange`       | Smart selection expansion -- progressively selects larger syntactic units                        |
| Linked Editing  | `textDocument/linkedEditingRange`   | Rename matching open/close HTML tags simultaneously                                              |
| Call Hierarchy  | `textDocument/prepareCallHierarchy` | Call hierarchy navigation for incoming and outgoing calls                                        |
| Document Link   | `textDocument/documentLink`         | Clickable links in source code (e.g., import paths, `src` attributes)                            |

## Code Intelligence

| Feature         | LSP Method                         | Description                                                                                                                                                                                               |
| --------------- | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Code Actions    | `textDocument/codeAction`          | Quick fixes, refactoring, organize imports (`source.organizeImports`), and extract component (`refactor.extract`). Supported kinds: `source.organizeImports`, `quickfix`, `refactor`, `refactor.extract`. |
| Code Lens       | `textDocument/codeLens`            | Code lens annotations displayed above code elements                                                                                                                                                       |
| Inlay Hints     | `textDocument/inlayHint`           | Inline hints showing DOM query matched elements and `useTemplateRef` matched refs                                                                                                                         |
| Semantic Tokens | `textDocument/semanticTokens/full` | Full semantic token support with 23 token types and 10 modifiers                                                                                                                                          |

### Semantic Token Types

The server provides the following 23 semantic token types:

`namespace`, `type`, `class`, `enum`, `interface`, `struct`, `typeParameter`, `parameter`, `variable`, `property`, `enumMember`, `event`, `function`, `method`, `macro`, `keyword`, `modifier`, `comment`, `string`, `number`, `regexp`, `operator`, `decorator`

### Semantic Token Modifiers

The following 10 modifiers can be applied to any token type:

`declaration`, `definition`, `readonly`, `static`, `deprecated`, `abstract`, `async`, `modification`, `documentation`, `defaultLibrary`

## Formatting and Color

| Feature             | LSP Method                   | Description                                      |
| ------------------- | ---------------------------- | ------------------------------------------------ |
| Document Formatting | `textDocument/formatting`    | Format the entire document                       |
| Color Information   | `textDocument/documentColor` | CSS color picker with color presentation support |

## Workspace Features

| Feature           | LSP Method                                             | Description                                              |
| ----------------- | ------------------------------------------------------ | -------------------------------------------------------- |
| Workspace Symbols | `workspace/symbol`                                     | Project-wide symbol search across all `.vue` files       |
| Workspace Folders | `workspace/workspaceFolders`                           | Multi-root workspace support with change notifications   |
| File Operations   | `workspace/didCreateFiles`, `workspace/didDeleteFiles` | Tracks file creation and deletion for cache invalidation |

## Document Synchronization

The server uses **incremental** text document synchronization (`TextDocumentSyncKind.Incremental`), meaning only the changed portions of a document are sent from the client on each edit. This minimizes communication overhead for large files.

## Position Encoding

The LSP server negotiates position encoding with the client during the `initialize` handshake. The preferred order is:

1. **UTF-8** (preferred -- no conversion needed since Rust strings are UTF-8)
2. **UTF-32**
3. **UTF-16** (fallback -- standard LSP default)

The negotiated encoding is announced in `ServerCapabilities.position_encoding` and applies to all positions in both standard and custom protocol messages.

## Architecture

The LSP binary is structured as follows:

```
main.rs               -- stdio transport, CLI argument parsing
server.rs              -- LSP message loop, request dispatch
documents/             -- Document tracking and incremental sync
features/              -- Individual LSP feature handlers
analysis/              -- Static analysis integration
css/                   -- CSS-specific language features
tsgo/                  -- TSGO type provider (LSP over stdio, resilient wrapper)
tsserver/              -- tsserver type provider (newline-delimited JSON, resilient wrapper)
sync_coordinator.rs    -- Background file sync with debounce (freeze prevention)
config.rs              -- Lint configuration discovery (.verterrc.json, ESLint, VS Code)
capabilities.rs        -- Server capability registration
```

Each feature module in `features/` handles one or more related LSP methods. The server dispatches incoming requests to the appropriate feature handler based on the LSP method.

### Type Provider

The LSP delegates TypeScript type checking to an external process. Two backends are supported:

| Backend      | Binary             | Protocol               | Use Case                             |
| ------------ | ------------------ | ---------------------- | ------------------------------------ |
| **TSGO**     | `tsgo` (Go binary) | LSP over stdio         | Fast, native TS checking (preview)   |
| **tsserver** | `node tsserver.js` | Newline-delimited JSON | Workspace TS version, plugin support |

Provider selection is configured via the `--type-provider` CLI arg or `verter.typeProvider` VS Code setting. See [Settings Reference](/editor/settings#type-provider) for details.

::: warning TSGO Limitation
TSGO has a known limitation: re-exported `.vue` components (e.g., barrel files) may lose their typing. This is why `auto` mode defaults to tsserver when a workspace TypeScript installation is found.
:::
