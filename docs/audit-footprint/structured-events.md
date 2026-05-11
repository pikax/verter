# Structured events — `StructuredAuditEvent`

The session-side trace macro `component_meta_trace_structured!`
expands to a push onto the active request's accumulator — nothing
writes to stderr, nothing writes to disk (apart from the
`VERTER_COMPONENT_META_AUDIT_JSON_OUT` dump at request end).

The authoritative enum and its variant payload types live in the
substrate at
[`crates/verter_audit/src/structured_event.rs`](../../crates/verter_audit/src/structured_event.rs)
(payloads in `verter_audit::origin_graph`). The session-side module
[`crates/verter_session/src/component_meta_audit/structured_event.rs`](../../crates/verter_session/src/component_meta_audit/structured_event.rs)
is a re-export so historic `verter_session::component_meta_audit::structured_event::*`
imports keep resolving. TS type:
[`StructuredAuditEvent`](../../packages/types/audit.generated.ts).

The enum is named `StructuredAuditEvent` (the prior
`StructuredComponentMetaEvent` name is gone — every emit site, every
TLS sink, and every test fixture key on this name).

## Variants

| Variant                                    | Fires when                                                                                       |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `RequestStart`                             | Audited request entry — any `RequestKind`, not just component-meta.                              |
| `RequestEnd`                               | Audited request completion.                                                                      |
| `IndexedReadyBuilt`                        | Fresh `IndexedReady` insert for a canonical.                                                     |
| `VfsRead`                                  | VFS read via the registered sink (overlay/snapshot/disk/etc.).                                   |
| `SharedLoadReuse`                          | Joiner attaches to a winner's in-flight artifact — captures the winner's request id and audited flag. |
| `DispatchEnter` / `DispatchExit`           | Semantic-query dispatch envelope. Exit carries `CacheOutcomeKind` + duration.                    |
| `MaterializeMemberRouteStart` / `End`      | `extract_member_route` envelope.                                                                 |
| `RematerializePublicPropTypeStart` / `End` | `rematerialize_public_prop_type` envelope.                                                       |
| `MaterializeDefinePropsMember`             | `defineProps` member materialization.                                                            |
| `FallthroughInheritanceComputed`           | Fallthrough inheritance resolver result.                                                         |
| `ResolveImportedTypeRoot`                  | Cross-file imported-type root resolution.                                                        |
| `CurrentEvalState`                         | Eval-state checkpoint.                                                                           |
| `MaterializeStructureEnter`                | Entry envelope for `materialize_component_meta_structure`.                                       |
| `MaterializeStructureExit`                 | Exit envelope for `materialize_component_meta_structure`.                                        |
| `MaterializeStructurePolicySkip`           | Materialiser policy gate rejected the input pre-dispatch.                                        |
| `MaterializeStructureCycleDetected`        | Same-key materialiser re-entry on the in-flight stack.                                           |
| `MaterializeStructureDepthFuseTripped`     | Materialiser depth fuse tripped (defensive hard cap).                                            |
| `Custom { name, detail }`                  | Escape hatch for ad-hoc events.                                                                  |

## Event sites — where structured events fire

The accumulator now captures events from every audited surface, not
just component-meta:

- **Request lifecycle.** `RequestStart` / `RequestEnd` fire from
  every `*_with_audit` entry-point on `VerterHost`
  (`get_component_meta_with_audit`, `resolve_type_with_audit`,
  `compile_with_audit`, `analyze_with_audit`,
  `audit_workspace_op`, `lsp_audit_begin`,
  `audit_mcp_tool_call`).
- **Scheduler dispatch.** `DispatchEnter` / `DispatchExit` fire at
  every `ProjectSemanticDispatch::execute` call site — the cache
  outcome on `Exit` distinguishes warm hits from cold builds and
  cooperative joins.
- **Cache layer hit/miss.** `record_cache_event(layer, hit)` on the
  `AuditObserver` populates `RequestStoreAudit::cache_layers` —
  every host-owned cache (`FileArtifactStore`,
  `ComponentMetaResultDb`, `MaterializeStructureDb`,
  `RefCycleResultDb`, `OwnerImportSurfaceDb`, `SemanticGraphStore`)
  routes through this hook.
- **Lock acquisition.** `record_lock_acquisition(name, wait_ns)`
  contributes to `WaitAudit::lock_wait_ns` /
  `lock_acquisitions`. Active when
  `AuditConfig::audit_timing_capture = true`.
- **Sampler tick.** The host-owned peak-RSS sampler thread
  (`SAMPLER_TICK = 50ms` on native, none on WASM) updates
  `process_rss_peak_bytes` on every active `RequestContext`.
- **VFS reads.** Every audited workspace read fires `VfsRead` with
  the canonical id, layer (overlay / snapshot / disk /
  dir-index-negative / missing), bytes-read, and cache-hit flag.
- **Shared-load reuse.** When a request joins a peer's in-flight
  slot, `SharedLoadReuse` records the winner's id and whether the
  winner itself was audited (drives the walker's terminal
  attachment).

## Materialiser-entry events

The five `MaterializeStructure*` variants instrument the
session-layer structural materialiser at
[`crates/verter_session/src/component_meta_materialize.rs`](../../crates/verter_session/src/component_meta_materialize.rs).
They are the audit-facing observability surface for the
component-meta structural materialiser.

### `MaterializeStructureEnter { base, scope_axis, mode, depth }`

- `base: Arc<str>` — stable display key for the input
  `SemanticNodeId` (consumes the audit's `SemanticNodeId →
  display_label` projection).
- `scope_axis: MaterializationScopeAudit` — `TopLevel` vs `Nested`,
  the materialiser-internal axis that gates the package-ref and
  function-property policies.
- `mode: ProjectionModeAudit` — caller-side projection mode the
  materialiser ran with (`Identity` / `Navigate` / `Shallow` /
  `Expanded` / `Skeleton`).
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
before dispatch. The arms map 1:1 to the materialiser's
pre-compute gates: `FunctionPropertyAtNested`,
`GenericRefWithArgsTopLevel`, `PackageRefTopLevel`,
`RegistryRouteNotInlineMaterialisable`, `NonStructuralTopLevel`,
`RegistryRouteCycleGuard`, `RecursiveHelperCycleGuard`. **Audit
consumer use case:** verify package types stay symbolic (no
expansion through `node_modules` boundaries) and function-typed
properties skip materialisation at Nested axis (Vue's
keep-function-bodies-symbolic invariant).

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

Every `u64` field in the structured-event variants (`request_id`,
`winner_request_id`, `bytes_read`, `duration_ns`) serializes as a
decimal string and types in TS as `string`. Same rule applies to
`i64` audit-record fields.

## TLS accumulator hot path

Lower crates emit by calling `verter_audit::observer::current_observer()`
and routing through one of the `record_*` trait methods (or the
`record_event` counter hook for unstructured events). The session's
`RequestContext` is the production implementer; absent any installed
observer (audit disabled or filter-rejected request) the lookup
returns `None` and emit sites short-circuit without allocating.

`install_observer(...)` returns an RAII `ObserverGuard` that
restores the prior observer on drop. `RequestContextGuard::install`
is the session-side wrapper that installs the request's
`RequestContext` as the observer for the duration of the audited
operation; sub-requests spawned by the closure inherit the parent's
observer through the same TLS slot.
