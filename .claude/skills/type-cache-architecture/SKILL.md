---
name: type-cache-architecture
description: "Verter fact-based cache architecture — env hash split, FileArtifactStore, content-addressed caches, query-identity caches, R1–R31 architectural rules, module augmentation, multi-candidate storage"
---

# Verter Fact-Based Cache Architecture

> AMENDMENT 2026-05-11-A — the integration branch for the fact-based cache
> overhaul is `refactor/semantic-db-overhaul` (the user-mandated rename of
> the older `fix/cutover-review-findings` branch). Same baseline; the swap
> is documentation-only.

Reference for the cache architecture in `verter_session`. Every architectural
rule referenced by tests, plans, and code comments resolves here. See
`.claude/skills/type-cache-architecture/SKILL.md` for the per-cache-layer key composition table.

## Why this architecture exists

Verter's IR is shallow-Refs + content-hashed parse + typed-IR-only resolver.
The cache substrate matches: **lazy-validate, fact-granular, read-side
authoritative**. The old model was **eager-evict, file-coarse, write-side
authoritative**, producing ~100 ms cold latency that failed to warm across the
loop. Cache validation moved from a write-side side-effect into a read-side
fact check.

End-state rule:

> Source updates produce semantic fact diffs. Cache entries are
> validated against the exact facts they read. Reuse is the default;
> recomputation is the exception. Overlays are views over the base
> host, not host mutations. Fact validation is the cache-correctness
> oracle; a view/content token remains the concurrency oracle.
> Shared semantic materialisations are keyed by resolved declaration
> **slot** identity (content-free); versioned declaration identity
> lives inside cached values.

## Cache Runtime Hard Rules

The generic final-component backing store is
`verter_type_engine::component_meta_result_db`. The session owns its payload
and publication policy. Production reads use the request dispatch's
`read_component_meta_result`; publication consumes the opaque evidence returned
by `traced_component_meta_compute` through `admit_component_meta_result`.
Admission checks the observed owner whole-hash root, generation and unchanged
external-input fingerprint before storage accepts a candidate. Warm reads
validate every retained fact against their request view. Raw candidate access and storage writes are
engine-private; seed/read/remove seams exposed for fixtures require test support.
`FactValidation::publication_input_fingerprint` derives the fence from the
captured request base and returns no fence for an unproven base. Normalize the
request's immutable overlay out of this external comparison; additive loading
and completion are not external supersession. Sampling a new live fingerprint
at trace entry would incorrectly vouch for a request captured before an edit.

Part of the `Cache Architecture (CRITICAL)` rule in `CLAUDE.md`. Binding for
the cache-runtime overhaul and any feature admitting cache entries.

1. Cache correctness is read-side authoritative. A warm hit is correct
   only after validation against the caller's current `StoreView`.
2. A cache key must include every deterministic input that changes the
   value. If not possible, the value is not cacheable.
3. Query-identity keys must not include content hashes, version hashes,
   or `fact_dep_signature`. Version identity belongs on the cached value.
4. The five env hash dimensions stay split. `parse_env_hash`,
   `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, and
   `project_identity` must not be bundled into a single
   `project_config_hash`.
5. Signature width is never a refusal. Every observed fact is recorded;
   a signature wider than one evidence page is sealed into immutable,
   retention-charged pages and validated in full. Empty means
   dependency-free.
6. A non-cacheable read, budget exhaustion, cancellation, generation
   supersession, incomplete self-rooting, and unresolved provenance all
   route through `ReturnOnly`.
7. `ReturnOnly` never publishes a cache entry, never registers
   reverse-index metadata, and never becomes a persistent artifact.
8. Reverse dependency graphs are not cache-eviction authority. They may
   support observability, prefetch, diagnostics, and targeted stale
   sweeps only. A ROOT-OWNED resolution graph may additionally advance
   derived resolution-currency facts, on three conditions: the edges are
   recorded exhaustively by the same observation operation that creates
   the witness; propagation runs inside the resolution mutation
   publication protocol; and every dependent cache still RECORDS the
   derived fact and VALIDATES it on read. It may not evict an entry and
   it may not stand in for read-side validation.
9. Same-canonical edits must be caught by strict self-root validation,
   not by eager own-canonical drains.
10. Cross-file edits invalidate consumers lazily through recorded facts,
    not through reverse-dependent eviction cascades.
11. Base cache and overlay cache writes are separate. Overlay/session
    results must not populate base-only artifacts or persistent cache
    entries.
12. Pure artifacts may be persisted only when their keys contain all
    semantic, compiler, env, profile, plugin, and source-map-policy
    dimensions.
13. Fact-validated semantic query results are memory-only until every
    query family has audited strict self-root validation, complete env
    keys, and typed non-cacheable admission.
14. Singleflight is required for every cold cacheable node. Concurrent
    callers for the same key produce at most one cold computation per
    miss window.
15. Joiners on an in-flight computation must validate the winner's
    published entry against their own view before returning it.
16. Cache admission must be typed. Boolean flags, empty arrays, sentinel
    hashes, or side-channel `RefCell` state must not decide cacheability.
17. Cacheable entries must be immutable after publish. Mutation creates
    a new versioned value or a new artifact key.
18. A cache hit must not allocate audit payloads when no request
    accumulator is active.
19. Public APIs must expose cache semantics when behavior differs. A
    single ambiguous compile path must not hide `stateless`, `content`,
    and `session` behavior.
20. Benchmarks must report cache mode, source-map policy, batch shape,
    thread count, hit count, and fallback count. A benchmark without
    those dimensions is not an architecture signal.
21. Retained bytes are admitted against ONE process-local aggregate
    account. See "Aggregate retention account" below: a store must not
    carry a private byte quota beside it, and a refused admission
    returns its COMPLETE value uncached under
    `NonAdmissionReason::RetentionPressure`.

## Aggregate retention account

`crates/verter_retention` owns `SemanticRetentionAccount` and its charge
primitives (re-exported unchanged as `verter_session_query::retention`). The
crate sits below both `verter_type_expr` and `verter_session_query`, so the
type-expression DTO and every identity type carry its owned handles directly.
It owns
`SemanticRetentionAccount` — the single process-local byte account every
host-owned semantic store charges. One account per PROCESS (not per
project): `ProjectTypeStore::retention_account()` hands back
`SemanticRetentionAccount::process_local()`, so loading N projects does
not multiply the ratified ceiling by N. The module is `pub` because the
account is process-wide rather than crate-wide: the LSP server's
`ProviderSurfaceStore` charges the SAME account (see Participants), so
provider-surface bytes and semantic-cache bytes contend for one ceiling
instead of each claiming it independently.

**Limits** (`RetentionLimits::defaults()`, the ratified aggregate
semantic-memory budget): 1 GiB `aggregate_ceiling_bytes`, 512 MiB
`pin_threshold_bytes`, 4 MiB `max_entry_bytes`, and a 512 MiB
process-wide `active_ceiling_bytes`. They are constants, not a config
surface; a test injects a private account
(`ProjectTypeStore::with_retention_account`,
`ComponentMetaResultDb::with_account_for_test`,
`DeclLoweringService::new_with_account`) rather than mutating them.

**Three charge classes.** `Retained` (warm cache-entry bytes) and
`Active` (in-flight bytes, additionally sub-limited) are REFUSABLE;
`Pinned` (bytes a live handle obliges the process to keep — the
lease-pinned parse snapshots in `decl_lowering`) is charged
UNCONDITIONALLY. Refusing a pin would force a live artifact to re-parse
or revoke a live handle, so pins consume headroom instead of being
refused, and crossing `pin_threshold_bytes` is REPORTED
(`under_pin_pressure()`) rather than enforced. The hard invariant is
therefore over the refusable classes: `active + retained` never exceeds
`aggregate_ceiling_bytes − pinned`, under any concurrent interleaving
(the admission is a CAS, never a check-then-reserve).

**Reserve / commit / release.** `reserve` hands back a
`RetentionCharge`, an RAII token whose `Drop` is the ONLY release path —
so cancellation, a failed build, an abandoned publish, a cap eviction,
a same-discriminant replacement and a whole-map clear all release
exactly once with no explicit call to forget. Ownership transfer is a
move; a charge shared by several owners lives behind an `Arc` so one
allocation is charged once (a same-payload memo backfill, an SCC
component's co-resident members). A charge stored beside a cache entry
outlives an eviction for as long as a reader snapshot holds the entry —
which is correct, because the bytes are still resident.

**ACTIVE-occupancy windows** (`verter_retention/src/active_window.rs`,
optional test/tooling state compiled only under `test`, `test-support` or
the default-off `soak-support` — forwarded by
`verter_session_query/soak-support` and `verter_session/soak-support`;
`ACTIVE_OCCUPANCY_WINDOW_AVAILABLE` reports the build). The snapshot's
`peak_total_bytes` mixes every class over the account's lifetime, so it
cannot say how much ACTIVE a given workload held. A window measures ONE
explicit `ActiveOwner` (`mint_active_owner()`, account-bound); the workload
reserves through `reserve_as(&owner, ..)` (`ChargeCtx::owned_by` for holds
backing) and `open_active_window(&owner)` (host route:
`VerterHost::mint_active_occupancy_owner` + `open_active_occupancy_window`)
registers the window under the admission lock. The owner travels with the
charge through sharing, `split_off`, `absorb`, holding adoption, release and
class transfers; a charge merging two different owners, and every
`reserve(..)` charge, is UNKNOWN. A holding adoption settles its provenance
change and class change in ONE account-locked transaction (`adopt_backing`):
the held bytes move between ACTIVE balances with the total unchanged, and no
window observes the adoption half-applied. `absorb` merges settled
(non-ACTIVE) charges only. The account keeps each identified owner's ACTIVE
bytes AND the UNKNOWN balance exactly (they sum to the account's ACTIVE bytes). Every ACTIVE transition — ACTIVE reserve and
release, `commit_retained`, an ACTIVE `transfer_to_pinned`, a holds slice
joining an ACTIVE charge through `holding` — is noted under that same lock:
the account keeps each owner's ACTIVE bytes exactly, so opening or finishing
races lose nothing and overlapping windows never reset each other.
`finish()` / `observe()` return an `ActiveWindowReport`: the interval on the
account's ACTIVE transition sequence, the owner's ACTIVE at open and close, its
peak (starting at the ACTIVE held at open, never a lifetime maximum), rise and
fall bytes (`open + rise − fall == close`), and the `foreign_transitions`
(identified other owners, excluded) and `unknown_transitions` counts.
`attributed()` yields a peak a crossing check may read unless unknown
transitions occurred in the interval, UNKNOWN ACTIVE bytes were already charged
when the window opened (occupancy spanning the window makes no transition, yet
could be the owner's work), or the owner's own ACTIVE bytes were held at open or
still held at close. A window
changes no admission decision. Production builds carry no registry, sequence
or provenance (`Provenance` is zero-sized there); current occupancy stays in the
snapshot.

**What a charge measures.** A charge covers the allocations its owner
keeps reachable, by CAPACITY, under the declared conventions of
`verter_retention/src/allocation.rs` (heap block = payload rounded to 16 B plus a
16 B header; `Arc` blocks include their counters; inline field bytes ride
the block that embeds them) — never a fixed per-item proxy. Fact rails are
measured by `allocation::fact_slice_bytes` (every fact's owned canonical
`String` and leaf names), engine carriers (diagnostics and their errors,
materialised points, dependency rails, query values, request prefixes,
cost identities) by `semantic_query/allocation_footprint.rs`'s
`OwnedHeap`. Each distinct `Arc` counts once per charge (`AliasSet`); an
allocation that carries its own charge (an evidence page, a cost receipt,
a pooled identity string, a graph node) is never charged by a holder.

**Self-charged delivered allocations** (`verter_retention/src/owned.rs`). Everything
a read hands its caller — the dependency rail (`DepSignature`, whose
canonicals are `OwnedStr`s), the walker-diagnostic list
(`WalkerDiagnostics`, whose names are `OwnedStr`s), and a value's
outermost allocation with the lists nested in it (`OwnedSlice` /
`Owned<T>`: flow-return and call results, candidate table, relation
bindings, overload list, projection alternatives, runtime kinds, cycle-gate
reasons) — and every identity string any of them, a graph payload, an error
or a diagnostic names (declaration, slot, scope, locator-anchor, function,
root and property identities, `QueryError` names, budget contexts) — is a
sealed `Owned` cell (`OwnedSlice`, `OwnedStr`) that carries its charge inside
the allocation. An empty `OwnedSlice` allocates nothing and holds nothing. Every owning clone shares the one charge and the charge is
released in the step that frees the allocation, so a clone a caller keeps
past every read, cache and store stays charged until its own drop, with no
sweep. No accessor exposes the raw `Arc`. A delivery pins the cells it
mints (`Owned::pin`, never refused); a cache retaining them takes an
`OwnedHold` (`Owned::hold`), refusable, which adopts the pin into
`Retained` together with every nested cell, and hands the charge back
(`Retained → Pinned`, one step at the account's linearization point) when
its last hold drops. A nested cell is offered to its parent's measurement,
never counted in it, so a cell several parents share is charged once.
A measurement returns `Footprint { bytes, handles }`: the bytes the owner
charges itself and the handles it pins (caller) or holds (cache). A debug
build panics when an owned cell's value reaches a raw `Arc`
(`AliasSet::for_cell`), when a measurement drops the handles it collected,
and when a delivered carrier counts heap outside its handles
(`OwnedHandles::of`). The arena pins the owned handles its payloads reach,
so a name cloned out of a node payload stays charged after the arena lets
go; a released slot whose payload a reader still holds keeps the payload's
block and heap charged until the last reader drops. `NodePayload::drop`
releases that charge immediately; no arena mutation or reap is required.

The cell's `Nested` box and child-registration slice are included in its
charge. Cache holds additionally charge their backing slice for exactly
the holds' lifetime (`HeldHandles`, also used by `RetentionCharge::holding`).
Dropping the cache holds releases that bookkeeping while surviving cells
return to `Pinned`. A weak view (`WeakOwned`) exists only for a cell built
for one (`Owned::new_with_shared_block`, e.g. an interned dependency rail):
the cell and every view share a `BlockShare`, so the block stays charged
until both its strong owners and every view let go, in either order;
`Owned::downgrade` of any other cell is `None`. Owned strings are named
without a view by `OwnedStr::identity`, a never-reused stamp that keeps
nothing allocated (the arena's thread-local scope digest uses it).

**Shared registered allocations** (`verter_retention/src/shared.rs`). An allocation
cache owners alias without handing it out — a build's fact carrier shared
by its root, prefix backfills and SCC members; the self-root list; a raw leaf
string a registered carrier still reaches (a fact's canonical, a self-root) — is charged ONCE through the
store's `AliasCharges` registry, keyed by allocation address (the
registered charge co-owns the allocation, so the address cannot be reused
while it lives). Cache owners hold `CacheHold`s, callers `CallerHold`s;
the same `Retained → Pinned` transfer and refusable re-adoption apply, and
a holder reviving a charge whose last holder is releasing it keeps the
charge's leaf holds in the reviver's class. A memo candidate's charges are
`MemoRetention { own, carrier, delivered }`: `own` is cache-only
bookkeeping (its entry in every slot it may populate, its point record,
its lease metadata, its reverse-index registrations) and is released the
moment the candidate drains; `delivered` are its `OwnedHold`s; a refusal
summary uses the same split.

**Graph storage is pinned** (`verter_retention/src/granular.rs`). The arena's live
chunks, payload blocks and dedup tables, every signature epoch's tables
(boxcar buckets, record heap, dedup indexes, side maps), and every flight
cell are liveness obligations of the graph: `Pinned`, charged through
`GranularPin`s settled in whole granules (never below the live total, by
less than two granules), released with the storage — a retired epoch with
its last holder. `HostRetentionSnapshot::semantic_graph_storage_bytes`
reports the arena's and current epoch's part of `pinned_bytes`. A memo
family's container (key, slot lists, budget-ledger record) is charged when
the family is keyed and released with it. The origin-edge store's tables,
edge lists, source lists and pooled fence blocks are part of the memo's
bookkeeping pin, settled on every mutation.

**Artifact and request caches.** The content-addressed artifact caches driven by
`cache_runtime::lookup` (`ArtifactNode`: the component-meta shape,
declaration-lookup, owner-collection and resolvability caches, the
flow-slice stores, the compile-output caches) reserve entry metadata, key,
facts, roots and measured payloads as well as their evidence pages.
Direct compile publication uses the same reservation path. Independently
delivered generated code and source maps are `OwnedStr` cells.
Fresh delivery pins them through `pin_generated_texts`, including stateless,
refused or fenced publication, runtime-render batches, and caller-supplied
runtime/IDE product requests. Pinning a cell already retained by a cache
shares its existing charge.
Flow graph publication and peeks deliver `FlowGraphBundleRef` (`OwnedValue`):
the bundle's payload pin follows readers past store teardown, while its
map slot and separately allocated key backing release with the entry.
The bundle's skeleton, binding map and graph are its own data, borrowed
only and not `Clone`; a holder keeping one past a borrow keeps the bundle's
handle (`SkeletonRef` / `BindingsRef` / `GraphRef`), the skeleton
observation port included. Every table and name a borrow reaches — the
skeleton's name, region, binding, site, return, write and closure tables
and their nested rails, edge and read paths, binding identities, the
function declaration name and the key's canonical — is an `OwnedSlice` /
`OwnedStr` charged by its own cell, so a clone of one keeps exactly that
allocation charged past the bundle. `OwnedSlice::make_mut` edits a fresh,
uncharged list in place and copies a shared or charged one.
Key measurement primes its alias set with the bundle's entire payload: shared backing
counts once, while independently allocated equal strings and deep copies
are charged separately.
`ValidatedFactCache` delivers `OwnedValue` cells and its fact signatures as
`OwnedFactSignature` (`OwnedSlice<FactVersionRef>`) cells; its candidate
reservation holds both and keeps cache bookkeeping separate from the
reader-shared charges. A value or signature the route and imported-root
drivers deliver without admitting it is pinned to the caller.
Stable component-meta requests measure their resolved state, and prepared
bundles measure their tables and retain each lazily committed declaration
and name-resolution table as a separate cell. Reverse-indexed candidates
reserve their metadata and hold their owned values.

`OwnedTree` measures the raw allocations reachable through a plain tree;
it does not by itself seal independently clonable inner `Arc` fields, so
an allocation several owners share, or a caller can keep, is its own
owned cell. A published type source (`SharedTypeSource`) and a lowered
flow slice (`SharedFlowSliceIR`) are strict `Owned` cells, not trees:
every list and name their values keep is an `OwnedSlice` / `OwnedStr`, and
a debug build fails the `HeapSize` measurement of such a cell that reaches
a raw `Arc` (`HeapMeter::first`). Further owned cells: a file analysis snapshot's lists (`SharedList`, shared with the
host analysis lists and the artifact's export signatures), template
analyses (the raw template slot holds the one it serves, refusably with
its evidence), module-export names, namespace targets and proofs (a
table's retention record holds the strings its own bindings introduce
before it claims its record and evidence; the HAMT backing keeps a
declared per-written-slot convention), and the import-type carrier's
specifier, and a component-meta analysis (`SharedComponentMetaAnalysis`:
the final-result cache holds it and every `ComponentMetaDelivery` shares
it). A terminal output unwrap (`OutputPayload::into_type_expr`)
moves the tree out of its cell when the carrier is the cell's last holder
(`Owned::try_unwrap`, releasing the charge as the tree leaves) and
otherwise hands the caller an unshared deep copy
(`verter_retention::unshared::Unshared`, `#[derive(Unshared)]`), never an
alias of a retained payload. The parse-artifact layer (`FileArtifactStore`
entries, `DeclBodyMemo`, scheduler snapshots) stays outside the account;
a prepared-declaration bundle counts what it reaches of it while it lives
and charges none of the layer's own storage.

**Refusal routing.** `RetentionRefusal::{Oversized, Pressure,
ActiveExhausted}` all map to `NonAdmissionReason::RetentionPressure`,
whose propagation is `LocalOnly`: the value is complete and correct, so
only the refusing family declines to store it and an enclosing
derivation may still cache its own result. This is deliberately DISTINCT
from `BudgetExceeded`, which means the compute stopped early and its
value may be partial.

