//! The canonical union / intersection algebra — the single normalization
//! authority union and intersection CONSTRUCTION is closed over.
//!
//! Owns three things:
//!
//! 1. **The structural comparator** ([`compare_structural`]): semantic
//!    constituent identity sufficient for canonicalization WITHOUT relying on
//!    fresh [`SemanticNodeId`] equality. "Scope-insensitive" means ignoring
//!    ONLY the arena sidecar scope
//!    ([`SemanticGraphStore::node_scope`]) — scope-bearing semantic PAYLOAD
//!    fields (`BareRef` scope, declaration identity, value roots,
//!    infer-binder identity) remain identity. The comparator mirrors
//!    `SemanticNodeData`'s manual equality rules, recursively replacing child
//!    ordinals with child structural identity, and returns
//!    [`Equal | Distinct | Incomplete`](StructuralIdentity). It is iterative
//!    (heap worklist, never the Rust call stack) and cycle-safe (a revisited
//!    in-flight pair is assumed equal — bisimulation). `Incomplete` (a
//!    missing payload or an exhausted work budget) preserves both arms and
//!    suppresses canonical warm admission.
//!
//! 2. **The canonical builders** ([`canonical_union`] /
//!    [`canonical_intersection`]): recursive same-kind flattening, the §22
//!    lattice absorption laws (`X | never = X`, `X | any = any`,
//!    `X | unknown = unknown`, `X & never = never`, `X & any = any`,
//!    `X & unknown = X`, error-carrier domination on both operators),
//!    checker-mirroring literal subsumption on unions (`string | "a"` is
//!    `string`), structural `T | T = T` / `T & T = T` via the comparator,
//!    and PROVEN-disjoint scalar intersection collapse to `never` via
//!    [`tag_level_disjoint`] — an undecided relation is never guessed. A
//!    derived multi-arm composite interns under `Global`; a singleton
//!    normalization returns its retained member unchanged, with that
//!    member's own scope; an empty member set folds to `Primitive(Never)`.
//!
//! 3. **Freshness evidence** ([`CanonicalEvidence`]): every file-scoped node
//!    whose structure the identity walk inspected — INCLUDING discarded
//!    duplicates and descendants reached through `Global` intermediates —
//!    is recorded as a `(canonical, observed_whole_hash)` self-root, and an
//!    incomplete or budget-tripped comparison marks the evidence
//!    `incomplete`, which the dispatch funnel folds into the active
//!    cold-build's `cache_suppress` (ReturnOnly, never warm).
//!
//! The dispatch-level funnel is
//! `ProjectSemanticDispatch::intern_normalized_union_or_intersection`
//! (`build.rs`), which routes through these builders and deposits the
//! evidence ambiently. Callers without a dispatch (graph-only helpers) call
//! the builders directly and own their evidence disposition explicitly.
//!
//! Memoization discipline: identity decisions are cached only per
//! canonicalization ([`canonicalize`]'s decided-pair map — each entry is a
//! completed ROOT comparison of a fresh depth-0 traversal). No child result
//! is ever spliced into another traversal, and no store-level or global
//! fingerprint/representative cache exists here.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::absorb::SpecialKind;
use crate::semantic_query::{
    authored_property_key_child, LiteralValue, NodeScopeId, PrimitiveKind, SemanticNodeData,
    SemanticNodeId, SignatureReturnCarrier, SurfaceEntry,
};
use crate::semantic_query_memo::{ObservedGraphSelfRoot, SemanticGraphStore};

/// The witness that a [`CompositeList`](crate::semantic_query::composite::CompositeList)
/// member list was produced by THIS module's canonical builders. Its field is
/// private and no constructor is exported: only the canonical algebra can
/// mint the `Canonical` carrier category.
pub(crate) struct CanonicalMint {
    _sealed: (),
}

/// Structural-identity verdict of one comparator run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructuralIdentity {
    /// The two nodes are structurally and payload-scope equal (arena sidecar
    /// scope ignored).
    Equal,
    /// The two nodes are structurally distinct (or conservatively treated as
    /// distinct — see the opaque-payload arms). Distinct never collapses.
    Distinct,
    /// The comparison could not complete (missing payload, exhausted work
    /// budget). Both arms are preserved and the result never warms.
    Incomplete,
}

/// Freshness evidence of one canonicalization.
#[derive(Debug, Default)]
pub(crate) struct CanonicalEvidence {
    /// One `(canonical, observed_whole_hash)` self-root per file-scoped node
    /// whose payload the flatten / lattice / identity walk inspected —
    /// including DISCARDED duplicates and descendants reached through
    /// `Global` intermediates. An edit to any inspected file must miss the
    /// warm read even when the node it invalidates no longer appears in the
    /// canonical result.
    pub(crate) inspected_file_roots: Vec<ObservedGraphSelfRoot>,
    /// `true` when any structural comparison returned
    /// [`StructuralIdentity::Incomplete`] or a bounded scan was skipped for
    /// size: the result is correct but not proven canonical, so it is
    /// ReturnOnly — never a warm canonical result.
    pub(crate) incomplete: bool,
}

