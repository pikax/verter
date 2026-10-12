# Counted semantic-node ownership substrate

`SemanticGraphStore` production constructors retain canonical-sweep reclamation.
The arena's counted last-root destruction is inactive in production, including
ordinary constructors in a build with `test-support`. Canonical ownership,
ordinal watermarks and `SemanticActivityGate` remain the close/reload authority.
Preparatory roots do not promise survival across a legacy canonical release.
Signature epochs, identity records, cache reservations and ordinary `Arc` owners
still release their own storage normally.

`semantic_query_memo::ownership_harness::CountedOwnershipHarness` is available
only under `cfg(test)` or `test-support`. Its `run` method establishes an
unaudited construction scope on the calling thread. This is the sole controlled
activation interface; production callers cannot configure counted destruction.
The production absence contract is an automatically discovered session
compile-fail fixture; ordinary-constructor and legacy-sweep negative controls
are arena unit tests.

`ProjectSemanticDispatch::with_ownership_harness` consumes that same fixture
and binds its counted graph to a test context. It opens no construction scope,
so session composition tests can exercise the gaps between real dispatches.
This adapter is also restricted to `test` or `test-support` builds.

## Roots, retained edges and destruction

`NodeRootSet` retains an immutable operand multiset in its issuing arena.
Clones share one acquisition; independently acquired sets count independently.
`NodeRoot` qualifies a node with that set. `NodeDataLease` couples an arena read
with its root. The rooted interning interface validates the operand set's arena
before accepting children. Raw ordinal carriers are still exposed by existing
public APIs; sealing those carriers is separate work.

`SemanticNodeData::for_each_retained_child` includes semantic children, opaque
error operands, derived object indexes, deferred callable parts, pending-frame
binders and distribution operands. Repeated fields are repeated edges. Typed
query-result and diagnostic visitors follow the same rule. Provenance identities
and observation records never become roots. A test-only independent multiset
guard distinguishes omitted edges from duplicated edges.

An interned slot acquires each child occurrence and a construction root. Dedup
holds only weak payload references. Lookup checks the candidate's live slot and
acquires its root under the arena lock while holding the dedup shard lock; a dead
external payload cannot hide an equal live peer. Last release processes a
worklist, releases outgoing occurrences and retires weak entries iteratively.
Only the twelve global primitive vocabulary nodes are permanent in the harness.

Production sweeps tombstone dormant owner counts along with their node slots,
retire dedup entries and notify signature storage of released tokens. Dropping
an older root then skips the tombstone. It cannot revive an ordinal or decrement
a newly allocated slot: arena ordinals are not reused.

## Internal delivery and retained holders

`CacheRead::lease` carries `SemanticDeliveryLease`: result and diagnostic roots,
optional signature epoch ownership, and candidate key roots. Admission refusal
returns a complete rooted delivery without a memo entry. Flights, subscriptions,
published candidates and refusal summaries retain the same envelope. Value-domain
mapping transfers it. Newly issued and cloned deliveries are captured by the
active construction scope, so a nested builder may consume a raw value after
dropping its read. Nested scopes transfer these delivery attachments to their
enclosing scope before releasing intermediates; audit capture is unrelated.

Prepared query identities own key operands and signature descriptors' epochs.
Interned relation/call identities, intersection recipes, substitutions,
inferable parameters, flow and contextual keys attach roots without changing
semantic equality, hashing, ordering or digests. Node-bearing weak identity
records are qualified by arena for storage reuse; semantic content remains the
identity. This prevents an equal recipe in another graph from retaining the
first graph's nested owners.

Substitute and deferred-conditional memos, cached union views, cost identities
and failed-prefix receipts retain their operands. Scope, forced-operand and
materialized-output transports retain internal values through lowering and
raising. Shape-cache keys and output values, macro products and their host
mirrors, and the session scratch cache retain operands as well. Cache hits keep
their roots in the active construction scope before exposing a raw node.

Terminal shape/output assembly can occur after nested dispatch construction
scopes have returned. Materialized output values acquire roots from their
sealed payload's issuing engine identity; member shape keys acquire from the
output authority's engine. Neither uses the ambient scope to select an arena.
Unbound test carriers still require an explicit fixture construction scope.

Session key-domain builders open their own construction scope and return a
`NodeRoot` through the consuming Pick/Omit dispatch. Macro payload and surface
producers likewise return rooted nodes, including merged emit objects and
incomplete positive subsets. Publication tokens retain the surface and member
operands through enumeration, admission and terminal sink consumption. These
internal owners do not rely on an enclosing request or audit scope.
Route-admission carriers acquire the gated witness's node from its issuing
graph before the orchestration scope closes, retaining it through the route
publication sink. Their semantic admission gates remain unchanged.

The dedup/final-release regression uses per-arena test rendezvous: acquisition
wins with the arena lock held from live-slot validation through root reservation;
release wins with a tombstoned slot still present in the weak index and an
external payload Arc still alive. Both returned roots remain live and their
final drops drain the slots and index. Rendezvous hooks compile only under
`cfg(test)`.

Signature epochs retain graph-token roots through an arena attachment that is
weak on the store side. `SignatureEpochLease` pins its exact epoch and supplies
a read view after epoch replacement. `SignatureVersion` travels on signature
set/result values; delivery uses that version rather than reacquiring whichever
epoch happens to be current. No permanent list of delivered versions is added.

## Current counts and observation policy

`SemanticGraphStore::node_ownership_counts` derives current live nodes, roots,
retained child occurrences and permanent nodes under the arena lock. The host
retention snapshot exposes `semantic_node_ownership`; statistics expose
`semanticNodeRoots`, `semanticNodeInternalEdges` and `semanticNodePermanentNodes`.
These are required current ownership counts, not optional histories. Root
attachments, retention policy charges and scoped transport are required in all
builds. Detailed provenance/attribution remains behind the existing default-off
`semantic-observe` feature; the substrate adds no always-on observation history.

Field/store/hook classification is recorded in
[`SKR-OWN-PREP.md`](../../../../crates/verter_type_engine/observe-inventory/SKR-OWN-PREP.md).
