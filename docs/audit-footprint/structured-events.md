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
| `Custom { name, detail }`                  | Escape hatch for ad-hoc events.                                |

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
