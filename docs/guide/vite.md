# Vite Integration

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter provides a Vite plugin that serves as a drop-in replacement for `@vitejs/plugin-vue`. It registers itself as `vite:vue` so that downstream plugins (such as `unplugin-vue-macros` and `unplugin-vue-i18n`) that discover the Vue plugin by name continue to work.

## Installation

::: code-group

```sh [pnpm]
pnpm add -D @verter/unplugin
```

```sh [npm]
npm install -D @verter/unplugin
```

```sh [yarn]
yarn add -D @verter/unplugin
```

:::

## Basic Configuration

```ts
// vite.config.ts
import { defineConfig } from 'vite'
import VerterVite from '@verter/unplugin/vite'

export default defineConfig({
  plugins: [VerterVite()],
})
```

## Options

All options are optional. The plugin works with zero configuration for standard Vue projects.

### `include`

**Type:** `string | RegExp | (string | RegExp)[]`
**Default:** `[/\.vue$/]`

File patterns to include for Vue SFC compilation. Matches the `include` option from `@vitejs/plugin-vue`.

```ts
VerterVite({
  include: [/\.vue$/, /\.custom\.vue$/],
})
```

### `componentId`

**Type:** `(filename: string, source: string, isProd: boolean) => string`
**Default:** Hash-based ID generation

Custom component ID generator. The ID is used for scoped CSS (`data-v-xxxxxxxx`) and HMR tracking.

```ts
VerterVite({
  componentId: (filename, source, isProd) => {
    // Return a unique string ID for this component
    return myCustomHash(filename)
  },
})
```

### `preCompile`

**Type:** `boolean`
**Default:** `false`

Pre-compile all `.vue` files during Vite's `buildStart` phase. This scans the project root for `.vue` files (excluding `node_modules` and dot-directories), compiles each one, and populates an internal cache. When Vite's `transform()` hook later processes the same file, it gets an instant cache hit if the source has not been modified by other plugins.

```ts
VerterVite({
  preCompile: true,
})
```

This is particularly useful for large projects where you want to front-load compilation work. Files in `node_modules` are excluded from scanning and compile on-demand during `transform()`.

### `crossFileOptimize`

**Type:** `boolean`
**Default:** `false`

Enable cross-file prop constness optimization. Requires `preCompile: true`. Only effective in production builds.

After all files are pre-compiled, Verter analyzes the render tree to determine which props are always passed constant values by every parent component. Those props skip dynamic tracking in the compiled output, reducing runtime overhead.

```ts
VerterVite({
  preCompile: true,
  crossFileOptimize: true,
})
```

### `template`

**Type:** `object`
**Default:** `undefined`

Template compiler options. Accepted for compatibility with `@vitejs/plugin-vue`, but currently only `isCustomElement` is forwarded to the Rust template compiler.

```ts
VerterVite({
  template: {
    compilerOptions: {
      isCustomElement: (tag) => tag.startsWith('my-'),
    },
  },
})
```

## Migration from @vitejs/plugin-vue

1. Install `@verter/unplugin`:

   ```sh
   pnpm add -D @verter/unplugin
   ```

2. Update your Vite config:

   ```ts
   // Before
   import vue from '@vitejs/plugin-vue'

   export default defineConfig({
     plugins: [vue()],
   })

   // After
   import VerterVite from '@verter/unplugin/vite'

   export default defineConfig({
     plugins: [VerterVite()],
   })
   ```

3. Optionally remove the old plugin:

   ```sh
   pnpm remove @vitejs/plugin-vue
   ```

The `template.compilerOptions.isCustomElement` option carries over directly. Other `@vitejs/plugin-vue` options (such as `reactivityTransform` or `script.defineModel`) are handled natively by Verter and do not need explicit configuration.

## HMR Behavior

Verter supports Vite's Hot Module Replacement out of the box. When you edit a `.vue` file:

- **Template changes** trigger a component re-render without full page reload.
- **Script changes** trigger a full component reload via HMR.
- **Style changes** are applied instantly via Vite's CSS HMR.

## Build Timing

Set the `VERTER_TIMING` environment variable to see per-file compilation timing in the terminal output:

```sh
VERTER_TIMING=1 pnpm build
```

This logs the time spent compiling each `.vue` file, which is useful for identifying slow components in large projects.
