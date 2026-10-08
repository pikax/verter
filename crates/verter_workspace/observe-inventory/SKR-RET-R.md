# Base-resolution ownership inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_workspace

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Workspace-lane slots (`WorkspaceResolutionSlots::slots`: key → up to `CANDIDATE_CAP` candidates) | Warm reuse of a resolution answer and the decision node it roots on | REQUIRED | Engine-owned; a slot retires with its importer (deletion, rename away, removed subtree, overlay close over a known-absent path) or is evicted oldest-first past the slot cap | `src/lazy_resolution_cache.rs` | always |
| Per-candidate retention charge (`RetainedCandidate::_charge`) | The aggregate retention account's admission decision; covers the candidate, its decision node and edges | REQUIRED-lifetime | Held beside the candidate; released exactly once when the candidate ages out, its slot is evicted or its importer retires | `src/lazy_resolution_cache.rs`, `src/engine.rs` | always |
| Owner index (`WorkspaceResolutionSlots::owners`: normalized importer → slot keys, ordered) | Retiring one importer's slots, or every importer under a subtree with one range seek | REQUIRED-lifetime | Mirrors the slots; an owner leaves with its last slot | `src/lazy_resolution_cache.rs` | always |
| Admission queue (`WorkspaceResolutionSlots::order`) | Slot-cap eviction order | REQUIRED-lifetime | Mirrors the slots; stale entries compacted once they outnumber live slots | `src/lazy_resolution_cache.rs` | always |
| Slot cap (`WORKSPACE_LANE_SLOT_CAP`) | Bounds owners that never retire | REQUIRED-lifetime | Constant per Engine | `src/lazy_resolution_cache.rs` | always |
| Retained candidate count (`WorkspaceResolutionSlots::candidates`) | Residency snapshot | REQUIRED-lifetime | Maintained at every admit and removal | `src/lazy_resolution_cache.rs` | always |
| Retired-node set (`ResolutionFactRoot::retired_nodes`) | Tombstone retirement: which stored versions the fold drops | REQUIRED-lifetime | Per immutable root; a node enters when removed, leaves when republished or folded | `src/resolution_currency.rs` | always |
| Derived floor (`ResolutionFactRoot::derived_floor`) | Version of every derived node with no stored version; keeps a folded node from revisiting a version a witness holds | REQUIRED | Per root; only rises, one fresh version per fold | `src/resolution_currency.rs` | always |
| Edge count (`ResolutionFactRoot::edge_count`) | Residency snapshot | REQUIRED-lifetime | Maintained at every edge attach and detach | `src/resolution_currency.rs` | always |
| Owner-set auto-removal in `ResolutionFactRoot::remove_derived` | Keeps an owner set from outliving its last decision | REQUIRED | Runs inside the removing mutation | `src/resolution_currency.rs` | always |
| Retirement hooks (`retire_importer_in_world`, `retire_importers_under_in_world`, `retire_closed_overlay_importer`, `retire_resolution_decisions`) | Retire slots, decisions and edges together under the world or session publication gate | REQUIRED | Called from the content and overlay mutations and the admission fence | `src/engine.rs` | always |
| `ResolutionResidency` in `WorkspaceResourceSnapshot::resolution` (slots, candidates, owners, derived nodes, decision edges, dependency buckets, retired decisions) | None in production yet: occupancy evidence read by the ownership tests and resource snapshots, derived from current collection sizes under each published root | REQUIRED-lifetime | Snapshot lives with the caller | `src/engine.rs`, `src/traits.rs` | always |