**Participants** (each charges the account, none keeps a parallel byte
quota): `IdentityInterner` (each pooled `OwnedStr` is held by the pool and
charged by its own cell, its map slot is a separate charge,
`pooled_entry_bytes`; sheds pool-only entries and retries once on refusal,
else returns the string un-pooled; when the pool lets go, the string's
charge passes to the identities still holding it; a host close and a
completed host release boundary also shed its pool-only entries
(`shed_pool_only`), so a closed host keeps pooled only the identities a live
reader still holds),
`ComponentMetaResultDb` via `BoundedCandidateMap` (the charge rides the
`RetentionCandidate`), the `SemanticGraphStore` family memo (the charges
ride `MemoEntry::retention`; `warm_publish_one` refusal returns
`WarmPublishOutcome::Skipped`, and an SCC batch is reserved all or
nothing — members' own shares, their shared holds and every newly keyed
family's container — so a component can never publish torn), the
substitution and deferred-evaluation memos, refusal summaries, the arena,
the signature kernel and flight cells (see above), `DeclLoweringService`/`SnapshotShard` (one `Pinned` charge per retained
snapshot, owned by the `ShardEntry` so a second lease on the same
snapshot never charges twice), and — outside `verter_session` —
`verter_lsp`'s `ProviderSurfaceStore` (one `Pinned` charge per synced
provider surface, riding INSIDE the `Arc`-shared
`ProviderSurfacePayload` — the snapshot's CONTENT half — so the bytes are
released exactly when the last owner drops it: the store's map slot, an
in-flight rename / navigation capture that outlived it, or another
generation SHARING the same payload). Pinned is the right class there for
the same reason as a parse-snapshot lease — a surface the provider is
already holding must remain mappable, so refusing to retain it would mean
silently losing the map a returned offset has to travel back through.

That store's own retention is bounded by REACHABILITY, not history: a
`record` that supersedes a path's current generation, and a `forget` that
retires one, each drop the store's reference to the displaced generation,
so the map holds at most one entry per live path. A re-record whose bytes
are IDENTICAL to the path's current payload SHARES that payload instead
of rebuilding it, so a byte-identical re-sync mints a fresh generation
(basis identity is preserved) while charging the account ONCE — not two
fresh UTF-16 line indexes, two content hashes and a second reservation
for bytes the account is already charging. A capture pins the
`Arc`, never the map slot, so dropping the slot cannot invalidate an
in-flight request. Records happen on every provider sync — i.e. every edit
of every open carrier — so an insert-only map there retained every version
of every document for the life of the session.

Per-family COUNT caps (`GlobalRetentionBudget`, `FamilyKey::candidate_cap`)
are unchanged and still hold; this account bounds the byte SUM across
them.

**No account-less retaining store.** A store that retains candidates holds
its account as a `StoreAccount` — a handle with no `None` variant, so
"retains candidates, charges nothing" is unrepresentable rather than merely
unused. `StoreAccount::default()` resolves to `process_local()`, NOT to a
freshly minted private account: a per-store account would be exactly the
private byte quota beside the aggregate ceiling this contract forbids, and
would let N stores admit against N ceilings. `SemanticGraphStore` and
`ComponentMetaResultDb` therefore charge the one account through EVERY
reachable constructor, including `new()` / `Default`; `reserve_memo_candidate`,
`reserve_scc_batch` and `ComponentMetaResultDb::publish_core` have no
uncharged arm. A test that needs deterministic pressure binds a private
account explicitly (`with_account`, `with_account_for_test`,
`ProjectTypeStore::with_retention_account`).

The shared candidate substrate under those stores is closed the same way.
`BoundedCandidateMap::admit` takes a `RetentionCharge` BY VALUE, not an
`Option<RetentionCharge>`: a `RetentionCandidate` cannot be constructed
without a granted reservation, so there is no admission path — production
or test — that retains a candidate the account never saw. A caller with
nothing meaningful to reserve mints a zero-byte charge from an account
rather than skipping one.

**The byte figures are estimates.** `RetainedFootprint` produces an
accounting estimate; nothing in cache VALIDITY reads it, so an imprecise
estimate costs hit rate and never correctness. Bytes reached only
through an `Arc` a different charged owner already accounts for are
deliberately excluded so one allocation is charged once — notably the
shared semantic node arena is not charged per memo candidate.

The existing `Cache Architecture` guard cluster covers the current production
subset. The plan in `.claude/skills/type-cache-architecture/SKILL.md` names the
additional guards and discriminating tests that must land with each implementation
block before the corresponding rule becomes executable policy.

### Flow graph binding authority

`FlowGraphBundle` owns its content key and the immutable skeleton, dependence graph,
and `Arc<FlowBindingMap>` for one `FlowSliceFunctionKey` content version; all fields
are private and `FlowGraphBundle::build` derives the graph and binding map from one
`KeyedFunctionStructure`. The key is a request, not a certificate:
`KeyedFunctionStructure::bind` admits EVERY key axis against `FlowSourceIdentity`, and
a mismatch is a typed miss that publishes nothing. Function, both body hashes, parse
environment, exact parse identity and language row are compared against the serving
artifact; canonical (the id the artifact was served under) and toolchain (this
process's own fingerprint) are request-side axes that refuse a key minted for another
file or build. The graph store refuses to publish a bundle under any key but its
own, and `BoundFlowGraph` is a handle on the keyed bundle with no constructor taking a
separate key. Because the key is admitted on every axis, the content-addressed
artifacts keep their empty fact signatures. The graph store publishes the complete bundle once; every
demand and selected-content lowering shares those artifacts. Caller-provided
inventories must never initialize a cached map. A correspondence error remains a
typed source/store error, publishes nothing, and follows failed non-admission at
the hash/lower nodes; it is never cached as absence or replaced by an empty map.

The retained parse owns an immutable callable/call address table initialized once with its function index. Exact-key lease callbacks borrow that table; arena references remain on the worker and never enter `FlowGraphBundle` or the shared index. Locator misses stay misses and never trigger sibling-body rediscovery.

`DeclBodyMemo::function_flow_structure` invokes the indexed current-frame
builder under the retained parse lease and returns `PreparedFunctionBodySkeleton`
(skeleton and binding map together, private fields with immutable views). The production graph builder accepts only that prepared witness, and never resolves names. Nested capture subjects, including write-only captures, and actual read paths come from the pinned
index; this ingress never recursively constructs child skeletons. The source
derives the graph from that prepared skeleton and publishes all three artifacts
atomically.

Static member reads carry `ReadProjection` edges that prepend the authored path
to the demand suffix. A real `CapturedBinding` graph node owns each exact captured
identity, with the same definition, path-write, and execution adjacency as a
local hub; captured read/write construction remains linear and never fabricates
a local declaration. Only `PathWriteSource::ObjectLiteralEntry` can suppress an
earlier definite duplicate property; mutable assignments remain candidates until
control flow executes them. `ControlInput` edges keep the value
that governs an executed region selected. The planner interns projection tails
and bounds their count plus value visits with `max_value_states`; a growing
projection cycle returns typed `ValueStates` non-admission. Selected projection
payloads are content-bearing slice-hash inputs. The peeker alone constructs `ReturnSlicePlan`; immutable getters expose its selection and measured work. Only the cache owner can construct a production `PlannedFlowSlice` pair, so callers cannot understate retained work while reusing the same hash.

The graph key includes the serving `ParseKey`, whose content digest pins the
whole lexical source context. Inserting an ancestor declaration can change an
unchanged child's captured binding slot; the changed parse key prevents reusing
that child's old skeleton, map, or graph. An unrelated edit may preserve a narrow
slice hash while still producing a distinct graph artifact key.

## Architectural rules (R1–R31)

### Mutation semantics

**R1.** `host.upsert(canonical, source)` is a cache-state no-op iff the
`(canonical, content_hash, parse_env_hash, resolve_env_hash, lib_env_hash)`
quintuple is unchanged. No cache mutation, no semantic invalidation, no
`bump_store_view_epoch`, no scheduler round-trip beyond the quintuple check.

**R2.** `upsert` means "the source changed." Cache eviction is an explicit
method with a stated scope; never a side effect of `upsert`.

**R3.** Eager *reverse-dependent* cache invalidation is forbidden. Cache
entries validate on read against the exact facts they recorded; cross-file
staleness is detected lazily through `fact_dep_signature` checks. An owner
upsert does NOT iterate `reverse_deps_for` to drain its dependents — no
reverse-dependent cascade. `smart_invalidate_dependents`, a
`reverse_deps_for`-driven eviction loop, `invalidate_canonical` from upsert,
and `*_db::invalidate_canonical` as a public API are all banned in production
(the `host_upsert_performs_no_reverse_dependent_eviction` guard enforces
this).

*Narrow exception — derived resolution currency.* Advancing a derived
resolution fact (`ResolutionFactKey::Decision` /
`::OwnerResolutionSet`) through the root-owned reverse edges is CURRENCY
PROPAGATION, not eager cache invalidation. No reverse-dependent cache entry
is drained: the entry stays exactly where it is and becomes cold only when
its own recorded derived version fails ordinary read-side validation.
Enforcement is structural plus behavioural, never a name-keyed scanner: the
exhaustive `classify_resolution_observation` match is a compile rail that
forces every fact variant into a direct-leaf / derived-node / terminal
disposition. `ResolutionFactRef::is_decision` and
`ResolutionFactRef::is_owner_resolution_set` are the read-only consumer
oracles for identifying those derived nodes without exposing the key fields,
and
`reverse_decision_propagation_does_not_evict_cache_entries`
(`verter_workspace`) proves the executable non-eviction behaviour.

Distinct from the banned cascade: the **own-canonical drain**.
`upsert_via_scheduler_with_priority` still drains the upserted canonical's
*own* query-identity caches at upsert time
(`resolver.runtime.evict_canonical(&canonical_id)`,
`project_type_store.evict_canonical(&canonical_id)`,
`resolved_type_cache().clear()`).

**Self-version rooting of query-identity caches.** Each query-identity cache
entry carries a self-root `FileWholeHash` for its keyed canonical(s) inside
`fact_dep_signature` (prepended by the central fact-signature helpers). The
warm-read validator decides how strictly that self-root is checked:

- The **component-meta query DBs** — `declaration_lookup_db`,
  `imported_registry_db`, `resolvability_db`, `owner_collection_db`,
  `prepared_target_db`, `prepared_surface_db`, `prepared_member_db`,
  `routed_expr_surface_db`, `materialize_memo_db` — validate the self-root
  **strictly** via `validate_fact_signature_with_self_roots` (passing the
  entry's keyed canonical(s)): a same-canonical content edit, or a keyed
  canonical the live store view no longer tracks, rejects the entry on both
  the warm-hit and post-compute-revalidation paths. `prepared_target_db` roots
  the active scope, the original declaring canonical, AND the FINAL routed
  declaring canonical (when the requested name re-exports through an
  intermediate module to a third file) — the entry carries an explicit
  `self_root_canonicals` set because the cache key encodes only the first two;
  the routed canonical's observed hash is read from the prepared-decl bundle
  (`PreparedDeclBundle::owner_whole_hash`) actually used for the value.
  `materialize_memo_db` additionally merges every canonical observed during
  materialization as a cross-file dependency fact. Cross-file *dependency*
  facts keep the lazy "untracked → accept" permissiveness — only self-roots
  are strict.
- The **`SemanticGraphStore` query nodes** validate their self-root
  **strictly**. Each query-node `MemoEntry` records its `self_root_canonicals`
  — the keyed canonical for `ResolveDecl` / `TypeOf` / `Instantiate` /
  `ResolveMacroPayload`, or the file-derived origin of every input node for the
  node kinds keyed by interned `SemanticNodeId`s (`ProjectPath` /
  `ProjectMember` / `IndexedAccess` / `KeyOf` / `MappedType` / `Conditional` /
  `ReduceUnion` / `ReduceIntersection`). The carrier is built by the
  provenance-pure producer `semantic_graph_read_set_signature` (prepends a
  self-root `FileWholeHash` per observed self-root, merges the traced fact set,
  returns `None` — non-cacheable — on a conflicting self-root hash or a traced
  `FileWholeHash` that disagrees with an observed self-root). The warm-read
  validator — the memo protocol's lookup (`begin_query_claim`), `get_validated`,
  the claim's warm re-read, and the relation memo's `get_relation` — validates every
  self-root strictly via `validate_fact_signature_with_self_roots` /
  `ReadSetSignature::validate_with_self_roots`. `get_unvalidated` has no
  production warm-read caller (test/debug only).
- There is NO separate structural-materialiser store: the former
  `MaterializeStructureDb` and its producer are deleted, and every structural
  surface roots through the `SemanticGraphStore` memo / `ShapeCacheDb` rails
  above.
  The **materialization cycle gate** — the retired `RefCycleResultDb`'s
  replacement — is the sealed `ClassifyMaterializationCycleGate`
  semantic-query family (`project_semantic_dispatch::cycle_gate`), NOT a
  standalone DB: its producer runs the same ordered bounded walk (FIFO
  queue, 64-dequeue bound, first-visit-wins signal, empty-args
  `StructuralTransit`/`Skeleton` `Instantiate` per hop, cycle recognition
  only `child == root || child == current`) and records EVERY visited
  non-builtin declaration (root included) as an observed self-root, so the
  generic finalizer's strict self-root validation covers exactly the files
  the old DB's `self_root_canonicals` covered. Only the `Decided` outcome
  admits; a `LegacyFallback` (hop-limit / nested incomplete observation /
  missing graph data / scanner or root-collector limit / cancellation /
  unstable generation) always suppresses admission and is partial for
  every reason except `HopLimit`. The family carries the live-generation
  gate (`family_requires_live_generation_gate`), closing the
  bare-generation-bump hole the old DB closed with
  `validated_at_generation`.
  `Instantiate { base, args: [], context: InstantiateContext {
  projection_reduction, resolve_env_hash } }` with
  `context.projection_reduction.mode = ProjectionMode::Skeleton`
  (Skeleton-mode instantiation, empty args) so unbound type parameters become
  `TypeParam` shells — preserving Conditional branches that would otherwise
  collapse to `never` for unbound generics. `current_route_surface_hash` is the
  single route-fact production helper: the current-content `IndexedReady`
  artifact is the SOLE route-surface authority (no secondary route-surface
  artifact), gated by `indexed_surface_is_current` — edge currency plus the
  `project_generation` stamp for surfaces with cross-file edges.

When a completed structural carrier is wider than one evidence page
(`FACT_PAGE_WIDTH`) and carries explicit self-roots, the terminal publisher
replaces those precise `FileWholeHash` roots with one `StrictSelfRootWorld`. Minting validates every root strictly in the exact
effective view before and after the validation loop. The witness identity is
the process-unique workspace authority id, its dedicated trackedness
generation, the captured scheduler/artifact root epochs, and the exact
`ViewPopulation` (including request-completion id/revision). It is unavailable
during an authority transition and on an artifact-only backend whose external
`file_exists` mutations are not completely bridged. A generic Content
`DomainGeneration` is not a substitute for this witness. Every completed
carrier is then sealed whole through `seal_canonical_signature` (paged when
wide); width never makes it `ReturnOnly`.

The own-canonical drain serving the edited file's own caches is retained only
as a redundant fast eviction now that every query-identity cache validates its
self-root strictly; the warm-read validator rejects a same-canonical edit on
its own.

**R4.** Source-content changes produce a semantic fact diff (publishable via
`compute_upsert_changes_from_parse` for LSP / observability callers).
Invalidation is not propagated; the fact diff is the public observability of
what changed.

**Import-route admission ownership.** `DerivedRawState.import_routes` is
exclusively the CALLER-SUPPLIED authoritative route table:

- `VerterHost::set_import_dependencies` is the **single producer**. Its entries
  serve until the caller replaces them; their currency rides the
  `ExactResolution` facts the same push installs.
- `VerterHost::configure_projects` and
  `VerterHost::upsert_via_scheduler_with_priority` may `.clear()` it.
- The host memoises NO resolution there. The host-side positive-route memo
  (`cache_positive_import_route_result`), its `PositiveRouteStamp` sidecar, and
  the known-miss generation sidecar are DELETED: a second copy of the
  workspace's bounded owner-edge candidate slot needed a global
  `content_generation` equality to decide whether it was still true, which was
  the last global-generation warm-resolution validity test in the session.

**Parse/resolve artifact ownership.** `IndexedReady` and `ShallowFileState`
retain NO resolved canonical, and materialisation performs ZERO import
resolution. The complete artifact reuse gate is parse-env equality
(`indexed_surface_is_current`); `built_at_content_generation` is a
CONTENT-domain per-canonical stamp read only by
`artifact_only_candidate_is_fresh`. The `Route` derived fact is a pure
parse-domain digest; the resolve-domain half of a route answer rides the
path-precise import-route resolution witness. See
`docs/contributing/path-precise-resolution-currency.md`.

The integration guard at `crates/verter_session/tests/cases/g_misc3/import_route_writer_guard.rs`
enforces both halves statically: no direct `import_routes` mutation outside the
named writers, and `positive_route_memo_producer_is_deleted` rejects a
reintroduced host-side route memo.

### Cache identity & validation

**R5.** Caches divide into two families:

- **Content-addressed artifact caches** — `FileArtifactStore`,
  `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`,
  `MemberDisplayFactStore`, `ModuleAugmentationIndex`. Keys include
  `content_hash` or a derived parse-stable hash.
- **Query-identity caches** — `RouteDb`, `SemanticGraphStore` query nodes,
  `ComponentMetaResultDb`.
  Keys exclude version hashes; concurrent variants coexist as candidates inside
  one slot.

Version rooting for query-identity caches lives **inside the cached value**,
never in the key. The rail per layer:

- The `SemanticGraphStore` family memo (`Instantiate` / `ResolveMacroPayload` query
  nodes), and `ShapeCacheDb` root via the candidate's `ReadSetSignature.facts` +
  `self_root_canonicals` + `validated_at_generation`, with the live whole-hash
  re-sourced at value-compute time (NOT carried in the key).
- `RouteDb` (per-name + barrel) roots via the value-side `ValidatedFactCache`
  fact signature (`RouteResult` / `BarrelRouteSurface.fact_dep_signature`).
- `ComponentMetaResultDb` roots via the owner whole-hash candidate discriminant
  (`BoundedCandidateMap<…, Hash16, …>`) + `ReadSetSignature.facts` +
  `validated_at_generation`; the owner `FileWholeHash` stays in the value facts.

The selected engine `MemoPublish` for `ComponentMetaResultDb` and its resolved-meta
sidecar takes those facts from the finalized request fact tracer. Storage accepts
owned entries; `MemoRead` owns generation/fact validation and accounting through
the request-bound `FactValidation` port. Candidate stores clone ordered `Arc`
snapshots under their existing guards; the engine tests them only after those
guards drop. Shape lifecycle validation deliberately keeps the original entry
guards while testing node liveness, then removes collected keys in order. Dispatch paths without their own cache boundary
fan dependency signatures into that tracer; they must not maintain a parallel
TLS accumulator or reconstruct a curated signature after the compute. A
non-cacheable or mutation-unstable tracer still returns the best computed value to the
caller, carries the typed refusal rail, and skips every warm-cache write.
The resolved-meta request trace always observes the request-captured owner
`FileWholeHash` before resolution begins (and uses the session-view hash for an
overlay request). This self-root is mandatory even for malformed or degraded
surfaces that never reach a macro-body dispatch read.

In every case the live whole-hash / version is re-sourced at value-compute time
via `ensure_indexed_ready_serve` and validated against the live `StoreView` on
each warm hit — never carried in the content-free key. See the per-key-context
detail below.

**R6.** Cache keys never include `fact_dep_signature` or content / version
hashes (for query-identity caches). Signatures and version info live on the
cached value.

The `SemanticGraphStore` family memo applies R6 to its
`SemanticQueryKey::Instantiate.base`, `SemanticQueryKey::ResolveMacroPayload.owner`,
and `SemanticQueryKey::TypeOf.value_root`
fields (mirrored on the `FamilyKey` memo identity). The first two carry the
env-bearing, content-free `ResolvedDeclSlotIdentity` (`defining_canonical`,
`merged_symbol_name`, `symbol_space` + the `project_identity` / `type_env_hash`
/ `lib_env_hash` ENV dims — `Instantiate` / `ResolveMacroPayload` base/owner
are always `symbol_space = Type`); `TypeOf.value_root` carries the env-bearing,
content-free `ValueRootSlotIdentity` (the scoped `ValueRootKey` root + the same
`J`/`T`/`L` env dims — `build_typeof` does env-sensitive name/export
resolution, so an env-free value root would warm-hit across envs). The extra
`resolve_env_hash` (`R`) dim rides
on a dedicated per-key `InstantiateContext { projection_reduction,
resolve_env_hash }` / `MacroPayloadContext { resolve_env_hash, mode }` /
`TypeOfContext { projection_reduction, resolve_env_hash }` (NOT the
shared `ProjectionReductionContext`, which stays a pure projection-demand
identity — §2.6 per-key-context rule); the production derivation points are
`type_slot_for` + `instantiate_context_for` / `macro_payload_context_for` /
`typeof_key_for`; `provenance` + `merge_role` stay at
FAMILY-IDENTITY on `FamilyKey` for EVERY context-bearing projection-reduction
family — `Instantiate`, `KeyOf`, `MappedType`, and `ProjectPath` all thread
both axes into their `FamilyKey`, so two `KeyOf` (or `MappedType`) queries
differing only in provenance / merge_role get DISTINCT `(FamilyKey, slot)`
identities and never alias onto one memo entry. The slot is **content-free** —
its `T,L,J` env dims are KEY dims, but content/version hashes (`whole_hash` /
`content_hash` / `parse_stable_hash` / `fact_dep_signature`) are FORBIDDEN; the
versioned `DeclIdentity` type (`{ canonical_id, whole_hash, decl_name }`)
survives as a value-side payload for `SemanticNodeData::{TypeParam, DeclRef,
InstantiationRef}` and `ShallowDiagnostic`, but is forbidden inside any
derived-`Hash` query-identity key (the retired content-free `DeclKey`
query-key struct must NOT be reintroduced). The cold-build path
(`build_instantiate`, `build_resolve_macro_payload`) re-sources the live file
content version from
`IndexedInputs::ensure_indexed_ready_serve(base.defining_canonical)`'s serve carrier `indexed.whole_hash` at
value-compute time and rolls it into the published `MemoEntry`'s
`ReadSetSignature.facts` + `self_root_canonicals` rails. The slot is U2-DERIVED
at query-key construction via `ProjectSemanticDispatch::type_slot_for` (reads
the live host env); test fixtures use the env-agnostic
`ResolvedDeclSlotIdentity::type_slot_unscoped`. Non-file bases (`__builtin__`,
empty global, `<synthetic>`) do NOT fabricate a `FileWholeHash` self-root —
they root via `args` nodes only. A real-file base whose `ensure_indexed_ready_serve`
returns `None` is a stale key and returns `cache_suppress = true`. Each
`(family, slot)` in `FamilySlots` holds a candidate list bounded at the family's
exhaustive, wildcard-free `candidate_cap()` (`U3.ADAPTIVE_FAMILY_RETENTION` —
floor 4; the inference/substitution-heavy live families `Instantiate` / `TypeOf` /
`Conditional` / `MappedType` hold 8); two
content-versions of the same content-free key coexist as distinct candidates
inside one slot under R20 overlay isolation. Admission identity is the pair
`(validated_at_generation, ReadSetSignature.facts)` — same exact discriminant
replaces in place and becomes freshest; a different view is ALWAYS admitted after
local eviction: at the family cap the publish evicts a candidate INVALID against
the publishing caller's stable store view FIRST (planned snapshot/validate/
reacquire OUTSIDE the `entries` mutex, `admission_seq` identity recheck under it),
then the front of the slot's LRU order (the least-recently validated-hit
candidate). The process-wide candidate-memory ceiling + typed non-admission are
deferred full-`U3.CACHE_FACT_MODEL` work — cacheability never depends on memory
pressure. Warm lookup scans every candidate and returns the FIRST
that passes BOTH §3.4 gates (below); `validated_at_generation` is LRU-recency
metadata, NOT a semantic-validity oracle.

