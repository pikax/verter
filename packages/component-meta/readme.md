# @verter/component-meta

Extract Vue component metadata through Verter's native host/runtime.

This package supports two public metadata surfaces:

- `@verter/component-meta`: pooled project/session API
- `@verter/component-meta/compat`: `vue-component-meta`-compatible checker API

Legacy JS extraction, JS type parsing, and adapter-driven metadata entrypoints are not supported.

## Install

```bash
npm install @verter/component-meta
# or
pnpm add @verter/component-meta
```

`@verter/native` is required for Node.js usage. `@verter/wasm` is optional for browser consumers that only need the browser entrypoint and shared type utilities.

## Project API

Use the root package when you want a pooled native runtime and direct project/session control.

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

  // Full Verter-native metadata is available on the extension payload.
  console.log(meta._verter?.models);
  console.log(meta._verter?.components);
  console.log(meta._verter?.templateRefs);
  console.log(meta._verter?.imports);
  console.log(meta._verter?.bindings);
  console.log(meta._verter?.vueApiCalls);
  console.log(meta._verter?.styles);
  console.log(meta._verter?.flags);
} finally {
  project.close();
}
```

### MetaProject API

- `openMetaProject(config)`: open or reuse a pooled native runtime
- `project.getComponentMeta(filePath)`: get metadata for a component
- `project.updateFile(filePath, source)`: apply an in-memory overlay
- `project.deleteFile(filePath)`: delete an in-memory overlay file
- `project.reload()`: re-read touched files from disk
- `project.clearCaches()`: clear host/runtime caches
- `project.close()`: close the session
- `evictMetaProject(config)`: evict a pooled engine
- `shutdownMetaRuntime()`: stop all pooled engines

## Compat API

Use `./compat` when you need the `vue-component-meta` API shape.

```ts
import { createChecker } from "@verter/component-meta/compat";

const checker = await createChecker("./tsconfig.json");

try {
  const meta = await checker.getComponentMeta("./src/MyButton.vue");

  console.log(meta.props);
  console.log(meta.events);
  console.log(meta.slots);

  // Full native metadata is still exposed on the extension payload.
  console.log(meta._verter?.components);
} finally {
  checker.dispose();
}
```

You can also create a checker from inline config:

```ts
import { createCheckerByJson } from "@verter/component-meta/compat";

const checker = await createCheckerByJson("/project/root", {
  include: ["src/**/*.vue"],
  compilerOptions: { strict: true },
});
```

### Compat API

- `createChecker(tsconfigPath, options?)`
- `createCheckerByJson(root, config, options?)`
- `ComponentMetaChecker`

The compat path is the only compatibility surface this package supports. It matches `vue-component-meta` shape while delegating parsing, analysis, import following, type resolution, and caching to native Verter.

Compat schema notes:

- `schema: true` keeps the default consumer-facing behavior.
- `schema: { literalBooleanSchema: true }` enables benchmark/parity-focused `true | false` enum schema expansion without changing native types or compat display strings.

## Native Vs Compat Contract

The official Verter metadata payload is the semantic source of truth. `./compat` is an interoperability projection for `vue-component-meta`, not a second semantic engine.

- Native `_verter` metadata may include more information than Volar, including `models`, `publicInstance`, `acceptedProps`, `acceptedEvents`, `acceptedSurfaceCompleteness`, `rootReachability`, and `fallthroughSurface`.
- Native type descriptors preserve TypeScript meaning. For example, native `boolean` stays `boolean`; any Volar-specific display or schema normalization belongs in compat formatting, not the native payload.
- Benchmark comparisons against `vue-component-meta` should compare equivalent surfaces only. Native-only `_verter` fields are additional Verter metadata, not compat regressions by themselves.
- Current parity totals explicitly exclude native-only `models` because `vue-component-meta` has no equivalent surface for them.

## Root And Attrs Metadata

Native Verter metadata already models root fallthrough and root attrs/listeners:

- `acceptedProps` includes inherited attrs with `kind: "attr"`
- `acceptedEvents` includes inherited listeners with `kind: "listener"`
- `rootReachability` and `fallthroughSurface` describe single-root / multi-root / conditional-root behavior and root target kind
- `rootInfo` is a first-class summary with `kind: "none" | "single" | "conditional" | "multiple"` plus direct root targets when known
- consumed root attrs/listeners live on `rootReachability.branches[].consumed`

This is part of the native API, not compat-only behavior.

## Returned Metadata

Both supported metadata APIs return Volar-shaped top-level metadata and attach the full Verter-native metadata on `_verter`.

Compat `exposed` stays on the analysis `defineExpose` / Options API `expose` surface. The native `publicInstance` sidecar is available separately on `_verter.publicInstance` for ref-accessible runtime instance data.

The `_verter` payload includes:

- props
- events
- slots
- models
- exposed
- publicInstance
- typeRegistry
- acceptedProps
- acceptedEvents
- acceptedSurfaceCompleteness
- rootInfo
- rootReachability
- fallthroughSurface
- components
- template refs
- imports
- bindings
- Vue API calls
- styles
- component flags

`components` preserves call-site detail from template analysis, including prop expressions, referenced bindings, spread markers, and structured `vModelEntries` alongside the existing summary fields.

`typeRegistry` keeps both the expanded native type descriptor and the original declaration provenance when the host can resolve it, including `rawType` source text and `declaration.canonicalSource`.

## Type IR And Adapters

The package also exports:

- `TypeDescriptor` helpers from `./type-ir`
- compat schema helpers from `./compat`
- adapters for Storybook, Histoire, Zod, and JSON Schema

These operate on native metadata results. They do not reintroduce JS parsing or JS type evaluation.

## Browser Entry

`@verter/component-meta/browser` is the browser-safe entrypoint for shared types and Type IR helpers. It does not expose the Node.js project/session API.

## Audit-Only Fields

`meta.origin` (the origin/derivation graph) is populated only when the host is constructed with both `audit_enabled: true` and `footprint_capture: true`. With the default configuration this field is `undefined`. The contract matches the LSP hover-provenance gate: provenance data is captured only when the audit infrastructure is enabled.

## Removed Public Surfaces

These legacy public APIs are not supported:

- `extractComponentMeta`
- `snapshotToMeta`
- `parseType`
- `runtimeTypeToDescriptor`
- `createAdapter`
- `createNapiAdapter`
- `createWasmAdapter`
- `wrapNapiHost`
- `wrapWasmHost`

## License

MIT
