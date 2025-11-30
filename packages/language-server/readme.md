# @verter/language-server

The Language Server Protocol (LSP) implementation for Verter, providing IDE features for Vue Single File Components.

## Overview

`@verter/language-server` is the backbone of Verter's IDE support. It:

- Manages TypeScript language services per `tsconfig.json`
- Provides completions, diagnostics, hover information, and go-to-definition
- Handles `.vue` file document management and virtual file mapping
- Communicates with editors via the Language Server Protocol

## Installation

```bash
pnpm add @verter/language-server
```

## Architecture

### High-Level Architecture

```
VS Code Extension
        │
        │ LSP
        ▼
┌──────────────────────────────────────────────────────────┐
│                    Language Server                        │
│  ┌─────────────────┐      ┌─────────────────────────────┐│
│  │ DocumentManager │◄────►│     VerterManager           ││
│  │ (File tracking) │      │ (TS services per tsconfig)  ││
│  └─────────────────┘      └─────────────────────────────┘│
│           │                           │                   │
│           ▼                           ▼                   │
│  ┌─────────────────┐      ┌─────────────────────────────┐│
│  │   VueDocument   │      │   TypeScript LanguageService ││
│  │ (Parsed .vue)   │      │   (Per project)             ││
│  └─────────────────┘      └─────────────────────────────┘│
└──────────────────────────────────────────────────────────┘
```

### Directory Structure

```
src/
├── server.ts             # LSP server entry point
├── logger.ts             # Logging utilities
├── utils.ts              # Shared utilities
├── importPackages.ts     # Dynamic package imports
└── v5/
    ├── helpers.ts        # Response formatting helpers
    ├── documents/
    │   ├── index.ts      # Document exports
    │   ├── utils.ts      # URI/path utilities
    │   ├── manager/
    │   │   └── manager.ts # DocumentManager class
    │   └── verter/
    │       ├── manager/
    │       │   └── manager.ts # VerterManager class
    │       ├── vue/
    │       │   ├── vue.ts     # VueDocument class
    │       │   └── sub/       # Sub-document types
    │       │       ├── typescript.ts # VueTypescriptDocument
    │       │       └── style.ts      # VueStyleDocument
    │       └── typescript/
    │           └── typescript.ts # TypescriptDocument
    └── services/         # LSP service implementations
```

## Core Concepts

### DocumentManager

Tracks open files and manages document snapshots:

```typescript
class DocumentManager {
  // Get or create a document snapshot
  getDocument(uri: string): Document;
  
  // Handle document open/close/change
  onDidOpenTextDocument(doc: TextDocument): void;
  onDidChangeTextDocument(params: DidChangeTextDocumentParams): void;
  onDidCloseTextDocument(doc: TextDocument): void;
}
```

### VerterManager

Manages TypeScript language services per `tsconfig.json`:

```typescript
class VerterManager {
  // TypeScript services keyed by tsconfig folder
  readonly tsServices: Map<string, ts.LanguageService>;
  
  // Parsed tsconfig configurations
  readonly tsConfigMap: Map<string, ts.ParsedCommandLine>;
  
  // CSS language services (css, scss, less)
  readonly cssServices: Map<string, CSSLanguageServices>;
  
  // Initialize with workspace folders
  init(params: { workspaceFolders?: WorkspaceFolder[] }): void;
  
  // Get TypeScript service for a file
  getTsService(filePath: string): ts.LanguageService | undefined;
}
```

### VueDocument

Represents a `.vue` file, broken down into multiple virtual sub-documents:

```typescript
class VueDocument {
  // File path
  readonly filepath: string;
  
  // Sub-documents
  typescript: VueTypescriptDocument;  // Script content as TSX
  styles: VueStyleDocument[];         // Style blocks
  
  // Parsed SFC blocks
  readonly blocks: ParsedBlock[];
  
  // Get position mapping between source and generated
  getSourcePosition(generatedPosition: number): SourcePosition;
  getGeneratedPosition(sourcePosition: number): GeneratedPosition;
}
```

### Virtual File Mapping

A `.vue` file is represented as multiple virtual files:

| Virtual File | Content |
|--------------|---------|
| `{path}.vue.ts` | Bundled TypeScript module |
| `{path}.vue.script.ts` | Script block (imports render) |
| `{path}.vue.render.tsx` | Template as TSX |
| `{path}.vue.style.css` | Style blocks |
| `{path}.vue.{block}.ts` | Custom blocks |

## LSP Features

### Completions

Provides intelligent code completion for:
- Component props and events
- Template expressions
- Style selectors
- Import paths

```typescript
// Trigger characters
triggerCharacters: [".", "@", "<", ":", " "]
```

### Diagnostics

Reports TypeScript errors and warnings:
- Type errors in script blocks
- Template type checking
- Missing imports

### Hover Information

Shows type information on hover:
- Variable types
- Function signatures
- Component prop definitions

### Go to Definition

Navigate to definitions for:
- Imported modules
- Component definitions
- Variable declarations

## Usage

### Starting the Server

```typescript
import { startServer } from "@verter/language-server";

// Start with default options
startServer();

// Or with custom options
startServer({
  connection: myConnection,
  logErrorsOnly: true
});
```

### Connection Options

```typescript
interface LsConnectionOption {
  // Custom connection (default: stdio or IPC)
  connection?: Connection;
  
  // Log errors only (default: false)
  logErrorsOnly?: boolean;
}
```

### Command Line

```bash
# Start with stdio
node dist/server.js --stdio

# Start with IPC (for VS Code)
node dist/server.js
```

## Server Capabilities

```typescript
{
  textDocumentSync: {
    openClose: true,
    change: TextDocumentSyncKind.Full,
    save: { includeText: false }
  },
  completionProvider: {
    resolveProvider: true,
    triggerCharacters: [".", "@", "<", ":", " "]
  },
  definitionProvider: true,
  hoverProvider: true,
  diagnosticProvider: {
    documentSelector: "*.vue",
    interFileDependencies: true
  }
}
```

## Protocol Extensions

The server uses `@verter/language-shared` for custom protocol extensions:

```typescript
import { patchClient, NotificationType } from "@verter/language-shared";

// Patched connection with typed notifications
const connection = patchClient(originalConnection);

// Send custom notifications
connection.sendNotification(NotificationType.ShowCompiledCode, {
  uri: "file:///path/to/file.vue",
  code: "// Generated TSX..."
});
```

## Dependencies

- `vscode-languageserver` - LSP server implementation
- `vscode-languageserver-protocol` - LSP types
- `typescript` - TypeScript language service
- `vscode-css-languageservice` - CSS/SCSS/LESS support
- `@verter/core` - SFC transformation
- `@verter/language-shared` - Protocol extensions

## Inspired By

- [sveltejs/language-tools](https://github.com/sveltejs/language-tools)
- [vuedx/languagetools](https://github.com/vuedx/languagetools)
- [typescript-language-server](https://github.com/typescript-language-server)
- [VS Code Language Server Guide](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide)

## License

MIT

