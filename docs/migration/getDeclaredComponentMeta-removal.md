# Removing `getDeclaredComponentMeta`

`@verter/native` no longer exports `getDeclaredComponentMeta`. The same
information is now produced by projecting the canonical
`getComponentMeta(...)` payload through one of two helpers exposed by
`@verter/component-meta/compat`:

- `projectDeclaredOnlyNativeResult(meta)` — for callers that already
  hold a decoded `NativeComponentMetaResult` (typically because they
  use `@verter/component-meta`'s `ProjectSession`, which decodes the
  protobuf payload internally).
- `projectDeclaredOnlyFromNativePayload(payload)` — for callers
  holding a raw `Buffer` payload (typically because they use the raw
  `@verter/native` `ComponentMetaSession` directly). This helper
  decodes the buffer and then delegates to
  `projectDeclaredOnlyNativeResult`.

Both helpers return `NativeComponentMetaResult | null`. They never
produce a Volar shape — Volar mapping stays at the caller via
`nativeComponentMetaToComponentMeta` plus `mapComponentMeta(options,
typeRegistry)` exactly as before.

## Why

`getDeclaredComponentMeta` forked the cache identity of the canonical
component-meta query. Two distinct query modes meant the resolver
maintained two independent cold-paths and dependency-edge accounting
families. Folding the declared projection into a TS-side projection on
top of the canonical query keeps a single Rust query identity and a
single warm cache.

## Migration

### Raw `@verter/native` `ComponentMetaSession` (Buffer payload)

```ts
// Before
const session: ComponentMetaSession = host.openComponentMetaSession()
const buffer = session.getDeclaredComponentMeta('/project/src/Button.vue')
// `buffer` was a `Buffer | null` carrying the declared-only payload.

// After
import { projectDeclaredOnlyFromNativePayload } from '@verter/component-meta/compat'

const session: ComponentMetaSession = host.openComponentMetaSession()
const payload = session.getComponentMeta('/project/src/Button.vue')
const declared = projectDeclaredOnlyFromNativePayload(payload)
// `declared` is `NativeComponentMetaResult | null` — already decoded
// and projected to the declared-only surface.
```

### `@verter/component-meta` `ProjectSession` (decoded result)

```ts
// Before
const session: ProjectSession = engine.openSession()
const meta = session.getDeclaredComponentMeta('/project/src/Button.vue')
// `meta` was the decoded declared-only `NativeComponentMetaResult`.

// After
import { projectDeclaredOnlyNativeResult } from '@verter/component-meta/compat'

const session: ProjectSession = engine.openSession()
const fullMeta = session.getComponentMeta('/project/src/Button.vue')
const meta = projectDeclaredOnlyNativeResult(fullMeta)
// `meta` is the decoded declared-only `NativeComponentMetaResult`.
```

### Producing a Volar shape

If a caller previously fed `getDeclaredComponentMeta` into the Volar
mappers, the post-migration shape is a straight composition:

```ts
import {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from '@verter/component-meta'
import {
  mapComponentMeta,
  projectDeclaredOnlyFromNativePayload,
} from '@verter/component-meta/compat'

const session: ComponentMetaSession = host.openComponentMetaSession()
const declared = projectDeclaredOnlyFromNativePayload(
  session.getComponentMeta('/project/src/Button.vue'),
)
if (declared) {
  const typeRegistry = nativeTypeRegistryToMap(declared)
  const componentMeta = nativeComponentMetaToComponentMeta(declared)
  const volar = mapComponentMeta(componentMeta, options, typeRegistry)
}
```

The Volar output is byte-for-byte equivalent to the pre-change
`getDeclaredComponentMeta` output once it has been run through
`mapComponentMeta`.

## What stays unchanged

- `getComponentMeta(...)` — the canonical query.
- `getComponentMetaWithAudit(...)` — synchronous audit bundle.
- `getResolvedComponentMeta(...)` — resolved-surface query (now
  formally typed in `packages/native/dist/index.d.ts`).
- `mapComponentMeta`, `nativeComponentMetaToComponentMeta`, and the
  Volar shape — these continue to be the recommended way to render
  Volar-compatible metadata.

## Trade-offs

- The new path computes the canonical (with-fallthrough) component
  meta and then projects to the declared surface. If your workload was
  dominated by inheritance-heavy components, expect a small additional
  cost compared to the prior declared-only path; the steady-state
  warm-cache hit on `getComponentMeta` recovers that on the second
  call.
- The declared-only payload-cache slot is gone. Both helpers operate
  on the in-memory decoded `NativeComponentMetaResult` produced from
  the canonical payload cache; consumers do not need a separate cache
  layer.