**Fallthrough override identity (R6 content-free key dimension).** The fallthrough
cache key (`FallthroughNodeKey`, on `resolver_core`) records whether the request
carries a non-empty child prop-type override set via the internal enum
`FallthroughOverrideIdentity`
(`crates/verter_session/src/resolver_core/fallthrough_override_key.rs`) —
`NoOverrides` | `Uncacheable`. INTERNAL cache-key type only (no public / wire DTO);
content-free (R6 — a unit discriminator carrying NO `whole_hash` / `content_hash` /
`parse_stable_hash` / `fact_dep_signature` / raw `SemanticNodeId`).
`for_overrides(None or empty) → NoOverrides`; `for_overrides(non-empty) →
Uncacheable`. Override-bearing fallthrough is WHOLESALE uncacheable: an
`Uncacheable` key skips warm lookup, cache admission, AND singleflight (computed
cold, returned-only), enforced CENTRALLY at the owner layer
(`run_fallthrough_request`, `resolver_core/fallthrough_request.rs`) plus the
`FallthroughNodeKey::is_cacheable` cache gates, so two genuinely-different override
sets can never alias one warm surface. The SAME identity also keys
`FallthroughNodeKey::ConsumedBindingEvaluation`. No-override fallthrough, intrinsic
surfaces, the semantic-graph caches, and the final component-meta result cache carry
the warm-state value. (A durable override-specialized cache would only be
reintroduced with measured evidence, as a narrowly-scoped graph-instance / request
memo — not a query-identity key mirroring all semantic node structure.) Separately,
NO fallthrough promotion — runtime node store or legacy `cached_fallthrough` mirror —
admits a result computed by a request that tripped its shared `request_budget`
(gated in `cache_fallthrough_result`).

**§3.4 materialised-record satisfaction (the two-gate warm hit + recorded
backfill).** A warm hit is decided by the RECORDED materialised `(path, point)`
set the candidate's compute ACTUALLY produced —
`MemoEntry.satisfied_projection: MaterializedSet` — NOT by the candidate's
nominal slot/mode, and NOT by enum rank. Each `MemoEntry` carries the terminal
point (`Demand::from(terminal_mode)` at the query path) PLUS one
`Demand::navigate(prefix)` per actually-walked intermediate (`ProjectPath`
records these in `build_project_path`; each `PrefixBackfill` records its own
single `Navigate` point; a non-path build defaults to the single terminal point
for the canonical key). A warm hit requires **two independent gates, both must
pass**: (1) `cached_satisfies(satisfied_projection, requested_point_for_key(key))`
— some recorded point dominates the request at the SAME path (interned-id
equality, not prefix; regime-equal componentwise via `semantically_dominates`,
so `display_needs` never gates typed-value reuse); (2)
`read_set_signature.validate_with_self_roots` against the live view. Recording
the NOMINAL demand instead of the recorded materialised set silently collapses
soundness to wrong warm hits (a deep compute that only `Navigate`-walked an
intermediate serving a `Shallow`/`Expanded` request it never materialised) —
the discriminating guard
`cache_satisfaction_is_materialized_point_not_nominal_demand` is the sole
defense. **Backfill** clones a broader entry UNCHANGED (recorded points
verbatim, never a meet/nominal point) into a projection-depth-narrower target
slot (`slot_domain_siblings`: the legacy `Expanded→Shallow→Navigate→Identity`
DIRECTION), GATED by `cached_satisfies`. The direction stays narrower-only
because the lattice's `Navigate ⊒ Shallow` (higher normalization/operator
rungs) does NOT mean a `Navigate` next-hop result can serve a `Shallow` shell
surface — it carrier-stops without materialising the shell; an all-peers
backfill would hide cyclic-heritage expansions. The gate still REJECTS the
unsound legacy `Shallow → Navigate` clone (`Shallow ⊅ Navigate`:
`normalization_depth None < NavigateOnly`). Guards:
`cache_satisfaction_is_materialized_point_not_nominal_demand`,
`cache_satisfaction_requires_path_exact_not_prefix` (the path-axis
discriminator — a deep recorded point never satisfies a strict-prefix request),
`backfill_writes_only_recorded_materialized_points` (in
`semantic_query_memo::tests`); the enum-rank `backfill_targets` unconditional
fan-out is RETIRED.

**R7.** Shared semantic materialisations key by `ResolvedDeclSlotIdentity`
(content-free) as the cache slot identity. The cached value carries
`VersionedDeclIdentity` (with content / version info) for fence seeding, scope
identity, and version-aware semantic operations. Cross-owner reuse is the
architectural invariant — a `ChatMessageProps` reached from N owners
materialises once.

```rust
struct ResolvedDeclSlotIdentity {
    defining_canonical: Arc<str>,
    merged_symbol_name: InternedName,
    symbol_space: SymbolSpace,
    project_identity: ProjectId,
    type_env_hash: Hash16,
    lib_env_hash: Hash16,
}

struct VersionedDeclIdentity {
    slot: ResolvedDeclSlotIdentity,
    content_hash: Hash16,
    parse_env_hash: Hash16,
    merged_parts: SmallVec<[(DeclPartId, FactHash); 2]>,
}
```

`merged_symbol_name` is **stable across declaration reordering and TypeScript
declaration merging**. Same-scope `interface Foo` parts merge into one
`merged_symbol_name`. Per-part fingerprints live in
`VersionedDeclIdentity.merged_parts` for diagnostics and overload surfacing —
they are NOT the cache validation oracle.

**R8.** Only final per-owner payloads (`ComponentMetaResultDb`) are
owner-keyed. The slot key is content-free — `(owner_canonical,
options_fingerprint)` — per the query-identity-cache model: the owner's content
version (`owner_whole_hash`) is NOT in the slot key, it is carried by the
per-slot candidate and validated strictly on read. Concurrent owner content
versions coexist as bounded candidates inside one slot (R20).

**R9.** Reuse is the default; recomputation is the exception. A cache miss
under steady-state load is treated as evidence of a fact-graph bug or a real
semantic change — never as a routine event.

### Fact model

**R10.** Facts use stable `FactKey`s, not `Vec` indices. Removed facts validate
as misses (registry lookup returns `None`). Reordering a file does not change
`FactKey`s.

**R11.** Every binding-naming fact key carries `SymbolSpace ∈ {Type, Value,
Namespace}`. **`BothTypeValue` is forbidden.** A `class Foo` declaration
occupying both spaces emits TWO distinct facts: `Export("Foo", Type)` and
`Export("Foo", Value)`.

**R12.** Facts split strictly by domain. **Parse-domain facts (`FileFacts`,
parse_env keyed) never reference resolved canonicals.** **Resolve-domain facts
(`ResolvedImportFacts`, `RouteDb`, resolve_env keyed) carry resolutions.** The
producer of a parse-domain fact does not run the resolver; it only emits the
syntactic shape. `FactKey::domain()` routes validator lookups to the correct
store.

**R13.** Each `Fact` carries `semantic_hash` (alpha-normalised structural
fingerprint) and `display_hash` (cosmetic — JSDoc, param names, comments).
Signatures record per-observation `lane: Semantic | Display` so cosmetic edits
invalidate display-bearing materialisations only. Storage of semantic vs
display facts is **physically split** at the cache layer
(`MemberSemanticFactStore` keyed on `parse_stable_hash`, `MemberDisplayFactStore`
keyed on `content_hash`) so a cosmetic edit hits only the display store and
does not recompute semantic facts.

**R14 (path-precise).** `Export(name).semantic_hash` is computed over the
**export's body alone**, with every cross-decl reference (same-file local decl,
same-file member, imported binding) recorded as a **reference-shape edge** by
name + space, NOT by inlining the referent's body. Editing an unused local in
the same file does NOT invalidate consumers of an export that does not reach it.
Editing a member that `Pick<Foo, "a">` does not select does NOT invalidate that
consumer.

**R15.** `SyntacticExportSet` (parse-domain) records the exported-name set.
`SyntacticRouteInterface` separately fingerprints the exact authored
import/export routing geometry while excluding bodies, spans, resolved
canonicals, and project state. Its resolve-domain counterpart is
`RouteDb` — per-name route resolution (`RouteNameKey`) plus the barrel wildcard
surface (`BarrelRouteSurface.wildcard_edges`, specifier → resolved canonical).
The parse-owned interface and resolved route domains cannot be merged.

**R16.** Semantic fingerprints are alpha-normalised structural hashes.
Source-text hashes, span-based hashes, position offsets, or any hash that
changes under cosmetic edits (whitespace, comments, generic param rename,
declaration reordering) are forbidden as semantic hashes. Cosmetic changes live
in `display_hash` only.

### Sessions & concurrency

**R17.** Sessions are views over the base host. A `SessionView` never mutates
the host. `host.upsert` is not called from any query path. Overlay artifacts
are stored under the overlay's content hash and coexist with base artifacts
under different keys. Byte-identical overlay collapses to base hash
automatically.

`SessionView::content_hash_for` is a **view-authoritative current-content
oracle**, consistent with `SessionView::source` by contract — it returns the
hash of the exact bytes `source()` yields. An overlay-covered canonical
resolves to the overlay source's hash; every other canonical resolves through
`VerterHost::authoritative_current_content_hash` (the scheduler authority, no
permissive fallback). It is NOT a content-agnostic `FileArtifactStore` scan: a
stale pre-edit `IndexedReady` lingering past a same-canonical edit (lazy
invalidation) does not surface — an evicted / deleted canonical reports `None`.
The overlay materialiser (`materialize_overlay_indexed_ready_with_view`)
derives BOTH the overlay source and its content hash from the `SessionView`
itself (one authority) — a caller cannot pass a separate source/hash pair, so a
stale hash can never be paired with a fresh source. `FileArtifactStore` exposes
NO content-agnostic *currency oracle* — a canonical-only `Option`-returning
accessor that scans `artifacts` is forbidden by the
`file_artifact_store_defines_no_unpinned_currency_oracle` architecture guard.
The only intentional content-agnostic escapes are `get_any` /
`get_artifacts_any`, each guarded at every call site.

**R18.** The private request adapter captures `SessionView` and its completion
overlay at entry. Engine consumers use the five request-bound ports and owned
observations; `ResolverContext` returns no view or overlay borrow. Thread-local
"current view" globals remain forbidden.

**R19.** Fact validation is the **cache-correctness oracle**.
`StoreViewCompatToken` is the **concurrency oracle**: singleflight lane
separation, mid-query change detection, write admission against superseded
computations. The two are orthogonal and must not be conflated.

**R20.** Multi-candidate storage isolates concurrent overlay variants in the
same query-identity slot. Default cap = 4 candidates per slot. **Eviction is
insertion-order (FIFO) on write only.** No LRU bookkeeping on read; warm reads
are `&self` shared borrows with zero atomic write or lock contention.
Concurrent sessions never overwrite each other's results.

Vue runtime props/emits heritage filtering stays inside this same
`SemanticGraphStore` family memo. `VueRuntimeSurfaceShallow` is an internal
slot selected by `ReductionDemand::VueRuntimeObjectSurface`, isolated from the
unfiltered `MacroSurfaceShallow` slot with no cross-backfill. The runtime
constructor additionally sets the orthogonal content-free
`VueHeritagePolicy::SuppressIgnored`; default contexts retain all heritage.
Every `ProjectionReductionContext`-bearing family and mapped-member identity
encoding carries this policy because a declaration-carrier unwrap demotes the
demand to `StructuralTransit` while retaining runtime filtering. Thus filtered
and unfiltered transit computations remain distinct even though they select the
same transit mode slot. Fresh-context transitions inherit the policy only via
the exhaustive `ProjectionReductionContext::with_orthogonal_axes_from`
adapter; adding a context field fails its no-`..` witness until ownership is
classified. This is not a new durable cache or a new
`ProjectionMode`; version state remains exclusively on each candidate's
prepared-declaration/LowerLocator read set and file self-roots per R6/R20.
The associated `ClassifyBroadRuntime` family is likewise keyed by a
content-free canonical macro payload/member locator plus split env dimensions,
never by its transient `SemanticNodeId`. Each cold build rehydrates the live
carrier from the indexed owner and publishes its exact value-side read set;
anonymous graph-instance classifications are ReturnOnly and have no family
entry.

**Bounded query-identity retention substrate.** Every durable query-identity
cache accumulates concurrent content-version CANDIDATES in its content-free
slots — the version state lives on each candidate's value-side carrier (per R6),
NOT in the key, so successive content edits co-locate as candidates in ONE slot
rather than minting fresh top-level keys. The bounded caches:
`ComponentMetaResultDb` (owner whole-hash candidate discriminant),
the `SemanticGraphStore` family memo (which also carries the relation memo's
entries under its `Relate` family — the retired dedicated `BudgetedRelationMemo`
folded into the family substrate) + named-type index
(`ResolvedDeclSlotIdentity` slots; per-version state on the value) + the
`SemanticGraphStore` derivation/origin store (`DerivationStore` — `edges` keyed by
`(SemanticNodeId, OriginEdgeKind)`, plus its `signature_pool` fence interner;
content-addressed graph-instance state, NOT a query-identity slot) — are all
bounded by the shared `verter_type_engine::bounded_query_retention` substrate. Each
distinct content edit appends a fresh CANDIDATE (or, for the content-addressed
`DerivationStore`, a fresh edge bucket); the substrate is the routine
memory-reclamation path (the eager own-canonical drain that formerly reclaimed
these is retired). Two cooperating pieces, tuned per cache:

- `GlobalRetentionBudget<K>` — a FIFO insertion-ordered total-size cap. A cache
  records each admitted entry's key from its write-side `post_publish` hook;
  the budget returns the oldest keys to evict once the count exceeds the cap.
  the `SemanticGraphStore` family / relation / named-type budgets use
  `DEFAULT_BUDGET_CAP` (4096); the `DerivationStore` bounds its edge-bucket
  count (`DERIVATION_EDGE_BUCKET_CAP` = 4096) and its signature-interning pool
  (`DERIVATION_SIGNATURE_POOL_CAP` = 4096) with two such budgets, evicted
  write-side from `record` / `intern_signature`. The cache's invalidation paths
  call `forget` so the ledger stays consistent with the map.
  **Group admission with self-exemption** (`record_admissions_exempt`) is the
  second entry: a publisher writing a set of entries that is only coherent
  WHOLE — the batched SCC member publish, whose members are readable only while
  their fenced root stays resident — records the entire set as ONE budget step
  whose victim selection exempts every key the set needs. Per-entry admission
  cannot see that constraint: under cap pressure entry *n*'s FIFO victim can be
  the publisher's own root or an entry `0..n` it already wrote, leaving a
  torn SUFFIX whose surviving members depend on iteration ORDER. The exemption
  obliges the caller to REFUSE a group wider than the cap (an exemption set
  larger than `cap` would pin the ledger above its bound); the store-side entry
  is `SemanticGraphStore::record_family_admissions_locked`
  (`semantic_query_memo/family_retention.rs`), and the SCC publish's whole-batch
  refusal lives in `scc_publish.rs`.
- `BoundedCandidateMap<K, D, V>` — a content-free-keyed slot map with a bounded
  per-slot candidate list (`DEFAULT_CANDIDATE_CAP` = 4) AND a global budget.
  `ComponentMetaResultDb` is built on it: the slot key drops `owner_whole_hash`,
  which becomes the per-candidate discriminant `D`. A fifth owner version in a
  slot FIFO-evicts the oldest candidate; the global budget (`GLOBAL_BUDGET` =
  512) caps the total across all slots.

Eviction is stale-first when cheaply detectable, then FIFO. Evicting a *valid*
entry is permitted — it only forces a recompute, never an incorrect result. A
reader clones the candidate `Arc` before validating, so a concurrent removal
never invalidates an in-flight reader; removal is keyed by an insertion
sequence number unique per admission. `BoundedCandidateMap::admit` holds the
`DashMap` shard write guard across its candidate push, so an empty-slot
reaper's `remove_if`-detach cannot interleave between "slot observed empty" and
an in-flight admit's push — a published candidate is never stranded in a
detached slot.

**Single write-side consistency domain (rule).** Every budgeted cache must have
exactly one write-side consistency domain: either the map + budget + reverse
index are mutated under one exclusive lock, or every gap is closed by BOTH
atomic same-key admission AND identity-scoped removal. New budgeted caches must
prefer structural (single-lock) serialization. Concretely:
`BoundedCandidateMap::admit` runs the slot mutation, the removed-seq
`forget_seq`, AND the new candidate's `record_admission` inside one
continuously-held slot `Mutex` critical section (the `retention_gate` is only a
reset fence — a shared read guard does not serialise two admits of the same
content-free slot); global-budget victim eviction runs after that slot lock is
released (lock order `retention_gate.read → DashMap shard/slot → slot Mutex →
budget Mutex`, victim slot lock last — no AB-BA). The named-type index
(`BudgetedNamedTypeIndex`) holds a per-wrapper `admission_lock` across the
`DashMap::entry` decision + `record_admission` + victim `remove_if` (and
across `retain_for_canonical`'s map removal + `forget_seq`
loop), making its single-write-domain structural rather than
"safe-by-construction". (The `SemanticGraphStore` relation memo rode the
same pattern as the retired `BudgetedRelationMemo`; it is now rehomed into
the family memo's `Relate` family, whose write domain is the single global
`entries` lock — see `semantic_query_memo/mod.rs`.) A budgeted
`ReverseIndexedCandidateStore` publish holds the `publish_fence` (the store's
`retention_gate`) across `entries.insert` + `post_publish`, and `post_publish`
does `bump_live_counter` + `record_admission` together — so the map insert, the
`live_counter` increment, and the budget admission are one fenced write-side
step.

**Per-MapK publish linearization (by-flight-key shape).** In the
`cooperative_admit_with_post_publish_by_flight_key` adapter the published map
key (`MapK`) is independent of the in-flight coalescing identity (`FlightK`) —
e.g. two overlays on the same cache key flight on distinct `FlightK` lanes but
publish under one `MapK`. Both can become cold winners and both reach the
publish path, so the second publish DISPLACES the first and the displaced entry
must receive `removal_cleanup`. The `publish_fence` is a SHARED read guard: it
serializes publish-vs-`clear` (write guard) but does NOT serialize
publisher-vs-publisher. A bare `map.insert` plus a post-hoc cleanup of the
returned displaced value is therefore unsound — the displacing publisher can
run `removal_cleanup` for an entry whose own `post_publish` has not yet run,
underflowing a `live_counter` (the matching bump never happened) or orphaning a
reverse-index registration. The full publish triple (insert/replace →
displaced `removal_cleanup` → new `post_publish`) must be ONE operation atomic
per `MapK` against other publishers. The substrate
(`publish_entry_linearized_per_map_key`) rides the map's per-key shard lock —
`DashMap::entry` holds the shard write guard for the `Entry`'s lifetime, and a
`DashMap` shards by key — rather than a new global lock; contention stays
per-shard, identical to a plain insert. A displaced entry is then observable
only to a shard guard acquired AFTER the displaced publisher released its own,
i.e. after that entry's `post_publish` completed: the displacing publisher's
cleanup is the linearized successor of the displaced publisher's completed
publish. Consequence: a hook published under this linearized shape MUST NOT
re-enter the SAME `entries` map (it would self-deadlock on the held shard write
guard) — the by-flight-key consumers' hooks are lock-free `live_counter`
atomics or touch a SEPARATE `CanonicalReverseIndex` map, so they are safe.

