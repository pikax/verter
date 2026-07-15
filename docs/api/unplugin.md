# @verter/unplugin

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Universal bundler plugin for Verter framework carriers. The existing default and per-bundler entry points remain Vue-pinned drop-in replacements for `@vitejs/plugin-vue`; additive named and framework subpath exports enable Svelte or mixed projects.

## Installation

```bash
pnpm add -D @verter/unplugin
```

## Framework entry points

Existing imports such as `@verter/unplugin/vite`, `@verter/unplugin/rollup`, and the package default remain Vue-only for backward compatibility. The root package also exports three unplugin instances:

- `VerterVue` — pinned to `.vue`;
- `VerterSvelte` — pinned to `.svelte`;
- `Verter` — auto-detects `.vue` and `.svelte` per file, or accepts `lang` to pin one framework.

The framework subpaths re-export the corresponding pinned instance. For a Svelte Vite project:

```ts
// vite.config.ts
import { defineConfig } from "vite";
import VerterSvelte from "@verter/unplugin/sveltejs";

export default defineConfig({
  plugins: [VerterSvelte.vite()],
});
```

Install and pin `svelte@5.56.3` in the application; emitted `svelte/internal/client` imports are deliberately resolved from the application's Svelte package.

For a mixed Vue/Svelte project:

```ts
import { defineConfig } from "vite";
import { Verter } from "@verter/unplugin";

export default defineConfig({
  plugins: [Verter.vite({ lang: "auto" })],
});
```

::: warning Experimental Svelte support
Svelte support is **experimental — not yet validated in real-world use**. The client compiler, external scoped CSS, dependency routing, and preview mount path are integrated. Unsupported runtime surfaces, including unavailable server output, fail closed with typed diagnostics rather than producing successful empty modules.
:::

## Bundler Setup

Each bundler has a dedicated entry point that exports the corresponding unplugin instance.

### Vite

```ts
// vite.config.ts
import Verter from "@verter/unplugin/vite";

export default defineConfig({
  plugins: [
    Verter({
      // options
    }),
  ],
});
```

The Vite plugin registers as `vite:vue`, matching `@vitejs/plugin-vue`, so downstream plugins that discover the Vue plugin by name (e.g., `unplugin-vue-macros`, `unplugin-vue-i18n`) work correctly.

### Rollup

```ts
// rollup.config.js
import Verter from "@verter/unplugin/rollup";

export default {
  plugins: [
    Verter({
      // options
    }),
  ],
};
```

### webpack

```ts
// webpack.config.js
import Verter from "@verter/unplugin/webpack";

export default {
  plugins: [
    Verter({
      // options
    }),
  ],
};
```

### esbuild

```ts
import { build } from "esbuild";
import Verter from "@verter/unplugin/esbuild";

build({
  plugins: [
    Verter({
      // options
    }),
  ],
});
```

### Rspack

```ts
// rspack.config.js
import Verter from "@verter/unplugin/rspack";

export default {
  plugins: [
    Verter({
      // options
    }),
  ],
};
```

### Rolldown

```ts
// rolldown.config.js
import Verter from "@verter/unplugin/rolldown";

export default {
  plugins: [
    Verter({
      // options
    }),
  ],
};
```

### Farm

```ts
// farm.config.ts
import Verter from "@verter/unplugin/farm";

export default defineConfig({
  plugins: [
    Verter({
      // options
    }),
  ],
});
```

## Options

### `VerterPluginOptions`

```ts
import type { VerterPluginOptions } from "@verter/unplugin";
```

| Option              | Type                                                            | Default         | Description                                                              |
| ------------------- | --------------------------------------------------------------- | --------------- | ------------------------------------------------------------------------ |
| `lang`              | `"auto" \| "vue" \| "sveltejs"`                            | entry-dependent | Framework selection; the named `Verter` instance defaults to `auto`      |
| `include`           | `string \| RegExp \| (string \| RegExp)[]`                      | entry-dependent | File patterns to include                                                  |
| `componentId`       | `(filename: string, source: string, isProd: boolean) => string` | hash-based      | Custom component ID generator                                             |
| `preCompile`        | `boolean`                                                       | `false`         | Pre-compile selected carrier files during `buildStart()` for cache warming |
| `crossFileOptimize` | `boolean`                                                       | `false`         | Vue-only prop constness optimization; requires `preCompile: true`         |
| `template`          | `object`                                                        | --              | Vue template options compatible with `@vitejs/plugin-vue`                 |

