# Cross-file rename: generation-pinned provider-surface snapshots

## Problem

Renaming a Vue prop must rename the parent's uses **and** the child component's
`defineProps` declaration. The child's prop declaration surfaces to the type
provider (tsserver/tsgo) via the synthesized virtual `{child}.vue.ts` PUBLIC-API
surface (the `defineProps<{ … }>` props lifted into the `$props` / `new(props?)`
declaration). The provider returns a rename target **against that API surface**,
whose byte offsets must map back onto the child `.vue` source through the API
surface's `CodeTransform` source map.

Three properties make this hard:

- **Async, many-pathed sync.** The `{child}.vue.ts` surface is synced to the
  provider from MANY code paths (did-open, did-change coordinator, owner-resolved
  slow path, background drain, workspace scanner, lightweight imported-carrier
  sync, provider restart replay). tsserver's `open` / `updateOpen` are
  **no-response** notifications.
- **The carrier may be CLOSED.** The child `.vue` need not be open in the editor;
  its source must be resolvable from the host/VFS, not a `DocumentRegistry` doc.
- **Fail-closed is mandatory.** A wrong mapping writes the new name at the wrong
  byte range and **corrupts the user's `.vue`**. On any uncertainty the edit must
  be dropped, never emitted at a guessed range.

Two prior attempts failed:

1. **Live `get_public_api()` map at merge time.** tsserver's rename offsets index
   whatever `{child}.vue.ts` content was LAST SYNCED; the live source map may be
   fresher (the API surface sync is deferred relative to the TSX sync). Mapping
   stale provider offsets through a fresh source map emits a WRONG edit.
2. **A latest-only blake3 identity gate.** It checked the LATEST synced identity,
   not the generation that produced the offsets — so it both **over-dropped**
   (a fresher latest entry rejected a valid in-flight result) and left a residual
   stale/fresh race; it was also wired at only 3 of 7 sync sites and had no
   closed-carrier path.

## Architecture: immutable, generation-stamped snapshots

The authority is `ProviderSurfaceStore` (`crates/verter_lsp/src/provider_surface_store.rs`),
**owned by the shared `DocumentRegistry`** so every consumer that already holds a
`&DocumentRegistry` (server, sync coordinator, background-drain free functions)
reaches the same store; the workspace scanner (host-centric) carries an explicit
clone on its config.

### Structures

```
ProviderSurfaceStamp    { provider_path, generation, content_hash }
ProviderSurfaceSnapshot {
    stamp, kind: CarrierIde | CarrierApi | Shadow | Real,
    source_canonical,
    provider_content, provider_utf16_line_index,   // source-map generated space (UTF-16)
    source_map,                                     // parsed from the SAME provider_content
    carrier_source, carrier_utf16_line_index,       // source-map source space (UTF-16)
    source_hash,
}
ProviderQuerySnapshot   { by_path: provider_path -> Arc<ProviderSurfaceSnapshot> }
```

