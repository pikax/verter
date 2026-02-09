# @verter/vite-plugin

> [!WARNING]
> This project is **experimental and under active development**. APIs, architecture, and package boundaries may change without notice.

Vite plugin for compiling Vue Single File Components with the Verter compiler. Drop-in replacement for `@vitejs/plugin-vue` that uses the Rust-powered Verter template compiler for fast, native-speed SFC transformation.

## Overview

`@verter/vite-plugin` integrates the Verter compiler into the Vite build pipeline. It intercepts `.vue` file requests, compiles them through `@verter/native`'s `compileForVite` API, and serves the result as split virtual modules (script, template, styles) that Vite processes through its standard CSS and TypeScript pipelines.

The plugin handles the full lifecycle: compilation with caching, virtual module resolution for style blocks, TypeScript stripping via esbuild, scoped style IDs, component metadata injection, and Hot Module Replacement (HMR) in development.

## Architecture

```mermaid
graph TD
    VueFile[".vue file"] --> Transform["Vite transform hook"]
    Transform --> Native["@verter/native<br/>compileForVite()"]
    Native --> Split["Split result"]
    Split --> Script["Script block<br/><i>component definition</i>"]
    Split --> Template["Template block<br/><i>render function</i>"]
    Split --> Styles["Style blocks<br/><i>CSS / SCSS / Less</i>"]

    Script --> Assemble["generateMainModule()"]
    Template --> Assemble
    Assemble --> Esbuild["esbuild<br/><i>strip TypeScript</i>"]
    Esbuild --> Output["Transformed JS output"]

    Styles --> VirtualModules["Virtual module IDs<br/><code>App.vue?vue&type=style&index=0&lang.css</code>"]
    VirtualModules --> ViteCSS["Vite CSS pipeline"]

    style VueFile fill:#98d898,stroke:#2e8b57
    style Native fill:#deb887,stroke:#8b6914
    style Output fill:#b0c4de,stroke:#4682b4
    style ViteCSS fill:#b0c4de,stroke:#4682b4
```

### Plugin Hook Flow

```mermaid
sequenceDiagram
    participant Vite
    participant Plugin as @verter/vite-plugin
    participant Native as @verter/native
    participant Cache as Descriptor Cache

    Note over Vite,Plugin: configResolved
    Vite->>Plugin: Resolved config (command, ssr, etc.)
    Plugin->>Plugin: Store config reference

    Note over Vite,Plugin: transform (App.vue)
    Vite->>Plugin: transform(code, "App.vue")
    Plugin->>Native: compileForVite(code, options)
    Native-->>Plugin: { script, template, styles }
    Plugin->>Cache: setDescriptor("App.vue", result)
    Plugin->>Plugin: generateMainModule() + esbuild strip
    Plugin-->>Vite: { code, map }

    Note over Vite,Plugin: resolveId (style virtual module)
    Vite->>Plugin: resolveId("App.vue?vue&type=style&index=0")
    Plugin-->>Vite: id (resolved)

    Note over Vite,Plugin: load (style virtual module)
    Vite->>Plugin: load("App.vue?vue&type=style&index=0")
    Plugin->>Cache: getDescriptor("App.vue")
    Cache-->>Plugin: { styles[0].code }
    Plugin-->>Vite: { code: cssContent }

    Note over Vite,Plugin: handleHotUpdate
    Vite->>Plugin: handleHotUpdate({ file: "App.vue" })
    Plugin->>Cache: deleteDescriptor("App.vue")
    Plugin->>Vite: ws.send({ type: "full-reload" })
```

## Installation

```bash
npm install @verter/vite-plugin
# or
pnpm add @verter/vite-plugin
```

### Peer Dependencies

| Dependency | Versions |
|------------|----------|
| `vite` | `^4.0.0 \|\| ^5.0.0 \|\| ^6.0.0 \|\| ^7.0.0` |

The `@verter/native` package is a direct dependency and will be installed automatically with the correct platform binary.

## API / Usage

### Basic Setup

```typescript
// vite.config.ts
import { defineConfig } from 'vite';
import { verter } from '@verter/vite-plugin';

export default defineConfig({
  plugins: [verter()],
});
```

### Custom Component ID

By default, Verter generates component IDs by hashing the filename (production) or filename + source content (development). You can override this with a custom generator:

```typescript
import { defineConfig } from 'vite';
import { verter } from '@verter/vite-plugin';

export default defineConfig({
  plugins: [
    verter({
      componentId: (filename, source, isProd) => {
        // Return a unique 8-character string
        return myCustomHash(filename);
      },
    }),
  ],
});
```

### Options

```typescript
interface VerterPluginOptions {
  /**
   * Custom component ID generator.
   * The component ID is used for scoped style attributes (data-v-xxxxxxxx),
   * HMR record tracking, and Vue devtools identification.
   *
   * @param filename - Absolute path to the .vue file
   * @param source   - Raw SFC source code
   * @param isProd   - Whether this is a production build
   * @returns An 8-character identifier string
   */
  componentId?: (filename: string, source: string, isProd: boolean) => string;
}
```

### Plugin Behavior

| Hook | Behavior |
|------|----------|
| `configResolved` | Captures the resolved Vite config (command mode, SSR flag, etc.) |
| `resolveId` | Resolves virtual module IDs for style block requests (`?vue&type=style&index=N`) |
| `load` | Serves cached style block content for virtual module requests |
| `transform` | Compiles `.vue` files: runs `compileForVite`, assembles the main module, strips TypeScript via esbuild |
| `handleHotUpdate` | Clears the descriptor cache and triggers a full reload for changed `.vue` files |

### Main Module Assembly

When a `.vue` file is transformed, the plugin assembles a main module from the split compilation result:

1. **Style imports** -- Virtual module import statements so Vite routes CSS through its pipeline
2. **Script block** -- Component definition (`export default` rewritten to `const _sfc_main =`)
3. **Template block** -- Compiled render function
4. **Render attachment** -- `_sfc_main.render = render`
5. **Metadata** -- `__scopeId` (scoped styles), `__file` (devtools)
6. **HMR setup** -- Development-only `import.meta.hot` handler with `__VUE_HMR_RUNTIME__` integration
7. **Export** -- `export default _sfc_main`

## Development / Build

### Building

```bash
# Build the plugin (CJS + ESM + type declarations)
pnpm run build

# Watch mode for development
pnpm run dev
```

Both commands use `tsdown`:

```bash
# Production build
tsdown src/index.ts --format cjs,esm --dts

# Development watch
tsdown src/index.ts --format cjs,esm --dts --watch
```

### Testing

```bash
pnpm test    # runs: vitest run
```

### Source Structure

```
packages/vite-plugin/src/
  index.ts    # Plugin factory, Vite hooks, compiler integration
  main.ts     # Main module assembler (generateMainModule)
  utils.ts    # Vue request parser, descriptor cache
```

## Dependencies

| Dependency | Type | Purpose |
|------------|------|---------|
| `@verter/native` | runtime | Rust template compiler (provides `compileForVite`) |
| `vite` | peer | Vite build tool (`transformWithEsbuild`, plugin types) |
| `tsdown` | dev | TypeScript bundler |
| `typescript` | dev | Type checking |
| `vitest` | dev | Test runner |

## License

ISC
