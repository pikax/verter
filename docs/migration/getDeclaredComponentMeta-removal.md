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
produce a Volar shape. They filter accepted props and events by declared
provenance and preserve the component's real fallthrough fact, including a
branches surface. They never invent a no-template reason or replace an
unavailable fact.

Use `projectNativeComponentMeta(meta, options)` from
`@verter/component-meta/compat` to produce the Volar shape directly from
one decoded native result.

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
import { decodeComponentMetaPayload } from '@verter/component-meta'
import { projectNativeComponentMeta } from '@verter/component-meta/compat'

const session: ComponentMetaSession = host.openComponentMetaSession()
const payload = session.getComponentMeta('/project/src/Button.vue')
const native = payload === null ? null : decodeComponentMetaPayload(payload)
const volar = projectNativeComponentMeta(native, options)
```

Checker and public-session scalar and batch calls use this same projection.
A null native answer produces empty Volar metadata; a native failure throws.
A session without the batch capability throws instead of issuing scalar calls.

## What stays unchanged

- `getComponentMeta(...)` — the canonical query.
- `getComponentMetaWithAudit(...)` — synchronous audit bundle.
- The raw `@verter/native` `getResolvedComponentMeta(...)` binding remains
  available. The JavaScript runtime uses the canonical `getComponentMeta`
  lane for both native and compat queries and exposes no separate resolved method.
- `mapComponentMeta` and `nativeComponentMetaToComponentMeta` remain available
  for callers that explicitly compose the lower-level conversions.

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