- `generation` is a **session-monotonic** counter advanced on every record AND
  every close (a retired path's generation is permanently spent).
- Snapshots are **immutable historical**, keyed by `(provider_path, generation)`,
  with a separate "current generation per path" map. A close (`forget`) removes
  the path from the current map and advances the counter, but **never** removes a
  historical snapshot — so an in-flight request that captured a generation keeps
  mapping correctly.
- Each snapshot is fully self-contained: it carries the carrier `.vue` source and
  the parsed source map, so merge-time mapping reads **nothing live** (no
  `get_public_api()`, no open-document read).

### The record / forget choke points

Two free helpers are the single funnel every sync/close path calls — completeness
is structural (auditable by grepping `record_carrier_api_surface` /
`record_carrier_api_surface_code_only` / `.forget(`):

- `record_carrier_api_surface(store, documents, host, canonical, provider_path, api_code, source_map_json)`
  resolves the carrier source (open buffer if `documents` is `Some` and the
  carrier is open, else `host.get_source` — the **closed-carrier** path), parses
  the source map from the SAME `api_code`, and records a fresh generation.
- `record_carrier_api_surface_code_only(...)` is used where only the synced
  `api_code` is in scope: it fetches the live `get_public_api()` source map and
  attaches it ONLY when the live code byte-matches `api_code`, so a snapshot never
  pairs the synced offsets with a map produced against drifted content.
- `forget(provider_path)` retires the active generation on close/evict.

Wired sites (record): did-change coordinator, server `sync_api_to_provider` /
`sync_carrier_api_unresolved` / `sync_compiled_carrier_to_provider` /
`sync_imported_carrier_api_lightweight`, background drain
(`sync_pending_carrier_provider_file`, `sync_owner_resolved_carrier_with_close_after_sync`,
`sync_api_to_provider_background_task`), and workspace scanner
(`sync_file_to_provider`). Wired sites (forget): every close choke —
`close_provider_paths` (server), `close_stale_provider_paths` (background drain),
`close_stale_paths` (workspace scanner), and the coordinator's stale-path close.

## The fenced rename transaction

`handle_rename` (`crates/verter_lsp/src/server/nav_features_navigation.rs`) runs as
one production transaction:

1. **Production sync-before-query.** `ensure_provider_synced(uri)` — the SAME
   contract `handle_goto_definition` runs — syncs the current file's IDE output
   AND every imported carrier's `{carrier}.ts` API surface BEFORE the query.
   Rename previously omitted this, so a closed child's API surface was never live
   and the child edit was dropped. Run before the fence so the sync's own provider
   commands are written, then pin the resulting generations under the fence.
2. **Fence.** Acquire `rename_provider_fence` (a real `tokio::sync::Mutex`) across
   capture → query → response, so no other rename transaction interleaves its own
   surface mutations mid-capture.
3. **Capture.** `capture_current_carrier_api_set()` snapshots the current
   `CarrierApi` generation for every tracked path BEFORE the query — an immutable
   `ProviderQuerySnapshot` of `Arc` snapshots.
4. **Query** the provider under the fence.
5. **Stamp + merge.** The merge's API resolver maps a returned `{carrier}.ts`
   location ONLY against the snapshot captured for that exact path, AND only if
   that capture is still **honored**: the captured generation is still current, OR
   the current surface is byte-identical to the captured one on **both** sides —
   the provider `{carrier}.ts` content AND the carrier `.vue` source
   (`content_hash` AND `source_hash` match). Both identities are required: the
   provider text can be byte-identical across two generations while the carrier
   `.vue` changed (a comment inserted before `<script setup>`, or template text
   edited — shifting `.vue` byte offsets while leaving the lifted `$props`
   public-API text identical), so honoring on the provider content alone would map
   the captured offsets through the OLD carrier source map onto the NEW `.vue` and
   corrupt it. A path absent from the captured set whose surface the store still
   knows as virtual (tracked or **tombstoned** by an in-flight close), a surface
   whose generation a content-changing background sync superseded after capture, or
   a closed/retired surface, all **fail closed (drop)** — the captured offsets may
   index content the current source map does not describe.

### Why this is race-free

Because captured snapshots are immutable historical `Arc`s, a concurrent
background sync that advances a path's generation, or a close that retires it,
**after** capture can never change what a captured entry maps through. The
honor re-check at merge — same generation, OR byte-identical provider content
**and** carrier source — converts the residual "background sync re-indexed between
capture and merge" window into a safe **over-drop**, never a corrupting
map-through-stale: a content-changing re-sync (on either side) drops, while a
true byte-identical re-sync (which only minted a fresh generation) is still
honored so it does not false-drop a legitimate in-flight rename. This is the
property the latest-only identity gate lacked.

### What `handle_rename`'s own sync does NOT do for a CLOSED child

The sync-before-query step above is what every navigation handler runs, and it
is necessary — but under tsserver it is **not independently sufficient** to make
a CLOSED child report a cross-file rename location. tsserver builds a
configured-project program for the parent; a child surface opened *after* that
program is built lands in its own inferred project, **outside** the parent's
program, so tsserver's rename returns only the parent's group. At rename time the
parent (`App.vue.tsx`) is already open (from `did_open`), so
`ensure_provider_synced` — which opens the parent first, then the children —
opens the child too late to join the parent's program.

What actually makes the closed child reportable is the `did_open`
**imported-carrier prewarm**: when the parent opens, it eagerly syncs each
imported child's `{carrier}.ts` PUBLIC-API surface into tsserver **before** the
parent's program is built, so the child is already a program member when the
rename runs. This was verified at the raw tsserver boundary: **prewarmed = 2
rename groups** (`MyComp.vue.ts` + `App.vue.tsx`), **unprewarmed = 1 group**
(`App.vue` only), stable across 90 one-second retries — so it is project
membership, not indexing latency.

Closed-child cross-file rename therefore **relies on the prewarm** to make
tsserver *report* the cross-file location; the generation-pinned snapshot is
still the correct, owned mechanism that *maps* the provider's `{carrier}.ts`
offsets back onto the child `.vue`. The two are complementary: prewarm →
reportability, snapshot → correct mapping.

The independent-sufficiency fix — re-rooting so a child opened at rename time is
forced into the parent's configured program (e.g. re-syncing the parent IDE TSX
to trigger a program rebuild once a new child surface is opened) — is
**cross-cutting** across every navigation handler that calls
`ensure_provider_synced` (definition / references / hover / rename), so it is
**not** part of this fail-closed merge/store fix. It is tracked as a separate
follow-up, **Block H-membership** (tsserver program-membership for cross-file
nav handlers). The `suppress_imported_carrier_prewarm` seam and the `#[ignore]`'d
`rename_cross_file_prop_child_closed_unprewarmed_tsserver` lane are exactly what
Block H-membership validates against.