impl CanonicalEvidence {
    fn record_file_root(&mut self, graph: &SemanticGraphStore, node: SemanticNodeId) {
        if let Some(NodeScopeId::File {
            canonical_id,
            whole_hash,
            ..
        }) = graph.node_scope(node)
        {
            if !self
                .inspected_file_roots
                .iter()
                .any(|(c, h)| *c == canonical_id && *h == whole_hash)
            {
                self.inspected_file_roots.push((canonical_id, whole_hash));
            }
        }
    }
}

/// One canonicalized composite: the interned node plus the freshness
/// evidence its construction produced.
#[must_use = "canonical evidence carries warm-admission suppression and \
              self-roots; dropping it silently loses freshness"]
pub(crate) struct CanonicalComposite {
    pub(crate) node: SemanticNodeId,
    pub(crate) evidence: CanonicalEvidence,
}

/// Work budget of one canonicalization's structural comparisons: total
/// DESCENT pairs (pairs beyond each comparison's root pair) inspected
/// across every dedup comparison. Root-pair checks are free — a wide union
/// of shallowly-distinct arms costs no budget — so exhaustion means genuine
/// deep structural work, and it marks the evidence incomplete (ReturnOnly),
/// never a wrong collapse.
const COMPARE_WORK_BUDGET: u32 = 4096;

/// Arm cap of the pairwise structural (tier-2) dedup: it runs only among
/// CHILD-BEARING arms (childless payloads are fully deduplicated by the
/// linear content-identity tier), and only when at most this many remain.
/// Beyond it the tier is skipped and the evidence marked incomplete — a
/// pathological wide composite of deep arms is served ReturnOnly rather
/// than either paying O(n²) deep comparisons or warm-publishing an
/// unproven canonical form.
const STRUCTURAL_DEDUP_ARM_CAP: usize = 128;

/// Maximum transparent `Alias` hops the lattice peek follows — mirrors the
/// absorb-table discipline (`absorb.rs::ALIAS_PEEK_HOPS`).
const ALIAS_PEEK_HOPS: usize = 8;

/// Canonical union construction over `members`.
pub(crate) fn canonical_union(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
) -> CanonicalComposite {
    canonicalize(graph, members, /* is_union */ true)
}

/// Canonical union construction for a caller OUTSIDE any dispatch cold
/// build — a graph-only consumer (typeinfo surface projection, meta-resolve
/// slot/key composition, relation parameter targets) whose publication rail
/// carries its own fact-validated read set. Evidence disposition, decided
/// ONCE here rather than ad hoc per site:
///
/// * inspected file roots — subsumed: every arm (a discarded structural
///   duplicate included) reached the caller through fact-recorded reads in
///   the SAME read set that validates the published result, so the file
///   dependency is already on the publication rail;
/// * `incomplete` — the arms are preserved verbatim (never a wrong
///   collapse), which is byte-for-byte the pre-canonical shape these
///   consumers published before; none of them claims canonical form. The
///   canonical-claiming surfaces (the `NormalizeUnion` /
///   `NormalizeIntersection` queries and every dispatch-context funnel
///   caller) route through
///   `ProjectSemanticDispatch::intern_normalized_union_or_intersection`,
///   where `incomplete` folds `cache_suppress` (ReturnOnly).
pub(crate) fn canonical_union_node_for_fact_railed_consumer(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
) -> SemanticNodeId {
    canonical_union(graph, members).node
}

/// Intersection twin of
/// [`canonical_union_node_for_fact_railed_consumer`] — same evidence
/// disposition.
pub(crate) fn canonical_intersection_node_for_fact_railed_consumer(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
) -> SemanticNodeId {
    canonical_intersection(graph, members).node
}

/// Canonical intersection construction over `members`.
pub(crate) fn canonical_intersection(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
) -> CanonicalComposite {
    canonicalize(graph, members, /* is_union */ false)
}

