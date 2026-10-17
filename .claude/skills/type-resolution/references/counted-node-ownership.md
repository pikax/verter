# Counted semantic-node ownership

Counted ownership is the sole lifetime authority of every production
`SemanticGraphStore`. A node lives while an owner holds it and its last
release destroys it, iteratively, with every child it alone owned. There is no
canonical payload sweep, no ordinal watermark, no activity gate and no
inactive or test-only reclamation mode: every constructor — `new`,
`with_account`, `with_provenance`, the host's composed store — reclaims the
same way. A document close evicts cache owners, never a held node (see
"Document close and sidecars" below).

`semantic_query_memo::graph_fixture::GraphFixture` (`test` / `test-support`)
wraps an ordinary production store for ownership tests: `run` opens an
unaudited construction scope on the calling thread and `live_nodes` reads the
live count. `ProjectSemanticDispatch::with_graph_fixture` binds the fixture's
graph to a test context; like every dispatcher it opens its own construction
scope over the graph it is bound to. Neither selects a different lifetime.

The sole-authority boundary is executable: `production_constructor_destroys_a_node_on_its_last_release`
(arena) and `every_production_graph_constructor_destroys_on_last_release`
(fixture module) fail for any constructor that leaves a slot to a later
sweep; `close_canonical_keeps_the_live_substrate_flat_across_open_close_cycles`
and `a_root_survives_its_documents_close_and_drains_with_its_last_clone` fail
for a close that revokes a held node or queues its release; and the host tests
`a_close_never_revokes_a_held_node_and_drains_when_the_holder_drops` /
`a_close_keeps_nodes_interned_after_it` fail for a deferred or watermarked
release. Public raw-ordinal routes stay sealed by the compile contracts.

## Roots, retained edges and destruction

`NodeRootSet` retains an immutable operand multiset in its issuing arena.
Clones share one acquisition; independently acquired sets count independently.
`NodeRoot` qualifies a node with that set. `NodeDataLease` couples an arena read
with its root. The rooted interning interface validates the operand set's arena
before accepting children. Public carriers are sealed on top of these roots; see
"Public carriers and capabilities".

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
Only the twelve global primitive vocabulary nodes are permanent.

Arena ordinals are never reused: a destroyed id reads as the shared
`Opaque(Miss)` placeholder and refuses every root acquisition as
`StaleSemanticOperand`; an id never handed out is `ForeignSemanticOperand`.
Fail-safe boundaries turn either into a typed, uncached refusal rather than a
panic or a resurrected node: key preparation (`try_prepare_rooted_key` →
`refused_operand_cache_read`), a settled build whose output cannot be leased
(its flight is retired), a deferred evaluation over a gone operand
(`EvaluateDeferredOutcome::refused_operand`) and identity-record rooting
(`root_in_arena` retains only live operands).

## Document close and sidecars

`SemanticGraphStore::close_canonical` (host: `ProjectTypeStore::close_canonical`,
reached from `VerterHost::evict`) evicts every CACHE owner bound to the closed
content: memo candidates whose key or result reaches it or whose validity is
bound to the canonical, union views over it, the hash-cons / substitute /
deferred memos, the signature store's roots on its current epoch's tokens
(replacing the epoch once stranded tokens dominate; a read view or
`SignatureEpochLease` of that epoch alive at the close keeps those roots until
the last one drops, so a delivered signature result never reads a destroyed
token node; re-rooting a stranded token uncounts it) and, through `memo::close_canonical_with_shapes`,
the shape-cache entries. "Closed content" is decided per cached node by
`NodeArenaCore::reaches_canonical_content` (file scope, a payload binding the
content identity, or a retained child reaching such a node), memoized for one
close — never by sweeping the arena. Nodes are destroyed when their last owner
lets go; a caller still holding one keeps reading it. Artifact-attached macro
mirrors own their carriers with their artifact, so an unchanged reload is
served the same live handle.