## Encoding

A `CodeTransform` source map indexes positions in **UTF-16** (the source-map
column space), independent of the LSP-negotiated encoding. The snapshot builds
its provider and carrier line indexes in UTF-16; `api_surface_range_to_carrier_range`
runs the whole source-map lookup in UTF-16, then re-emits the mapped carrier range
in the negotiated encoding via a byte-offset round-trip over the captured carrier
source. A prop after non-ASCII carrier text then lands on the correct range under
UTF-8 / UTF-16 / UTF-32 sessions.

## Tests

- Unit (`provider_surface_store_tests.rs`): generation-A capture survives a
  generation-B sync; an unknown generation drops; a close after capture preserves
  the captured snapshot; a closed-carrier record captures the host/VFS source with
  NO `DocumentRegistry`; a snapshot-derived context maps correctly under UTF-8 /
  UTF-16 / UTF-32; a snapshot with no source map fails closed.
- Merge boundary (`type_provider/merge/tests.rs`): a real on-disk `{carrier}.ts`
  edits in place and is never mapped into the `.vue`; an unsynced surface with no
  backing file drops; the UTF-8 non-ASCII-prefix mapping returns the correct
  range.
- E2E (`real_provider_tests/rename.rs`, tsserver-only; tsgo keeps a `== 1` canary):
  `rename_cross_file_prop_child_closed` opens ONLY the parent, keeps the child
  CLOSED, runs **with the `did_open` imported-carrier prewarm ACTIVE**, invokes the
  PRODUCTION rename handler (no test-only sync helper), asserts the workspace edit
  touches BOTH files, applies the edit, and asserts the old prop text is gone + the
  renamed prop **declaration** (`fooRenamed: string`, name with its type) present in
  the child. It discriminates the generation-pinned snapshot **MAPPING** (capture →
  `external_ide_context_from_snapshot` → `api_surface_range_to_carrier_range`): a
  mis-ranged or dropped child edit fails the `fooRenamed: string` / `!foo: string`
  pair. It does NOT discriminate `handle_rename`'s own sync-before-query — the
  prewarm masks that axis (the suppressed lane below owns it).
- Prewarm guard (`real_provider_tests/rename.rs`,
  `parent_did_open_prewarms_imported_child_carrier_api`, tsserver-only): opening
  ONLY the parent records a `CarrierApi` snapshot for the imported CLOSED child in
  the `ProviderSurfaceStore`. Because closed-child cross-file rename now depends on
  that prewarm, this guard fails loudly if a future change removes it (verified: it
  fails with `suppress_imported_carrier_prewarm(true)`).
- Suppressed lane (`rename_cross_file_prop_child_closed_unprewarmed_tsserver`,
  `#[ignore]`): the would-be discriminator for `handle_rename`'s OWN sync — it
  suppresses the prewarm so the child can only be synced from inside the rename
  handler. It is `#[ignore]`'d on the tsserver program-membership gap tracked as
  Block H-membership (see above); it goes green once that follow-up lands.