fn canonicalize(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
    is_union: bool,
) -> CanonicalComposite {
    let mut evidence = CanonicalEvidence::default();

    // 1. Recursive same-kind flattening: a union is never an arm of a union,
    //    an intersection never an arm of an intersection (mirrors the
    //    checker's own `getUnionType` flattening). Source order preserved.
    let flat = flatten_members(graph, members, is_union, &mut evidence);

    // 2. §22 lattice absorption over the flattened arms. The peeks follow
    //    transparent Alias redirects, so an aliased extreme absorbs too.
    let mut specials: Vec<Option<SpecialKind>> = Vec::with_capacity(flat.len());
    let mut error_node: Option<SemanticNodeId> = None;
    let mut has_any = false;
    let mut has_unknown = false;
    let mut has_never = false;
    for &m in &flat {
        let special = peek_special_via_graph(graph, m, &mut evidence);
        match special {
            Some((SpecialKind::Any, _)) => has_any = true,
            Some((SpecialKind::Unknown, _)) => has_unknown = true,
            Some((SpecialKind::Never, _)) => has_never = true,
            Some((SpecialKind::Error, id)) if error_node.is_none() => {
                error_node = Some(id);
            }
            Some((SpecialKind::Error, _)) => {}
            None => {}
        }
        specials.push(special.map(|(kind, _)| kind));
    }
    // Error dominates every other absorber on BOTH operators, so the error
    // CARRIER (node identity + `QueryError` payload) is never hidden behind
    // a Clean `any` / `unknown` / `never`.
    if let Some(err) = error_node {
        return CanonicalComposite {
            node: err,
            evidence,
        };
    }
    let extreme = if is_union {
        // `X | any = any`, `X | unknown = unknown`.
        if has_any {
            Some(PrimitiveKind::Any)
        } else if has_unknown {
            Some(PrimitiveKind::Unknown)
        } else {
            None
        }
    } else {
        // `X & never = never`, `X & any = any` (never dominates any).
        if has_never {
            Some(PrimitiveKind::Never)
        } else if has_any {
            Some(PrimitiveKind::Any)
        } else {
            None
        }
    };
    if let Some(kind) = extreme {
        return CanonicalComposite {
            node: graph.intern_node(SemanticNodeData::Primitive(kind)),
            evidence,
        };
    }
    // Union drops `never` arms (`X | never = X`); intersection drops
    // `unknown` arms (`X & unknown = X`). An all-dropped set folds below
    // (empty union ⇒ `never`; all-`unknown` intersection ⇒ `unknown`).
    let dropped_kind = if is_union {
        SpecialKind::Never
    } else {
        SpecialKind::Unknown
    };
    let mut arms: Vec<SemanticNodeId> = Vec::with_capacity(flat.len());
    for (index, &m) in flat.iter().enumerate() {
        if specials[index] != Some(dropped_kind) {
            arms.push(m);
        }
    }
    if !is_union && arms.is_empty() && has_unknown {
        return CanonicalComposite {
            node: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown)),
            evidence,
        };
    }

    // 3. Union literal subsumption, mirroring the checker's `getUnionType`
    //    reduction: a literal arm whose base primitive the union already
    //    carries adds no inhabitant (`string | "a"` is `string`). Structural
    //    (node-data) — never text — and scope-insensitive by construction.
    if is_union {
        let carries_primitive = |kind: PrimitiveKind, arms: &[SemanticNodeId]| {
            arms.iter().any(|member| {
                matches!(
                    graph.node_data(*member).as_deref(),
                    Some(SemanticNodeData::Primitive(primitive)) if *primitive == kind
                )
            })
        };
        let mut kept: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        for member in arms.iter().copied() {
            let literal_primitive = match graph.node_data(member).as_deref() {
                Some(SemanticNodeData::Literal(LiteralValue::String(_))) => {
                    Some(PrimitiveKind::String)
                }
                Some(SemanticNodeData::Literal(LiteralValue::Number(_))) => {
                    Some(PrimitiveKind::Number)
                }
                Some(SemanticNodeData::Literal(LiteralValue::BigInt(_))) => {
                    Some(PrimitiveKind::BigInt)
                }
                Some(SemanticNodeData::Literal(LiteralValue::Boolean(_))) => {
                    Some(PrimitiveKind::Boolean)
                }
                _ => None,
            };
            if !literal_primitive.is_some_and(|kind| carries_primitive(kind, &arms)) {
                kept.push(member);
            }
        }
        arms = kept;
    }

    // 4. Structural `T | T = T` / `T & T = T` — two tiers, both
    //    scope-insensitive:
    //
    //    Tier 1 (linear, COMPLETE for childless payloads): content-identity
    //    dedup by exact payload equality (hash-narrowed, exact-`Eq`
    //    confirmed — a hash match alone never deduplicates). Two arms whose
    //    payloads are `Eq` are structurally equal regardless of their arena
    //    sidecar scopes, which is precisely the cross-scope duplicate class
    //    (`Primitive(String)` interned under two files). For a CHILDLESS
    //    payload, payload equality IS structural identity, so childless
    //    arms are fully canonical after this tier — a wide union of scalar
    //    arms stays linear and never trips a budget.
    //
    //    Tier 2 (pairwise, budgeted): the recursive comparator over the
    //    CHILD-BEARING survivors only — the arms whose structural identity
    //    can differ from payload identity (children interned under
    //    different scopes). First occurrence survives; a discarded
    //    duplicate's inspected roots stay in the evidence; an `Incomplete`
    //    comparison (or an over-cap arm set) keeps every arm and taints
    //    the evidence.
    let hasher = std::collections::hash_map::RandomState::new();
    let mut seen_payloads: FxHashMap<u64, smallvec::SmallVec<[SemanticNodeId; 1]>> =
        FxHashMap::default();
    let mut kept: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
    let mut child_bearing: Vec<(usize, Arc<SemanticNodeData>)> = Vec::new();
    'tier1: for &m in &arms {
        let Some(data) = graph.node_data(m) else {
            // A dangling arm cannot be compared — preserve it and refuse
            // canonical warm admission.
            evidence.incomplete = true;
            kept.push(m);
            continue;
        };
        use std::hash::BuildHasher;
        let bucket = seen_payloads.entry(hasher.hash_one(&*data)).or_default();
        for &candidate in bucket.iter() {
            if candidate == m {
                continue 'tier1;
            }
            if graph
                .node_data(candidate)
                .is_some_and(|cand| *cand == *data)
            {
                // Content-identical cross-scope duplicate — discarded; its
                // root was recorded when the flatten walk inspected it.
                continue 'tier1;
            }
        }
        bucket.push(m);
        if !payload_is_childless(&data) {
            child_bearing.push((kept.len(), Arc::clone(&data)));
        }
        kept.push(m);
    }
    if child_bearing.len() > STRUCTURAL_DEDUP_ARM_CAP {
        evidence.incomplete = true;
    } else if child_bearing.len() > 1 {
        let mut budget = COMPARE_WORK_BUDGET;
        let mut discarded: Vec<usize> = Vec::new();
        for i in 1..child_bearing.len() {
            let (index_m, _) = child_bearing[i];
            for (index_k, _) in &child_bearing[..i] {
                let index_k = *index_k;
                if discarded.contains(&index_k) {
                    continue;
                }
                match compare_structural(
                    graph,
                    kept[index_m],
                    kept[index_k],
                    &mut evidence,
                    &mut budget,
                ) {
                    StructuralIdentity::Equal => {
                        discarded.push(index_m);
                        break;
                    }
                    StructuralIdentity::Distinct => {}
                    StructuralIdentity::Incomplete => {
                        evidence.incomplete = true;
                    }
                }
            }
        }
        if !discarded.is_empty() {
            let mut index = 0usize;
            kept.retain(|_| {
                let drop = discarded.contains(&index);
                index += 1;
                !drop
            });
        }
    }

    // 5. Intersection proven-disjoint scalar collapse: only a PROVEN empty
    //    tag-level domain collapses (`string & number = never`); an
    //    undecided relation is never guessed. Single pass over the deduped
    //    arms — exactly the pairwise [`tag_level_disjoint`] judgement,
    //    computed in O(n) over the childless scalar domain.
    if !is_union && scalar_domain_provably_empty(graph, &kept) {
        return CanonicalComposite {
            node: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)),
            evidence,
        };
    }

    // 6. Folds + interning. Empty ⇒ `Primitive(Never)`; singleton ⇒ the
    //    retained member unchanged (that member's own scope); a derived
    //    multi-arm composite sorts deterministically and interns under
    //    `Global` through the sealed canonical mint.
    kept.sort_by_key(|id| id.0);
    kept.dedup();
    let node = match kept.as_slice() {
        [] => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)),
        [only] => *only,
        _ => {
            let list = crate::semantic_query::composite::CompositeList::minted(
                Arc::from(kept.into_boxed_slice()),
                crate::semantic_query::composite::CompositeCarrierCategory::Canonical(
                    CanonicalMint { _sealed: () },
                ),
            );
            if is_union {
                graph.intern_node(SemanticNodeData::Union(list.members_arc()))
            } else {
                graph.intern_node(SemanticNodeData::Intersection(list.members_arc()))
            }
        }
    };
    CanonicalComposite { node, evidence }
}

