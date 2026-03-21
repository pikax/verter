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

## Returned Metadata

Both supported metadata APIs return Volar-shaped top-level metadata and attach the full Verter-native metadata on `_verter`.

The `_verter` payload includes:

- props
- events
- slots
- models
- exposed
- components
- template refs
- imports
- bindings
- Vue API calls
- styles
- component flags

## Type IR And Adapters

The package also exports:

- `TypeDescriptor` helpers from `./type-ir`
- compat schema helpers from `./compat`
- adapters for Storybook, Histoire, Zod, and JSON Schema

These operate on native metadata results. They do not reintroduce JS parsing or JS type evaluation.

## Browser Entry

`@verter/component-meta/browser` is the browser-safe entrypoint for shared types and Type IR helpers. It does not expose the Node.js project/session API.

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
