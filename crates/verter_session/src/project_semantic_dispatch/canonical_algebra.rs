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
//!    whose structure the identity walk inspected — INCLUDING the
//!    TRANSITIVE structure of discarded duplicates (a payload-equal discard
//!    is an identity decision resting on the shared children) and
//!    descendants reached through `Global` intermediates — is recorded as a
//!    `(canonical, observed_whole_hash)` self-root, and an incomplete,
//!    budget-tripped, or peek-undecided comparison marks the evidence
//!    `incomplete` (ReturnOnly, never warm).
//!
//! Evidence disposition is SINGLE: every construction site routes its
//! [`CanonicalEvidence`] to
//! `ProjectSemanticDispatch::deposit_canonical_evidence` — either through
//! the dispatch-level funnel
//! `ProjectSemanticDispatch::intern_normalized_union_or_intersection`
//! (`build.rs`) or by threading the evidence up to the nearest
//! dispatch-holding caller. Under an active cold-build frame the roots join
//! the frame's self-root set and `incomplete` folds `cache_suppress`;
//! without a frame the roots are subsumed by the site's own fact-railed
//! read set while `incomplete` marks the REQUEST result partial, so the
//! enclosing publication refuses warm promotion. The one exception is a
//! site whose evidence is PROVABLY empty (all arms freshly-interned
//! `Global` childless nodes), which asserts that instead of threading.
//!
//! Memoization discipline: no identity decision outlives one
//! canonicalization. Wide arm sets narrow pairwise candidates with fresh
//! depth-0 `structural_hash_of` fingerprints (a hash match is always
//! confirmed by the exact cycle-safe comparator, never deduplicated on the
//! hash alone). No child result is ever spliced into another traversal,
//! and no store-level or global fingerprint/representative cache exists
//! here.

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
    /// including DISCARDED duplicates (their transitive structure too — a
    /// payload-equal discard is an identity decision resting on the shared
    /// child structure) and descendants reached through `Global`
    /// intermediates. An edit to any inspected file must miss the warm read
    /// even when the node it invalidates no longer appears in the canonical
    /// result. Deterministic first-inspection order.
    pub(crate) inspected_file_roots: Vec<ObservedGraphSelfRoot>,
    /// `true` when any structural comparison returned
    /// [`StructuralIdentity::Incomplete`], a bounded scan was skipped for
    /// size, or a bounded alias peek could not decide: the result is correct
    /// but not proven canonical, so it is ReturnOnly — never a warm
    /// canonical result.
    pub(crate) incomplete: bool,
    /// Nodes already scope-classified — one sidecar lookup per unique node
    /// per canonicalization, regardless of how many walks revisit it.
    seen_nodes: FxHashSet<SemanticNodeId>,
}

impl CanonicalEvidence {
    /// Fold another canonicalization's evidence into this one — the
    /// threading primitive for helpers that run several canonical
    /// constructions before their caller reaches the single disposition
    /// point (`ProjectSemanticDispatch::deposit_canonical_evidence`).
    pub(crate) fn absorb(&mut self, other: CanonicalEvidence) {
        self.incomplete |= other.incomplete;
        for root in other.inspected_file_roots {
            if !self
                .inspected_file_roots
                .iter()
                .any(|(c, h)| *c == root.0 && *h == root.1)
            {
                self.inspected_file_roots.push(root);
            }
        }
        self.seen_nodes.extend(other.seen_nodes);
    }

