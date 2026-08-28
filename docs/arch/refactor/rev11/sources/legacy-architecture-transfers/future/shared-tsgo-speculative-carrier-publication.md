# Speculative carrier publication is dead work on the shared-tsgo route

**Status:** open follow-up. Not a correctness defect — a pure cost with no consumer on
this route. Recorded rather than fixed because the producer is shared with the tsserver
route, where the same records ARE consumed.

## Symptom

On the shared-tsgo route the background workspace scanner compiles and publishes every
carrier in the project into the shared overlay's content cache. After the editor-demand
scoping fix in `LazyOverlayCore::inject_all_dirty`, **nothing ever injects those
records**: the request path is scoped to carriers an editor lifecycle lane recorded, and
there is no other injector.

So on a 77-carrier project the scanner performs ~154 IDE/API companion compiles, ~154
`ProviderSurfaceStore` generations, and ~154 content-cache inserts whose bytes are never
read on this route.

## Mechanism

- Producer: `crates/verter_lsp/src/workspace_scanner.rs:431-480` (all-carrier compile and
  publication) → `sync_file_to_provider` at `:813` → `open_tsx_background` /
  `open_dts_background` (`crates/verter_lsp/src/type_provider/project_sync.rs:366-451`).
- Those forward to `TsgoCompositeProvider::open_file_background`
  (`crates/verter_lsp/src/tsgo/composite.rs`), whose only shared-overlay effect is
  `shared_record(path, content, OverlayPriority::Background)`.
- Consumer: `SharedTsgoOverlay::engage_provider` →
  `LazyOverlayCore::inject_all_dirty`, whose `in_scope` predicate now admits
  `priority >= OverlayPriority::Normal` plus the queried carrier's own companion family.
  A `Background`-lane record that is neither open nor imported is never admitted.

The records are not wrong — they are simply unread. They become read the moment the editor
demands the carrier (a `didOpen`, or an import the background import publication
delivers), because both record on the INTERACTIVE lane and
`record_content_at_priority` keeps the highest lane seen.

## Reproduction

Any workspace with more carriers than the editor opens. Open one SFC in a project of N
carriers on the shared-tsgo topology and compare the number of carriers the scanner
compiles with the number the overlay injects. The second number is the open document's
companion family plus its import closure; the first is N.

Synthetic shape: 40 `.vue` files, none importing the others; open one. The scanner
compiles 40; the overlay injects 2 (the opened carrier's IDE + API companions). The unit
test `an_editor_demand_sweep_is_not_charged_with_the_background_bulk`
(`crates/verter_lsp/src/tsgo/overlay_core_tests.rs`) pins exactly that ratio at the
overlay-core seam with 41 recorded carriers.

## Evidence

Measured on a private 77-carrier corpus, VS Code acceptance lane, `shared-tsgo`
topology, debug build, shared machine. Five launches each.

Removing the request-path whole-set injection did not change answer quality: the
engine-attributed hover count stayed at 40-42 of 51 (78-82%), identical to the best
baseline launch, with 0-1 empties (baseline's worst launch had 7). If the speculative
records had been load-bearing, dropping them from the Program would have shown up as
TS2307-shaped empties. It did not.

## A REJECTED alternative, with numbers

Before scoping the request path, the same fix was tried as "move the whole-set injection
to a detached background convergence sweep, published on the provider's BACKGROUND lane".
It works — deadline pinning disappears either way — but it is measurably worse than not
converging at all, because the sweep contends with the foreground IDE compile for the same
host read locks:

| variant | `did_open` max | receipt hover coldMax |
|---|---|---|
| baseline (whole set, inside the request) | 976 / 1041 / 1016 ms | 695-1516 ms |
| detached convergence sweep | 769-4612 ms | 3215-6240 ms |
| editor-demand scope, no sweep (landed) | 387-520 ms | 377-633 ms |

A pass-capped variant of the sweep was worse again: it released its singleflight latch
every 4 passes, so a fresh sweep started on essentially every engagement, and 13-14 hovers
per launch then hit the 1350 ms provider hop bound (the 1500 ms request budget less the
150 ms hop margin). Both convergence variants were discarded.

The lesson is not "background work is bad" — it is that this particular background work
had no consumer, so its only observable effect was contention.

## Why deferred

The producer is shared. On the **tsserver** route the carrier publish store is exactly how
companions reach the engine (`getExternalFiles` over ready IDE companions), so making the
scanner's carrier pass demand-driven cannot be done from the shared-tsgo side alone. It
needs the two delivery models separated first — the same split
`docs/arch/future/`-adjacent work that the ratified deadline/file-set specification calls
"primary provider vs editor carrier store".

## Proposed fix and falsifiable prediction

Split the scanner's carrier pass by delivery model: for a shared-tsgo session, walk
carriers only far enough to establish project ownership and companion identities (the
`register_owned` path already exists at
`crates/verter_lsp/src/external_ts/tsserver_backend.rs:176-205`) and do not compile or
publish closed carriers.

Prediction: on the same corpus, carriers compiled during the initial scan falls from N to
the open set plus its import closure; the scan's wall time and peak RSS fall
proportionally; and the acceptance lane's engine-attributed hover count and empty count
are unchanged (40-42 of 51, 0-1 empty). If the hover count drops or empties appear, the
speculative publication was load-bearing after all and the change must be reverted.

## Blast radius

- Shared-tsgo: none today — the records are unread.
- tsserver: total. The store IS the delivery mechanism there; any change must keep it.
- The scanner's carrier pass also feeds diagnostics publication at
  `crates/verter_lsp/src/background_init.rs:455-531`. A demand-driven scan changes which
  files get project-wide diagnostics at startup, which is a product decision, not just a
  performance one.