**Path-split: the unified-key form does NOT linearize.** The unified-key entry
point (`cooperative_admit_with_post_publish`, `MapK == FlightK`) coalesces every
caller of one key onto ONE cold compute via the in-flight table, so there is
exactly ONE publisher per key and the map slot is always vacant at publish — no
overwrite ever occurs and there is no publisher-vs-publisher to serialize. It
therefore publishes through `publish_entry_insert_then_post_publish`:
`map.insert` takes, mutates, and RELEASES the shard guard, then `post_publish`
runs with NO shard guard held. This is REQUIRED, not merely permitted: a
budgeted unified consumer (a `ReverseIndexedCandidateStore`) runs a
FIFO `register_post_publish` → `evict_budget_victim` →
`entries.remove_if(victim)` re-entry on the SAME map from inside `post_publish`.
Were the unified path to hold the shard write guard across `post_publish` (the
linearized shape), a victim hashing to the just-published key's shard would
deadlock the publishing thread on a non-reentrant same-shard write-lock
acquisition. The two publish shapes are selected by `cooperative_admit_impl`'s
`linearize_publish` flag (the public by-flight-key wrapper passes `true`, the
unified wrapper passes `false`); both ride the one winner/joiner state machine.
Discriminators: `by_flight_key_displaced_cleanup_runs_after_its_own_post_publish`
proves the by-flight-key ORDERING (A's `post_publish` happens-before B's
cleanup-of-A via an event log + a signed-counter low-water invariant), distinct
from the aggregate-count test
`by_flight_key_overwrite_cleans_up_displaced_entry` which only proves the counts
net out; and `unified_budgeted_post_publish_eviction_does_not_self_deadlock`
proves the unified path's deadlock-freedom (a cap-1 budgeted cache whose
`post_publish` re-enters the map to evict a same-shard victim during
publication completes under a 5s watchdog — it hangs against the linearized
shape).

`SemanticGraphStore::invalidate_all` (the project-generation bump) clears EVERY
`SemanticNodeId`-keyed semantic cache on the store — the family memo (whose
clear also drops the relation memo's `Relate`-family entries), the
in-flight admission table, the named-type index, the
`DerivationStore` (edges + signature pool), and the Γ.B reverse index — so no
stale judgement survives a tsconfig / SDK / workspace-folder change. Each
cleared `SemanticNodeId`-keyed cache has its retention ledger cleared in
lockstep. Adding a new `SemanticNodeId`-keyed semantic cache to the store
obliges extending `invalidate_all`'s clear set. `invalidate_all` does NOT touch
the node arena: `SemanticNodeId` is a raw `u64` arena index with no generation
tag, so the arena's dense `nodes` / `scopes` storage is **append-only** — every
`SemanticNodeId` handed out stays valid for the store's lifetime. Reclaiming
the arena id space would require a generational `SemanticNodeId` redesign (a
mid-flight arena shrink under concurrent index-holding readers is unsafe) and
is out of scope of the bounded-retention substrate. The substrate bounds the
`SemanticNodeId`-keyed *caches* listed above; the node arena itself is
unbounded by design pending that redesign.

Concrete substrate (the per-domain target form):

- Outer table: `DashMap<K, Arc<CacheEntry<V>>>`.
- `CacheEntry<V> { candidates: ArcSwap<SmallVec<[Arc<Candidate<V>>; 2]>> }`.
- Admission writes use RCU; write contention serialised by
  `StoreViewCompatToken`-keyed singleflight.

Live observation storage: `FactReadSet::observe` and
`observe_borrowed_signature` suppress exact duplicates in a bounded recent
window before cloning or appending them. Replaying a small warm witness must
not grow the outer tracer per read. The window bounds insertion work; it
does not bound the answer or replace final canonicalization. Distinct versions
of the same fact remain distinct observations.

Per-query frame cost: every cold query seals its tracer, builds its completed
carrier and registers its memo candidate, so these steps copy nothing per fact
beyond the output they own. `FactReadSet::finalise` merges absorbed canonical
runs over borrowed facts (comparison count unchanged) and clones each distinct
fact once; fan-out to an enclosing tracer copies a fact only when its recent
window lacks it; `semantic_graph_read_set_signature` keys self-roots in a sorted
small list and leaves sorting to the one seal. A sealed traced set that
already holds the build's fence and every observed self-root at its observed
version is shared as the completed carrier rather than copied and sealed
again. The reverse index
(`semantic_query_memo::ReceiptReverseIndex`, shared by the family memo, the
refusal-summary table and `cache_runtime::ReverseIndexedCandidateStore`)
registers DIRECT edges, each once: canonical → the evidence (carrier, receipt
or page, cached or not) whose own facts read it, consumed receipt → the
evidence that consumed it, carrier → its candidates, dispatch fence → the
candidates sharing that fence allocation (one node per interned fence, so its
canonicals register once), plus each candidate's strict self-roots. A chain or diamond costs
`O(F + E + C)` physical registrations however many candidates share it —
never a per-candidate canonical closure — and `invalidate_canonical` walks up
from the canonical, each node once. Unregistering a candidate drops every
evidence node and canonical only it reached, so the index drains to empty with
its last candidate; edges naming a candidate are charged by that candidate's
reservation, everything else by the index's own pin. Each charge follows the
allocation it covers, under the declared heap-block convention
(`retention::allocation::heap_block_bytes`): every table and holder set by the
layout of its backing allocation (`hashbrown` `allocation_size`), which no
removal — tombstoned or not — shrinks, from its allocation until it is rebuilt
(at a quarter live) or freed; each candidate's extra-canonical list and each
canonical's text by its own block. A full drain, a clear or a drop releases
exactly what was charged. The allocator canaries
(`allocator_canaries` `reverse_index_backing`) measure the blocks the index
holds from the allocator's layouts and require the index's charge plus its
owners' reservations to cover them at every stage, within the charge granule. There is no
account-less index: `Default` charges the process-local account, and
`ReceiptReverseIndex::charging_registrations` (the candidate store, whose
candidates charge nothing) charges the candidates' share too. The index drives
explicit-invalidation cleanup only, never validity; its one leaf lock is taken
inside each store's existing consistency boundary. Every store registers a
candidate in the step that makes it visible — the family memo's publishes
(ordinary, prefix backfill, SCC member) under the `entries` hold that installs
it, the candidate store under the slot guard — and only then unregisters the
candidates that step displaced, so evidence they share stays indexed across a
replacement and a read of the index owes no registration work
(`a_publish_registers_its_candidate_and_a_read_links_nothing`,
`replacing_a_registration_keeps_its_shared_evidence`). The
allocation canary `recursive_paths_allocations_per_entity_stay_flat` guards the
per-entity allocation ceiling and its flat growth across layouts.

Delivering tracer scopes: an observation fans out from the innermost tracer
down to the first DELIVERING scope inclusive (`fact_tracing::tracing::
install_delivering`), not to every level. A scope is delivering only when its
owner hands its whole completed read set, or a carrier holding at least those
facts, to the scopes active when it completes: the semantic-memo cold build
(`install_delivering_fact_tracer`; `ProducerLease`/`SettledProducer::complete`
bubbles the carrier — traced facts, fence and prelude facts, self-roots —
whether or not the result is admitted, and a joiner receives the same carrier)
and the stepwise continuation tracers (`OwnedFactTracer`: an evaluation frame
replays its sealed set into its parent frame, the root frame and every frame of
a stopped evaluation into the tracers around the evaluation; an instantiation
frame's carrier is replayed by the consuming frame or the drive's entry).
Every other installer stays eager. Non-cacheability marks and recorders still
reach every level. A scope below a delivering one therefore receives that
scope's facts once, deduplicated, instead of once per observation at every
depth. The fact-work canary `recursive_paths_fact_work_per_entity_stays_flat`
guards the per-entity fact work and its flat growth.

Root self-evaluation: a deferred-evaluation ROOT that finishes as itself
(`evaluates_to_itself`) is admitted without a frame, tracer or memo entry: its
cold run is one step under its own `(node, context, demand)` identity with an
empty read set, so the dispatcher admits a receipt of exactly that cost
(cached per dispatcher, one per identity) — charged once per connected demand,
request operations marked paid, recorded as the consumer's prerequisite in
place. A demand that cannot pay runs the evaluation and reports its limit as a
cold one does.

Construction ownership: an intern records its construction count on the
innermost construction owner of its arena as a bare ordinal, released in one
batch when that owner closes. A delivery lease is held by the OLDEST open
owner of its arena (`NodeRootScope::capture_delivery`; the innermost owner
when the lease names no arena). A suspended task's construction owner
(`TaskConstructionOwner`) is a floor while a poll has it installed: the
search starts at the newest installed task owner of the arena, so a delivery
a task's step reads stays with the task across `Pending` and is never handed
to an older owner the polling thread opened for unrelated work. A closing owner hands its deliveries to
another open owner of the arena, so a delivery lives until no owner of its
arena is open on the thread whichever owner holds it, and holding it at the
oldest spares the hand-on at every nested scope's close. A closing owner that
does hold deliveries merges the smaller holding into the larger. A
dispatcher's construction lane is never an owner: interns, roots and
deliveries skip it, so the outermost operation's owner releases them when the
operation ends and a result outlives it only through the carrier it is
delivered in (`SemanticQueryOutput`, `CacheRead`) or an explicit builder owner.

Request completion tier: the results a REQUEST computed and may serve again
live in one request-shared tier (`project_semantic_dispatch/request_completions.rs`),
owned by the request context (`RequestContext::semantic_completions`), one
tier per graph and store view (`RequestCompletions::tier_for(graph,
compat_token)`). Every dispatcher of the request on that graph and view holds
the same tier — one per host operation, and every semantic task a drive lends
its state to (`task_dispatcher` shares the tier; `exchange_request_state` does
not swap it) — so a result one dispatcher completed answers every other. A
dispatcher made with no request context installed owns a tier its tasks share.
The tier is reclaimed with its owner: dropping the request context releases
every result it kept.

The tier keeps (`keep_published`) the results the request's builds published
— admitted results, kept for a claim the memo cannot serve back to the
request's own view (the candidate records facts newer than the view) — and
its request-local completions, each with its read (and the receipt its
computation sealed), its carrier, and the workspace edit clocks it was
computed at. Every entry holds a `Retained`
charge on the process retention account: a refused reservation keeps nothing,
and the result's delivery stays its only owner. A kept result serves
(`own_published_read`, `kept_value_read`) only while its edit clocks are
current and its kernel epochs remain live; the read replays its receipt into
the active demand (`served_read`) and delivers its carrier. A result the
demand cannot pay for is recomputed, never served. Results a lookup SERVED are
not kept: a memo candidate is validated against the live view on every read,
and the edit clocks alone miss a file the host reloads mid-request
(`invalidation_imported_prop_type_file_edit`).

Request-local completion: a NESTED read of an intermediate family —
`Conditional`, `TemplateLiteralReduce`, `ProjectPath`, `SignaturesOfType` and
`Relate` (`completes_request_locally` in `project_semantic_dispatch/mod.rs`) —
uncaptured, with no operand force active, no family-memo producer of the key
open on this task, and outside every checker obligation
(`ObligationRuntime::decides_root`) unless the family completes inside one
(`completes_request_locally_in_obligations`: a callable-shape read decides no
obligation, so inside a relation, flow or call frame it is the result the memo
would admit there), completes for the request. A relation does so only as the
root of its checker transaction read by another computation (a decision's
relation of each distributed member): its publication capture stays empty.
The completion reads the request's kept result first, then makes the family
memo's validated lookup exactly as a claim does (`begin_query_claim`: a stored
result answers, one the demand cannot pay for is recomputed). A key another
task is producing (`SemanticGraphStore::has_flight`) is joined through the
family memo's claim instead, never computed beside it, so two computations
that need each other across tasks meet the wait-cycle cut. Otherwise the
completion runs the same traced build under the producer's cost identity
(`ClaimAttempt::cost_identity`) and settles its output through the SAME
`settle_output` the memo's `ProducerLease::settle` uses
(`SemanticGraphStore::complete_request_local`): the carrier is bubbled, the
read carries its receipt, and a complete priced result is kept in the tier.
It registers no flight and is never admitted: the evaluation or build that
reads it is itself retained (the deferred-evaluation memo, the enclosing
instantiation or root query), so an entry of its own would be claimed,
admitted, reverse-indexed and FIFO-evicted without a later request reaching
it — a conditional distributed over `n` members would leave `2n` relation and
`2n` signature entries behind. A re-entry of a key whose local completion is
open answers the same-path recursion carrier on either path. Root reads,
other obligation-frame reads and captured reads keep the family memo.

A relation root completed for the request keeps its component with it
(`relation_keep_completed_component`): the root and every member the
component completed that a published root's batched member publish would
admit — decided binary relation members, proven flow members and call
members, each with a flight — kept under the root's carrier, receipt and
edit clocks (`completed_member_read`), under ONE charge for the
whole component, all or nothing (`RequestCompletionTier::keep_component`). The
members' flights end unpublished (a joiner computes again; a reader in the
request finds the kept member), and the inline relation, flow-return and
call-resolution warm reads consult the tier after the memo
(`relation_warm_step`, `warm_flow_return_read`, `kept_call_result`). A root
that is not kept (partial, unpriced, cancelled) releases its members unkept,
as an unpublished root does. Guarded by
`recursive_paths_admit_their_instantiations_not_their_nested_steps`,
`repeated_infer_retains_its_declarations_not_its_per_member_work`,
`dispatchers_of_one_request_share_its_completed_relations`,
`mixed_component_member_entry_self_roots_cover_all_component_files` and the
tier's own reclamation and pressure tests (`request_completions_tests.rs`).

Transaction file serves: every instantiation reads its declaration file's
`IndexedReady` record (`ProjectSemanticDispatch::transaction_indexed_record`), as do its global-interface, member-index and augmentation-candidate reads,
and so do the other dispatcher reads that need only the record: a `typeof`
build's and a `LowerLocator` build's own file, module-kind, exported-function
augmentation and value-signature reads, namespace merging, a call's pending
state and the flow evaluator's callee-augmentation check (through
`FlowDemandDriver::transaction_indexed_record`).
The first serve of a canonical on a transaction is recorded with
`record_transaction_read` and kept in `CheckerDispatchTransaction::indexed_serves`
under the workspace edit clocks it was served at; a later read under the same
clocks replays that read (`replay_transaction_read`: its facts, refusal,
self-roots and canonical evidence) into the scopes live at the later read
instead of re-serving. The replay is the authority for what the later read
depends on: the record alone carries none of the serve's facts, so a reader
that skipped it would publish a carrier missing the file's parse and route
facts (`a_replayed_declaration_serve_publishes_the_facts_its_first_serve_does`,
`a_sibling_declaration_answered_from_the_recorded_serve_publishes_its_facts`
fail with the replay removed). Lifetime: a kept serve lives as long as its
transaction; an edit moves the clocks and the file is served afresh; a refused
or non-reusable serve is never kept, and a read where no channel is
observable (no live tracer or build-local frame, or an already partial
cold-compute scope) is served directly and never kept.

Paged signature evidence: a `fact_dep_signature` is never refused for its
width. `FactReadSet::finalise` (and `seal_canonical_signature` for a signature
a producer assembles by hand) cuts a canonical set wider than
`FACT_PAGE_WIDTH` (1024) into fixed-width contiguous runs, each held by one
immutable evidence page — a `ResultReceipt` of `EvidenceKind::Page` — and pages
the level of pages again until the top level fits one page. Pages are
deterministic (cut points are positions in the canonical order), lossless,
validated leaf by leaf through the same receipt walk as a consumed result's
receipt, registered in the reverse index as evidence nodes like any consumed
receipt (each page once, under the canonicals its own facts read and the
evidence that consumed it), and shared by `Arc` with every candidate, refusal summary and
enclosing signature that absorbs them. Each page holds ONE retention charge for
its whole life, sized by what it stores (entry array, owned fact strings, the
canonical-set storage it does not share), however many holders share it. A page
is born `Pinned` (the live signature that sealed it owns it); the first cache
admission retaining a signature that holds it claims it through
`receipt::reserve_retained_with_evidence`: the candidate's own bytes and every
unclaimed page's bytes are ONE refusable `Retained` reservation (so a wide
candidate is refused as `Oversized`/under pressure for what it would newly
retain, and its complete value is delivered uncached), and each claimed page's
pin is exchanged for its split-off share. The claim walks every receipt that
can reach a page — a consumed result's receipt included, since a candidate
retains the pages behind it — skipping evidence that reaches none or whose
pages are already claimed. `ValidatedFactCache` (route, imported-root,
resolved-import, fallthrough, component-meta runtime) claims the pages its own
sealing mints at admission; a refused claim admits nothing. A retaining store
that charges none of its own bytes claims through
`receipt::claim_evidence_pages` (the same reservation with no own bytes; a
signature reaching no unclaimed page reserves nothing): the generic
`cache_runtime` artifact and query-candidate publish paths (a refused claim
bubbles the signature and returns the value `ReturnOnly`), compile slots,
binder-identity facts, owner import surfaces, framework surface and resolved
script-fact stores, the raw-template analysis slot's class-fact signature,
and the app-config proof seed. Workspace and request-overlay lazy-resolution
candidates reserve their own bytes (including decision edges in the workspace
lane) together with reachable unclaimed pages through
`ResolutionRetentionAccount::reserve_retained_with_evidence`; the production
`ResolutionRetention` adapter performs the one combined reservation. Byte-only
test accounts refuse signatures requiring an evidence transfer; tests that
admit wide signatures inject `ResolutionRetention::new(account)`.
Retention refusal never bypasses the cache runtime's post-compute generation
and self-root validation or a query node's rejection lowering. Compile companion
publication uses the session node's actual `SessionPublishOutcome`: a refused
slot publishes neither scheduler artifacts nor raw-template companions.
A lowered invalid query propagates `PostComputeRevalidationFailed` to its
consumer, even when its page claim also failed. Only validated complete
delivery keeps `RetentionPressure` family-local. A refused compile evicts
previous companions under its captured source-version/witness fence; a late
refusal cannot evict a newer publication.
A refused claim stores
nothing and the complete value is delivered uncached. A later candidate sharing a claimed
page is not charged for it again; the page's charge drains when its last holder
drops. Candidate footprints therefore count a page entry once, not its contents.
Self-root discrimination reads through pages: a disagreeing traced
`FileWholeHash` on a page is a `SelfRootConflict`. A carrier whose width is
held by pages keeps its precise self-roots (validated strictly through the
pages); only a carrier whose TOP level overflows trades them for a
`StrictSelfRootWorld`, which is bound to the view that minted it.
`ReadSetSignature { facts }` is a complete rail; `entries()` reads it with its
pages read through. Producers still consume a hierarchical fact (a consumed
result's receipt, a downstream `semantic_hash`) instead of flattening
transitive facts: paging bounds a signature's storage, never its honesty.

### Non-cacheability marking and source evidence

The reuse vocabulary (`NonCacheableReadReason`, `ReuseClass`,
`classify_reuse`, `dominant_refusal`, the pure `ReuseClass::refusal_replay`
description) is pure data in `verter_session_query::facts::reuse`. APPLYING a
refusal belongs to the one tracing runtime, `verter_type_engine::fact_tracing`: the
tracer/recorder stacks (`tracing.rs`), the typed refusal-observation scopes
(`refusal_scope.rs`, `replay_reuse_refusal`), and the direct-call fan-out
`note_non_cacheable_read_fan_out` / `note_non_cacheable_propagation`. There is
no process-global receiver or callback: a mark reaches the running thread's
tracers only by a direct call into that module, which depends on nothing but
`verter_session_query`, the audit leaf and std.

Source-side code (the decl-body memo, decl lowering, flow-slice content, the
parsed eval program) never calls the tracer. A collapsing body accessor returns
`SourceRead<T> { value, refusal }` (`DemandOutcome::into_source_read`: a
`LeaseMiss` is `None` + `Some(LeaseMiss)`, a `Ready(None)` absence carries no
refusal and stays cacheable). Composition keeps every consulted read's evidence
(`SourceRead::or_else` keeps the refused read's reason even when a fallback
answers). The FIRST engine consumer calls `fact_tracing::consume_source_read`
before any early return, admission decision or memo insertion. Typed
`DemandOutcome` consumers keep their own matches and marks.

### Completed results and their receipts

An aborted singleflight never delivers its retained completion to a subscriber.
`Subscription::wait` retires and retries within its retry allowance; exhaustion
returns a typed `ExecutionAborted(Superseded)` read with suppression, partiality,
and no cost receipt. That disposition takes precedence over a retained value or
an `Other` sentinel, and dispatch propagates it into enclosing evaluations and
the audited FlowReturn API. A fresh clean claim can still compute or reuse a
validated result.

Completion and retention are separate verdicts. A flow evaluation that
completes as its own component's root lands in the dispatch transaction's
demand-keyed result table (`FlowResultTable` in `dispatch_txn.rs`), classified
on the shared reuse rail (`verter_session_query::facts::reuse`): `Shared` (complete and
retained), `RequestOnly` (complete, persistent admission refused for a
deterministic reason, whose refusal replays into every consumer) or never kept
(incomplete, transient or unattributed refusal). The table is separate from
the ordered publication queue the machinery root drains, so a refused or
aborted publication never makes a completed child run again on its
transaction.

A kept result's evidence is hierarchical: its own facts plus the receipt of
every completed result it consumed (`FactVersionRef::Receipt`, the
`ResultReceipt` in `verter_semantic::facts::receipt`), never a copy of the
consumed result's facts. When the result completes, every tracer and recorder
live around it replaces what the result's computation fanned into it (the range
between the two `mark_evidence` marks) with the receipt
(`complete_with_receipt`); a later consumer observes only the receipt.
`drop_subsumed_receipts` removes a receipt a sibling receipt consumed, so a
chain's scopes keep its top receipt. Every validator reads a receipt through to
the facts it reaches (`validates_through_receipts`), applying its own per-fact
rules (the strict self-root rule included) inside the receipt, visiting each
distinct receipt once per validated signature (`ReceiptWalk`). Signature
projections read receipt summaries (`canonical_ids`, `aggregated_domains`) or
walk them (`resolution_*`). Domain compaction never lifts a receipt. Receipts
form a DAG (a child is minted before its parent) and drop from an explicit
stack. A member closed inside a larger component keeps no receipt of its own:
the component root's receipt, whose evaluation covered every member, is the
sealed component receipt.