    fn record_file_root(&mut self, graph: &SemanticGraphStore, node: SemanticNodeId) {
        if !self.seen_nodes.insert(node) {
            return;
        }
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

    /// Root the TRANSITIVE structure of a DISCARDED duplicate arm. A
    /// payload-equal discard (tier 1, or the comparator's payload-equal
    /// fast path) asserts structural identity through the shared children
    /// WITHOUT descending, so the children's file scopes must still enter
    /// the cache evidence — an edit to a descendant's file misses the warm
    /// read. Bounded (shares the visited set with the rest of the walk);
    /// a truncated walk marks the evidence incomplete rather than serving
    /// a warm result on a possibly-narrow root set.
    fn record_subtree_roots(&mut self, graph: &SemanticGraphStore, root: SemanticNodeId) {
        const SUBTREE_ROOT_WALK_CAP: usize = 4096;
        let mut stack: Vec<SemanticNodeId> = vec![root];
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut children: Vec<SemanticNodeId> = Vec::new();
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            if visited.len() > SUBTREE_ROOT_WALK_CAP {
                self.incomplete = true;
                return;
            }
            self.record_file_root(graph, node);
            let Some(data) = graph.node_data(node) else {
                self.incomplete = true;
                continue;
            };
            children.clear();
            if !push_child_ids(&data, &mut children) {
                // The payload's children are not enumerable from here (a
                // sealed composition payload). Its subtree therefore never
                // enters the root set, so the evidence must NOT claim a
                // complete root walk — the discarded arm is served
                // `ReturnOnly` rather than warm on a possibly-narrow root
                // set. Honouring this signal is the whole point of the
                // return value; discarding it is the silent-under-rooting
                // class this walk exists to close.
                self.incomplete = true;
            }
            stack.extend(children.iter().copied());
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

/// Minimum child-bearing arm count before the pairwise (tier-2) dedup
/// narrows candidates by structural prehash. Below it, direct pairwise
/// comparison is cheaper than hashing every arm; above it, hash-distinct
/// arms skip the pairwise tier entirely, so a wide union of same-shaped
/// but structurally distinct arms stays within the work budget instead of
/// being permanently denied warm admission.
const STRUCTURAL_PREHASH_MIN_ARMS: usize = 8;

/// Push every semantic child id of `data` onto `out`, mirroring the
/// comparator's descent topology (the full manual-`Eq` field set — the
/// possibly-diverged `Object` kind-specific collections included). Returns
/// `false` when the payload's children are NOT enumerable from here (the
/// sealed [`DeferredCallable`](SemanticNodeData::DeferredCallable)
/// composition payload) — the caller must treat the walk as incomplete.
/// EXHAUSTIVE (no wildcard): a new variant fails to compile here until its
/// child topology is classified.
#[must_use = "a `false` return means the walk did NOT enumerate this \
              payload's children — the caller MUST mark its evidence \
              incomplete, or the unwalked subtree is silently unrooted \
              and a stale warm read is served"]
fn push_child_ids(data: &SemanticNodeData, out: &mut Vec<SemanticNodeId>) -> bool {
    use SemanticNodeData as D;
    match data {
        D::Primitive(_)
        | D::Literal(_)
        | D::Opaque(_)
        | D::RawFallback { .. }
        | D::Infer { .. }
        | D::InferRef { .. }
        | D::DeclRef { .. } => true,
        D::Alias(inner) | D::KeyOf { base: inner } => {
            out.push(*inner);
            true
        }
        D::Object(view) => {
            for entry in view.entries.iter() {
                match entry {
                    SurfaceEntry::Member(member) => {
                        out.extend(authored_property_key_child(&member.key));
                        out.push(member.value);
                    }
                    SurfaceEntry::CallSignature(node) | SurfaceEntry::ConstructSignature(node) => {
                        out.push(*node);
                    }
                    SurfaceEntry::IndexSignature(signature) => {
                        out.push(signature.key_type);
                        out.push(signature.value_type);
                    }
                }
            }
            for member in view.positive_members() {
                out.extend(authored_property_key_child(&member.key));
                out.push(member.value);
            }
            out.extend(view.call_signatures.iter().copied());
            out.extend(view.construct_signatures.iter().copied());
            for signature in view.index_signatures.iter() {
                out.push(signature.key_type);
                out.push(signature.value_type);
            }
            out.extend(view.keyspace);
            true
        }
        D::ObjectSpreadProgram(program) => {
            out.extend(program.child_nodes());
            true
        }
        D::Union(members)
        | D::Intersection(members)
        | D::MergedDecl {
            contributors: members,
        } => {
            out.extend(members.iter().copied());
            true
        }
        D::Array { element, .. } => {
            out.push(*element);
            true
        }
        D::Tuple { elements, .. } => {
            out.extend(elements.iter().map(|element| element.value));
            true
        }
        D::TemplateLiteral { expressions, .. } => {
            out.extend(expressions.iter().copied());
            true
        }
        D::IndexedAccess { object, index } => {
            out.push(*object);
            out.extend(authored_property_key_child(index));
            true
        }
        D::Mapped { source, mapper } => {
            out.push(*source);
            out.push(mapper.parameter_node);
            out.push(mapper.key_space);
            out.push(mapper.value_expr);
            out.extend(mapper.name_remap);
            true
        }
        carrier @ (D::TypeOf(_) | D::BareRef(_) | D::ImportType(_)) => {
            out.extend(carrier.carrier_type_args().iter().copied());
            true
        }
        D::TypeParam {
            constraint,
            default,
            ..
        } => {
            out.extend(*constraint);
            out.extend(*default);
            true
        }
        D::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => {
            out.extend([*check, *extends, *true_branch_ref, *false_branch_ref]);
            true
        }
        D::Signature {
            params,
            return_type,
            type_parameters,
            return_carrier,
            ..
        } => {
            out.extend(params.iter().map(|param| param.ty));
            out.push(*return_type);
            if let SignatureReturnCarrier::Declared(node) = return_carrier {
                out.push(*node);
            }
            for decl in type_parameters.iter() {
                out.push(decl.param);
                out.extend(decl.constraint);
                out.extend(decl.default);
            }
            true
        }
        // Sealed composition payload — its parts are readable only by a
        // consumer witness, so its children cannot be enumerated here.
        D::DeferredCallable(_) => false,
        D::InstantiationRef { args, .. } => {
            out.extend(args.iter().copied());
            true
        }
        D::SyntheticBinding { value_node, .. } => {
            out.push(SemanticNodeId(*value_node));
            true
        }
    }
}

/// TypeScript numeric-literal identity for two f64 payloads — SameValueZero,
/// the keying the checker's literal-type interning uses: `0` and `-0` are
/// ONE literal type, `NaN` equals `NaN`, and every other pair compares by
/// value. Returns `true` only when the two payloads are PROVABLY distinct
/// literal types (a proven-empty intersection); `false` preserves the
/// undecided/equal pair.
pub(crate) fn numeric_literal_values_disjoint(a: f64, b: f64) -> bool {
    let same_value_zero = a == b || (a.is_nan() && b.is_nan());
    !same_value_zero
}

/// Canonical union construction over `members`.
pub(crate) fn canonical_union(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
) -> CanonicalComposite {
    canonicalize(graph, members, /* is_union */ true)
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

    // 1a. Singleton normalization returns its retained member UNCHANGED —
    //     with that member's own scope — BEFORE the absorbers run: a
    //     singleton file-scoped `any` / `never` / `unknown` (or an alias to
    //     one) is the member itself, never the distinct Global primitive a
    //     lattice fold would intern (the ruling admits no singleton
    //     exception for absorbers, and aliases are never inlined).
    if let [only] = flat.as_slice() {
        return CanonicalComposite {
            node: *only,
            evidence,
        };
    }

    // 2. §22 lattice absorption over the flattened arms. The peeks follow
    //    transparent Alias redirects, so an aliased extreme absorbs too.
    let mut specials: Vec<Option<SpecialKind>> = Vec::with_capacity(flat.len());
    let mut error_node: Option<SemanticNodeId> = None;
    let mut has_any = false;
    let mut has_unknown = false;
    let mut has_never = false;
    for &m in &flat {
        let special = peek_special_via_graph(graph, m, Some(&mut evidence));
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
    //    different scopes). Wide arm sets first narrow candidates by
    //    structural prehash (`structural_hash_of`, sanctioned for candidate
    //    narrowing only — a hash MATCH is always confirmed by the exact
    //    cycle-safe comparator, never deduplicated on the hash alone), so
    //    hash-distinct arms skip the pairwise tier. First occurrence
    //    survives; a discarded duplicate's TRANSITIVE structure roots stay
    //    in the evidence (the identity decision rests on the shared
    //    children); an `Incomplete` comparison (or an over-cap arm set)
    //    keeps every arm and taints the evidence.
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
                // Content-identical cross-scope duplicate — discarded. The
                // payload-equality claim rests on the SHARED CHILD ids, so
                // the discarded arm's transitive structure roots enter the
                // evidence even though tier 1 never descends.
                evidence.record_subtree_roots(graph, m);
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
        // Candidate narrowing for wide arm sets: bucket by structural
        // prehash so the pairwise tier runs only within equal-hash groups.
        // Hash-distinct arms are treated as candidates-for-nothing (both
        // kept — never a collapse), which keeps a wide union of distinct
        // same-shaped arms off the shared budget entirely. The prehash is
        // deliberately skipped for small sets, where hashing every arm
        // costs more than the direct pairwise walk. Known bounded
        // divergence: the prehash includes `TypeParam.display_name`, which
        // canonical identity excludes — a pair equal-except-display-name
        // would narrow into different groups and stay two arms (a missed
        // dedup, never a wrong collapse); `display_name` is derived from
        // the declaration identity the hash also covers, so the pair
        // cannot arise from one coherent generation.
        let prehash_groups: Option<FxHashMap<crate::types::Hash16, Vec<usize>>> =
            if child_bearing.len() > STRUCTURAL_PREHASH_MIN_ARMS {
                let mut groups: FxHashMap<crate::types::Hash16, Vec<usize>> = FxHashMap::default();
                for (position, (_, data)) in child_bearing.iter().enumerate() {
                    let hash =
                        crate::component_meta_audit::footprint_structural_hash::structural_hash_of(
                            graph, data,
                        );
                    groups.entry(hash).or_default().push(position);
                }
                Some(groups)
            } else {
                None
            };
        let mut budget = COMPARE_WORK_BUDGET;
        let mut discarded: Vec<usize> = Vec::new();
        let compare_group = |group: &[usize],
                             evidence: &mut CanonicalEvidence,
                             discarded: &mut Vec<usize>,
                             budget: &mut u32| {
            for i in 1..group.len() {
                let (index_m, _) = child_bearing[group[i]];
                for &earlier in &group[..i] {
                    let (index_k, _) = child_bearing[earlier];
                    if discarded.contains(&index_k) {
                        continue;
                    }
                    match compare_structural(graph, kept[index_m], kept[index_k], evidence, budget)
                    {
                        StructuralIdentity::Equal => {
                            // The comparator's payload-equal fast path can
                            // skip shared subtrees — root the DISCARDED
                            // arm's full structure explicitly.
                            evidence.record_subtree_roots(graph, kept[index_m]);
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
        };
        match &prehash_groups {
            Some(groups) => {
                for group in groups.values() {
                    if group.len() > 1 {
                        compare_group(group, &mut evidence, &mut discarded, &mut budget);
                    }
                }
            }
            None => {
                let all: Vec<usize> = (0..child_bearing.len()).collect();
                compare_group(&all, &mut evidence, &mut discarded, &mut budget);
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
/// redirects (bounded), records inspected file roots on `evidence` when the
/// caller makes a CANONICAL claim (`Some`), and returns the kind plus the
/// RESOLVED node id (so an `error` operand's carrier is reused verbatim,
/// preserving its `QueryError` payload and node identity).
///
/// A `None` return is a PROVEN non-special only when the peek terminated on
/// a resolvable non-special payload. An exhausted hop bound or a dangling
/// alias target is UNDECIDED: with evidence attached it marks the
/// canonicalization incomplete (a deeply-aliased extreme must never escape
/// absorption into a warm canonical result); evidence-free callers use the
/// peek as a fast-reject only and make no canonical claim.
pub(super) fn peek_special_via_graph(
    graph: &SemanticGraphStore,
    id: SemanticNodeId,
    mut evidence: Option<&mut CanonicalEvidence>,
) -> Option<(SpecialKind, SemanticNodeId)> {
    let mut cur = id;
    // bounded-loop: ALIAS_PEEK_HOPS transparent Alias redirects.
    for _ in 0..ALIAS_PEEK_HOPS {
        if let Some(evidence) = evidence.as_deref_mut() {
            evidence.record_file_root(graph, cur);
        }
        let Some(data) = graph.node_data(cur) else {
            // Dangling target — undecided, never a proven non-special.
            if let Some(evidence) = evidence {
                evidence.incomplete = true;
            }
            return None;
        };
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
    // Hop bound exhausted — undecided, never a proven non-special.
    if let Some(evidence) = evidence {
        evidence.incomplete = true;
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
        // Literal identity is TS literal identity, not payload bit identity:
        // numbers compare SameValueZero (`0` and `-0` are ONE literal type —
        // the intern boundary normalizes `-0.0`, and a stray unnormalized
        // payload must still never PROVE disjointness).
        (
            SemanticNodeData::Literal(LiteralValue::Number(x)),
            SemanticNodeData::Literal(LiteralValue::Number(y)),
        ) => numeric_literal_values_disjoint(*x, *y),
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
                    // TS literal identity: numeric pairs compare
                    // SameValueZero (`0` / `-0` are one literal type).
                    let provably_distinct = match (prev, value) {
                        (LiteralValue::Number(x), LiteralValue::Number(y)) => {
                            numeric_literal_values_disjoint(*x, *y)
                        }
                        _ => prev != value,
                    };
                    if provably_distinct {
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

/// Shallow identity of one [`SurfaceEntry::Member`] / positive-member pair:
/// every non-child `SurfaceMember` field compares per the manual `Eq`
/// (12/12) and the key/value children descend structurally.
fn compare_surface_member(
    ma: &crate::semantic_query::SurfaceMember,
    mb: &crate::semantic_query::SurfaceMember,
    work: &mut Vec<(SemanticNodeId, SemanticNodeId)>,
) -> bool {
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
    true
}

/// Shallow identity of one [`crate::semantic_query::IndexSignature`] pair.
fn compare_index_signature(
    ia: &crate::semantic_query::IndexSignature,
    ib: &crate::semantic_query::IndexSignature,
    work: &mut Vec<(SemanticNodeId, SemanticNodeId)>,
) -> bool {
    if ia.readonly != ib.readonly
        || ia.spans != ib.spans
        || ia.declaration_origin != ib.declaration_origin
    {
        return false;
    }
    work.push((ia.key_type, ib.key_type));
    work.push((ia.value_type, ib.value_type));
    true
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
            // The FULL manual-`Eq` field set participates: `entries`,
            // `keyspace`, AND the kind-specific collections (`members`,
            // `call_signatures`, `construct_signatures`, `index_signatures`,
            // `has_index_signature`). The collections are usually derived
            // indexes of `entries`, but production CAN diverge them —
            // `call_shape_transform` rebuilds a surface via
            // `with_positive_members`, transforming `members` while
            // `entries` keeps the originals — and two nodes the arena
            // interns as DISTINCT must never compare `Equal` here.
            if sa.entries.len() != sb.entries.len()
                || sa.positive_members().len() != sb.positive_members().len()
                || sa.call_signatures.len() != sb.call_signatures.len()
                || sa.construct_signatures.len() != sb.construct_signatures.len()
                || sa.index_signatures.len() != sb.index_signatures.len()
                || sa.closed().has_index_signature() != sb.closed().has_index_signature()
            {
                return false;
            }
            if !push_opt(work, sa.keyspace, sb.keyspace) {
                return false;
            }
            for (ea, eb) in sa.entries.iter().zip(sb.entries.iter()) {
                match (ea, eb) {
                    (SurfaceEntry::Member(ma), SurfaceEntry::Member(mb)) => {
                        if !compare_surface_member(ma, mb, work) {
                            return false;
                        }
                    }
                    (SurfaceEntry::CallSignature(a), SurfaceEntry::CallSignature(b))
                    | (SurfaceEntry::ConstructSignature(a), SurfaceEntry::ConstructSignature(b)) => {
                        work.push((*a, *b));
                    }
                    (SurfaceEntry::IndexSignature(ia), SurfaceEntry::IndexSignature(ib)) => {
                        if !compare_index_signature(ia, ib, work) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            for (ma, mb) in sa.positive_members().iter().zip(sb.positive_members()) {
                if !compare_surface_member(ma, mb, work) {
                    return false;
                }
            }
            for (a, b) in sa.call_signatures.iter().zip(sb.call_signatures.iter()) {
                work.push((*a, *b));
            }
            for (a, b) in sa
                .construct_signatures
                .iter()
                .zip(sb.construct_signatures.iter())
            {
                work.push((*a, *b));
            }
            for (ia, ib) in sa.index_signatures.iter().zip(sb.index_signatures.iter()) {
                if !compare_index_signature(ia, ib, work) {
                    return false;
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
