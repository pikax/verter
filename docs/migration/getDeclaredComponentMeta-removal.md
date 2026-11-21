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
provenance and reset the fallthrough surface to `{ kind: "none" }`, because
the declared-only surface carries no inherited members. An existing `none`
surface keeps its producer reason.

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

## Compat display and API changes

Descriptor conversion is now a mechanical transcription of the producer's
wire node, and every compat surface is one projection of one decoded native
result. JavaScript no longer substitutes registry rows, navigates indexed
access, evaluates `keyof` or mapped types, merges intersections, or keys on
declaration, prop or tag names. The user-visible consequences are listed
below. A row that needs a different answer is a Rust producer change, never
JavaScript logic.

### Display: the mechanical transcription

| Before | After |
| --- | --- |
| `Foo['bar']` resolved through registry rows (object unions, intersections, chained access, `number` keys) | stays an `indexedAccess` descriptor and renders as `Foo["bar"]` unless the producer publishes the terminal |
| Finite mapped helpers (`Id<T>`, variant helpers, type-parameter defaults) materialized into object descriptors | an unreduced mapped node is `unknown` carrying its printed text |
| Open-domain mapped types (`{ [P in string]: V }`, number, symbol and union key domains) rebuilt as `Record<K, V>` | `unknown` carrying the printed mapped text, also for `+readonly` and key-remapped forms |
| `keyof T` evaluated against registry rows | `unknown` carrying the printed `keyof` text |
| `Id<{}> & …` empty-object intersections collapsed after expansion | intersections are kept as published |
| A slot binding typed `(props?: Record<string, any>) => string` rendered `(props?: Record<string, any> \| undefined) => string` | `(props?: unknown \| undefined) => string` |
| The descriptor memo distinguished registry-backed and registry-free conversions | one memo entry per graph node id |

### Display: registry rows

A symbolic reference renders through a producer `typeRegistry` row joined at
depth one, for a reference without type arguments. There is no name-keyed
expansion and no row lookup by rendered application text.

| Before | After |
| --- | --- |
| `HTMLAttributeReferrerPolicy` expanded to its nine literal values | the alias stays symbolic (schema `{ kind: "object", type: "HTMLAttributeReferrerPolicy" }`) unless the producer sends a row |
| `Booleanish` expanded to `"false" \| "true" \| false \| true` | the alias stays symbolic unless the producer sends a row |
| `Numberish` rendered `Numberish` with schema `number \| string` | a producer row renders its published arms (`number \| string`); a user alias named `Numberish` stays that alias |
| A `type` prop with `"button" \| "submit" \| "reset"` arms rendered as an HTML button-type union | rendered like any other literal union; no role is inferred from the prop name |
| `ComponentSlots` / `ComponentUI` refs projected to slot-name and UI-helper shapes | rendered from the published descriptor; no helper is recognized by name |
| `Box<string>['value']` expanded from a row named `Box<string>` | stays shallow (`Box<string>["value"]`); rendered type arguments never identify a row |
| The compat schema of `RecordShape['value']` looked up the member `value` in the `RecordShape` row and rendered its type (`string`; an optional member became `string \| undefined`) | a registry row is never navigated by member name: the schema stays `{ kind: "object", type: 'RecordShape["value"]', schema: {} }` unless the producer publishes the terminal |
| A name published by several declarations used one of them | a name whose rows have different declarations is withheld from the registry map |

### Display: published order and `any` arms

| Before | After |
| --- | --- |
| A branded string union (`target`) was reordered to put `(string & {})` first, and `rel` arms were sorted | arms follow the producer's published union order |
| A published union descriptor with an `any` arm collapsed to `any` | the union keeps its published arms: an `icon` prop whose descriptor is the union of `string`, `any` and `undefined` keeps the schema arms `string`, `any`, `undefined` |
| A prop tagged `@IconifyIcon` rendered `any` | rendered from its descriptor |
| A `@defaultValue` literal moved to the front of a literal union | the published order is kept |

A member whose descriptor is exactly `any`, including an unannotated member,
still renders `any` with schema `any` (TypeScript reduces `any | undefined` to
`any`). An unannotated event payload renders `any[]`. Arms are kept only
when the producer publishes them: an authored `as?: any | { root?: any, img?: any }`
prop reaches compat as exactly `any`, because TypeScript reduces the union to
`any`, so it renders `any` with schema `any`.

A `F | F[]` prop whose arms are symbolic references renders as a
function-or-array union when each arm, and the array element published on the
prop, joins its own producer row. A reference that only appears inside a joined
row stays symbolic: `Handler | HandlerList`, where the row `HandlerList` is
`Handler[]`, does not become a function-or-array union.

### Known gap: instantiated indexed-access terminals over a generic helper

Before, JavaScript substituted a generic helper's type argument into its
registry row to name the terminal of an indexed access. That substitution is
gone, and the producer does not yet publish the instantiated terminal with a
symbolic `typeof` argument. Until it does, the terminal is transcribed as the
producer publishes it:

| Case | Before | Now |
| --- | --- | --- |
| `type Button = ComponentConfig<typeof theme, …>`, prop `ui?: Button['slots']` (compat schema arm) | `{ kind: "object", type: "ComponentSlots<unknown>", schema: {} }` | `{ kind: "object", type: 'Button["slots"]', schema: {} }` |
| The same prop in the native result, with the helper declared in the component's own file | `ref ComponentSlots<unknown("typeof theme")>` | `indexedAccess` of `ref Button` by the literal `"slots"` |

Both are owed by the producer. Compat will not substitute generic arguments
or navigate rows to recover them.

### API: one projection for every surface

| Before | After |
| --- | --- |
| A batch call on a session without the batch capability fell back to one scalar `getComponentMeta` per id | it throws; there are no scalar follow-up calls |
| `ProjectSession.getResolvedComponentMeta(...)` routed native project queries through the resolved-surface method | removed from the JavaScript runtime; native and compat queries both use `getComponentMeta`. The raw `@verter/native` binding is unchanged |
| A session without resolved metadata fell back to the plain native query | there is one query lane, so there is nothing to fall back to |
| Checker, session scalar and session batch calls each projected native results their own way | all use `projectNativeComponentMeta(meta, options)` |

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