## Interactive request-surface capture (every provider-backed handler)

The interactive query path rides the SAME generation-stamped store. The shared
context builders (`provider_projection_context` → `type_provider_context`, and
`virtual_file_context` for `verter-virtual://` documents) build the query
context EXCLUSIVELY from ONE captured immutable `ProviderSurfaceSnapshot`
(`VerterLanguageServer::capture_provider_request_surface`): the provider path,
content, mapper, and both line indexes all come from one recorded surface, so a
concurrent `did_change`/`did_close` can never tear the tuple the way the former
independent live reads (committed path + live-compiled IDE content + projection
mapper + document line index) could.

Capture resolves canonical → projection kind → provider path → the store's
CURRENT snapshot, and fails closed on: no recorded surface, a role mismatch
(`CarrierIde` for a carrier, `Shadow` for a self-file rune module), a foreign
`source_canonical`, or an open-document source that no longer byte-matches the
captured carrier source (an un-synced edit ⇒ querying the provider would pair
its OLD surface with a NEW mapper — wrong, not merely stale).

Producers record on SUCCESS only: the tsserver publish path records through
`record_and_version_carrier_companions`; the server-side interactive IDE syncs
record through the `record_carrier_ide_snapshot` method choke; the free-function
direct-open producers — the debounced coordinator (owner-resolved DirectOpen +
open-unresolved preserve), the background drain (owner-resolved DirectOpen +
open-unresolved preserve), and the workspace scanner — record through the
`record_carrier_ide_surface` free choke; the self-file shadow sync records a
`Shadow` surface (content + rewrite-aware mapper) in
`sync_self_file_shadow_state`. A failed sync records nothing. Completeness is
auditable by grepping the two IDE chokes the same way as the API choke.

Every provider-backed handler (hover, completion + resolve, definition,
type-definition, references, rename, document highlights, signature help, code
actions, semantic tokens, inlay hints, `$/verter/getBindingTypes`, the
diagnostics merge) re-validates the captured surface AFTER the provider await
(`provider_context_still_valid` / `provider_request_surface_still_valid`:
`captured_snapshot_still_honored` AND live-source match) and DROPS the provider
contribution on mismatch. Rename, code actions, and completion-resolve import
edits are STRICT — the whole provider edit/action set drops (a corrupt edit is
worse than no edit); read-only features fall back to the Verter-native result.

Guards: `crates/verter_lsp/src/server/request_surface_guard_tests.rs`
(builders use the captured surface, handler modules never read the live
context ingredients, every handler runs the post-await gate) plus the
mid-request race regression tests in `server_tests.rs` (the mock `on_query`
seam) and the gated real-provider determinism lane
(`real_provider_tests/request_surface.rs`).

## Follow-up

**Block H-membership — tsserver program-membership for cross-file nav handlers.**
The closed-child cross-file rename currently relies on the `did_open`
imported-carrier prewarm to make tsserver *report* the child location (see "What
`handle_rename`'s own sync does NOT do for a CLOSED child"). Making
`ensure_provider_synced` independently sufficient — forcing a child opened at
rename time into the parent's configured program — is cross-cutting across every
nav handler (definition / references / hover / rename) and is tracked separately
as Block H-membership. The `suppress_imported_carrier_prewarm` seam and the
`#[ignore]`'d `rename_cross_file_prop_child_closed_unprewarmed_tsserver` lane are
the exact validation harness for that work; the lane's `#[ignore]` is lifted when
it lands.

References and code-actions now capture their OWN document's request surface and
run the post-await gate (see "Interactive request-surface capture" above), but
their CROSS-FILE `{carrier}.ts` legs still map through the live
`external_ide_context` resolver rather than a fenced multi-surface capture like
rename's `capture_current_carrier_api_set`. Adopting the fenced capture +
snapshot-anchored merge for those cross-file legs remains follow-up work. The
merge-time mapping helpers (`external_ide_context_from_snapshot`,
`api_surface_range_to_carrier_range`) and the capture API are already reusable
for those surfaces.
