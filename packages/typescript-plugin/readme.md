# @verter/typescript-plugin

TypeScript plugin that enables `.vue` file import resolution in TypeScript/JavaScript files.

## Overview

`@verter/typescript-plugin` intercepts TypeScript's module resolution to handle `.vue` imports. When you import a Vue component in a `.ts` or `.tsx` file, this plugin:

1. Detects the `.vue` import
2. Parses and transforms the SFC using `@verter/core`
3. Returns the generated type declarations to the TypeScript language service

This enables full type inference for Vue components in non-Vue files.

> **Note**: The plugin generates typed representations for TypeScript analysis — the output is used for type-checking and IDE features, not for runtime execution.

## Installation

```bash
pnpm add -D @verter/typescript-plugin
```

## Configuration

Add the plugin to your `tsconfig.json`:

```json
{
  "compilerOptions": {
    "plugins": [
      {
        "name": "@verter/typescript-plugin"
      }
    ]
  }
}
```

## How It Works

### Module Resolution

The plugin intercepts TypeScript's module resolution:

```typescript
// TypeScript 5.x
languageServiceHost.resolveModuleNameLiterals = (moduleNames, containingFile) => {
  return moduleNames.map(({ text: moduleName }) => {
    if (isVue(moduleName)) {
      // Parse .vue file and return resolved module
      return { resolvedModule: parseVueModule(moduleName) };
    }
    // Fall back to default resolution
    return defaultResolve(moduleName);
  });
};

// TypeScript 4.x (legacy support)
languageServiceHost.resolveModuleNames = (moduleNames, containingFile) => {
  // Similar handling for older TS versions
};
```

### SFC Transformation

When a `.vue` import is detected:

1. The plugin reads the `.vue` file
2. Parses it using `@verter/core`
3. Transforms it to TypeScript
4. Caches the result for performance
5. Returns type information to TypeScript

### Type Inference

For a Vue component like:

```vue
<script setup lang="ts">
defineProps<{
  message: string;
  count?: number;
}>();

defineEmits<{
  (e: 'update', value: number): void;
}>();
</script>
```

The plugin generates type information allowing:

```typescript
import MyComponent from './MyComponent.vue';

// Full type inference for props and events
<MyComponent 
  message="hello"  // ✓ Required
  count={5}        // ✓ Optional
  onUpdate={(v) => {}} // ✓ Event handler
/>;
```

## Architecture

```
src/
├── index.ts           # Plugin entry point
└── helpers/
    ├── utils.ts       # Utility functions
    └── getDtsSnapshot.ts # SFC parsing and caching
```

### Plugin Factory

```typescript
const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  return {
    create(info: tsModule.server.PluginCreateInfo) {
      // Create proxied language service host
      const languageServiceHost = new Proxy(info.languageServiceHost, {
        get(target, key) {
          // Override module resolution methods
          if (key === 'resolveModuleNameLiterals') {
            return customResolver;
          }
          return target[key];
        }
      });
      
      // Return language service with custom host
      return ts.createLanguageService(languageServiceHost);
    }
  };
};
```

## Usage with VS Code

The Verter VS Code extension automatically configures this plugin when you open a Vue project:

```typescript
// In extension activation
commands.executeCommand(
  "_typescript.configurePlugin",
  require.resolve("@verter/typescript-plugin"),
  { enable: true }
);
```

## Caching

The plugin caches parsed Vue files for performance:

- File content hash-based cache invalidation
- Watches for file changes
- Clears cache on project reload

## Compatibility

| TypeScript Version | Support |
|-------------------|---------|
| 5.x | Full support via `resolveModuleNameLiterals` |
| 4.x | Support via `resolveModuleNames` |

## Dependencies

- `typescript` - TypeScript language service types
- `@verter/core` - SFC transformation

## Troubleshooting

### Plugin Not Loading

1. Ensure the plugin is in `tsconfig.json` plugins array
2. Restart the TypeScript language server
3. Check that the package is installed

### Type Inference Issues

1. Verify `@verter/core` is installed
2. Check for parsing errors in the `.vue` file
3. Ensure the SFC uses `<script setup lang="ts">`

### Performance

For large projects:
- The plugin caches transformed files
- First load may be slower as files are parsed
- Subsequent loads use cached results

## License

MIT
