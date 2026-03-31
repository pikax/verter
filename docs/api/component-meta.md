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

Compat schema notes:

- `schema: true` keeps the default consumer-facing behavior.
- `schema: { literalBooleanSchema: true }` enables parity-focused `true | false` enum schema expansion without changing native types or compat display strings.

## Native Vs Compat Contract

The official Verter metadata payload is the semantic source of truth. The compat checker is a projection for `vue-component-meta` interoperability, not a separate semantic engine.

- Native `_verter` metadata may include more information than Volar, including `models`, `publicInstance`, `acceptedProps`, `acceptedEvents`, `acceptedSurfaceCompleteness`, `rootReachability`, and `fallthroughSurface`.
- Native type descriptors preserve TypeScript meaning. For example, native `boolean` stays `boolean`; Volar-specific display or schema normalization should not rewrite the native payload.
- Benchmark comparisons against `vue-component-meta` should compare equivalent surfaces only. Native-only `_verter` fields are additional Verter metadata, not parity regressions by themselves.
- Current parity totals explicitly exclude native-only `models` because `vue-component-meta` does not expose an equivalent surface.

## Root And Attrs Metadata

Verter's native metadata already models root fallthrough and root-bound attrs/listeners beyond what `vue-component-meta` exposes.

- `acceptedProps` includes both declared props and inherited root attrs. Use `kind: "declaredProp" | "attr"` to tell them apart.
- `acceptedEvents` includes both declared emits and inherited root listeners. Use `kind: "declaredEmit" | "listener"` to tell them apart.
- `rootReachability` tells you whether the template has no fallthrough surface, a single root, or conditional root branches.
- `rootReachability.branches[].target` and `fallthroughSurface.branches[].rootChain` describe the root target type: native element, component root, or unresolved root.
- `rootReachability.branches[].consumed.attrs` / `.listeners` record which names are consumed on the root and therefore should not fall through.
- Native/public API generation already uses this root information to type `$attrs` for native roots.

Current limitation:

- The public component-meta API does not yet expose arbitrary SFC `<template ...>` block attributes as first-class metadata. The host currently tracks template `lang` / `src` for parsing and invalidation, but not a full exported template-block attribute record.

## Returned Metadata

Both metadata surfaces return Volar-shaped top-level fields:

- `props`
- `events`
- `slots`
- `exposed`

Compat `exposed` keeps the analysis `defineExpose` / Options API `expose` semantics. The native ref-accessible runtime surface is exposed separately on `_verter.publicInstance`.

The `_verter` extension carries the full native metadata payload:

- `props`
- `events`
- `slots`
- `models`
- `exposed`
- `publicInstance`
- `typeRegistry`
- `acceptedProps`
- `acceptedEvents`
- `acceptedSurfaceCompleteness`
- `rootInfo`
- `rootReachability`
- `fallthroughSurface`
- `components`
- `templateRefs`
- `imports`
- `bindings`
- `vueApiCalls`
- `styles`
- `flags`

`components` preserves call-site detail from template analysis, including prop expressions, referenced bindings, spread markers, and structured `vModelEntries` alongside the existing summary fields.

`typeRegistry` preserves both the expanded native type descriptor and the resolved declaration provenance when available, including the pre-expansion `rawType` text and `declaration.canonicalSource`.

`rootInfo` is the coarse root summary for consumers that do not want to reconstruct it from the full branch graph. It reports `kind: "none" | "single" | "conditional" | "multiple"` and includes direct root targets when those targets are known.

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
