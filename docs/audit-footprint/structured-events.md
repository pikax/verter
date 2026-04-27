# Structured events — `StructuredComponentMetaEvent`

The session-side trace macro `component_meta_trace_structured!` is
the only way to emit a component-meta telemetry event. Every call
expands to a push onto the active request's accumulator — nothing
writes to stderr, nothing writes to disk (apart from the
`VERTER_COMPONENT_META_AUDIT_JSON_OUT` dump at request end).

Defined at
[`crates/verter_session/src/component_meta_audit/structured_event.rs`](../../crates/verter_session/src/component_meta_audit/structured_event.rs).
TS type: [`StructuredComponentMetaEvent`](../../packages/types/audit.generated.ts).

## Variants

| Variant                                    | Fires when                                                     |
| ------------------------------------------ | -------------------------------------------------------------- |
| `RequestStart`                             | Component-meta request entry.                                  |
| `RequestEnd`                               | Component-meta request completion.                             |
| `IndexedReadyBuilt`                        | Fresh `IndexedReady` insert for a canonical.                   |
| `VfsRead`                                  | VFS read via the registered sink (overlay/snapshot/disk/etc.). |
| `SharedLoadReuse`                          | Joiner attaches to a winner's in-flight artifact.              |
| `DispatchEnter` / `DispatchExit`           | Semantic-query dispatch envelope.                              |
| `MaterializeMemberRouteStart` / `End`      | `extract_member_route` envelope.                               |
| `RematerializePublicPropTypeStart` / `End` | `rematerialize_public_prop_type` envelope.                     |
| `MaterializeDefinePropsMember`             | `defineProps` member materialization.                          |
| `FallthroughInheritanceComputed`           | Fallthrough inheritance resolver result.                       |
| `ResolveImportedTypeRoot`                  | Cross-file imported-type root resolution.                      |
| `CurrentEvalState`                         | Eval-state checkpoint.                                         |
| `MaterializeStructureEnter`                | Entry envelope for `materialize_component_meta_structure`.     |
| `MaterializeStructureExit`                 | Exit envelope for `materialize_component_meta_structure`.      |
| `MaterializeStructurePolicySkip`           | Materialiser policy gate rejected the input pre-dispatch.      |
| `MaterializeStructureCycleDetected`        | Same-key materialiser re-entry on the in-flight stack.         |
| `MaterializeStructureDepthFuseTripped`     | Materialiser depth fuse tripped (defensive hard cap).          |
| `Custom { name, detail }`                  | Escape hatch for ad-hoc events.                                |

## Materialiser-entry events (plan §3.3)

The five `MaterializeStructure*` variants instrument the
session-layer structural materialiser at
[`crates/verter_session/src/component_meta_materialize.rs`](../../crates/verter_session/src/component_meta_materialize.rs).
They are the audit-facing observability surface for plan §1.5 /
§10's materialiser cutover.

### `MaterializeStructureEnter { base, scope_axis, mode, depth }`

- `base: Arc<str>` — stable display key for the input
  `SemanticNodeId` (consumes the audit's `SemanticNodeId →
  display_label` projection).
- `scope_axis: MaterializationScopeAudit` — `TopLevel` vs `Nested`,
  the materialiser-internal axis that gates the package-ref and
  function-property policies.
- `mode: ProjectionModeAudit` — caller-side projection mode the
  materialiser ran with (`Identity` / `Navigate` / `Shallow` /
  `Expanded`).
- `depth: u16` — materialiser stack depth post-increment.

Fires on every `materialize_component_meta_structure` call, before
the warm-peek check. **Audit consumer use case:** trace the
materialiser's per-request work on a base-level shape; pair with
the matching `Exit` event to bound durations.

### `MaterializeStructureExit { base, scope_axis, mode, outcome, duration_ns }`

Same identity fields as `Enter` plus:

- `outcome: CacheOutcomeKind` — Hit / Miss / JoinedWait / Sentinel /
  ColdBuild / InflightAbortedRetry / ColdAbortSwept / **Tainted**.
- `duration_ns: u64` (decimal-string serialized) — wall-clock
  duration from `Enter` to `Exit`.

Fires once per Enter, regardless of how the materialiser exited
(warm hit, cold build, policy skip, cycle, depth-fuse trip). The
`Tainted` outcome carries the depth-fuse / scope-unloaded /
recursive-sub-call signal upward — `MaterializeOutcome::Tainted`
is non-cacheable and propagates to the caller. **Audit consumer
use case:** measure cache hit rate (`Hit` / `JoinedWait` vs
`ColdBuild` / `Miss`) and isolate the non-cacheable `Tainted`
fraction.

### `MaterializeStructurePolicySkip { base, scope_axis, reason }`

- `reason: MaterializeSkipReason` — see [api-reference.md](api-reference.md).

Fires when the materialiser's policy table rejects an input
before dispatch. The five `MaterializeSkipReason` arms map 1:1
to the materialiser's pre-compute gates:
`FunctionPropertyAtNested`, `GenericRefWithArgsTopLevel`,
`PackageRefTopLevel`, `RegistryRouteNotInlineMaterialisable`,
`NonStructuralTopLevel`. **Audit consumer use case:** verify
package types stay symbolic (no expansion through `node_modules`
boundaries) and function-typed properties skip materialisation
at Nested axis (Vue's keep-function-bodies-symbolic invariant).

### `MaterializeStructureCycleDetected { base, scope_axis, mode, depth }`

Fires when the materialiser's thread-local in-flight stack
detects same-key re-entry. The materialiser returns
`MaterializeOutcome::Recursive` (mapped to `Tainted` at the
session boundary), so caches do not warm with an in-progress
result. **Audit consumer use case:** flag declaration graphs that
self-reference through structural materialisation; absent in
healthy fixtures.

### `MaterializeStructureDepthFuseTripped { base, scope_axis, mode, depth }`

Fires when the materialiser's defensive depth fuse trips (input
depth exceeded the hard cap). Like `CycleDetected`, the result
is `Tainted` and non-cacheable. **Audit consumer use case:** an
event here in production indicates a runaway materialisation
chain; absent in healthy fixtures.

## `Custom` construction policy

`Custom` is an escape hatch. Every call site MUST place a
`// Custom justified: <reason>` comment on the line immediately
preceding the construction. The
`every_custom_variant_construction_site_has_justification_comment`
grep test enforces this — a `Custom {` with no preceding
justification fails CI.

## `u64` / `i64` fields

Every `u64` field in the structured-event variants (request_id,
winner_request_id, bytes_read, duration_ns) serializes as a
decimal string and types in TS as `string`. Same rule applies to
`i64` audit-record fields (plan §3.B Commit 7.A).

## Retired legacy events

Plan §3 Commit 5 (F4 squash) deleted:

- `component_meta_trace_scope!` + `component_meta_trace_event!` macros
- `component_meta_trace_scope_impl`, `component_meta_trace_write_line`,
  `component_meta_trace_event_impl` helpers
- The legacy TLS stack + span-id counter
- `VERTER_COMPONENT_META_TRACE*` env vars (kept only
  `VERTER_COMPONENT_META_AUDIT_JSON_OUT`)
- All `format!("k=v")` detail strings feeding the old macros

Clean-cut verification: `grep -rn -E 'component_meta_trace_scope!
|component_meta_trace_event!|VERTER_COMPONENT_META_TRACE\b' crates/`
returns ZERO. Plan §6 item 11.
