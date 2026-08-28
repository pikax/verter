# Large locals held across `.await` on LSP hot paths

**Audit verdict (2026-07-22): NEGATIVE.** No large inline payload or measured user-visible cost was found; the retained values are small headers and heap-backed handles, while the remaining size is control-flow layout.

## Symptom

Anything alive across an `.await` is stored in the async state machine and
contributes to the multi-tens-of-KiB handler futures measured in
`lsp-handler-async-state-machine-sizes.md`. This note names the concrete
types and await points on the hottest paths — not the full compiler dump of
every temporary.

## Mechanism

### Pattern the code already uses well

`TypeProviderContext` / `ProviderProjectionContext`
(`server/mod.rs`) are explicitly designed so **DashMap guards are dropped
before construction**, and the context is safe to hold across provider
awaits:

```text
TypeProviderContext {
  tsx_path: String,           // 24 B header
  tsx_content: Arc<str>,      // 16 B — payload on heap
  mapper: ProviderPositionMapper, // 32 B
  tsx_line_index / carrier_line_index: LineIndex, // 56 B each
  snapshot: Arc<ProviderSurfaceSnapshot>, // 16 B — snapshot body ~440 B on heap
}
// size_of TypeProviderContext = 192 B
// size_of ProviderProjectionContext = 192 B
```

Holding the **struct** across await is 192 B of future state, not a full
copy of the TSX text (the text is `Arc`).

### Where bulk still enters the future

1. **Nested async layers** (audit → deadline → timeout → body) — each layer
   embeds the inner future type. Wrapper inflation dominates: a 9,000 B
   completion body becomes a 37,912 B audited wrapper even though
   `run_with_audit(tiny)` is only 752 B.
2. **Many sequential await arms in one `async fn`** — e.g.
   `handle_goto_definition` (`nav_features_navigation.rs`):
   - `ensure_provider_synced(uri).await` (future itself 7,832 B nested)
   - `tp.get_definition(...).await` (provider hop boxed to 16 B)
   - optional loop of nav-probe `get_definition` awaits
   - all while keeping `ctx: TypeProviderContext`, `verter_result`,
     `foreign_ide_set`, params-derived data live across those points
3. **Params and response scaffolding** stay live for the whole method:
   - `GotoDefinitionParams` 136 B, `HoverParams` 112 B,
     `CompletionParams` 168 B, `Uri` 80 B (headers; string data on heap)
4. **Audit payload** `LspRequestPayload` = 120 B (plus heap strings for
   canonical id / error text) lives for the duration of `run_with_audit`.

### What is *not* large in the future itself

| type | `size_of` | note |
|---|---|---|
| `TypeProviderContext` | 192 B | Arcs point at heap |
| `ProviderProjectionContext` | 192 B | same |
| `ProviderSurfaceSnapshot` | 440 B | usually behind `Arc` in contexts |
| `LspRequestPayload` | 120 B | |
| `LineIndex` | 56 B | |
| `ProviderPositionMapper` | 32 B | |
| `Arc<str>` / `String` headers | 16 / 24 B | content not inline |

The **multi-KiB** future sizes are therefore mostly **control-flow state
machine layout** (union of all locals across all await points in a large
function + nested futures), not a single multi-megabyte buffer stored
inline.

### Background paths

`background_drain`, `sync_coordinator`, and `workspace_scanner` hold
cloned `String` ids, snapshots, and `ProjectSync` handles across many
awaits, but those futures run on **spawned tasks**, not on the serve-loop
`buffer_unordered` slots. They do not multiply by `LSP_MAX_CONCURRENCY`
on the serve thread.

## Reproduction

```bash
cargo test -p verter_lsp --lib future_size_measure -- --nocapture --ignored
```

Struct sizes are printed at the end of `measure_handler_future_sizes`.
Cross-check await points by reading
`nav_features_navigation.rs` / `nav_features.rs` / `audit_harness.rs`.

## Evidence

| measurement | value |
|---|---|
| `size_of::<TypeProviderContext>()` | 192 B |
| `size_of::<ProviderProjectionContext>()` | 192 B |
| `size_of::<ProviderSurfaceSnapshot>()` | 440 B |
| `size_of::<LspRequestPayload>()` | 120 B |
| `handle_completion` body future | 9,000 B |
| `handle_completion_with_audit` | 37,912 B |
| `ensure_provider_synced` | 7,832 B |
| `TypeProvider::get_definition` | 16 B |

Body futures of 8–9 KiB for definition/completion/rename already exceed the
sum of the “context” structs by ~40× — the remainder is nested futures +
compiler state-machine padding for the large branched `async fn`s.

## Why deferred

Recording only. Shrinking futures by splitting handlers is a later
refactor, not an LSP-correctness fix for the current pass.

## Proposed fix + falsifiable prediction

- After the first provider await, **drop** pre-await working sets that are
  not needed for mapping (or re-fetch cheaply).
- Split “sync phase” and “query phase” into separate `async fn`s so the
  outer future does not keep the entire sync state machine after sync
  completes (`ensure_provider_synced` is already 7,832 B alone).

**Prediction:** splitting so `handle_goto_definition` awaits
`ensure_provider_synced` in a child future that is dropped before the
provider query should shrink the body future by roughly the
`ensure_provider_synced` size (~7.8 KiB) if that child is not stored in a
later state. Measure with the same `size_of_val` harness.

## Blast radius

- **Depends:** every handler that awaits the provider after building a
  context; post-await surface revalidation relies on the context
  snapshot identity.
- **If fixed carelessly:** dropping the snapshot before revalidation
  would re-open the “map a superseded surface” fail-open class.
- **If left alone:** contributes to the measured 37 KiB-class trait
  futures and their capacity × size heap under storms.