Per-node sidecars (structure bits, member-ordinal indexes, origin edges,
stable-key classes) own nothing. Each marks the slot it describes
(`track_sidecar`); destroying a marked slot appends its id to the arena's
pending log, which a close, any sidecar write past
`DESTROYED_DRAIN_THRESHOLD`, or the release that grows the log past that
threshold drains (`reclaim_destroyed_sidecars`; the release drains through the
store's `SidecarSink`, weak handles bound into the arena, without blocking on a
held sidecar lock). The log never outgrows the stale entries it removes and
stays within the threshold at rest even when a holder's final drop follows the
last close; `pending_destroyed_nodes` is its current count.

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
semantic equality, hashing, ordering or digests. Each roots its operands in
the graph that issued them: public constructors take that graph
(`CanonicalTypeSubstitution::new`, `FlowNarrowingKey::new`,
`ContextualTypingKey::new`, `InferableParamSetId::new`,
`IntersectionInputRef::from_operands`/`from_steps`), the memo interns relation
and call family keys with its own arena (`family_and_slot_in`), the signature
kernel uses the graph it is bound to, and a composed substitution inherits its
sources' graph. None roots in whichever construction owner is innermost, which
may be another graph's dispatcher. Node-bearing weak identity
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
weak on the store side and bound once to the final arena during construction.
The signature-store accessor only borrows that store. Union-view ownership is
acquired and released outside the union table lock. Derived-node construction
reservations include the root owner, scope vector capacity and arena count slots.
`SignatureEpochLease` pins its exact epoch and supplies
a read view after epoch replacement. `SignatureVersion` travels on signature
set/result values; delivery uses that version rather than reacquiring whichever
epoch happens to be current. No permanent list of delivered versions is added.

## Public carriers and capabilities

A `SemanticNodeId` is an ordinal: an identity label that authorizes no read
outside the engine. A consumer reads graph payload only through `NodeRef<'a>`,
a `Copy`, lifetime-bound, arena-qualified capability whose fields are private.
Only an owner mints one: `NodeRoot::node_ref`, `NodeRootSet::node_ref` (an
operand the set holds), `NodeDataLease::node_ref` / `child` (a retained child of
the leased payload), `CacheRead::node` / `SemanticDeliveryLease::node_ref` (an
ordinal the delivery roots), `SignatureSetValue::node`, `ReducedOutputNode::node`,
`MaterializedOutputTypeExpr::node` and `NodeRootScope::node_ref` (a construction
ordinal, rooted in that scope). `NodeRef::read` leases the payload under a fresh
root; `SemanticGraphStore::read` additionally refuses a capability another graph
minted (`ForeignSemanticOperand`). A destroyed slot reads as
`StaleSemanticOperand`, never as a live payload. `NodeRef::to_root` upgrades to
an owning `NodeRoot`; two roots are equal when they name the same node of the
same graph.

Ordinal payload reads (`SemanticGraphStore::node_data`, `node_data_for`, the
signature `type_token_node` accessors), ordinal rooting (`root_nodes`) and scope
entry (`enter_node_root_scope`) are crate-private in production and exist
publicly only as `test`/`test-support` fixture seams. So is every other routine
that reads or writes graph state from a bare ordinal without a lane check —
store methods, warm memo reads and producer claims keyed by a query identity,
stable keys and union orderings, the canonical union / intersection builders,
union-surface merges over caller-built surfaces, merged-declaration reduction,
printers, classifiers and `display` — declared through the crate's
`construction_ordinal_seam!`. A public wrapper that forwards caller ordinals
into one of these is sealed with it. Where session code needs one in
production it calls a lane-gated twin (`display_in_scope`,
`intern_ordered_union_in_scope`) that answers nothing outside a lane, or a
dispatcher method that runs it in the dispatcher's own lane
(`spread_formula_positive_members_for_macro`). The construction lane —
reading and rooting bare ordinals — opens only through a
`NodeConstructionGrant`: non-forgeable, minted solely by `EngineStores::create*`
with its graph, held privately by the host's `ProjectTypeStore` (session code
reaches it through `construction_grant()`) and carried by each `EngineBinding`
(`EngineBinding::new` rejects a grant for another graph). Holding the graph alone
grants nothing. Construction-domain producers outside the engine (component-meta
projectors, framework surface producers, audit miners) open scopes with
`grant.enter_scope()` or a dispatcher's `enter_construction_scope()`, root with
`grant.root_nodes` / `NodeRootScope::root_nodes`, and read with `read_in_scope` /
`root_in_scope` (a `Result`: foreign or stale operands are typed errors, which
the host typeinfo entry points surface as faults rather than `Ok(None)`);
`enter_nested_scope` bounds a reader's intermediates inside a lane it already
holds and grants nothing new. `copy_in_scope` is the sealed builder copy of a
member, entry, index signature, surface, payload or diagnostic, rooting the
copy's operands in the active scope.

