# @verter/oxc-bindings

Helper package for downloading and resolving platform-specific OXC parser binaries.

## Overview

`@verter/oxc-bindings` provides utilities to:

- Detect the current platform (OS and architecture)
- Download the appropriate OXC parser binary
- Resolve the binding path for runtime use

[OXC](https://oxc-project.github.io/) is a high-performance JavaScript parser written in Rust that Verter uses for fast AST parsing.

## Installation

```bash
pnpm add @verter/oxc-bindings
```

## Usage

### Downloading Bindings

```typescript
import { resolveAndDownloadBinding } from "@verter/oxc-bindings";

// Download OXC binary to extension path
await resolveAndDownloadBinding("/path/to/extension");
```

### Resolving Binding Path

```typescript
import { resolveBinding } from "@verter/oxc-bindings";

// Get path to OXC binary for current platform
const bindingPath = resolveBinding();
```

## Platform Support

| Platform | Architecture | Binary |
|----------|--------------|--------|
| macOS | x64 | `darwin-x64` |
| macOS | arm64 | `darwin-arm64` |
| Windows | x64 | `win32-x64` |
| Linux | x64 | `linux-x64-gnu` |
| Linux | arm64 | `linux-arm64-gnu` |

## Architecture

```
src/
├── index.ts           # Main exports
├── download.ts        # Binary download logic
└── resolveBinding.ts  # Platform detection and resolution
```

### Download Process

1. Detect current platform and architecture
2. Check if binary already exists
3. Download from OXC releases if needed
4. Verify binary integrity
5. Make binary executable (Unix)

## Why OXC?

Verter uses OXC alongside Babel for JavaScript parsing because:

- **Speed**: OXC is significantly faster than pure JavaScript parsers
- **Memory**: Lower memory usage for large files
- **Compatibility**: Produces Babel-compatible AST

## Error Handling

```typescript
try {
  await resolveAndDownloadBinding(extensionPath);
} catch (error) {
  // Fall back to Babel parser
  console.warn("OXC binding not available, using Babel");
}
```

## License

MIT
