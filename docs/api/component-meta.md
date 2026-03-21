# `@verter/component-meta`

`@verter/component-meta` exposes native-backed Vue component metadata. It does not support the removed JS extractor pipeline.

Supported public surfaces:

- `@verter/component-meta`: pooled project/session API
- `@verter/component-meta/compat`: `vue-component-meta`-compatible checker API
- `@verter/component-meta/browser`: browser-safe entrypoint for shared types and Type IR helpers

## Install

```bash
npm install @verter/component-meta
# or
pnpm add @verter/component-meta
```

`@verter/native` is required for Node.js usage. `@verter/wasm` is optional for browser-only consumers.

## Project API

Use the root package for first-class metadata access.

```ts
import { openMetaProject } from "@verter/component-meta";

const project = await openMetaProject({
  root: ".",
  tsconfig: "./tsconfig.json",
});

try {
  const meta = await project.getComponentMeta("./src/MyButton.vue");
  console.log(meta.props);
  console.log(meta.events);
  console.log(meta.slots);
  console.log(meta._verter?.styles);
} finally {
  project.close();
}
```

### Root exports

- `openMetaProject(config)`
- `MetaProject`
- `evictMetaProject(config)`
- `shutdownMetaRuntime()`

### `MetaProject`

- `getComponentMeta(filePath)`
- `getExportNames(filePath)`
- `updateFile(filePath, source)`
- `deleteFile(filePath)`
- `reload()`
- `clearCaches()`
- `close()`

`getComponentMeta()` returns Volar-shaped top-level metadata and attaches the full Verter-native metadata object on `_verter`.

## Compat API

Use the compat entrypoint only when you need `vue-component-meta` API compatibility.

```ts
import { createChecker } from "@verter/component-meta/compat";

const checker = await createChecker("./tsconfig.json");

try {
  const meta = await checker.getComponentMeta("./src/MyButton.vue");
  console.log(meta.props);
  console.log(meta._verter?.components);
} finally {
  checker.dispose();
}
```

### Compat exports

- `createChecker(tsconfigPath, options?)`
- `createCheckerByJson(root, config, options?)`
- `ComponentMetaChecker`
- `typeDescriptorToSchema(type, options?, registry?)`
- `typeDescriptorToString(type)`

The compat path is the only compatibility surface this package supports.

## Returned Metadata

Both metadata surfaces return Volar-shaped top-level fields:

- `props`
- `events`
- `slots`
- `exposed`

The `_verter` extension carries the full native metadata payload:

- `props`
- `events`
- `slots`
- `models`
- `exposed`
- `components`
- `templateRefs`
- `imports`
- `bindings`
- `vueApiCalls`
- `styles`
- `flags`

## Type IR And Adapters

The package also exports:

- `TypeDescriptor` types and constructors from `./type-ir`
- adapter transforms for Storybook, Histoire, Zod, and JSON Schema

These operate on native metadata results. They do not add a second parsing or type-evaluation pipeline.

## Browser Entry

`@verter/component-meta/browser` exposes shared types and Type IR helpers without the Node.js runtime/session API.

## Removed Public Surfaces

The following legacy public APIs are removed and unsupported:

- `extractComponentMeta`
- `snapshotToMeta`
- `parseType`
- `runtimeTypeToDescriptor`
- `createAdapter`
- `createNapiAdapter`
- `createWasmAdapter`
- `wrapNapiHost`
- `wrapWasmHost`