An `Options` type alias is also exported for compatibility with code importing `Options` from `@vitejs/plugin-vue`.

### `include`

Controls which files the plugin processes. The legacy/default and `VerterVue` entries match `.vue`; `VerterSvelte` matches `.svelte`; `Verter` in auto mode matches both.

```ts
Verter({
  // Only process .vue files in src/
  include: [/src\/.*\.vue$/],
});
```

When a string is provided, it matches via `filename.endsWith(pattern)`. When a `RegExp` is provided, it matches via `pattern.test(filename)`. An array applies "any match" (OR) logic.

### `componentId`

Overrides the default hash-based component ID generation. The component ID is used for scoped CSS attribute selectors (`data-v-<id>`) and HMR tracking.

```ts
Verter({
  componentId(filename, source, isProd) {
    // Custom ID based on filename
    return createHash("sha256").update(filename).digest("hex").slice(0, 8);
  },
});
```

**Parameters:**

- `filename` -- Absolute path to the `.vue` file
- `source` -- The raw SFC source text
- `isProd` -- Whether this is a production build

**Returns:** A string used as the component scope ID.

### `preCompile`

When enabled, scans the project root for all carrier files selected by the active framework entry during `buildStart()` and compiles them upfront. Subsequent `transform()` calls for the same unchanged content get cache hits from the host.

```ts
Verter({
  preCompile: true,
});
```

**Architecture details:**

1. During `buildStart()`, recursively scans the project root for `.vue` files, excluding `node_modules` and dot-directories.
2. For each file: upserts it into the in-memory host, resolves external `src` attributes (e.g., `<style src="./foo.less">`) and macro type dependencies (e.g., `import type { Props } from './types'` used in `defineProps<Props>()`), then triggers compilation.
3. When `transform()` later receives the same content, the host detects the content match via internal hashing and returns the cached result instantly.
4. If another plugin modifies the file before `transform()`, the host detects the content change and recompiles.
5. Third-party `.vue` files in `node_modules` compile on-demand during `transform()` -- no pre-compilation overhead for dependencies.

### `crossFileOptimize`

Enables cross-file prop constness optimization. **Requires `preCompile: true`.** Only effective in production builds.

After all files are pre-compiled, analyzes the render tree to determine which props are always passed constant values by every parent component. Those props skip dynamic tracking in the compiled output, reducing runtime overhead.

```ts
Verter({
  preCompile: true,
  crossFileOptimize: true,
});
```

### `template`

Template compiler options, accepted for compatibility with `@vitejs/plugin-vue`. Currently only `compilerOptions.isCustomElement` is forwarded to the Rust compiler.

```ts
Verter({
  template: {
    compilerOptions: {
      isCustomElement: (tag) => tag.startsWith("my-"),
    },
  },
});
```

## Vite-Specific Behavior

When used with Vite, the plugin emits compiled output as script sub-requests (matching `@vitejs/plugin-vue`'s architecture):

- **Script**: `?vue&type=script&lang.ts` -- processed by `vite:esbuild` for TS stripping
- **Style**: `?vue&type=style&lang.less` -- processed by Vite's CSS pipeline (SCSS/Less/Stylus preprocessing)

This ensures downstream plugins (`@vitejs/plugin-vue-jsx`, external globals, etc.) receive properly processed JavaScript.

For non-Vite bundlers (webpack, Rspack, esbuild, Rollup, Rolldown, Farm), the plugin inlines everything into a single module and handles TypeScript stripping internally via the `forceJs` compile profile flag.

## HMR

Hot Module Replacement strategy is auto-detected from the bundler framework:

| Framework             | HMR Strategy |
| --------------------- | ------------ |
| Vite, Rolldown        | `"vite"`     |
| webpack, Rspack       | `"webpack"`  |
| esbuild, Rollup, Farm | `"none"`     |

In production builds, HMR is always disabled regardless of the bundler.

## Build Timing

Set `VERTER_TIMING=1` to enable per-phase build timing instrumentation:

```bash
VERTER_TIMING=1 vite build
```

This logs upsert, dependency resolution, and compile timings for all `.vue` files, plus host-level metrics (cache hit rates, Rust parse/compile totals).