Construction owners are thread-local and addressed by token, never by stack
position, so scopes and dispatchers close in any order. Every
`ProjectSemanticDispatch` shares ONE construction owner per graph on its thread
with every other live dispatcher of that graph (`DispatchConstructionScope`,
declared last on the dispatcher): an ordinal any of them interns or reads stays
owned until the graph's last dispatcher on the thread drops, so interleaved
dispatchers never release one another's operands. A lexical `NodeRootScope`
releases its roots when it closes and hands its delivery attachments to the
nearest remaining owner of its graph. An owner keeps each delivery once: every
clone of a delivery shares one owner, so repeated reads and clones of a result
under a dispatcher do not grow it. Records rooted at mint
(`SignatureSetValue`, `OneLevelSurface`) root in the graph that produced them,
never in whichever graph's scope is innermost; a runtime-node `SemanticOperand`
owns its node through the delivery lease of the forced read that produced it.

Aggregate payload carriers have no public copy: `SemanticNodeData`,
`SurfaceView`, `SurfaceMember`, `SurfaceEntry`, `IndexSignature`,
`ShallowDiagnostic`, `ShallowSurface`/`ShallowSurfaceEntry`/`ShallowSurfaceMember`,
`SignatureCandidateNodes`, the payload member records (`TupleElement`,
`FunctionParam`, `TypeParamDecl`, `ObjectConstructionEffect` and its
`Authored*Effect` records, `ObjectSpreadProgram`) and composite arms
(`CompositeList`, `CompositeMembers`) copy only crate-privately (exhaustive
field copies, so a new field cannot be skipped); test builds add
`copy_for_tests`. Nested payload arrays are `PayloadArray`s: borrowed slices
with no public copy and no exposed `Arc`. A `QueryError` copies opaquely: its
`RecursiveRef` arguments are a `RecursiveRefArgs` exposing only a length.
Query-identity children (`MapperKey`, `IndexKey`, carriers, deferred callables,
pending substitutions) copy with the keys they identify; their ordinals are
labels that authorize no read. `OneLevelSurface` is an owning adapter whose
`from_view` is crate-private. A
`SurfaceView` exposes its stream and indexes as borrowed slices, never as shared
`Arc`s. `CacheRead` fields are crate-private: a consumer borrows `value()` and
`walker_diagnostics()`; a clone of a read clones its lease. `ShallowDiagnostic`'s
recorded query failure is a `DiagnosticError` that renders but exposes no
`QueryError` to copy. `SignatureSetValue` owns its candidate nodes: its candidate
table and their roots share one `Arc`, so every clone owns them. Public results
that hand out a surface or a terminal node deliver owning carriers:
`resolve_typeinfo_surface` returns a `SurfaceLease`, and the host typeinfo entry
points (`resolve_named_symbol*`, `evaluate_type_expression_with_audit`) return
`NodeRoot`s that `project_node_to_type_expr_json_bytes` /
`project_node_to_semantic_type_graph` validate against the host graph before
projecting. The NAPI and WASM two-call projections carry that root between the
calls. The task inventory
[`SKR-OWN-PUBLIC.md`](../../../../crates/verter_type_engine/observe-inventory/SKR-OWN-PUBLIC.md)
lists every carrier, its route, its rule and its proof.

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
[`SKR-OWN-PREP.md`](../../../../crates/verter_type_engine/observe-inventory/SKR-OWN-PREP.md)
and, for the production lifetime authority, close and sidecar cleanup, in
[`SKR-OWN-3.md`](../../../../crates/verter_type_engine/observe-inventory/SKR-OWN-3.md).