/// Iterative recursive same-kind flattening. Preserves source order; records
/// the file root of every inspected node (spliced composite shells
/// included); a revisited composite shell (impossible for well-formed
/// append-only ids, guarded anyway) is dropped rather than re-spliced.
fn flatten_members(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
    is_union: bool,
    evidence: &mut CanonicalEvidence,
) -> Vec<SemanticNodeId> {
    let mut out: Vec<SemanticNodeId> = Vec::with_capacity(members.len());
    let mut spliced: FxHashSet<SemanticNodeId> = FxHashSet::default();
    let mut stack: Vec<SemanticNodeId> = members.iter().rev().copied().collect();
    while let Some(m) = stack.pop() {
        evidence.record_file_root(graph, m);
        match graph.node_data(m).as_deref() {
            Some(SemanticNodeData::Union(nested)) if is_union => {
                if spliced.insert(m) {
                    for n in nested.iter().rev() {
                        stack.push(*n);
                    }
                }
            }
            Some(SemanticNodeData::Intersection(nested)) if !is_union => {
                if spliced.insert(m) {
                    for n in nested.iter().rev() {
                        stack.push(*n);
                    }
                }
            }
            _ => out.push(m),
        }
    }
    out
}

/// Graph-level lattice-extreme peek — the shared body behind
/// `ProjectSemanticDispatch::peek_special`. Follows transparent `Alias`
/// redirects (bounded), records inspected file roots, and returns the kind
/// plus the RESOLVED node id (so an `error` operand's carrier is reused
/// verbatim, preserving its `QueryError` payload and node identity).
pub(super) fn peek_special_via_graph(
    graph: &SemanticGraphStore,
    id: SemanticNodeId,
    evidence: &mut CanonicalEvidence,
) -> Option<(SpecialKind, SemanticNodeId)> {
    let mut cur = id;
    // bounded-loop: ALIAS_PEEK_HOPS transparent Alias redirects.
    for _ in 0..ALIAS_PEEK_HOPS {
        evidence.record_file_root(graph, cur);
        let data = graph.node_data(cur)?;
        match &*data {
            SemanticNodeData::Alias(inner) => {
                let next = *inner;
                drop(data);
                cur = next;
                continue;
            }
            SemanticNodeData::Primitive(PrimitiveKind::Any) => {
                return Some((SpecialKind::Any, cur))
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                return Some((SpecialKind::Never, cur))
            }
            SemanticNodeData::Primitive(PrimitiveKind::Unknown) => {
                return Some((SpecialKind::Unknown, cur))
            }
            SemanticNodeData::Opaque(err) if err.is_error_type() => {
                return Some((SpecialKind::Error, cur))
            }
            _ => return None,
        }
    }
    None
}

