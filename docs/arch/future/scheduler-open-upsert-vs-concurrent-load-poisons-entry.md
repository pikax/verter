# Concurrent host load/compile racing a `did_open` upsert can leave a scheduler entry that never loads again

## Symptom

A file that is open in the editor, present on disk, readable through the VFS
workspace (`workspace_read().read_file(...)` returns `Some`), and registered in
the LSP `DocumentRegistry`, ends up in a host state where — persistently, for
the rest of the session:

- `VerterHost::get_source(canonical)` returns `None`;
- `VerterHost::ensure_loaded(canonical)` returns `false` on every call;
- `VerterHost::get_analysis(canonical)` returns `None`.

Observed while integrating the LSP's background import-dependency publication:
a detached background task compiled an imported child carrier
(`get_public_api` / `ensure_loaded` via `sync_imported_carrier_api_lightweight`)
at the same moment the user's `did_open` of that child committed its host
upsert. Afterwards every native feature depending on the child's analysis
failed (`resolve_child_prop_usage_at_cursor` → child document resolution →
`ensure_component_ready` gate), which in one real-provider rename flow silently
degraded a confirmed cross-file child-prop rename into a usage-only partial
because the sync classification could no longer see the child.

## Mechanism

`crates/verter_session/src/host_lifecycle.rs:787` (`ensure_loaded`):

- Fast path (`:798-805`): entry evicted-flag + `scheduler.try_get_source` —
  both fail for the poisoned entry (source gone from the scheduler).
- Slow path submits a `TargetStage::Analysis` load request and waits
  (`wait_or_drive`, `:845-855`). For the poisoned entry the completion state is
  never `Ready`, so `ensure_loaded` returns `false` — on every retry, forever.

The trigger is the interleaving of two writers for one canonical:

1. the `did_open` path's host upsert (registry commit → `host.upsert`), and
2. a concurrent host load/compile of the same canonical from another task
   (`ensure_loaded` → scheduler load, or `get_public_api`/`ensure_compiled`).

The exact scheduler-internal state left behind (a superseded/cancelled node
that later `submit_request` + `wait_or_drive` cannot revive) was not chased
further because the semantic engine is out of scope for this effort; the
reproduction below makes it observable from the outside.

## Reproduction (synthetic, self-contained)

1. Build an LSP server over any host (mock provider is enough to drive it, but
   the race is host-level).
2. Task A: `host.ensure_loaded(child)` + `host.get_public_api(child)` in a
   `tokio::spawn`ed background task (this is what the import-dependency
   publication does for an imported carrier).
3. Task B, concurrently: the full `did_open` lifecycle for `child` (registry
   `did_open` → host upsert with the same bytes).
4. Afterwards poll `host.get_source(child)` / `host.ensure_loaded(child)`.

Interleaving-dependent (a real race): in the in-repo real-provider suite it
reproduced deterministically-enough on
`real_provider_tests::rename::rename_cross_file_imported_prop_fails_closed_tsgo`
(open parent → its background publication touches the child exactly while the
test opens the child) — failing on every one of 5+ consecutive runs, and
passing on every run once either (a) the publication start was delayed past the
opens (600 ms experiment) or (b) the per-document lifecycle-lane serialization
below was in place.

## Evidence

Instrumented run of the failing rename flow (traces since removed):

```
BISECT ready-gate: .../ImportedPropChild.vue get_source=None ensure_loaded=false \
    ws_read=true registry_doc=true parent_source=true analysis=false
```

- `ws_read=true`: the workspace reads the file fine.
- `registry_doc=true`: the document registry holds it (it is open).
- `parent_source=true`: the sibling parent file is intact in the host — the
  loss is per-entry, not a global wipe.
- Delaying the racing background pass by 600 ms turned the same test green;
  serializing the pass on the document lifecycle lane (see below) turned it
  green with no delay.

## Why deferred

The scope directive for the LSP performance effort (ratified 2026-07-21) forbids
modifying `verter_session` internals: the fix belongs in the host/scheduler
load-vs-upsert lifecycle, which is the product owner's domain. The LSP layer
now avoids creating the racing pattern instead: every background import-
dependency sync of a document serializes on that document's lifecycle lane
(`ide_sync_lifecycle_lease`) — the same lane `did_open`/`did_close` hold across
their commits — in `sync_imported_carrier_api_lightweight` and the barrel legs
of `crates/verter_lsp/src/server/import_publication.rs`. That removes the
LSP-side exposure but leaves the underlying host race latent for any other
concurrent caller (MCP, NAPI batch, future background work).

## Proposed fix + falsifiable prediction

Make the host's per-canonical load/upsert transition atomic with respect to
concurrent `ensure_loaded`/compile: either serialize `upsert` and the
scheduler-load integration per canonical, or make a superseded load request
re-submittable (the poisoned terminal state must be recoverable by the next
`submit_request`). Prediction: with the fix, the reproduction above converges
(`ensure_loaded` returns `true` within one retry) for 100 consecutive races,
with the LSP-side lane serialization REMOVED; today it fails persistently.

## Blast radius

- Left alone: any concurrent host consumer can permanently lose a file's host
  state for the session; downstream this silently degrades any feature that
  reads analysis (in the observed case it turned a fail-closed cross-file
  rename into a shipped partial edit — a correctness failure, not a latency
  one).
- Fixed: the LSP-side lifecycle-lane serialization in
  `import_publication.rs` / `sync_orchestration.rs` becomes defense-in-depth
  and could be simplified; no behavior depends on the poisoned state.
