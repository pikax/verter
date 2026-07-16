# @verter/typescript-plugin

TypeScript plugin for resolving **carrier** imports (`.vue` / `.svelte`) through Verter's generated public API surface.

> [!WARNING]
> This package is experimental. Virtual file suffixes and configuration may change.

## Overview

`@verter/typescript-plugin` intercepts TypeScript module resolution for carrier components and serves Verter-generated declaration snapshots instead of the IDE TSX output.

**Vue**

- Normal files resolve `Foo.vue` to the public API virtual (`Foo.vue.verter.ts` / accepted `Foo.vue.d.ts` spelling)
- Test files can resolve `Foo.vue` to `Foo.vue.__verter_test.ts` when `exposeBindingsTesting` is on
- Source maps are cached so go-to-definition and auto-import cleanup still point back to the original `.vue`

**Svelte** (first-class, equal routing — different testing contract)

- Normal and test files resolve `Foo.svelte` to the **same** public API virtual (`Foo.svelte.verter.ts`)
- There is **no** `Foo.svelte.__verter_test.ts` (the adapter `testing_api_suffix` is null by design today)

### Vue testing surface (VTU-style)

When enabled for Vue, the testing surface is meant to match Vue Test Utils `wrapper.vm` property access:

- internal `<script setup>` bindings are visible on the instance
- `defineExpose()` does not narrow that testing surface
- the normal public surface is unchanged for non-test imports

## Installation

```bash
pnpm add -D @verter/typescript-plugin
```

Add the plugin to your `tsconfig.json` or `jsconfig.json`:

```jsonc
{
  "compilerOptions": {
    "plugins": [{ "name": "@verter/typescript-plugin" }],
  },
}
```

Enable the test-aware surface explicitly when you want `.vue` imports in test files to expose internal bindings:

```jsonc
{
  "compilerOptions": {
    "plugins": [
      {
        "name": "@verter/typescript-plugin",
        "exposeBindingsTesting": true,
      },
    ],
  },
}
```

If you use the Verter VS Code extension, the plugin is configured automatically. The extension only forwards `verter.experimental.exposeBindingsTesting` when you set it explicitly, so `tsconfig.json` can remain the project default.

## Test-aware Resolution

With `"exposeBindingsTesting": true`, the plugin classifies importers using:

- filename heuristics: `*.spec.*`, `*.test.*`, `__tests__/`, `__specs__/`
- nearest Vitest/Vite config include patterns when they can be read
- nearest Jest `testMatch` / `testRegex` patterns when they can be read
- heuristic fallback if config parsing is unsupported or times out

That lets the **same Vue component** coexist with two type shapes in one program:

- app code gets the normal public instance
- test code gets the VTU-style debug instance

Svelte components always use a single public shape for both app and test importers, even when the flag is on (safe in mixed workspaces).

## VS Code

The Verter VS Code extension configures the plugin through `_typescript.configurePlugin`.

Use this setting to opt into the **Vue** testing surface from the editor (Svelte public types unchanged):

```jsonc
{
  "verter.experimental.exposeBindingsTesting": true,
}
```

User-facing docs: [Experimental Features → Expose Bindings Testing](https://verterjs.dev/guide/experimental.html#expose-bindings-testing-spects--test-importers) (or `docs/guide/experimental.md` in-repo).

## Development

```bash
pnpm --filter @verter/typescript-plugin build
pnpm --filter @verter/typescript-plugin test
```

Restart the TypeScript server after rebuilding the plugin.