/// O(tag) disjointness — the sole proven-disjoint authority for both the
/// canonical intersection collapse and the relation engine's
/// contravariant-candidate intersection collapse (`relation.rs` delegates
/// here). `true` ONLY for pairs whose intersection is provably empty at tag
/// level — distinct concrete primitives (modulo the `undefined`/`void`
/// widening pair), distinct literals, or a literal against a mismatched base
/// primitive. Conservative `false` for every other shape: an undecided
/// relation is never guessed and the structural carrier is kept.
pub(crate) fn tag_level_disjoint(
    graph: &SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> bool {
    let (Some(a_data), Some(b_data)) = (graph.node_data(a), graph.node_data(b)) else {
        return false;
    };
    fn literal_base(lit: &LiteralValue) -> PrimitiveKind {
        match lit {
            LiteralValue::String(_) => PrimitiveKind::String,
            LiteralValue::Number(_) => PrimitiveKind::Number,
            LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
            LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
        }
    }
    fn concrete(kind: PrimitiveKind) -> bool {
        !matches!(
            kind,
            PrimitiveKind::Any | PrimitiveKind::Unknown | PrimitiveKind::Never
        )
    }
    match (&*a_data, &*b_data) {
        (SemanticNodeData::Primitive(x), SemanticNodeData::Primitive(y)) => {
            let widening_pair = matches!(
                (*x, *y),
                (PrimitiveKind::Undefined, PrimitiveKind::Void)
                    | (PrimitiveKind::Void, PrimitiveKind::Undefined)
            );
            concrete(*x) && concrete(*y) && x != y && !widening_pair
        }
        (SemanticNodeData::Literal(x), SemanticNodeData::Literal(y)) => x != y,
        (SemanticNodeData::Literal(lit), SemanticNodeData::Primitive(prim))
        | (SemanticNodeData::Primitive(prim), SemanticNodeData::Literal(lit)) => {
            concrete(*prim) && literal_base(lit) != *prim
        }
        _ => false,
    }
}

/// Whether a payload carries NO child node ids — for such a payload,
/// payload equality IS structural identity, so the linear content-identity
/// dedup tier is complete over it. EXHAUSTIVE (no wildcard): a new variant
/// fails to compile here until its child topology is classified.
fn payload_is_childless(data: &SemanticNodeData) -> bool {
    use SemanticNodeData as D;
    match data {
        D::Primitive(_) | D::Literal(_) | D::Opaque(_) | D::RawFallback { .. } => true,
        D::Infer { .. } | D::InferRef { .. } | D::DeclRef { .. } => true,
        // A TypeParam without constraint/default has no children.
        D::TypeParam {
            constraint,
            default,
            ..
        } => constraint.is_none() && default.is_none(),
        D::Alias(_)
        | D::Object(_)
        | D::ObjectSpreadProgram(_)
        | D::Union(_)
        | D::Intersection(_)
        | D::Array { .. }
        | D::Tuple { .. }
        | D::TemplateLiteral { .. }
        | D::KeyOf { .. }
        | D::IndexedAccess { .. }
        | D::Mapped { .. }
        | D::TypeOf(_)
        | D::Conditional { .. }
        | D::Signature { .. }
        | D::DeferredCallable(_)
        | D::InstantiationRef { .. }
        | D::MergedDecl { .. }
        | D::BareRef(_)
        | D::ImportType(_)
        | D::SyntheticBinding { .. } => false,
    }
}

/// Single-pass equivalent of "∃ arm pair with [`tag_level_disjoint`] =
/// true" over the whole arm set: the intersection's scalar tag-level
/// domain is PROVABLY empty. Non-scalar arms contribute nothing (they are
/// never tag-level decidable); an undecided shape is never guessed.
fn scalar_domain_provably_empty(graph: &SemanticGraphStore, arms: &[SemanticNodeId]) -> bool {
    fn concrete(kind: PrimitiveKind) -> bool {
        !matches!(
            kind,
            PrimitiveKind::Any | PrimitiveKind::Unknown | PrimitiveKind::Never
        )
    }
    fn literal_base(lit: &LiteralValue) -> PrimitiveKind {
        match lit {
            LiteralValue::String(_) => PrimitiveKind::String,
            LiteralValue::Number(_) => PrimitiveKind::Number,
            LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
            LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
        }
    }
    let mut seen_concrete: Option<PrimitiveKind> = None;
    let mut seen_literal: Option<LiteralValue> = None;
    for &arm in arms {
        match graph.node_data(arm).as_deref() {
            Some(SemanticNodeData::Primitive(kind)) if concrete(*kind) => {
                if let Some(prev) = seen_concrete {
                    let widening_pair = matches!(
                        (prev, *kind),
                        (PrimitiveKind::Undefined, PrimitiveKind::Void)
                            | (PrimitiveKind::Void, PrimitiveKind::Undefined)
                    );
                    if prev != *kind && !widening_pair {
                        return true;
                    }
                    // Keep the NON-void representative so a later literal
                    // still checks against a base-comparable kind.
                    if prev == PrimitiveKind::Void {
                        seen_concrete = Some(*kind);
                    }
                } else {
                    seen_concrete = Some(*kind);
                }
                if let Some(lit) = &seen_literal {
                    if literal_base(lit) != *kind {
                        return true;
                    }
                }
            }
            Some(SemanticNodeData::Literal(value)) => {
                if let Some(prev) = &seen_literal {
                    if prev != value {
                        return true;
                    }
                } else {
                    seen_literal = Some(value.clone());
                }
                if let Some(kind) = seen_concrete {
                    if literal_base(value) != kind {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// The scope-insensitive structural comparator. See the module docs for the
/// identity contract. Iterative bisimulation: a worklist of node-id pairs, a
/// visited-pair set (a revisited pair is assumed equal — the coinductive
/// cycle rule), and a caller-owned work budget shared across one
/// canonicalization. Every inspected node's file root is recorded on
/// `evidence` — including nodes of a comparison that ends `Distinct`.
pub(crate) fn compare_structural(
    graph: &SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
    evidence: &mut CanonicalEvidence,
    budget: &mut u32,
) -> StructuralIdentity {
    let mut work: Vec<(SemanticNodeId, SemanticNodeId)> = vec![(a, b)];
    let mut visited: FxHashSet<(u64, u64)> = FxHashSet::default();
    while let Some((x, y)) = work.pop() {
        if x == y {
            continue;
        }
        if !visited.insert((x.0, y.0)) {
            continue;
        }
        // The ROOT pair is free: a shallowly-distinct pair costs no budget,
        // so wide composites of distinct arms never trip it. Only DESCENT
        // work (every pair beyond the first) charges.
        if visited.len() > 1 {
            if *budget == 0 {
                evidence.incomplete = true;
                return StructuralIdentity::Incomplete;
            }
            *budget -= 1;
        }
        let (Some(dx), Some(dy)) = (graph.node_data(x), graph.node_data(y)) else {
            evidence.incomplete = true;
            return StructuralIdentity::Incomplete;
        };
        evidence.record_file_root(graph, x);
        evidence.record_file_root(graph, y);
        // Fast path: payload-equal (child ids included) — identical subtrees
        // by arena content identity, no descent needed.
        if dx == dy {
            continue;
        }
        if !compare_shallow(&dx, &dy, &mut work) {
            return StructuralIdentity::Distinct;
        }
    }
    StructuralIdentity::Equal
}

/// One shallow layer of the comparator: compare every non-child field per
/// `SemanticNodeData`'s manual equality rules and push child-id pairs for
/// structural descent. Returns `false` for a shallow mismatch (⇒ Distinct).
///
/// EXHAUSTIVE over the variant matrix — no `_` catch-all on the same-variant
/// arms — so a new `SemanticNodeData` variant fails to compile here until
/// its identity rule is written down. Deliberate conservative refinements
/// (all fail toward `Distinct`, which only preserves both arms):
///
/// * `ObjectSpreadProgram` / `DeferredCallable`: opaque program payloads —
///   payload-unequal pairs are treated `Distinct` rather than descended.
/// * `Union` / `Intersection` / `MergedDecl` children compare elementwise in
///   order: `MergedDecl` order is semantic; canonical composites are
///   deterministically ordered, and a permuted-but-equal pair from a
///   pre-canonical producer stays `Distinct` (both arms kept).
fn compare_shallow(
    dx: &SemanticNodeData,
    dy: &SemanticNodeData,
    work: &mut Vec<(SemanticNodeId, SemanticNodeId)>,
) -> bool {
    use SemanticNodeData as D;
    if dx.discriminant_index() != dy.discriminant_index() {
        return false;
    }
    // Option-pair helper: both absent is fine, both present descends,
    // mixed presence is a mismatch.
    fn push_opt(
        work: &mut Vec<(SemanticNodeId, SemanticNodeId)>,
        a: Option<SemanticNodeId>,
        b: Option<SemanticNodeId>,
    ) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                work.push((a, b));
                true
            }
            _ => false,
        }
    }
    match (dx, dy) {
        (D::Alias(a), D::Alias(b)) => {
            work.push((*a, *b));
            true
        }
        (D::Object(sa), D::Object(sb)) => {
            // `entries` is the primary stored surface (the kind-specific
            // collections are derived indexes of it); `keyspace` is the one
            // additional child slot.
            if sa.entries.len() != sb.entries.len() {
                return false;
            }
            if !push_opt(work, sa.keyspace, sb.keyspace) {
                return false;
            }
            for (ea, eb) in sa.entries.iter().zip(sb.entries.iter()) {
                match (ea, eb) {
                    (SurfaceEntry::Member(ma), SurfaceEntry::Member(mb)) => {
                        if ma.optional != mb.optional
                            || ma.readonly != mb.readonly
                            || ma.method_kind != mb.method_kind
                            || ma.has_implementation_body != mb.has_implementation_body
                            || ma.visibility != mb.visibility
                            || ma.spans != mb.spans
                            || ma.declaration_origin != mb.declaration_origin
                            || ma.declared_in_macro_type_arg != mb.declared_in_macro_type_arg
                            || ma.merge_role != mb.merge_role
                            || ma.excess_origin != mb.excess_origin
                        {
                            return false;
                        }
                        match (
                            authored_property_key_child(&ma.key),
                            authored_property_key_child(&mb.key),
                        ) {
                            (Some(ka), Some(kb)) => work.push((ka, kb)),
                            (None, None) => {
                                if ma.key != mb.key {
                                    return false;
                                }
                            }
                            _ => return false,
                        }
                        work.push((ma.value, mb.value));
                    }
                    (SurfaceEntry::CallSignature(a), SurfaceEntry::CallSignature(b))
                    | (SurfaceEntry::ConstructSignature(a), SurfaceEntry::ConstructSignature(b)) => {
                        work.push((*a, *b));
                    }
                    (SurfaceEntry::IndexSignature(ia), SurfaceEntry::IndexSignature(ib)) => {
                        if ia.readonly != ib.readonly
                            || ia.spans != ib.spans
                            || ia.declaration_origin != ib.declaration_origin
                        {
                            return false;
                        }
                        work.push((ia.key_type, ib.key_type));
                        work.push((ia.value_type, ib.value_type));
                    }
                    _ => return false,
                }
            }
            true
        }
        // Opaque construction-program payload: conservative. The payload-Eq
        // fast path already admitted identical programs; an unequal pair
        // stays Distinct (both arms kept) rather than descending a program.
        (D::ObjectSpreadProgram(_), D::ObjectSpreadProgram(_)) => false,
        (D::Union(a), D::Union(b)) | (D::Intersection(a), D::Intersection(b)) => {
            if a.len() != b.len() {
                return false;
            }
            for (ca, cb) in a.iter().zip(b.iter()) {
                work.push((*ca, *cb));
            }
            true
        }
        // Childless payloads: payload equality was the fast path; an unequal
        // pair is Distinct.
        (D::Primitive(_), D::Primitive(_))
        | (D::Literal(_), D::Literal(_))
        | (D::Opaque(_), D::Opaque(_))
        | (D::RawFallback { .. }, D::RawFallback { .. }) => false,
        (
            D::Array {
                element: ea,
                readonly: ra,
            },
            D::Array {
                element: eb,
                readonly: rb,
            },
        ) => {
            if ra != rb {
                return false;
            }
            work.push((*ea, *eb));
            true
        }
        (
            D::Tuple {
                elements: ta,
                readonly: ra,
            },
            D::Tuple {
                elements: tb,
                readonly: rb,
            },
        ) => {
            if ra != rb || ta.len() != tb.len() {
                return false;
            }
            for (ea, eb) in ta.iter().zip(tb.iter()) {
                if ea.label != eb.label || ea.optional != eb.optional || ea.rest != eb.rest {
                    return false;
                }
                work.push((ea.value, eb.value));
            }
            true
        }
        (
            D::TemplateLiteral {
                quasis: qa,
                expressions: xa,
            },
            D::TemplateLiteral {
                quasis: qb,
                expressions: xb,
            },
        ) => {
            if qa != qb || xa.len() != xb.len() {
                return false;
            }
            for (ea, eb) in xa.iter().zip(xb.iter()) {
                work.push((*ea, *eb));
            }
            true
        }
        (D::KeyOf { base: a }, D::KeyOf { base: b }) => {
            work.push((*a, *b));
            true
        }
        (
            D::IndexedAccess {
                object: oa,
                index: ia,
            },
            D::IndexedAccess {
                object: ob,
                index: ib,
            },
        ) => {
            match (
                authored_property_key_child(ia),
                authored_property_key_child(ib),
            ) {
                (Some(ka), Some(kb)) => work.push((ka, kb)),
                (None, None) => {
                    if ia != ib {
                        return false;
                    }
                }
                _ => return false,
            }
            work.push((*oa, *ob));
            true
        }
        (
            D::Mapped {
                source: sa,
                mapper: ma,
            },
            D::Mapped {
                source: sb,
                mapper: mb,
            },
        ) => {
            if ma.optionality != mb.optionality || ma.readonly != mb.readonly || ma.kind != mb.kind
            {
                return false;
            }
            if !push_opt(work, ma.name_remap, mb.name_remap) {
                return false;
            }
            work.push((*sa, *sb));
            work.push((ma.parameter_node, mb.parameter_node));
            work.push((ma.key_space, mb.key_space));
            work.push((ma.value_expr, mb.value_expr));
            true
        }
        // The three opaque head-carriers: head fields are semantic identity
        // (value root / bare-ref name AND PAYLOAD scope / import specifier
        // stay identity per the ruling); structural type args descend.
        (a @ D::TypeOf(_), b @ D::TypeOf(_)) => {
            if a.typeof_head() != b.typeof_head() {
                return false;
            }
            let (xa, xb) = (a.carrier_type_args(), b.carrier_type_args());
            if xa.len() != xb.len() {
                return false;
            }
            for (ca, cb) in xa.iter().zip(xb.iter()) {
                work.push((*ca, *cb));
            }
            true
        }
        (a @ D::BareRef(_), b @ D::BareRef(_)) => {
            if a.bare_ref_head() != b.bare_ref_head() {
                return false;
            }
            let (xa, xb) = (a.carrier_type_args(), b.carrier_type_args());
            if xa.len() != xb.len() {
                return false;
            }
            for (ca, cb) in xa.iter().zip(xb.iter()) {
                work.push((*ca, *cb));
            }
            true
        }
        (a @ D::ImportType(_), b @ D::ImportType(_)) => {
            if a.import_type_head() != b.import_type_head() {
                return false;
            }
            let (xa, xb) = (a.carrier_type_args(), b.carrier_type_args());
            if xa.len() != xb.len() {
                return false;
            }
            for (ca, cb) in xa.iter().zip(xb.iter()) {
                work.push((*ca, *cb));
            }
            true
        }
        (
            D::TypeParam {
                decl: da,
                param_index: pa,
                constraint: ca,
                default: fa,
                // Excluded from identity, mirroring the manual Eq (F11).
                display_name: _,
            },
            D::TypeParam {
                decl: db,
                param_index: pb,
                constraint: cb,
                default: fb,
                display_name: _,
            },
        ) => {
            if da != db || pa != pb {
                return false;
            }
            push_opt(work, *ca, *cb) && push_opt(work, *fa, *fb)
        }
        (
            D::Infer {
                name: na,
                binder: ba,
            },
            D::Infer {
                name: nb,
                binder: bb,
            },
        )
        | (
            D::InferRef {
                name: na,
                binder: ba,
            },
            D::InferRef {
                name: nb,
                binder: bb,
            },
        ) => na == nb && ba == bb,
        (
            D::Conditional {
                check: ca,
                extends: ea,
                true_branch_ref: ta,
                false_branch_ref: fa,
                distributive: da,
            },
            D::Conditional {
                check: cb,
                extends: eb,
                true_branch_ref: tb,
                false_branch_ref: fb,
                distributive: db,
            },
        ) => {
            if da != db {
                return false;
            }
            work.push((*ca, *cb));
            work.push((*ea, *eb));
            work.push((*ta, *tb));
            work.push((*fa, *fb));
            true
        }
        (
            D::Signature {
                kind: ka,
                params: pa,
                return_type: ra,
                type_parameters: ta,
                occurrence: oa,
                return_carrier: ca,
                signature_span: sa,
                return_type_span: rsa,
            },
            D::Signature {
                kind: kb,
                params: pb,
                return_type: rb,
                type_parameters: tb,
                occurrence: ob,
                return_carrier: cb,
                signature_span: sb,
                return_type_span: rsb,
            },
        ) => {
            // Spans, occurrence and the return-carrier discriminant all
            // participate in identity (provenance-aware interning).
            if ka != kb
                || oa != ob
                || sa != sb
                || rsa != rsb
                || pa.len() != pb.len()
                || ta.len() != tb.len()
            {
                return false;
            }
            match (ca, cb) {
                (SignatureReturnCarrier::Declared(a), SignatureReturnCarrier::Declared(b)) => {
                    work.push((*a, *b));
                }
                (SignatureReturnCarrier::Function(a), SignatureReturnCarrier::Function(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            for (fa, fb) in pa.iter().zip(pb.iter()) {
                if fa.name != fb.name
                    || fa.optional != fb.optional
                    || fa.rest != fb.rest
                    || fa.span != fb.span
                {
                    return false;
                }
                work.push((fa.ty, fb.ty));
            }
            for (da, db) in ta.iter().zip(tb.iter()) {
                if da.name != db.name || da.is_const != db.is_const {
                    return false;
                }
                work.push((da.param, db.param));
                if !push_opt(work, da.constraint, db.constraint)
                    || !push_opt(work, da.default, db.default)
                {
                    return false;
                }
            }
            work.push((*ra, *rb));
            true
        }
        // Opaque callable-composition payload: conservative Distinct (the
        // payload-Eq fast path already admitted identical pairs).
        (D::DeferredCallable(_), D::DeferredCallable(_)) => false,
        (D::DeclRef { identity: a }, D::DeclRef { identity: b }) => a == b,
        (
            D::InstantiationRef { base: ba, args: aa },
            D::InstantiationRef { base: bb, args: ab },
        ) => {
            if ba != bb || aa.len() != ab.len() {
                return false;
            }
            for (ca, cb) in aa.iter().zip(ab.iter()) {
                work.push((*ca, *cb));
            }
            true
        }
        (D::MergedDecl { contributors: a }, D::MergedDecl { contributors: b }) => {
            // Contributor order is semantic (source-order overload
            // accumulation) — elementwise, never sorted.
            if a.len() != b.len() {
                return false;
            }
            for (ca, cb) in a.iter().zip(b.iter()) {
                work.push((*ca, *cb));
            }
            true
        }
        (
            D::SyntheticBinding {
                id: ia,
                value_node: va,
            },
            D::SyntheticBinding {
                id: ib,
                value_node: vb,
            },
        ) => {
            if ia != ib {
                return false;
            }
            // The value ordinal is a genuine graph child (mirrors the manual
            // Eq, which compares it) — descend structurally.
            work.push((SemanticNodeId(*va), SemanticNodeId(*vb)));
            true
        }
        // Cross-variant pairs are unreachable (discriminant guard above);
        // this arm keeps the match total without weakening the same-variant
        // arms' exhaustiveness.
        _ => false,
    }
}
