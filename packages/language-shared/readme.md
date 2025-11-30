# @verter/language-shared

Shared protocol types and utilities for Verter's language client and server communication.

## Overview

`@verter/language-shared` provides:

- Custom notification types for Verter-specific LSP extensions
- Custom request types for extended functionality
- Virtual file utilities for `.vue` sub-document management
- Type-safe client/server communication helpers

## Installation

```bash
pnpm add @verter/language-shared
```

## API

### Client Patching

The `patchClient` function adds type-safe notification and request methods:

```typescript
import { patchClient } from "@verter/language-shared";
import { LanguageClient } from "vscode-languageclient/node";

const client = new LanguageClient(/* ... */);
const patchedClient = patchClient(client);

// Now has typed notification methods
patchedClient.sendNotification(NotificationType.ShowCompiledCode, {
  uri: "file:///path/to/file.vue",
  code: "// Generated TSX..."
});
```

### Notification Types

Custom notifications for Verter-specific features:

```typescript
import { NotificationType } from "@verter/language-shared";

// Show compiled TSX code
NotificationType.ShowCompiledCode

// Progress updates
NotificationType.Progress
```

### Request Types

Custom request/response pairs:

```typescript
import { RequestType } from "@verter/language-shared";

// Get compiled code for a file
const compiled = await client.sendRequest(RequestType.GetCompiledCode, {
  uri: "file:///path/to/file.vue"
});
```

### Virtual Files

Utilities for managing virtual sub-documents:

```typescript
import { VirtualFiles } from "@verter/language-shared";

// Check if URI is a virtual sub-document
VirtualFiles.isVirtual(uri);

// Get parent .vue file from virtual URI
VirtualFiles.getParentUri(virtualUri);

// Create virtual URI for sub-document
VirtualFiles.createUri(parentUri, type);
```

## Usage with Language Server

```typescript
// Server side
import { patchClient } from "@verter/language-shared";
import { createConnection } from "vscode-languageserver/node";

const connection = createConnection();
const patchedConnection = patchClient(connection);

// Handle custom requests
patchedConnection.onRequest(RequestType.GetCompiledCode, async (params) => {
  const compiled = await compileVueFile(params.uri);
  return { code: compiled };
});
```

## Usage with VS Code Extension

```typescript
// Client side
import { patchClient, NotificationType } from "@verter/language-shared";
import { LanguageClient } from "vscode-languageclient/node";

const client = new LanguageClient(/* ... */);
const patchedClient = patchClient(client);

// Listen for notifications
patchedClient.onNotification(NotificationType.ShowCompiledCode, (params) => {
  showCompiledCodePanel(params.uri, params.code);
});
```

## Type Safety

The package provides full type safety for custom protocol extensions:

```typescript
import type { PatchClient } from "@verter/language-shared";

// PatchClient<T> adds typed notification and request methods
type TypedClient = PatchClient<LanguageClient>;
```

## Directory Structure

```
src/
├── index.ts           # Main exports
├── notifications.ts   # Custom notification types
├── request.ts         # Custom request types
└── virtual.ts         # Virtual file utilities
```

## Dependencies

- `vscode-languageserver-protocol` - LSP types

## License

MIT

