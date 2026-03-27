# Cross-File Optimization

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter's cross-file optimization is a whole-program analysis pass that eliminates unnecessary runtime tracking for props that are always passed constant values. This produces smaller bundles and faster runtime performance.

## Overview

In a standard Vue build, every prop is tracked dynamically at runtime -- Vue checks whether the value has changed on each render and patches the DOM accordingly. But in many real-world applications, some props are always passed static values across all parent components:

```vue
<!-- Every usage of AppHeader always passes a constant title -->
<AppHeader title="Dashboard" />
<AppHeader title="Settings" />
<AppHeader title="Profile" />
```

When Verter can statically verify that a prop receives a constant value at every call site, it marks that prop as constant in the child's compiled output. In VDOM mode, this removes patch flags for those props. In Vapor mode, it removes the corresponding `renderEffect` wrappers. The result is less work at runtime.

## Enabling Cross-File Optimization

Cross-file optimization requires the `preCompile` option to be enabled, since Verter needs to compile all components before it can analyze the full render tree.

```ts
// vite.config.ts
import { defineConfig } from "vite";
import VerterVite from "@verter/unplugin/vite";

export default defineConfig({
  plugins: [
    VerterVite({
      preCompile: true,
      crossFileOptimize: true,
    }),
  ],
});
```

## How It Works

The optimization runs in three stages:

### 1. Pre-Compilation

When `preCompile: true` is set, Verter scans your project for all `.vue` files during `buildStart()` (excluding `node_modules` and dot-directories). Each file is compiled and cached in the in-memory host. This builds a complete picture of every component in your application.

### 2. Render Tree Analysis

After pre-compilation, Verter constructs the render tree -- a graph of which components render which other components, and what prop values they pass. For each child component, it collects every prop binding from every parent.

### 3. Constness Determination

For each prop on each component, Verter checks whether every parent always passes a constant value. A value is considered constant if it is:

- A string literal (`title="Dashboard"`)
- A number literal (`:count="42"`)
- A boolean literal (`:active="true"` or just `active`)
- A static expression that does not reference any reactive bindings

If ALL call sites pass a constant value for a given prop, that prop is marked as constant in the child's compiled output.

## What Changes in the Output

### VDOM Mode

In standard compilation, a component with dynamic props includes patch flags that tell Vue to check those props on every render:

```js
// Without cross-file optimization
_createVNode(AppHeader, { title: "Dashboard" }, null, 8 /* PROPS */, ["title"]);
```

With cross-file optimization, when `title` is always constant:

```js
// With cross-file optimization
_createVNode(AppHeader, { title: "Dashboard" });
```

The patch flag and dynamic prop list are removed, so Vue skips diffing `title` entirely.

### Vapor Mode

In Vapor mode, dynamic props are wrapped in `renderEffect` so Vue can re-execute them when dependencies change:

```js
// Without cross-file optimization
_renderEffect(() => {
  _setDynamicProp(n0, "title", "Dashboard");
});
```

With cross-file optimization:

```js
// With cross-file optimization
_setDynamicProp(n0, "title", "Dashboard");
```

The `renderEffect` wrapper is removed, and the prop is set once during initialization.

## Requirements and Limitations

- **Requires `preCompile: true`** -- The full component render tree must be available before analysis can run.
- **Production builds only** -- Cross-file optimization is intended for production builds where the full application is compiled together.
- **Application code only** -- Third-party components in `node_modules` are compiled on-demand during `transform()` and are not included in the render tree analysis.
- **Conservative analysis** -- If any call site passes a dynamic value for a prop, the prop is treated as dynamic everywhere. The optimization only applies when ALL usages are constant.

## Next Steps

- [Performance](./performance) -- Compilation benchmark results
- [Architecture](./architecture) -- How the compiler pipeline works
- [Getting Started](./getting-started) -- Install and configure Verter