Whole-module export proofs use `ResultReceipt::retained_signature` for an
already-canonical producer signature and `retained_join` for its dependency
receipts. Joins radix-order receipt digests, retaining exact comparison for
equal digests, and never copy a child's transitive facts. These result-evidence
allocations use the same pin-to-retained reservation transfer as evidence
pages, so their charges remain attached when an enclosing cache retains the
proof after the export table is evicted. Canonical summaries are persistent
hash tries; their public iteration stays lexical through a byte radix walk.
Radix ordering consumes common prefixes once per bucket (including small
insertion buckets) and visits only the occupied bucket range when names
diverge. Component publication builds one set of registration inputs — the
shared carrier and the dispatch-fence canonicals — and reuses it for every
member and populated slot; the reverse index walks that carrier's direct facts
and child receipts once, registering only the evidence not indexed yet, so the
members share one carrier node and each registers only its own admission
identity, its carrier edge and its fence edges. Export-table scale tests inspect all
suffixes through one request context and count proof work
across construction and those reads; the request's receipt validation memo
remains responsible for sharing completed validation.

## Typed SignatureAdmission gate (CRITICAL)

Producers convert their finalised fact tracer into a typed admission verdict via
[`SignatureAdmission::from_finalise(FactReadSetFinalise)`](../../crates/verter_type_engine/src/cache_runtime/admission.rs).
Two arms:

- `SignatureAdmission::Cacheable(ReadSetSignature)` — the tracer finalised with
  its complete (paged when wide) path-precise signature. The producer publishes its cache entry
  under this signature; the warm-hit oracle validates it against the live store
  view on every read.
- `SignatureAdmission::NonCacheable(NonAdmissionReason)` — the tracer
  consumed a non-cacheable read, a domain moved mid-scope, the self-root
  closure is incomplete, or another structured refusal applies. The verdict
  carries the typed refusal reason (`MutationUnstable`, `UnresolvedProvenance`,
  `SelfRootConflict`, `RouteGenerationDependency`, `ForcedTestRefusal`,
  `IntrinsicNonCacheable`, etc.) for audit.

Production callsites pattern-match on `SignatureAdmission` directly:
`Cacheable(sig)` admits the entry through the cache substrate, `NonCacheable(_)`
refuses it. The two consumer families differ in how they route the refusal:

- **Cooperative cache-runtime producers** (imported registry,
  materialize-structure, ref-cycle) construct `CacheAdmission::ReturnOnly {
  value, reason }` so the cooperative joiners receive the value without
  admitting a shared cache entry. The typed reason is bridged from the
  producer's `SignatureAdmission::NonCacheable(reason)` arm to the lowering site
  via a per-thread TLS slot (`cache_runtime::set_return_only_reason` /
  `cache_runtime::take_return_only_reason`) so the reason reaches
  `CacheAdmission::ReturnOnly { reason }` honestly instead of defaulting to
  a generic reason.
- **Non-cooperative producers that own their carrier slot** (notably
  `CompileSlot.fact_dep_signature: ReadSetSignature` on the compile-tier
  cold-build path) route the `NonCacheable(_)` arm to a **skip-publish
  refusal**: the producer holds the freshly computed result, returns it to its
  single caller, and refuses the `compile_slots.insert` so no slot lands. A
  prior successful slot for the same `(canonical, profile)` is ADDITIONALLY
  removed (not just left in place) so the carrier invariant `present in
  compile_slots ⇒ admitted cache entry for the current version` survives a
  refused recompute. The companion scheduler artifact commit is gated on
  `Cacheable` admission so the refused result is not observable through
  `try_get_artifact` either.

`SignatureAdmission::into_cacheable()` is a test-fixture / owned projection
helper that returns the carrier as `Option<ReadSetSignature>` (or `None` on
refusal). It is consumed by test scaffolds that need the owned rail; production
callsites match on the variants directly.

Workspace resolution carries the same distinction through
`ResolutionOutcome`. A result-only compatibility query must consume
`into_transient_result()` and must not retain the target. Any provider, route,
exact-edge, parsed-edge, or artifact sink must instead consume
`into_publication()`, whose `ResolutionPublication::{Admitted, Unretained,
Refused}` shape keeps an admitted miss distinct from a typed refusal.
`Unretained` is a complete, current answer the retention account declined to
keep (`RetentionPressure` at the Engine's workspace-lane reservation): it is
DELIVERED to its consumer, never withheld like an untrustworthy non-admission
(torn, partial, superseded, cancelled, budget-exceeded), and never admitted —
there is no conversion to `AdmittedResolution`, so no persistent or shared
sink can root on it. A consumer answers from it (`delivered()` /
`into_delivered()`), import-graph bookkeeping (dependency edges, stage
blockers, provider sync) follows it, and every session entry that converts an
outcome into a publication marks the reading compute non-cacheable through
`NonCacheableReadReason::UnretainedResolution` (transitive), so nothing derived
from it is retained either; request-scoped normalization memos replay that
mark. A composed answer that depends on an unretained lane is itself
unretained (`or_else_publication`, `unretained_with`). `AdmittedResolution`
has no external constructor: only the Engine transaction finalizer can mint
one, and projections (`map_result`, `map_admitted`, `replace_result`) require an
existing admitted carrier. The former `from_admitted_state` escape hatch is
deleted. Batch producers such as provider-buffer preparation resolve the full
batch first and propagate a single refusal across the whole
resolution-derived product; they do not publish an admitted subset or treat a
refusal as an ordinary miss.

Filesystem-backed resolution uses a two-step evidence bridge. Discovery may
use normal workspace caches only to learn the observation set. Freeze then
re-reads every observed path type, file/manifest value, realpath, and exact
directory membership through independent live filesystem operations that
bypass snapshot, probe, manifest, realpath, and directory caches. The replay
reader is immutable and captured-only; a missing or conflicting observation
marks the bridge incomplete and forces `ReturnOnly`.

Published resolution provenance is always real. Context selection must identify
an actual configured project and its resolve environment/provider projection.
Missing importer/project/config/provider identity produces
`ResolutionIncompleteProvenance`; the transient result remains usable but no
query cache, parsed/lazy edge, provider/session state, or context-version table
is published. Production eval/declaration canonical normalization re-enters the
Engine; its old direct `file_exists` normalizer is test-only.

Hard rules:

- Direct construction of `Arc::from(Vec::<FactVersionRef>::new())` outside
  `ReadSetSignature::empty()` (allocated through the substrate helper
  `fact_signature_helpers::empty_fact_signature`) is forbidden. A refusal
  never becomes an empty signature: a refused compute either keeps its
  evidence (`NonCacheable(facts)`) or refuses through
  `SignatureAdmission::NonCacheable`, never an empty rail that would
  validate vacuously.
- `ReadSetSignature` carries facts only. The cache entry's
  world-generation lives on `CacheEntry<V>` alongside the value
  (`validated_at_generation`). Conflating generation onto the signature blurs
  the responsibility boundary.
- Width is never a refusal: a signature wider than one evidence page is
  paged, admitted and validated in full; an edit to a fact on any page —
  the last included — misses the warm read.

Guards: `signatures_around_and_beyond_the_page_width_keep_and_validate_every_fact`,
`an_edit_on_every_page_including_the_last_invalidates`,
`validate_with_self_roots_checks_every_page_of_a_wide_carrier`,
`a_wide_work_refusal_carries_its_whole_proving_prefix`,
`compile_fact_tracer_refusal_does_not_publish_compile_slot`.

## Error-Tolerance Non-Admission + §22 Absorption (CRITICAL)

The error-tolerant admission decision and the type-lattice absorption are a
closed, fact-rooted contract (`.claude/skills/type-resolution/SKILL.md`
§18.2–18.3, §22). Three invariants:

1. **Admission keys on the rooting FACT, not the taint enum class (§18.2).**
   [`admit_decision(taint, sig)`](../../crates/verter_type_engine/src/semantic_query/admit.rs)
   maps a result's [`ResultTaint`] + its sound
   [`ReadSetSignature`](../../crates/verter_type_engine/src/fact_signature_helpers.rs)
   to `Admission::{Warm, ReturnOnly}`. `Clean` ⇒ `Warm`.
   `Partial(MissingDependency)` ⇒ `Warm` **iff**
   `sig.records_missing_dependency_fact()` (a `ResolveImports` fact carrying a
   captured resolution witness) else `ReturnOnly`; `Partial(UnresolvedReference)` ⇒ `Warm` **iff**
   `sig.records_negative_resolution_fact()` (a `ResolvedImportClause` /
   `ResolvedReexportBinding` whose `resolved_canonical` is the
   `UNRESOLVED_SENTINEL`) else `ReturnOnly`. `Partial(IncompleteDeclaration)`,
   `Broken(SyntaxError)`, and `Broken(TornRead)` are always `ReturnOnly`. The
   bare taint discriminant NEVER licenses a warm publish — the signature is the
   authority for whether the invalidation rail IS present. `admit_decision` is
   the sole admission gate in `finalise_traced_build_output`. The gate keys on
   the rooting fact KIND on the signature; correlating that fact to the SPECIFIC
   degraded reference is a §18.4 follow-up. Taint PRODUCERS (parser
   error-recovery, resolver degradation, completion-fence torn-read) are
   produced by the §18.4 producers; `taint` is currently always `Clean`, so the
   live publish behavior is unchanged and the partial/broken arms are exercised
   by the `admit_decision` unit tests.

2. **Taint join is monotone over `Clean ⊑ Partial ⊑ Broken` (§18.3).**
   [`ResultTaint::join`](../../crates/verter_type_engine/src/semantic_query.rs)
   propagates the MAX level; within a level it keeps the more-severe
   `BrokenInputClass` (severity order `MissingDependency < UnresolvedReference <
   IncompleteDeclaration < SyntaxError < TornRead`). Finite + monotone — taint
   only moves up, so propagation terminates.

3. **§22 absorption is the reducers' FIRST fast-reject, as separable helpers.**
   [`absorb_*`](../../crates/verter_type_engine/src/project_semantic_dispatch/absorb.rs)
   (`absorb_union`/`absorb_intersection`/`absorb_key_of`/`absorb_indexed_access`/`absorb_mapped`/`absorb_conditional`)
   are isolated entry hooks each reducer calls with ONE
   `if let Some(out) = self.absorb_*(...) { return out; }` line BEFORE its
   structural body — never invasive per-arm edits. They do **cheap `node_data`
   peeks only** (bounded transparent `Alias` unwrap; no `execute` /
   `evaluate_deferred` / resolver work). Table: `X|any=any`, `X|never=X`,
   `X|unknown=unknown`; `X&never=never`, `X&any=any`, `X&unknown=X`; `any[K]=any`,
   `never[K]=never`, `unknown[K]`=UNCONDITIONAL error, `keyof any/never =
   string|number|symbol`, `keyof unknown = never`; mapped over `never`=`{}`,
   direct mapped over `unknown`=error; conditional `any extends T ? X : Y = X|Y`
   (union of BOTH branches via `ReduceUnion`, mode-independent, except when
   `extends` is an `infer` pattern → fall through to the infer-binding path),
   DISTRIBUTIVE `never extends T = never`, NON-distributive `never extends T = `
   true branch (the fast-reject gates the collapse on `distributive`),
   `error extends T = error` (carrier-dominating, FIRST). `any`/`never`/`unknown`
   are `Clean` (legitimately cacheable). **`error` rides
   `SemanticNodeData::Opaque(QueryError)`** (no new `GraphTypeNode` wire arm): an
   `error` operand **carrier-dominates** every other absorber, so the error
   CARRIER (node identity + `QueryError` payload) is never hidden behind a
   `Clean` extreme — relation/display keep seeing the error type. This is
   carrier-dominating, NOT taint-propagating: the absorbed `QueryBuildOutput`'s
   `taint` defaults to `Clean` and absorption does NOT join any operand's §18
   taint onto it. That is sound today because no producer emits non-`Clean`
   taint, so every absorbed type error is deterministic (`unknown[K]`,
   `keyof error`, …) and legitimately cacheable. An error becomes
   `ReturnOnly`-prone only when it is INPUT-DEGRADED — a §18.4 property routed
   through `admit_decision` once taint producers land (see the `TODO(§18.4)` in
   `absorbed_output`). `error` relates **bidirectionally like `any`** in
   the relation authority (`execute_relate` — the wildcard fires in both the
   O(tag) prefilter and the structural reducer, before the `never` bottom arm,
   so a broken sub-result never cascades spurious `NotAssignable`). `QueryError::DeclPlaceholder` is an expandable carrier, NOT
   the error type, and is excluded from both the absorption and the relation
   flip.

Guards:
`error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable`,
`error_any_never_propagation_lattice`,
`error_type_is_returnonly_prone_any_is_cacheable`
(`crates/verter_session/src/error_propagation_lattice_tests.rs`).

### Environment & GC

**R21.** Environment hashes split into **five** orthogonal dimensions (the
most-cited rule):

| Hash | Captures |
|---|---|
| `parse_env_hash` | Parser / SFC / compiler feature flags, syntax mode, language target |
| `resolve_env_hash` | `base_url`, `paths`, workspace aliases, project references, module resolution mode, package `exports`/`imports`, default extension order |
| `type_env_hash` | TS semantic options that change type meaning (`strict`, `noImplicitAny`, etc.) |
| `lib_env_hash` | TS built-in lib selection (`lib.dom.d.ts`, `lib.es*.d.ts`), `types`, `typeRoots`, registered ambient libs, global / module-augmentation corpus identity |
| `project_identity` | Project root, tsconfig path, provider root, workspace root, membership / owner selection |

**Scoping rule for `lib_env_hash` (per R21).** `lib_env_hash` enters a cache
key only when the cached value depends on lib data:

- `ResolvedImportFacts` (base import resolution) does **NOT** include
  `lib_env_hash`. A lib update does not change where `./theme` resolves.
- `RouteDb` per-name and effective-set caches **DO** include `lib_env_hash`
  because module augmentations stitch into the effective surface.
- Typed-IR resolve, `SemanticGraphStore`, `ComponentMetaResultDb` **DO** include `lib_env_hash`
  because semantic meaning depends on intrinsic types (`Array<T>`,
  `HTMLElement`, etc.).

A single bundled `project_config_hash` is forbidden.

The five hash functions live on `verter_session_query::resolution::IdeProjectConfig` (the `verter_workspace::env_hash` trait impls)
+ per-call inputs surfaced through `EnvHashInputs<'_>`:

```rust
cfg.parse_env_hash(&inputs);   // Hash16
cfg.resolve_env_hash(&inputs); // Hash16
cfg.type_env_hash(&inputs);    // Hash16
cfg.lib_env_hash(&inputs);     // Hash16
cfg.project_identity();        // Hash16
```

Each function mixes a per-dimension salt so the five hashes derived from
identical baseline state never collide.

### Module-Resolution Keying (CRITICAL)

Module/import resolution is keyed on the **split** env dimensions, and the lib
corpus is **NEVER folded into `resolve_env_hash`**. This is the
import-resolving refinement of R21 — `resolve_env` and `lib_env` are orthogonal
dimensions, and conflating them is a correctness bug (a `lib.dom.d.ts` bump must
not invalidate where `./theme` resolves; a `moduleResolution` change must not
invalidate intrinsic-type meaning).

Concrete contract:

- Every import-resolving cache key / `*Context` carries only the dims it
  depends on: `resolve_env_hash` always; `lib_env_hash` ONLY when the value
  consults the ambient/types corpus (module augmentations); `parse_env_hash`
  and `project_identity` per their own scoping. `ResolvedImportFactsKey` =
  `{parse_env_hash, resolve_env_hash}` (NO `lib_env_hash`);
  `AugmentationTargetKey` = `{project_identity, resolve_env_hash,
  lib_env_hash, population, target}` and `RouteDb`'s `RouteNameKey` /
  `BarrelSurfaceKey` both carry `lib_env_hash` (lib because the
  ambient/types corpus supplies module augmentations).
- **Resolve-domain ENV inputs** (hash into `resolve_env_hash` ONLY): the
  `moduleResolution` mode (`ModuleResolutionMode`), the active
  `exports`/`imports` condition set (`ConditionSet`), `base_url`/`paths`,
  workspace aliases, project references, extension order.
- **Lib-domain ENV inputs** (hash into `lib_env_hash`, NEVER
  `resolve_env_hash`): TS lib selection (`lib_names`), `typeRoots`, the
  ambient-corpus fingerprint.
- The module-resolution design vocabulary (`ModuleResolutionMode`,
  `SpecifierKind`, `ConditionSet`) is **content-free SHAPE** in
  `verter_workspace::module_resolution`. `SpecifierKind` is a per-specifier
  classification used by the U0 resolution-matrix walker — it is NOT an
  env-hash input. The matrix walker and the broken-input taint producers live
  in U0 `verter_session::resolver_core` (see
  `.claude/skills/type-resolution/SKILL.md` →
  `U0.RESOLVER_CORE_FOUNDATIONS`); this rule owns only the keying contract.

Guards (`crates/verter_workspace/src/env_hash_tests.rs`):
`module_resolution_keys_on_resolve_env_not_type_or_lib` (a module-resolution
input moves `resolve_env_hash` and leaves `type_env_hash`/`lib_env_hash`
untouched) and `resolve_env_does_not_fold_lib_dims` (a lib-only input moves
`lib_env_hash` and leaves `resolve_env_hash` untouched).

**R22.** Eviction is memory-bound, not correctness-bound. The reverse import
graph is content-addressed and serves reachability GC + LSP affected-files
reporting + diagnostics; it is never wired to cache EVICTION. Live-content
reachability + a global LRU floor are the sole GC mechanisms. The
root-owned resolution decision graph is a separate structure under the Hard
Rule 8 exception above: it advances derived resolution-currency facts and
drains nothing, and dependent caches still record and validate the derived
fact on read.

### Observability & cost

**R23.** Audit events emitted by this refactor on cache subsystem call paths use
typed `StructuredAuditEvent` variants (`FileArtifactCache`, `FactRegistryWrite`,
`FactValidationSummary`, `ExportRouteResolved`, `FactSignatureAdmissionRefused`,
`FactSignatureOverflow` (no producer; retained on the wire),
`ModuleAugmentationStitched`,
`ModuleAugmentationIndexShape`, `CacheDrainedAtUpsert`). `Custom` events for the
new emissions on these paths are forbidden.

The `CacheDrainedAtUpsert { layer, canonical_id }` variant fires at every
own-canonical drain site reached by the full `host.upsert(...)` path (the
upserted canonical's own caches — no reverse-dependent cascade). The
quintuple-unchanged fast path (R1) emits ZERO `CacheDrainedAtUpsert` events;
that absence is the direct read-side proof that the fast path is a cache-state
no-op. The variant is emitted via the in-tree helper
`verter_session::host_manage::push_cache_drained_at_upsert(layer, canonical_id)`
so all drain sites in `host_upsert.rs` flow through one construction point.
Layer-string vocabulary (each is a `&'static str`): `compile_cache_overrides`,
`compile_slots`, `derived_raw_cache`, `dependency_cache`, `resolver_runtime`,
`project_type_store`, `resolved_type_cache`, `semantic_invalidate`,
`workspace_parsed_edges`, `store_view_epoch`.

**R24.** Warm cache validation is counter-only — zero allocation, zero
structured payload emission per hit. Structured events emit only on cold /
stale / admit-refused paths or when an observer is explicitly sampling.
Warm-hit validation must stay under 50µs p99 and must not allocate (asserted
via test-allocator counter).

**R25.** The driver-scoped fact read tracer uses the selected `FactValidation`
clock authority and is active on cold-compute and write-admission paths only.
Its concrete reader preserves live clock sampling without per-frame port
dispatch. Warm hits validate stored signatures directly without instantiating a
tracer.

### Cache substrate

**`WorldSnapshot`** is the deterministic identity carrier for ONE in-flight
request. It is the lane identity that `cooperative_get_or_insert` coalesces on;
it is NOT a cache key. Cache layers project the snapshot down to scoped
dimensions via the `*_dims()` accessors (`parse_dims`, `resolve_dims`,
`type_dims`, `compile_dims`). Embedding the full snapshot as a single key field
on any cache layer violates R21 (the five env-hash dimensions must remain split
— bundling them into a single `project_config_hash` is forbidden).

Fields:

- `compat_token: StoreViewCompatToken` — singleflight lane identity (epoch +
  optional session); read through `FactValidation::compat_token(ctx)`.
- `project_identity, parse_env_hash, resolve_env_hash, type_env_hash,
  lib_env_hash: Hash16` — the five env-hash dimensions.
- `source_map_policy_hash, public_api_mode_hash: Hash16` — typed policy
  identities (typed `SourceMapPolicy` / `CompileCacheMode` enums introduced by a
  later block lower into these `Hash16`s through their `stable_hash()`
  conversions).
- `compiler_version, plugin_versions: Hash16` — host-side identity dimensions.
- `overlay_identity: Option<OverlayIdentity>` — base view is `None`, session
  overlay is `Some(OverlayIdentity(session_id))`.
- `generation: u64` — world generation under which the snapshot was constructed
  (stamped onto `CacheEntry.validated_at_generation` at admission).

`WorldSnapshot` lives at
`crates/verter_type_engine/src/cache_runtime/world_snapshot.rs`. The struct is
`pub(crate)`; it is exercised internally by `#[cfg(test)] mod tests` in the
owning module — there is no `for_tests` re-export and no parallel
`for_tests_from_raw` constructor on the production type. Construction-discriminator
tests drive `from_request` through the explicit compile-fenced direct-host test
context; production construction is request-bound by the sealed trait.

Architecture guard: `tests/cases/g_misc3/world_snapshot_is_not_a_cache_key.rs` parses every
production `.rs` file under `crates/verter_session/src/` with
`syn::parse_file`, walks every `ItemStruct` whose name ends `Key` or `Identity`,
and rejects any field whose `syn::Type` mentions `WorldSnapshot` at a
Rust-identifier path segment — REGARDLESS of the field's name. The AST walk
descends through wrapper constructors (`Arc<>`, `Option<>`, `Box<>`, `Rc<>`,
`RefCell<>`, references, raw pointers), tuple-struct positions, tuple-element
compounds (`(WorldSnapshot, u64)`), and multi-line wrapped field declarations —
none of these launder the rejection. A synthetic discriminator suite proves the
predicate fires across every shape and does NOT fire on prefix/suffix lookalikes
like `WorldSnapshotShim`.

### Substrate, stack-safety, granularity

**R26.** `ValidatedFactCache<K, V>` is the substrate. Multi-candidate (R20), the
extended `FactVersionRef` variants, and the new lane / space metadata land
**inside** this primitive — not as a parallel cache type. No second
fact-validation infrastructure may be introduced.

`StoreView` trait surface evolves with explicit per-domain validator methods
routed via `FactKey::domain()`:

```rust
trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;
    fn validates(&self, fact: &FactVersionRef) -> bool;
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool;
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool;
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool;
    fn validates_program_analysis_domain(&self, fact: &ProgramAnalysisFactRef) -> bool;
}
```

The dispatch table is bounded by `FactDomain` (4 variants), not by `FactKey`.
Adding a new `FactKey` extends the per-domain `*FactRef` enum but does NOT
widen the trait. The fourth variant, `ProgramAnalysis`, is the whole-function
`FlowBody` rail: candidates root on
`ProgramAnalysisFactRef::FlowBody { function, flow_body_stable_hash }` and
warm validation compares the indexed whole-body hash — it never re-lowers or
reruns reachability.

The resolve-imports domain is a CLOSED two-arm enum under ONE validator, not
two rails: `ResolveImportsFactRef::Semantic { .. }` (the session's
`ResolvedImportFactsDb` bindings) and `ResolveImportsFactRef::Resolution(..)`
(workspace resolution-currency observations, validated against the store
view's captured `CapturedResolutionWorld` — see `/host-session`). Both arms
land in the same `FactVersionRef::ResolveImports` variant, the same
`ReadSetSignature`, the same `SignatureAdmission` refusal convention, and the
same `ValidatedFactCache` admission path.

**R27.** All semantic fingerprint computation is **stack-safe**: implemented as
an explicit worklist with a `VisitedSet`. Cycles emit a stable
`CycleRef(visit_index)` placeholder. Visit order is canonical: lexicographic by
`(name, symbol_space)` at each unresolved-neighbor expansion; depth-budget
tie-break by `(canonical, name, symbol_space)`. `CycleRef` placeholder identity
is invariant under source-text reordering. Depth budget = 64; over-budget paths
emit `Opaque(BudgetExceeded)` and the cache entry is admitted as `NonCacheable`.

**R28.** Path-precise fact granularity is mandatory. Every cache that observes a
member, local, import edge, or member-presence must observe the corresponding
`Member` / `LocalDecl` / `MemberPresence` / `ImportRef` fact directly. Folding a
whole-file or whole-export closure into a single `semantic_hash` is forbidden.

**`MemberPresence` vs `Member` two-fact model** — orthogonal purposes and
orthogonal cache lifecycles. Consumers that select a specific member observe
BOTH:

| Fact | Purpose | Lifecycle | `semantic_hash` content |
|---|---|---|---|
| `MemberPresence(exporter, name, space)` | Existence + header | Phase 1 eager (parse-time) | `(name, member_kind, exporter_qualifier_salt)` |
| `Member(exporter, name, space)` | Canonical reusable member body fingerprint | Phase 2 lazy | Body alpha-normalised TypeExpr with cross-decl references kept as references |
| `MemberShape(exporter, space)` | Whole-surface fingerprint | Phase 1 eager | `sorted_by_name([(name, kind)])` |

Why both `MemberPresence` and `Member` exist: their cache lifecycles differ.
`MemberPresence` is cheap and eager — its body fingerprint is NOT included, so
adding member `b` does not force re-walking member `a`'s body. `Member` is the
canonical reusable navigation result — computed once per `(parse_stable_hash,
exporter, name, space)` and reused for every consumer that walks into the
member body.

Literal-key projections like `Pick<Foo, "a">` observe **`MemberPresence(Foo, "a")` + `Member(Foo, "a")`**:

- Adding `Foo.b` → new `MemberPresence(Foo, "b")` emitted; existing
  `MemberPresence(Foo, "a")` unchanged; `Member(Foo, "a")` unchanged → consumer
  NOT invalidated. `MemberShape` changes but is not in this consumer's
  signature.
- Editing `Foo.a` body → `MemberPresence(Foo, "a")` unchanged (header
  invariant); `Member(Foo, "a")` changes → consumer IS invalidated.
- Removing `Foo.a` → `MemberPresence(Foo, "a")` becomes a registry miss →
  consumer IS invalidated.

**R29 (Module augmentation).** Module augmentation is a fact-graph completeness
requirement. Parse-domain emits a syntactic fact `ModuleAugmentation {
specifier, owner, augmented_name, space }` per augmented binding. `owner` is
the exact neutral lexical [`TopLevelOwnerId`](../../crates/verter_type_expr/src/facts.rs)
of the authored `declare module 'x' { ... }` / `declare global { ... }`
statement. Same-name declarations in module and instance script regions are
distinct facts and distinct prepared contributors; the augmentation target
scope itself remains unpartitioned by lexical owner.
Resolution of the augmentation target happens at the resolver stage, producing a
typed `AugmentationTargetKey`:

```rust
enum AugmentationTargetKind {
    ExternalSpecifier(InternedSpecifier),     // declare module "vue" {}
    ResolvedRelativeCanonical(Arc<str>),      // declare module "./local" {}
    WildcardAmbient(InternedGlobPattern),     // declare module "*.css" {}
    GlobalAugmentation,                       // declare global {}
}

struct AugmentationTargetKey {
    project_identity: ProjectId,
    resolve_env_hash: Hash16,
    lib_env_hash: Hash16,
    population: AugmentationPopulation, // {Base, Session(overlay-set fingerprint)}
    target: AugmentationTargetKind,
}
```

A `ModuleAugmentationIndex { entries: DashMap<AugmentationTargetKey,
Arc<AugmenterSet>> }` lives on `FileArtifactStore`, providing the inverse lookup
"which augmenters target X under env E and population P?". Project / env
isolation prevents cross-project poisoning, and the `population` dimension keeps
a session overlay's augmenters in a `Session` slot distinct from the `Base` slot
(see the **Overlay-aware contract** below).

**Index population semantics.** The augmentation index is populated
**incrementally** as files enter `FileArtifactStore`. There is NO workspace-wide
eager scan; out-of-program files contribute nothing — matching TypeScript's own
augmentation visibility rule. A reachability-anchored query that needs the
augmenters for a target triggers `ResolvedImportFacts` resolution for the
relevant import graph, which pulls in any augmenter that is part of that
reachability.

**Augmenter-set identity fact.** A parse-domain-derived fact
`ModuleAugmentationIndexShape { target: AugmentationTargetKey,
augmenter_set_fingerprint }` — the SOLE `RouteSurface` fact; that domain is
one-arm — is observed by the augmentation-stitch consumers.
`augmenter_set_fingerprint = stable_hash(sorted([(augmenter_canonical,
augmenter_parse_stable_hash)]))`. Adding / removing an augmenter changes the
fingerprint → every stitched candidate that observed it invalidates.

**Stitching pipeline.** `ProjectSemanticDispatch::stitch_module_augmentations`
(through the shared `collect_augmentation_contributions` folder) is the cold
path. It calls
`FileArtifactStore::ensure_augmentation_index_populated(target_key)` on first
miss — the scan walks every loaded `FileArtifacts.augmentations`, sorts matched
augmenters by `(canonical, parse_stable_hash)`, computes the fingerprint,
inserts into `augmentation_index`, and emits a typed
`ModuleAugmentationIndexShape` audit event recording the install (or refresh on
re-population). Each `AugmenterSet` entry is an `AugmenterEntry` carrying the
**exact** `FileArtifactKey` of the scanned augmenter artifact (plus its
`parse_stable_hash`) — the stitcher re-fetches each augmenter's `.augmentations`
via `FileArtifactStore::get_artifacts(&key)` keyed by that exact key, never a
content-agnostic canonical-only scan, so the stitch reads precisely the
augmenter version the fingerprint was computed over. The stitcher then folds
each augmenter's `(owner, augmented_name, space)` contributions, together with
the base declaration's own contributors, into ONE `SemanticNodeData::MergedDecl`
carrier. The result's `ReadSetSignature.facts` records the
`RouteSurface(ModuleAugmentationIndexShape)` fact plus per-contributor
`FileWholeHash` anchors (with `self_root_canonicals = {base} ∪ {augmenters}`)
so:

- adding / removing an augmenter changes the augmenter-set fingerprint →
  consumer invalidates (G1);
- editing one augmenter's body changes that file's whole hash → consumer
  invalidates;
- editing an unrelated file (not in the augmenter set) leaves the signature
  valid → consumer warm-hits (R14 / R28 narrow scope).

**Program order fact.** Declarations of one symbol merged across files — and
which loaded library declares a global — follow the program's order
(`verter_session::program_order`; see `/type-resolution` →
`references/global-names.md`). A result whose answer depended on it observes
`RouteSurface(ProgramFileOrder { program })`, the fingerprint of the whole
sequence (libraries by priority, then program files). The per-file
`DeclContributionOrder` / `AugmentationContributionOrder` facts cannot carry
it: an import or `/// <reference lib>` edit in a file that contributes
nothing reorders the program. The published order records every input it read
(each visited file with the files it pulls in, every probed path, the
candidate file set, the registry, the configuration and resolution
generations); the validator re-stamps it while none moved, so an edit
elsewhere keeps warm results, and a rebuild re-reads only the changed files.
An order a refused resolution or an unpublished serve produced is never
published.
The same compositions supply imported/reference-only members' project context.
`ProgramMembershipRoot` captures their immutable map and snapshot with fixed-cost Arc clones beside
source/project roots; root-relative env validation uses that captured membership. Imported singleton
contributors also observe this dependency on every lookup, including project-view
cache hits, so removing their incoming edge retires a warm answer.
Program discovery runs before contributor snapshots and after the workspace's
batched source publication, so it cannot re-index stale pre-commit bytes.
Configuration fences use `WorkspaceAuthority` and captured snapshot identity:
reconstructing a project graph can repeat its scalar snapshot generation.

A typed `ModuleAugmentationStitched` audit event records each cold-path compute.
Both audit-event variants live on `verter_audit::StructuredAuditEvent`; the
`Custom` escape hatch is forbidden on the augmentation stitching surface (R23
scope-fence guarded by `tests/cases/g_audit/audit_event_shape.rs`).

**Overlay-aware contract (R6 key split).** Augmentation stitching is
OVERLAY-AWARE across two layers with deliberately DIFFERENT key discipline:

- The **content-addressed** augmentation index (`AugmentationTargetKey` on
  `FileArtifactStore`) carries `population: AugmentationPopulation {Base,
  Session(overlay-set fingerprint)}`. A `Base` scan filters to base
  (`FileArtifactKey::is_legacy`) artifacts; a `Session` scan unions the
  session's own overlay augmenters (matched by the session overlay discriminator
  derived from `SessionView::fingerprint`) with base. The fingerprint
  legitimately keys this slot because the index is a content-addressed compute
  cache — the fingerprint IS part of its content view identity, and it
  self-invalidates when overlay content/membership changes (new fingerprint →
  fresh scan). A `Session` slot is overlay ∪ base, distinct from `Base` without
  poisoning it; a base augmenter change invalidates the `Session` entries that
  include it.
- The **query-identity** consumers of that index — the `SemanticGraphStore` memo
  entry the stitch produces, and `ComponentMetaResultDb` downstream of it — keep
  CONTENT-FREE keys: the overlay-set content fingerprint NEVER enters a
  query-identity key (R6). Overlay CONTENT identity is rooted on the VALUE's
  `ReadSetSignature.facts` (the `ModuleAugmentationIndexShape` augmenter-set
  fingerprint fact + per-contributor `FileWholeHash` anchors), revalidated
  against the live view on every warm hit — so a within-session overlay edit
  invalidates through facts, not through a new key. The base/session split is
  therefore enforced at VALIDATION, not in the key: the route-surface validator
  looks the fingerprint up under the CALLER VIEW's own population, so a session
  consumer validates against the `Session(overlay-set fingerprint)` set and a
  base consumer against `Base` — a base warm entry can never satisfy a session
  lookup (the "base-as-session" hazard) and vice-versa.

The producer (the body stitch in `project_semantic_dispatch::build`) and the
route-surface validator (`HostStoreView::validates_route_surface_domain`) both
derive the content-addressed `(population, overlay_discriminator)` through the
single shared `session_view::augmentation_population_for_view`, so a session view
can never be validated against a base-only set. Overlay augmenters NEVER poison
the base index and NEVER cross sessions. The contract is locked by
`tests/cases/g_misc3/module_augmentation_stitching.rs` —
`session_overlay_augmenter_isolated_from_base_index` plus the base-augmenter
invalidation cases
(`base_augmenter_publish_invalidates_session_augmentation_index`,
`base_augmenter_remove_invalidates_index_via_evict`) — and by
`tests/cases/g_session/session_overlay_augmentation_isolation.rs::session_overlay_augmentation_isolated_from_base_meta`.
There is NO base-only `compat_token().session.is_none()` assert on this surface:
a session view is accepted under `Session` population.

**R30 (No heuristic cache semantics).** Cache identity and cache admission must
not encode semantic policy through local heuristics. Every dimension that can
change the returned type, published members, alias/provenance shape, exactness,
or completeness must be represented as one of:

- a typed cache-slot key dimension;
- a per-mode/per-policy entry inside a shared cache family;
- cached-value validation metadata such as a fact signature, self-root set, or
  store-view constraint;
- explicit result state such as `Exact`, `SurfaceOnly`, `Unsupported`,
  `BudgetExceeded`, or a recursion sentinel.

Rendered type text, raw source slices, path substrings, identifier spelling
conventions, arbitrary numeric caps, and candidate "better shape" scoring are
not cache correctness mechanisms. A fast path or gate may decide that work
should stay cold, avoid admission, or enter the shared semantic query layer; it
must not return or admit a different semantic result than the typed query
contract would produce. If a cached result depends on projection mode,
substitution environment, conditional context, package/workspace policy, scope
version, source generation, solver options, or component-meta schema options,
that dependency must be visible in the cache family shape or the cached value's
validation/provenance. Otherwise the result is non-cacheable.

**R31 (Exact policy identity and complete-result admission).** Query policy
dimensions are typed identities, not compressed flags. Projection mode,
traversal mode, package-boundary policy, output schema/options, substitution
environment, conditional context, and solver profile must be represented
exactly in either the cache slot, the per-policy entry, or the cached value's
validation/provenance. A boolean such as `is_navigate`, a mode-erased string,
or an enum subset is not a valid key when two concrete policies can diverge now
or after a new mode is added.

Cache admission also depends on result completeness. Entries may be admitted to
warm shared caches only when the value is complete for the policy it claims to
satisfy and its read-set/fact signature is known. `Partial`, `Unavailable`,
`Unsupported`, `BudgetExceeded`, `UnstableState`, recursion-sentinel,
bridge-truncated, and cancelled results may be returned to a caller, but they
must be marked non-cacheable or cached only in a typed degraded-result cache
whose callers cannot mistake them for exact answers.

**Vue macro compiler DTOs are request-local projections, not cache entries.**
`produce_vue_macro_codegen` may fulfill runtime and TSC demand together in one
SFC inventory pass, but it must not admit the aggregate
`MacroRuntimeBundle`/`MacroTscBundle` under a graph-instance id, raw source
slice, or whole-file semantic hash. The producer reuses the canonical
`SemanticGraphStore` query memos underneath it, returns the request's typed
completeness and fact footprint, and hands the DTOs directly to the compiler.
Only a future cache family with a complete typed policy identity and normal
read-side fact validation could retain such an aggregate.

**The producer's fact footprint crosses a thread boundary and must be replayed.**
`produce_vue_macro_codegen_with_ctx` runs its build through
`Scheduler::execute_scoped_cache_node`, which dispatches the closure onto a
scheduler CPU-pool worker; a caller that joins an in-flight rendezvous returns
from the flight terminal having run no closure at all. The fact tracer stack is
thread-local with no cross-thread bridge, so the tracer the producer installs is
invisible to the enclosing `Session` compile's tracer on the submitting thread.
`VueMacroCodegenOutput.fact_footprint` (`MacroFactFootprint`) therefore carries
the producer's finalised observation set AND its non-cacheable refusal back, and
the call site replays both on the CONSUMING thread — for the leader and the
follower arm alike. Without that replay the compile slot's `ReadSetSignature`
omits every fact rooted at a TRANSITIVELY reached file, and an edit to such a
file leaves the warm slot validating (the bundler emits stale runtime prop
validators). The factless arms are the trap: `FactReadSetFinalise::MutationUnstable`
carries NO facts, so replaying it as an empty observation set reproduces that
stale serve silently. All THREE factless cases must route to
`note_non_cacheable_propagation`, never to an empty replay:
`MutationUnstable`, `Unobserved` (a cancelled / shut-down / faulted /
re-entrant handoff), and a `RootedNonCacheable` whose fact set is empty — a
refusal raised before anything was observed. Warm inner memos are
already handled — the memo warm-hit paths bubble their recorded signatures
(`cache_runtime/node.rs`, `semantic_query_memo`), so a producer served entirely
from warm memos still seals the full transitive footprint. That sufficiency is
INHERITED, not owned here: it rests on the pre-existing R3/R26/R28 bubble
discipline holding at every warm-read layer, delivery is redundant across those
layers, and nothing in the replay carrier guards it — a future layer that stops
bubbling reopens the stale serve without failing any test in this file. Any
future producer that hands a value across `execute_scoped_cache_node` inherits
the replay obligation.

### Exact-owner version audit (2026-07-19)

The neutral lexical-owner cutover and later analyzer-carrier additions change
cached parse facts, declaration preparation, route results, semantic-query
identity, and carrier script facts. The coordinated invalidation boundaries
are:

- `CACHE_CLUSTER_SCHEMA_VERSION = 8` rejects the schema-7 cache cohort whose
  shallow import targets may retain a resolved canonical beside the authored
  specifier. Schema 7 added call-signature emit declaration spans; schema 5
  was the CREO migration; schema 4 was the exact-owner cutover.
- The session-owned `BuildToolchainFingerprint` rejects ordinary/base artifacts,
  carrier-script candidates, and flow slices whose private in-memory shape may
  predate authored-only import targets. It is not a syntax compatibility epoch.
- `ROUTE_DB_RESOLVER_VERSION = 2` rejects route values that do not carry the
  defining declaration owner.
- `SvelteScriptProvider::VERSION = 12` independently rejects Svelte candidate
  payloads without the persisted exact module-export inventory, export owners,
  owner-qualified binding keys, or the per-call `$props()` public-key/local-
  binding span inventory; it also rejects payloads predating exact-empty
  evidence, the syntax/resolution channel split, statement and inline
  `import("svelte").Snippet` identities, or resolved callable-role decisions.

Exact-owner shallow facts also govern qualified namespace roots. For
`import * as Ns from './dep'`, the cached binding for `(owner, Ns)` is a MODULE
handle (`is_namespace`), not a request to find an export literally named
`Ns`. Resolution first proves that exact-owner namespace fact (ambiguous
bindings fail closed), then sends only the qualified MEMBER through the shared
type/value export resolver. Re-export aliases therefore publish the terminal
declaration canonical/name rather than the namespace module or barrel
identity; there is no prepared-declaration or file-wide name-rematch fallback.

The other named versions are intentionally unchanged:

- `RESOLVED_IMPORT_FACTS_RESOLVER_VERSION = 1`: this cache serializes
  per-specifier resolution and resolved binding names/canonicals; its key and
  value schema do not embed `DeclKey`, prepared scopes, augmentation
  contributors, or route results.
- `TYPEINFO_GRAPH_SCHEMA_VERSION = 4`: the TypeInfo wire schema does not carry
  `VueMacroCodegenOutput`, `MacroRuntimeBundle`, or `MacroTscBundle`.
  `produce_vue_macro_codegen_with_ctx` constructs and returns those aggregates
  request-locally and performs no aggregate cache admission; only its
  underlying canonical semantic queries are memoized.
- `ORACLE_SCHEMA_VERSION = 3`: oracle snapshots serialize hover/probe capture
  shapes and migration fingerprints, not parser facts, lexical-owner
  declaration identities, prepared scopes, route results, or Vue macro
  compiler DTOs.

## Cache layer key composition

See `.claude/skills/type-cache-architecture/SKILL.md` for the canonical per-cache-layer key
composition table. Summary:

| Layer | Family | Key dimensions |
|---|---|---|
| `FileArtifactStore` | Content-addressed | `canonical, content_hash, parse_env_hash, parse_key, build_toolchain_fingerprint, file_language_id` — `parse_key` canonically identifies exact bytes, language, syntax profile, and compatibility domain/epoch; the single session-owned fingerprint invalidates private DTO shape without pretending syntax changed. `IndexedReady` retains the scheduler/runtime-authoritative `FileLanguage` used to parse those bytes, and every exact writer keys from that retained row. The store retains its existing per-store lanes. |
| `ModuleAugmentationIndex` (on `FileArtifactStore`) | Content-addressed | `project_identity, resolve_env_hash, lib_env_hash, population, target` (`population: AugmentationPopulation {Base, Session(overlay-set fingerprint)}`) |
| `ResolvedImportFacts` | Content-addressed | `canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version` (**no `lib_env_hash`** — R21) |
| Typed-IR resolve | Content-addressed | `canonical, content_hash, parse_env_hash, type_env_hash, lib_env_hash, build_toolchain_fingerprint` |
| `MemberSemanticFactStore` | Content-addressed | `canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space` |
| `MemberDisplayFactStore` | Content-addressed | `canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space` |
| `BinderIdentityFactsStore` (family A, the binder-identity substrate) | Content-addressed | `canonical, parse_stable_hash, parse_env_hash` — the pre-reducer binder-identity substrate: per-file lexical scope tree with stable structural `BinderScopeId`s, env-free `DeclarationSlotSeed`s (exactly the four env-free fields of `ResolvedDeclSlotIdentity`; the env-bearing slot is derived ONLY at query-key construction via the `ProjectSemanticDispatch::finalize_slot_seed` choke point), and declaration-order / overload-group / augmentation-contribution provenance. Demand-produced from `IndexedReady` (no eager pass), value-side `ReadSetSignature` over eager parse-lane facts; negative name lookup stays `ReturnOnly` (no corpus-completeness store in this substrate). |
| `RouteDb` per-name | Query-identity (multi-candidate) | `RouteNameKey { provider_canonical, exported_name, symbol_space, project_identity, resolve_env_hash, lib_env_hash, resolver_version }` (content-free; routes are resolve-domain so no `parse_env`/`type_env`, but `lib_env` enters — augmentations stitch the surface). Value-side `ValidatedFactCache` fact validation. |
| `RouteDb` effective barrel surface | Query-identity (multi-candidate) | `BarrelSurfaceKey { barrel_canonical, project_identity, resolve_env_hash, lib_env_hash, resolver_version }`. Value-side `ValidatedFactCache` over `BarrelRouteSurface.fact_dep_signature`. |
| `ClassifyMaterializationCycleGate` (materialization cycle gate — the retired `RefCycleResultDb`'s replacement) | Semantic-query family over the `SemanticGraphStore` memo | `MaterializationCycleGateKey { root: ResolvedDeclSlotIdentity, parse_env_hash, resolve_env_hash }` — content-free (fixed empty-args / `StructuralTransit` / `Skeleton` axes; no generation, no `DeclIdentity`, no algorithm version). Only `Decided` admits; value-side observed self-roots (walk root + every visited decl's file) + live-generation gate. |
| `SemanticGraphStore` query nodes | Query-identity (multi-candidate) | `SemanticQueryKey` slot identity (e.g. `Instantiate { base: ResolvedDeclSlotIdentity, args }`); the memo value version-roots on `ReadSetSignature.facts` + `self_root_canonicals`. |
| `AwaitedNormalize` / `AsyncReturnPayload` (the two awaited relations) | Semantic-query families over the `SemanticGraphStore` memo | `{ operand: SemanticNodeId, context: StructuralReduceContext { resolve_env_hash, type_env_hash, lib_env_hash, project_identity } }` — no decl slot and no content hash; the already-lowered operand carries the content, and the value roots on the operand's observed self-roots. Composite arms re-enter their OWN family, so every unwrap level is its own memo entry; builds are counted by `awaited_normalize_count` / `async_return_payload_count`, never `instantiate_count`. An honest refusal is `Error(Miss)` and never admits. |
| `SignaturesOfType` / `ReadSignatureResult` (shared signature discovery and demand-driven results) | Semantic-query families over the `SemanticGraphStore` memo | `SignaturesOfType { subject: SemanticNodeId, kind: SignatureKind, context: SemanticContextId }` and the sealed `ReadSignatureResultKey { descriptor, call_substitution, projection: ResultDemand, evaluation: ResultEvaluationContextId, semantic_context }` — the interned `SemanticContextId` owns options / environment / policy, so no content or version hash rides the key and a changed option is a different family. Values are epoch-qualified handles into the graph store's `SignatureStore` (`SignatureSet` / `SignatureResult`); the value roots on the subject's / return node's observed self-roots, and an incomplete outcome (unsettled subject, unresolved body obligation, budget, cancellation) never admits. Enumeration never forces a body; `SignatureStore::bodies_forced` counts forced body recipes. |
| `ShapeCacheDb` per-member slot | Generation/store-scoped graph-instance memo (NOT a durable content-free R6 query-identity key) | `ShapeCacheKey::surface_member_value_whole_with_context(scope, &SurfaceMember, context)` (`ShapeSubject::MemberValueNode`). Single-entry (not multi-candidate), not persistent, fact-validated + generation-gated. Warm reuse requires `validated_at_generation` + strict `ReadSetSignature` self-root validation over the exact `SurfaceMember.value` graph instance. |
| `ComponentMetaResultDb` | Query-identity (multi-candidate) | `ComponentMetaResultKey { owner_canonical, options_fingerprint, project_identity, parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash }` — content-free (owner whole-hash is the VALUE-side candidate discriminant, never a key field). Value-side owner whole-hash candidate + `ReadSetSignature.facts` + `validated_at_generation`. |

Every production `FileArtifactStore` read that can return a content candidate
must carry the authoritative `ParseKey` and `FileLanguage`. A content hash is
not an exact read identity: equal bytes parsed with distinct syntax profiles
coexist, and a convenience read must not select whichever sibling was inserted
first. Base reads recover the accepted source-stage identity. Overlay reads
retain the scheduler/runtime language for an existing canonical; only a
genuinely overlay-only canonical uses the shared classifier fallback. The
post-parse `IndexedReady.file_language` row and its actual `raw_source` bytes
are the writer authority, so `FileArtifactKey::for_indexed` is byte-identical
to `for_source_identity` for the same canonical bytes, parse environment, and
toolchain fingerprint. A path extension never overrides an explicit runtime
language.

CREO event rows are cached only as values derived from the stored canonical
resolved surface stream. Resolved callable identities carry the exact
resolver-minted graph subject; runtime identities carry the exact authored
canonical/range plus semantic event subject. Neither is a durable
query-identity key. Warm component-meta results preserve the same ordered
complete occurrence rows and never rebuild identity from names, kind-local
positions, ordinals, or analyzer output.

The split `MaterializeMemoDb`/`MemberShapeCacheDb` shape stores are RETIRED in
favour of `ShapeCacheDb`; the static guard
`crates/verter_session/tests/cases/g_block/block_6i_static_guards.rs::shape_cache_db_replaces_split_caches`
asserts neither may be re-introduced.

The `ShapeCacheDb` per-member subject is the sealed `MemberShapeNodeSubject(SemanticNodeId)`
newtype keying the `ShapeSubject::MemberValueNode` arm — a generation/store-scoped
graph-instance memo, NOT a durable content-free query-identity key (it carries a
`SemanticNodeId`, which is precisely why it is sealed behind the newtype rather than
exposed as a content-free key). Shape cache-key hygiene guards (all
RECORDED SOURCE SCANNERS — structural confinement is PRIMARY, the scanners are the
bounded residual supplement). Only the TWO `SemanticNodeId`-keyed scanners
(`no_unsanctioned_semantic_node_id_in_shape_key` +
`synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`) become structural
and are DELETED once the key-safety newtype substrate lands (newtype the env/content
hashes + seal the `SemanticNodeId`/`value_node` tuple fields), see
`.claude/skills/type-cache-architecture/SKILL.md`. The third scanner
`no_carrier_verdict_db` (retired-symbol absence) is NOT closed by that substrate —
newtyping hashes / sealing `SemanticNodeId` does not prevent reintroducing a private
`CarrierVerdictDb` / `carrier_verdicts` symbol — so it REMAINS a recorded scanner (or
would need its own separate structural closure for retired-symbol absence).
The three guards:
`no_unsanctioned_semantic_node_id_in_shape_key`
(`crates/verter_session/tests/cases/g_cache/r6_r21_query_identity_keys.rs` — forbids the
content/version field markers on the shape derived-`Hash` keys and pins the
`SemanticNodeId` allow-list to exactly one position, the sealed `MemberShapeNodeSubject` newtype); `no_carrier_verdict_db`
(`crates/verter_session/tests/cases/g_misc2/no_carrier_verdict_db.rs` — retired-symbol
absence); `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
(`crates/verter_session/tests/cases/g_misc2/synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs`
— bans the direct `SemanticNodeId(<ident>.value_node)` ordinal-key shape; the regular
`ShapeSubject::MemberValueNode` route reads `member.value` through the sealed
`MemberShapeNodeSubject` newtype and is structurally unmatched).

## Two-phase emission map (R28)

Parse-time emission (eager, shallow, O(file_size)) populates the parse-domain
`FactRegistry` on `FileArtifacts.facts`. The producer is
`verter_session::fact_emission::emit_parse_facts(&IndexedReady)`, which emits:

- `Export(name, space)` — per locally-declared exported binding.
- `LocalDecl(name, space)` — per NOT-exported local.
- `MemberShape(exporter, space)` — whole-surface fingerprint.
- `MemberPresence(exporter, name, space)` — header-only `(name, kind,
  exporter_salt)`; NO body fingerprint.
- `SyntacticExportSet` — whole-file surface fingerprint.
- `SyntacticRouteInterface` — exact authored import/export routing interface;
  excludes bodies, spans, resolved targets, whole-content hashes, and project
  state.
- `ImportRef(specifier, binding, space)` — syntactic import shape; NO resolved
  canonical (R12).
- `SyntacticReexportRef(specifier, source_name, target_name, space)`
- `ExportAlias(exported_as, space)` — per `export {X as Y}`.
- `ModuleAugmentation(specifier, owner, augmented_name, space)` — per augmented binding
  inside each `declare module "X" {…}` / `declare global {…}` block; `owner`
  is the exact lexical authoring region. `declare global` blocks key on the
  `$global` sentinel specifier.
- `AugmentationContributionSet(scope_kind_tag, specifier, owner)` — SET-shape
  fingerprint of one augmentation target's contribution set in one file (growth
  or a contribution shape edit moves it). Emitted for EVERY augmentation block,
  EMPTY ones included, so an empty → first-contribution edit moves it.
  `scope_kind_tag` (Global vs Module) keeps `declare global {…}` and
  `declare module "$global" {…}` in distinct target identities.
- `AugmentationContributionOrder(scope_kind_tag, specifier, owner)` —
  ORDER-sensitive fingerprint of one augmentation target's authored contribution
  sequence (ordered by each declaration's own source position; positions never
  enter the hash). Also emitted for empty blocks.
- `DeclContributionOrder(name, owner, space)` — ORDER-sensitive fingerprint of one
  file-surface declaration slot's authored contributor sequence (an overload /
  declaration swap moves it; comments between declarations do not). The
  per-contributor shape hashes the comment-stripped, whitespace-dropped
  declaration slice, so intra-declaration trivia edits stay cosmetic.
- `AugmentationTargetSet` — whole-file augmentation target set fingerprint
  (`(scope_kind_tag, specifier, owner)` blocks, EMPTY blocks included; a first
  `declare module "m" {…}` moves it with an unchanged skeleton, and the tag
  keeps `declare global` / `declare module "$global"` distinct).
- `NamespaceScopeSet` — whole-file `namespace N {…}` block set fingerprint
  (EMPTY blocks included).

Lazy emission (member body, on first member-access query) lives in TWO separate
stores keyed differently to physically separate semantic vs display:

| Store | Key | Keys-on |
|---|---|---|
| `MemberSemanticFactStore` | `(canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space)` | `parse_stable_hash` — cosmetic edits do NOT re-key |
| `MemberDisplayFactStore` | `(canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space)` | `content_hash` — cosmetic edits DO re-key |

Both stores admit through `entry().or_insert(...)`: insert-only-if-absent.
Producer races for the same key collapse to one canonical fact; downstream
consumers observe pointer-equal `Arc<Fact>` for the same key.

## R27 worklist algorithm

`verter_semantic::facts::hashing::compute_semantic_hash(body, space, lens) ->
HashOutcome` is the stack-safe + cycle-safe + path-precise fingerprinter.
Contract:

- **Stack-safe**: explicit `depth` counter, hard cap `MAX_HASH_DEPTH = 64`.
  Over-budget walks set `HashOutcome.budget_exceeded = true` and emit
  `BUDGET_EXCEEDED` placeholder bytes. Producers MUST admit the cache entry as
  `NonCacheable` (the admission guard lives downstream).
- **Cycle-safe**: per-node identity-key `VisitedSet` (`BTreeMap`). Re-entry
  through a node emits `CycleRef(visit_index)` placeholder rather than recursing.
  Identity key folds in `Arc` pointer addresses of owned sub-nodes — two
  physically identical `Arc`s ARE the same node and re-enter as `CycleRef`.
  Different `Arc`s carrying structurally identical content are distinct nodes.
- **Canonical visit order**: lexicographic by `(name, symbol_space)` at each
  unresolved-neighbor expansion; tie-break by `(canonical, name, symbol_space)`.
  `CycleRef` placeholder identity is therefore invariant under source-text
  reordering.
- **Path-precise**: cross-decl references resolve through `CrossDeclLens` and
  emit reference-shape edges (`LocalDecl(name, space)`, `ImportRef(spec, binding,
  space)`, `TypeOfRef(name)`, `Unresolved(name, space)`) WITHOUT inlining the
  referent's body (R14). Free type parameters alpha-normalise to binder-relative
  indices.

## MemberPresence vs Member detailed table (R28)

| Consumer touch | Observes |
|---|---|
| `Pick<Foo, "a">` | `MemberPresence(Foo, "a")` + `Member(Foo, "a")` |
| `Omit<Foo, "a">` | `MemberShape(Foo)` (whole surface) |
| `keyof Foo` | `MemberShape(Foo)` |
| `Foo["a"]` | `MemberPresence(Foo, "a")` + `Member(Foo, "a")` |
| `Foo["a"]["b"]` | both `Member` chains observed |
| `{ [K in keyof Foo]: T }` | `MemberShape(Foo)` |

The discrimination matrix:

- Adding `Foo.b` → new `MemberPresence(Foo, "b")` emitted; existing
  `MemberPresence(Foo, "a")` unchanged; `Member(Foo, "a")` unchanged.
  `MemberShape` changes but `Pick<Foo, "a">` consumer does NOT observe
  `MemberShape` → NOT invalidated.
- Editing `Foo.a` body → `MemberPresence(Foo, "a")` unchanged (header invariant);
  `Member(Foo, "a")` changes → consumer IS invalidated.
- Removing `Foo.a` → `MemberPresence(Foo, "a")` becomes a registry miss →
  consumer IS invalidated.

## Key concrete files

- `crates/verter_workspace/src/env_hash.rs` — five env-hash functions on
  `IdeProjectConfig` + `EnvHashInputs<'_>`.
- `crates/verter_session_query/src/facts/registry.rs` — the fact-schema owner:
  `FactKey`, `Fact`, `FactDomain`, `FactRegistry`, `SymbolSpace`, `MemberKind`,
  `FactLane`, `ObservedFact`, `MacroKind`, `MacroTargetKey`, `InternedSpecifier`,
  `InternedName`, `InternedGlobPattern`, `AugmentationTargetKindTag`, plus the
  semantic `MacroKind` conversion. `verter_workspace` consumes it; there is no
  workspace-side registry module.
- `crates/verter_session_query/src/facts/hashing.rs` — `compute_semantic_hash`,
  `compute_member_presence_hash`, `compute_member_shape_hash`, `CrossDeclLens`,
  `CrossDeclRef`, `HashOutcome`, `MAX_HASH_DEPTH = 64`.
- `crates/verter_session/src/file_artifact_store.rs` — `FileArtifactStore`,
  `FileArtifactKey`, `FileArtifacts`, `FileFacts` (registry-backed); re-exports
  `Interned{Specifier,Name,GlobPattern}` + `SymbolSpace` from
  `verter_semantic::facts::registry`; `AugmentationTargetKey`,
  `AugmentationTargetKind`, `ParsedEdges`, `ModuleAugmentationFact`,
  `ProjectIdentity`. The owned augmenter-set answer (`AugmenterSet`,
  `AugmenterEntry`) lives in `verter_session_query::resolution` beside
  `AugmentationTargetKey`.
- `crates/verter_session/src/fact_emission.rs` —
  `emit_parse_facts(&IndexedReady) -> ParseFactsEmission`,
  `GLOBAL_AUGMENTATION_TAG`, and typed augmentation-header fact emission.
- `crates/verter_session/src/member_semantic_fact_store.rs` —
  `MemberSemanticFactStore`, `MemberSemanticFactKey`, `make_member_fact`,
  `member_fact_key`.
- `crates/verter_session/src/member_display_fact_store.rs` —
  `MemberDisplayFactStore`, `MemberDisplayFactKey`, `make_member_display_fact`,
  `member_display_fact_key`.
- `crates/verter_session/src/parse_stable_hash.rs` —
  `compute_parse_stable_hash(&IndexedReady) -> Hash16`. `parse_stable_hash` is
  a structural hash over the post-shallow-analysis decl skeleton, invariant
  under cosmetic edits.
- `crates/verter_session_query/src/facts/version.rs` — the immutable fact-version
  value graph: `FactVersionRef`, `ParseFactRef`, `ResolveImportsFactRef`,
  `RouteSurfaceFactRef`, `ProgramAnalysisFactRef`, their hashes/stamps,
  populations, attribution, and compaction-domain values.
- `crates/verter_session_query/src/facts/resolution.rs` — the immutable resolution
  identity/value graph: canonical/specifier/context/query identities plus
  `ResolutionFactVersion`, `ResolutionFactKey`, and `ResolutionFactRef`.
- `crates/verter_workspace/src/{fact_cache,resolution_currency}.rs` — cache
  policy, validation authority, published resolution worlds, transactions,
  ledgers, and lawful re-exports of the semantic-owned immutable values. These
  modules define no second nominal fact or resolution-identity vocabulary.
- `crates/verter_session/src/resolver_core/mod.rs` — `ValidatedFactCache`,
  re-exported semantic fact values, `StoreView` (per-domain validator methods),
  and `StoreViewCompatToken`.
- `crates/verter_type_engine/src/semantic_query.rs` — `DeclIdentity` (the live
  value-side versioned identity) and `ResolvedDeclSlotIdentity` (the env-bearing
  content-free query-identity slot used as `Instantiate.base` /
  `ResolveMacroPayload.owner`; the migration to it has LANDED — the former
  content-free `DeclKey` query-key struct and `to_decl_key()` were deleted in
  the cutover). Also hosts `DeclarationSlotSeed` (the slot's env-free
  four-field projection the family-A artifact stores), `BinderScopeId` /
  `BinderScopeKind` (the stable structural lexical-scope id), and the
  `ScopeId.binder_scope_id` query-identity discriminator.
- `crates/verter_session/src/binder_identity_facts.rs` —
  `BinderIdentityFacts` artifact payload, `BinderIdentityFactsStore` (family-A,
  keyed `(canonical, parse_stable_hash, parse_env_hash)`),
  `project_binder_identity_facts` (the shallow-inventory projection), and the
  demand producer `produce_binder_identity_facts` (reached via
  `for_tests::binder_identity_facts_get_or_compute_for_tests` until the U2
  reducers consume it).

## Discriminating tests

- `crates/verter_workspace/src/env_hash_tests.rs` — env-hash split unit tests
  (R21).
- `crates/verter_session/src/file_artifact_store_tests.rs` — store unit tests
  (R5, R6, R28, R29).
- `crates/verter_session/tests/cases/g_misc0/env_hash_isolation.rs` — R21 scoping rule tests.
- `crates/verter_session/tests/cases/g_cache/cache_key_invariants.rs` — R5 / R6 key-shape
  tests.
- `crates/verter_session/tests/cases/g_misc1/parse_stable_hash_invariance.rs` —
  cosmetic-invariant + decl-shape-discriminating tests.
- `crates/verter_session/tests/cases/g_binder/binder_identity_facts.rs` — the
  binder-identity substrate guard set (cosmetic-stable /
  insertion-non-aliasing scope ids, symbol-space + owner-scoped env-free seeds,
  slot finalization, cosmetic-warm / semantic-invalidate, authored-order
  provenance, `binder_scope_id` query identity, negative lookup `ReturnOnly`).
- `crates/verter_session/tests/cases/g_file/file_artifact_store_smoke.rs` — consumer-side
  smoke tests.
- `crates/verter_session_query/src/facts/registry.rs` (`registry_tests` inline
  module) — `FactKey::domain()` routing per R12 / R26, `SymbolSpace` tag
  stability per R11.
- `crates/verter_session_query/src/facts/hashing.rs` (inline `tests` module) —
  alpha-normalisation under object member reorder (R16), stack-safety on
  200-deep nesting (R27), `MemberPresence`/`MemberShape` discrimination (R28).
- `crates/verter_session/src/fact_emission.rs` (inline `tests` module) —
  typed augmentation-inventory emission per R29 archetype and exact lexical
  owner.
- `crates/verter_session/src/member_semantic_fact_store.rs` /
  `member_display_fact_store.rs` (inline `tests` modules) — store admission,
  parse_stable_hash vs content_hash keying contract (R13).
- `crates/verter_session/tests/cases/g_fact/fact_fingerprint_stability.rs` —
  R10/R11/R13/R16 binding (cosmetic-invariance, namespace coexistence,
  decl-reorder stability, syntactic export set).
- `crates/verter_session/tests/cases/g_fact/fact_semantic_display_split.rs` — R13 binding
  (semantic store survives cosmetic edit; display store re-keys).
- `crates/verter_session/tests/cases/g_misc1/parse_resolve_domain_separation.rs` — R12 binding
  (`ImportRef` invariant under resolution change).
- `crates/verter_session/tests/cases/g_misc1/member_presence_vs_member.rs` — R28 two-fact
  model (`pick_literal_key.ts` invariant).
- `crates/verter_session/tests/cases/g_misc0/cycle_safety.rs` — R27 binding (stack-safe +
  cycle-safe + canonical visit order).
- `crates/verter_session/tests/cases/g_misc2/shallow_walk_invariant.rs` — R28 arch-guard
  (parse-time emitter does not call cross-decl AST traversal).
- `crates/verter_session/tests/cases/g_misc2/module_augmentation.rs` — R29 binding
  (per-archetype fact emission; `augmentation_index` stays empty at parse time).
- `crates/verter_session/tests/cases/g_misc0/declaration_merge_facts.rs` — R10 binding
  (merged `interface Foo` parts emit one `Export`).
- `crates/verter_session/tests/cases/g_fact/fact_lane_correctness.rs` — R13 lane binding
  (generic param rename invariant).
- `crates/verter_session/tests/cases/g_fact/fact_emission_parse_time_budget.rs` — emitter
  scales linearly on 10k-decl input.
- `crates/verter_session/tests/cases/g_misc3/storeview_per_domain_dispatch.rs` — R26 binding
  (dispatch table bounded by `FactDomain`, not `FactKey`).
- `crates/verter_session/src/session_view_current_content_tests.rs` — R17
  binding (`SessionView::content_hash_for` is a view-authoritative
  current-content oracle: base + overlay fallthrough return the
  scheduler-authoritative hash, never a stale lingering artifact's; the overlay
  materialiser does not serve a stale `IndexedReady`).
- `crates/verter_session/tests/cases/architecture_guards.rs`
  (`content_pinned_artifact_read_guards` module) — the named-currency-oracle
  closure: `file_artifact_store_defines_no_unpinned_currency_oracle`
  (definition-shape guard) + `no_named_currency_oracle_calls_in_production`
  (call-site ban) + a discriminating self-test.

## Block-vocabulary ban (CRITICAL)

Source comments under `crates/*/src/**` must not contain plan vocabulary
specific to the cache-runtime overhaul. The banned patterns are `\bblock \d+\b`,
`cache-runtime overhaul`, and `runtime cutover`. Production source must read as
final-state once the work lands; plan-management vocabulary leaks once-relevant
project context into the durable code base.

This ban is implementer- and reviewer-enforced, not guard-enforced: a scanner
keyed on plan vocabulary would itself be the name-keyed source scanner the
landed-guard policy forbids.

Durable architectural insights belong here in
`.claude/skills/type-cache-architecture/SKILL.md` or in `docs/arch/` — not in
source comments referencing plan blocks.

### U2 Value-Domain Key Identity (CRITICAL)

The U2 query-value-domain design (`.claude/skills/type-resolution/SKILL.md`)
locks how env dimensions attach to semantic-query keys. The cache-key
composition is two-tier, env stays on the key, and no env-less uniform envelope
is permitted.

- **Two-tier env model.** `ResolvedDeclSlotIdentity` bakes its three intrinsic
  decl-site dimensions (`type_env_hash`, `lib_env_hash`, `project_identity`) as
  DECLARATION IDENTITY. Each per-key `*Context` then adds ONLY the extra QUERY
  dimensions that key depends on — chiefly `resolve_env_hash` for
  import-resolving keys and `parse_env_hash` for body-reading keys. This keeps
  R21 per-layer honest (each layer carries exactly the dims it depends on) and
  stays R6-clean: NO content/version hash and NO `fact_dep_signature` on any
  query-identity key.
- **No env-LESS envelope.** The superseded env-less uniform-envelope design —
  `SemanticQueryEnvKey`, `TypeLibEnvKey`, and the "U2a/U2b" split — is
  FORBIDDEN. Env stays ON the key via per-key `*Context`; it is never lifted
  into a separate uniform env envelope. The env-less KEY types
  (`SemanticQueryEnvKey`, `TypeLibEnvKey`) must appear NOWHERE in production
  source. The env-less `GraphDeclSlotRef` query-identity wire slot HAS BEEN
  RETIRED (deleted from the proto and the `verter_protocol` typed surface) in
  favour of the env-BEARING `GraphResolvedDeclSlotIdentity`
  (`GraphQueryIdentity.resolved_roots`, tag 18; `TYPEINFO_GRAPH_SCHEMA_VERSION`
  bumped to 2, retired `roots` tag/name reserved at message scope). The
  `no_envless_semantic_query_env_key_envelope` guard now bans the
  `GraphDeclSlotRef` symbol across ALL `crates/*/src/**`, and
  `typeinfo_proto_retires_envless_decl_slot_ref` pins the proto surface.
- **Module-resolution keys on SPLIT env.** Each import-resolving surface keys
  only on the dims it consults: `ResolvedImportFacts` keys on `{parse_env_hash,
  resolve_env_hash}` (moduleResolution / paths / baseUrl / conditions /
  extension order) and NEVER `lib_env_hash`; the augmentation-aware
  `AugmentationTargetKey` and `RouteDb`'s `RouteNameKey` / `BarrelSurfaceKey`
  additionally key on `lib_env_hash` (typeRoots / types corpus) +
  `project_identity` because module augmentations come from the ambient/types
  corpus. Lib dimensions are NEVER folded into `resolve_env`. See
  `### Module-Resolution Keying (CRITICAL)`.
- **Slot-keyed `Instantiate` / `ResolveMacroPayload` / `TypeOf`.**
  `SemanticQueryKey::Instantiate` / `ResolveMacroPayload` key their `base` /
  `owner` on the env-bearing, content-free `ResolvedDeclSlotIdentity`;
  `SemanticQueryKey::TypeOf` keys its `value_root` on the env-bearing,
  content-free `ValueRootSlotIdentity` (the extra
  `resolve_env_hash` rides on the per-key `InstantiateContext` /
  `MacroPayloadContext` / `TypeOfContext`). The `provenance` + `merge_role`
  discriminators STAY at
  FAMILY-IDENTITY level on `FamilyKey` — they are NOT demoted into a `*Context`.
- **Symmetric relation admission.** `Identity` and `Comparable` use one sorted
  operand pair in typed constructors, raw-key lifting, and shared query admission
  before family lookup or singleflight. Policy, freshness, inference, substitution,
  and environment axes remain intact. Assignability and subtype relations retain
  operand direction; they never borrow the symmetric ordering rule.
- **Discriminating guards.** The env-scoping and value-domain guards are landed:
  `every_semantic_query_key_maps_to_exactly_one_value_domain`,
  `module_resolution_keys_on_resolve_env_not_type_or_lib`, and
  `semantic_query_key_spec_table_equals_enum`
  (`crates/verter_session/tests/cases/g_block/`); the per-key `*_do_not_warm_hit` set
  — including `instantiate_same_base_different_env_or_context_do_not_warm_hit`,
  `decl_self_type_or_lib_env_change_produces_distinct_instantiate_key`,
  `resolve_macro_payload_same_owner_different_env_or_context_do_not_warm_hit`,
  and `resolved_named_type_key_identity_is_env_scoped` — lives in
  `crates/verter_type_engine/src/semantic_query_memo/tests.rs`. The design-gate
  guards `no_envless_semantic_query_env_key_envelope` and
  `u2_value_domain_design_doc_locks_invariants` live in
  `crates/verter_session/tests/cases/g_block/u2_value_domain_design_guards.rs`.

## Carrier handle identity is never a cache key (PARSELOWER foundation)

The `TypeExpr`→handle migration carries graph handles on the hot path but keeps cache keys content-free (R6). Foundation contracts (additive, dormant):

- **`HotTypeRef`** is an opaque session handle around a graph-instance `SemanticNodeId`; R6 does not prohibit such IDs, although this boundary type deliberately omits `Hash`/`Ord`. Any key containing a `SemanticNodeId` must obtain content validity solely from value-side read sets and strict self-roots. Because `HotTypeRef` derives neither `Hash` nor `Ord`, it cannot itself be a map key nor be embedded in a `#[derive(Hash)]` cache key; the materialised VALUE roots its version through the node's `NodeScopeId` + read-set, never the handle.
- **`SyntheticBindingId`** is the content-free successor to `SyntheticCarrierKey.value_node: u64` — `(scope_canonical_id, surface_kind, slot_name, binding_name)` with NO arena ordinal. It is the identity a future synthetic-deepening key roots on; the `value_node` ordinal is demoted to value-side provenance on the `SemanticNodeData::SyntheticBinding` carrier (re-attached only at the compat materialisation boundary).
- **`CarrierResolverContext`** is value-side runtime resolution state, never hashed into a `SemanticQueryKey`.

Guard: `synthetic_binding_identity_is_content_free`. See `/type-resolution` for the full carrier set + the SEALED `OutputProjector` output-materialization capability fence (block 8-A3): the two boundary methods (`OutputProjector::materialize_output_type_expr` plain SHELL raise / `materialize_reduced_output_type_expr` REDUCE-then-raise) hand back the sealed carriers `OutputTypeExpr` / `MaterializedOutputTypeExpr` whose inner `TypeExpr` lives in a deeply-private nested `carrier::payload` vault (no readable `TypeExpr` field outside it; authority-gated unwrap: the engine-owned `OutputAuthority`, minted once per engine by `EngineStores::create` and lent by the host to its sinks through an inert `OutputLease` that only `output_sinks` opens), the session-owned sealed `OutputProjector` (`crate::output_sinks`) is implemented only for the SIX true-output-SINK caps (one PER-SINK, NOT per-subtree; `new()` is `pub(in <sink-module>)` — the projectors cap is DEFINED + minted in the dedicated TERMINAL sink submodule `meta_resolve::projectors::output_sink`, re-exported at `meta_resolve::projectors::MetaResolveProjectorsOutputCap`, so the parent `projectors`' non-sink helpers cannot mint), and the raw `raise_node_to_type_expr` primitive stays module-private (`raise_and_reduce_with_context` stays `pub(super)`; the shell seam `output_shell_raise_sealed` returns a SEALED `Option<OutputTypeExpr>`, never a bare `TypeExpr`). The PRIMARY barriers are COMPILER-ENFORCED — in safe production Rust OUTSIDE the payload vault: a hot / session / Kind-B mint is `E0624`/`E0451`, a hot unwrap has no `OutputAuthority` to pass AND the inner `TypeExpr` is not even a readable field, and a hot `.type_expr_for_test()` is `E0599` (the carrier `_for_test` accessors are `#[cfg(any(test, feature = "test-support"))]` — non-default: omitted from per-package artifact builds (LSP/napi/wasm) but compiled into whole-workspace builds through the compile-contract variant crate's normal dependency; not `debug_assertions`-present in debug builds); the residual trusted surface is the inline vault + the host composition code that stores the authority + the `output_sinks` registration (and the by-name identity of which owner types are sinks; the claim excludes guard deletion, edits inside the vault, and unsafe unless the crate forbids it globally). `pub(in P)` grants the mint to `P` AND every module at-or-under it, so the mint scope is a TERMINAL output sink whose entire reachable production module tree is itself output-only — that (not "per-leaf") is what fences a non-sink sibling (`meta_resolve::dispatch_helpers`, `host_manage::eval_env`) or a non-sink helper sibling: neither is reachable from any sink's mint scope ⇒ planted `*OutputCap::new` is `E0624`. The FFI boundary is the session-owned bytes facade `project_node_to_type_expr_json_bytes` (the old `project_node_to_type_expr` + `MaterializedTypeExpr` are DELETED). Over the BOUNDED trusted surface the residual `syn` scanners (`output_projector_residual_guards.rs` and the named inventories they hosted) are retired. Post-SIMP5 remaining enforcement is the accidental-regression `assert_not_impl_any!` carrier-escape CANARY in `crates/verter_type_engine/src/project_semantic_dispatch/output_materialization_guards.rs`, compile-time witnesses in `crates/verter_source_policy_gate/tests/cases/semantic_capability_witnesses.rs`, and the trybuild fixtures (`no_typeexpr_*`, `output_authority_not_forgeable`, `output_authority_not_duplicable`, `output_authority_not_recoverable_from_query_access`, `output_authority_not_reminted_from_live_stores`) run by `scripts/compile-contracts.mjs`. Historical (retired) syn-allowlist shape, not live enforcement: the EXACT owner-file module topology (inline `projector` / `projector::sealed` / `carrier` / `carrier::payload` only, with item/impl/trait-position macro invocations, `include!`, unknown attributes, a `sealed::Sealed` alias `use`, and any owner `TypeExpr` alias BANNED) + the sanctioned sink set (explicit `impl OutputProjector` / `impl sealed::Sealed` self-types by FULL self-type path as a MULTISET) by `output_projector_owner_registration_inventory`; a closed item/signature allowlist over the carrier/payload vault (every fn returning `TypeExpr` cap-gated or exactly test-gated) by `output_carriers_have_no_inherent_typeexpr_escape_method`; every carrier/payload field private regardless of spelled type by `output_carrier_payload_fields_are_private`; an accidental-regression CANARY (NOT proof-complete; completeness is the vault) for the common `Deref`/`AsRef`/`Borrow` trait escapes in `output_materialization_guards.rs`; the out-of-crate boundary by `output_projector_non_owner_impl_is_compiler_sealed`; the mintable `TestOutputCap` `#[cfg(test)]`-gate by `test_output_cap_not_visible_or_mintable_in_non_test_builds`; the terminal-sink mint scope by `output_cap_mint_scope_is_per_leaf_not_subtree` (a Rust-VISIBILITY reachable-module-tree model that default-DENIES any reachable non-sink module under a `mint: pub(in P)`, `#[cfg(test)]` modules excluded); the carrier `_for_test` test-support gate by `carrier_for_test_accessors_are_test_support_gated_not_debug_assertions`; the sealed raise seam by `raise_output_seam_returns_sealed_carrier_not_bare_type_expr` (which now pins that NO public/restricted raise.rs fn returns a bare `TypeExpr` — the retired Kind-B bridge leaves no sanctioned exception); the `#[cfg(test)]`-only `materialize_type_expr(HotTypeRef)` harness (the syn guard `materialize_type_expr_is_not_production_visible` is retired with SIMP5).

## whole_env() consumer graph-native readers (Stage 6-prep readiness)

The `DeclBodyMemo::whole_env()` whole-file env product (the lazy, lease-pinned env materialised once per content generation through the retained parse snapshot) has four consumers reaching it via `VerterHost::base_eval_env_arc`: `local_type_declaration_id` (C1), `peel_value_decl_alias` (C2), `build_fallthrough_eval_env_lightweight` (C3), `dependency_eval_env` (C4). Each now carries a NON-BREAKING bounded graph-native per-symbol reader routing through `ShallowFileState::{type_decl, value_decl, header_index}` instead of the whole env, while the legacy whole-env path is RETAINED as the equivalence oracle. The LANDED Stage-6 Option-B flip mints `HotTypeRef` handles at the dispatch boundary (the `decl_body_hot_ref` accessor) over the `Instantiate` query result (`build_instantiate`'s post-processed node; `lower_decl_body_with_provenance` produces the resolving-lowered body-SHAPE that `build_instantiate` post-processes into it) and does NOT remove `EvalEnv` / `whole_env()` (the oracle is retained as the parity rail); `EvalEnv` / `whole_env()` removal remains a LATER stage (the F3 oracle-deletion + Stage 7+ work), not landed. The readers never raise→re-lower (no new hot `materialize_type_expr` bridge). The residual scanners `handle_capable_consumer_guards.rs` and `whole_env_consumer_graph_native_inventory.rs` are retired; post-SIMP5 the capability fence is compile-time witnesses in `crates/verter_source_policy_gate/tests/cases/semantic_capability_witnesses.rs` plus the trybuild fixtures run by `scripts/compile-contracts.mjs`. C1's `DeclarationId` stays an OPAQUE in-process token (never a cache key, never wire-serialized — `FfiResolvedTypeDeclaration` has no `declaration_id`); its contract is stable-and-unique, not equal-to-oracle. Full per-consumer contract + the SFC-generic / svelte-rune oracle-divergence note: `/type-resolution` → IndexedReady Target Contract → whole_env() consumer graph-native readers.

## Related skills

- `/architecture` — high-level module map
- `/host-session` — `VerterHost` lifecycle + `ProjectTypeStore`
- `/type-resolution` — cross-file type resolver + macro traversal
- `/component-meta` — downstream consumer
- `/audit-infrastructure` — `StructuredAuditEvent` extensions

### Immutable structural summaries

`SemanticGraphStore` owns bounded binder-dependency and structural key-domain
summaries beside its substitution and deferred-evaluation memos. These summarize
immutable node payloads only, retain graph roots, reserve the shared retention
account, and clear on graph invalidation. Refused admission leaves the computed
summary usable without retaining it. Declaration references and symbolic key
patterns cannot produce a complete structural domain; their resolution remains
with the ordinary fact-backed query owners. A structural domain retains ordered
keys and a membership index, distinguishes broad domains from finite empty
ones, and never reads member values. Deferred evaluation separately keys shell
and selected-value demands with the complete projection context and retains
its ordinary dependency proof and cost receipt.
