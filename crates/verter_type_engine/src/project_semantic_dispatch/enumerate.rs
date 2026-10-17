//! `key_names_from_base_node` / `key_names_from_keyspace_node` — TS keyof
//! enumeration helpers.
//!
//! Shared builders walk the base node's [`SemanticNodeData`] shape and
//! return the concrete member names when enumeration succeeds, or `None`
//! when the base is still open (deferred shell). Unresolvable cases
//! surface to the caller which produces a canonical `Mapped` / `KeyOf`
//! deferred shell.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    HashValue, IndexKey, LiteralValue, PrimitiveKind, PropertyKey, SemanticNodeData, SemanticNodeId,
};
use verter_session_query::facts::registry::{FactKey, InternedName, SymbolSpace};

/// One enumerated key-domain member: the canonical published property
/// NAME plus the literal VALUE substituted for the mapper binder `K`.
///
/// `name` is the JS property-name string (`js_number_to_string` for
/// numeric literals); `literal` preserves the source literal KIND so
/// K-dependent mapped values and `as` remaps substitute `1`, never
/// `"1"` (pinned tsgo, probe12: `{ [K in 1]: K }` = `{ 1: 1 }`).
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct KeyDomainKey {
    pub(super) key: PropertyKey,
    /// Literal binder value for string/numeric keys. A unique-symbol key has
    /// no primitive literal representation; consumers must resolve its nominal
    /// value identity or explicitly defer.
    pub(super) literal: Option<LiteralValue>,
}

impl KeyDomainKey {
    /// Lift exact known surface keys into mapper key-domain entries.
    pub(super) fn from_keys(keys: Vec<PropertyKey>) -> Vec<KeyDomainKey> {
        keys.into_iter()
            .map(|key| KeyDomainKey {
                literal: match &key {
                    PropertyKey::String(name) => {
                        Some(LiteralValue::String(name.as_ref().to_string()))
                    }
                    PropertyKey::Number(number) => Some(LiteralValue::Number(number.get() as f64)),
                    PropertyKey::UniqueSymbol(_) => None,
                },
                key,
            })
            .collect()
    }
}

/// A key domain can contain literal keys, primitive key kinds, or both.
/// Only a domain with no primitive kinds is finite enough to enumerate.
#[derive(Clone, Default)]
pub(crate) struct KeySpaceDomain {
    keys: Vec<KeyDomainKey>,
    seen: FxHashSet<KeyDomainKey>,
    members: FxHashSet<PropertyKey>,
    broad: FxHashSet<PrimitiveKind>,
}

impl KeySpaceDomain {
    fn finite(keys: Vec<KeyDomainKey>) -> Self {
        let mut domain = Self::default();
        for key in keys {
            if domain.seen.insert(key.clone()) {
                domain.members.insert(key.key.clone());
                domain.keys.push(key);
            }
        }
        domain
    }

    pub(super) fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.broad.is_empty()
    }
    pub(super) fn is_finite(&self) -> bool {
        self.broad.is_empty()
    }
    pub(super) fn membership(&self, key: &PropertyKey) -> Option<bool> {
        if self.members.contains(key) {
            return Some(true);
        }
        let kind = match key {
            PropertyKey::String(_) => PrimitiveKind::String,
            PropertyKey::Number(_) => PrimitiveKind::Number,
            PropertyKey::UniqueSymbol(_) => PrimitiveKind::Symbol,
        };
        if self.broad.contains(&kind) {
            Some(true)
        } else if self.broad.is_empty() {
            Some(false)
        } else {
            None
        }
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.keys.capacity() * std::mem::size_of::<KeyDomainKey>()
            + self.seen.capacity() * std::mem::size_of::<KeyDomainKey>()
            + self.members.capacity() * std::mem::size_of::<PropertyKey>()
            + self
                .keys
                .iter()
                .map(|key| match &key.literal {
                    Some(LiteralValue::String(name)) => name.len(),
                    _ => 0,
                })
                .sum::<usize>()
                * 2
            + self.broad.capacity() * std::mem::size_of::<PrimitiveKind>()
    }

    fn admits(&self, key: &KeyDomainKey) -> bool {
        let kind = match (&key.literal, &key.key) {
            (Some(LiteralValue::String(_)), _) => PrimitiveKind::String,
            (Some(LiteralValue::Number(_)), _) => PrimitiveKind::Number,
            (None, PropertyKey::UniqueSymbol(_)) => PrimitiveKind::Symbol,
            _ => return false,
        };
        self.broad.contains(&kind) || self.seen.contains(key)
    }

    fn union(mut self, other: Self) -> Self {
        self.broad.extend(other.broad);
        for key in other.keys {
            if self.seen.insert(key.clone()) {
                self.members.insert(key.key.clone());
                self.keys.push(key);
            }
        }
        self
    }

    fn intersection(self, other: Self) -> Self {
        let mut keys = self
            .keys
            .iter()
            .filter(|key| other.admits(key))
            .cloned()
            .collect::<Vec<_>>();
        keys.extend(other.keys.iter().filter(|key| self.admits(key)).cloned());
        let mut result = Self::finite(keys);
        result.broad = self.broad.intersection(&other.broad).copied().collect();
        result
    }
}

enum KeySpaceLeaf {
    Finite(Vec<KeyDomainKey>),
    Next(SemanticNodeId),
}

enum KeySpaceFrame {
    Enter(SemanticNodeId),
    Leave(SemanticNodeId),
    Combine {
        node: SemanticNodeId,
        arms: usize,
        union: bool,
    },
}

/// Worklist frame for the iterative `key_names_from_base_node`
/// driver. `Expand` advances one node; `Combine*` reduces the top N
/// prior results (one per arm) into the compound's key enumeration.
enum KeyNamesFrame {
    Expand(SemanticNodeId),
    CombineIntersection { arm_count: usize },
    CombineUnion { arm_count: usize },
}

/// One arm's key-name enumeration outcome inside
/// [`ProjectSemanticDispatch::key_names_from_base_node`]. Distinguishes
/// an open construction program from other unresolvable shapes so the
/// intersection combine can poison on the former (tsc keeps the key
/// domain of an intersection with an unresolved constituent open) while
/// keeping the pre-existing drop-and-accumulate rule for the latter.
enum KeyNamesArm {
    Names(Vec<PropertyKey>),
    /// Unresolvable for a reason OTHER than an open construction program
    /// (deferred shells, primitives, literals, TypeParams, projection
    /// failures): dropped from an intersection combine, poisons a union
    /// combine.
    Unresolvable,
    /// An open / multi-alternative construction program: its key domain
    /// is not closed, so an intersection containing it has no exact
    /// keyof — the arm poisons the intersection combine (and propagates
    /// upward as the poisoned verdict).
    OpenConstruction,
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Iterative `keyof` enumeration: a heap-backed worklist drives
    /// per-arm descent so deeply-nested Intersection / Union arm
    /// chains do not grow the Rust call stack.
    ///
    /// **Intersection accumulation contract.** The Intersection arm
    /// accumulates the union of keys across every **enumerable** arm
    /// and returns `None` only when every arm is unresolvable. An
    /// all-or-nothing `?` here would lose enumerable keys from `A`
    /// when `B` is unresolvable in `keyof (A & B)`.
    ///
    /// `pub(crate)` (not `pub(super)`) so the `keyof` reduction in
    /// [`crate::project_semantic_dispatch::build`] reuses this single
    /// shared `keyof`-level enumerator rather than forking a second
    /// member-name walker. Callers pass the `ResolveDecl` root node they
    /// already hold; this enumerator owns the declaration-placeholder
    /// unwrap + Intersection/Union accumulation so they do not
    /// re-implement either.
    pub fn key_names_from_base_node(&self, base: SemanticNodeId) -> Option<Vec<PropertyKey>> {
        let mut work: Vec<KeyNamesFrame> = Vec::new();
        let mut results: Vec<KeyNamesArm> = Vec::new();
        work.push(KeyNamesFrame::Expand(base));

        while let Some(frame) = work.pop() {
            match frame {
                KeyNamesFrame::Expand(id) => {
                    self.key_names_step(id, &mut work, &mut results);
                }
                KeyNamesFrame::CombineIntersection { arm_count } => {
                    // Accumulate enumerable arms; ignore unresolvable ones.
                    // Only return None when EVERY arm is unresolvable —
                    // EXCEPT an open construction program arm, which
                    // poisons the whole intersection: `keyof (A & B)`
                    // stays open with an unresolved constituent (tsc), so
                    // the enumerable subset is never exact.
                    let start = results.len().saturating_sub(arm_count);
                    let arm_results: Vec<_> = results.drain(start..).collect();
                    let mut names: Vec<PropertyKey> = Vec::new();
                    let mut seen: FxHashSet<PropertyKey> = FxHashSet::default();
                    let mut any_enumerable = false;
                    let mut open_construction = false;
                    for arm in arm_results {
                        match arm {
                            KeyNamesArm::Names(arm_names) => {
                                any_enumerable = true;
                                for name in arm_names {
                                    if seen.insert(name.clone()) {
                                        names.push(name);
                                    }
                                }
                            }
                            KeyNamesArm::Unresolvable => {}
                            KeyNamesArm::OpenConstruction => open_construction = true,
                        }
                    }
                    results.push(if open_construction {
                        KeyNamesArm::OpenConstruction
                    } else if any_enumerable {
                        KeyNamesArm::Names(names)
                    } else {
                        KeyNamesArm::Unresolvable
                    });
                }
                KeyNamesFrame::CombineUnion { arm_count } => {
                    // Keyof (A | B) = common keys across ALL arms (intersection
                    // of enumerated sets). Unresolvable arm → whole union None.
                    // An OPEN CONSTRUCTION arm propagates its poison verdict:
                    // collapsing it to a plain unresolvable arm would let an
                    // enclosing intersection combine drop it and publish a
                    // false-exact key domain.
                    let start = results.len().saturating_sub(arm_count);
                    let arm_results: Vec<_> = results.drain(start..).collect();
                    let mut common: Option<FxHashSet<PropertyKey>> = None;
                    let mut unresolvable = false;
                    let mut open_construction = false;
                    for arm in arm_results {
                        match arm {
                            KeyNamesArm::Names(arm_names) => {
                                let arm_set: FxHashSet<PropertyKey> =
                                    arm_names.into_iter().collect();
                                common = Some(match common {
                                    Some(current) => current
                                        .intersection(&arm_set)
                                        .cloned()
                                        .collect::<FxHashSet<_>>(),
                                    None => arm_set,
                                });
                            }
                            KeyNamesArm::Unresolvable => {
                                unresolvable = true;
                                // Cannot early-break — must drain remaining
                                // results from the stack to keep `results`
                                // aligned for the next combine.
                            }
                            KeyNamesArm::OpenConstruction => {
                                open_construction = true;
                            }
                        }
                    }
                    let combined = if open_construction {
                        KeyNamesArm::OpenConstruction
                    } else if unresolvable {
                        KeyNamesArm::Unresolvable
                    } else {
                        let mut names: Vec<PropertyKey> =
                            common.unwrap_or_default().into_iter().collect();
                        names.sort_unstable();
                        KeyNamesArm::Names(names)
                    };
                    results.push(combined);
                }
            }
        }

        match results.pop() {
            Some(KeyNamesArm::Names(names)) => Some(names),
            _ => None,
        }
    }

    /// Expand one node worth of key-name enumeration. Pushes either a
    /// direct result (`Some(names)` / `None`) onto `results`, or child
    /// expansions + a combine frame onto `work`.
    fn key_names_step(
        &self,
        base: SemanticNodeId,
        work: &mut Vec<KeyNamesFrame>,
        results: &mut Vec<KeyNamesArm>,
    ) {
        let resolved = self.evaluate_deferred_semantic_node(base);
        let data = match self.graph().node_data(resolved) {
            Some(d) => d,
            None => {
                results.push(KeyNamesArm::Unresolvable);
                return;
            }
        };
        match data.as_ref() {
            SemanticNodeData::ObjectSpreadProgram(_) => {
                drop(data);
                let keys = match self.project_object_spread_for_consumer(
                    resolved,
                    crate::semantic_query::ObjectProjectionSelector::Surface,
                    crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
                ) {
                    crate::semantic_query::QueryResult::Value(formula) => {
                        match self.object_spread_exact_keyof(&formula) {
                            Ok(Some(keyof)) => KeyNamesArm::Names(keyof),
                            // An open / multi-alternative construction
                            // program has no exact key domain — poison
                            // the enclosing intersection combine.
                            Ok(None) => KeyNamesArm::OpenConstruction,
                            Err(_) => KeyNamesArm::Unresolvable,
                        }
                    }
                    crate::semantic_query::QueryResult::Recursive(_)
                    | crate::semantic_query::QueryResult::Error(_) => KeyNamesArm::Unresolvable,
                };
                results.push(keys);
            }
            SemanticNodeData::Object(surface) => {
                // `keyof ClassType` yields only public keys (TS semantics):
                // private/protected members are not part of the keyspace, so
                // mapped types (`{ [K in keyof T]: V }`, `Partial<T>`) and
                // `Pick`/`Omit` over a class never carry them. This is the
                // key-name-enumeration chokepoint; native_props reads the
                // surface directly and is unaffected.
                let names = surface
                    .positive_members()
                    .iter()
                    .filter(|member| member.visibility.is_public())
                    .filter_map(|member| member.key.cloned_known())
                    .collect();
                results.push(KeyNamesArm::Names(names));
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = arms.members_arc();
                drop(data);
                let n = arms.len();
                if n == 0 {
                    results.push(KeyNamesArm::Names(Vec::new()));
                    return;
                }
                work.push(KeyNamesFrame::CombineIntersection { arm_count: n });
                for arm in arms.iter().rev() {
                    work.push(KeyNamesFrame::Expand(*arm));
                }
            }
            SemanticNodeData::Union(arms) => {
                let arms = arms.members_arc();
                drop(data);
                let n = arms.len();
                if n == 0 {
                    results.push(KeyNamesArm::Names(Vec::new()));
                    return;
                }
                work.push(KeyNamesFrame::CombineUnion { arm_count: n });
                for arm in arms.iter().rev() {
                    work.push(KeyNamesFrame::Expand(*arm));
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                // `keyof` over a merged interface is the UNION of every
                // contributor's keys — the same accumulation the intersection
                // combine performs.
                let contributors = Arc::clone(contributors.as_arc());
                drop(data);
                let n = contributors.len();
                if n == 0 {
                    results.push(KeyNamesArm::Names(Vec::new()));
                    return;
                }
                work.push(KeyNamesFrame::CombineIntersection { arm_count: n });
                for contributor in contributors.iter().rev() {
                    work.push(KeyNamesFrame::Expand(*contributor));
                }
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                results.push(KeyNamesArm::Names(Vec::new()));
            }
            // DeclPlaceholder — expand via Instantiate before
            // enumerating keys. The placeholder's `whole_hash` is
            // payload-only diagnostic context; the `Instantiate` key
            // is content-free (R6) and `build_instantiate` re-sources
            // the live content hash from `ensure_indexed_ready_serve` at
            // value-build time.
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash: _,
            }) => {
                let base = self.type_slot_for(canonical_id.clone(), *owner, name.clone());
                let owner_canonical = canonical_id.clone();
                drop(data);
                let read = self.execute_read(crate::semantic_query::SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        base,
                        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        // Key-name enumeration consumes the body's structural
                        // shape (Object members, Union arms, etc.) — Expanded
                        // is required so the next Expand frame can read keys
                        // off the unwrapped surface, not a lazy Ref shell.
                        // Demand-driven reducer spec: key
                        // enumeration is a legitimate publication-grade
                        // demand (the keyspace is the explicit consumer
                        // surface), so the context stays `Published +
                        // Expanded`.
                        self.instantiate_context_for(
                            &owner_canonical,
                            crate::semantic_query::ProjectionReductionContext::published(
                                crate::semantic_query::ProjectionMode::Expanded,
                            ),
                        ),
                    ),
                ));
                // A2 signal-split: fold a genuinely-incomplete keyspace
                // instantiation onto the request's sticky partial flag.
                crate::request_context::observe_component_meta_read_suppress(&read);
                match read.value {
                    crate::semantic_query::QueryResult::Value(instantiated)
                        if instantiated != resolved =>
                    {
                        work.push(KeyNamesFrame::Expand(instantiated));
                    }
                    _ => {
                        results.push(KeyNamesArm::Unresolvable);
                    }
                }
            }
            // Unresolvable shapes fall through to None — catch-all
            // matches deferred shells, primitives other than Never,
            // Literals, TypeParams, etc.
            _ => {
                results.push(KeyNamesArm::Unresolvable);
            }
        }
    }

    /// Resolve a base node to its one-level core [`SurfaceView`] —
    /// members + call / construct / index signatures + keyspace — via the
    /// empty-path `Shallow` `ProjectPath` synthesiser.
    ///
    /// This is the SINGLE shared surface reader for the macro-shape and
    /// object-filter paths: it routes through the SOLE query-time type
    /// resolver (`SemanticQueryKey::ProjectPath { path: [] }`, the canonical
    /// "expand the whole surface" shape) and returns the resolver's own
    /// terminal `SemanticNodeData::Object(view)` verbatim. Because it reads
    /// the core [`SurfaceView`] (not the lossy `MacroSurfaceView`) it PRESERVES
    /// construct signatures, index signatures, the keyspace, and the
    /// `has_index_signature` flag — the cross-file `Omit<Base, K>` carrier path
    /// that the old reader silently dropped.
    ///
    /// `context.mode` MUST be [`ProjectionMode::Shallow`] so member values stay
    /// reference-style (one-level surface, no recursive expansion). The
    /// empty-path Shallow synthesiser already owns the declaration-placeholder
    /// unwrap, the `A & B` Intersection own-body-shadows-heritage merge, the
    /// union-common-member synthesis, and the cross-file `DeclRef` /
    /// `InstantiationRef` carrier resolution — so this helper carries none of
    /// that logic itself.
    ///
    /// Returns `None` when the projection errors or the terminal is not an
    /// `Object` surface (a primitive / unresolvable carrier has no one-level
    /// member surface).
    pub(crate) fn resolve_typeinfo_surface_view(
        &self,
        base: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<crate::semantic_query::SurfaceView> {
        self.resolve_typeinfo_surface_view_with_node(base, context)
            .map(|(view, _terminal)| view)
    }

    /// The one-level surface [`Self::resolve_typeinfo_surface_view`] reads,
    /// delivered as an owning read of its terminal `Object` node, so the
    /// surface and every operand it names stay valid through the final
    /// holder. The node IS the composed surface (see
    /// [`Self::resolve_typeinfo_surface_view_with_node`]).
    pub fn resolve_typeinfo_surface(
        &self,
        base: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<crate::semantic_query_memo::node_data_lease::SurfaceLease> {
        let terminal = self.typeinfo_surface_terminal(base, context)?;
        crate::semantic_query_memo::node_data_lease::SurfaceLease::from_lease(
            self.graph().lease_for_delivery(terminal)?,
        )
    }

    /// As [`Self::resolve_typeinfo_surface_view`], but also returns the terminal
    /// `Object` NODE the one-level surface was read from. The node IS the
    /// composed surface (the same `SurfaceView` it carries), so folding the
    /// node-domain raised-shape facts over it yields the EXACT materializedness
    /// of the composed surface — distinct from the carrier-intact `base` decl
    /// anchor, whose own raise keeps heritage / import carriers unresolved.
    pub(crate) fn resolve_typeinfo_surface_view_with_node(
        &self,
        base: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<(crate::semantic_query::SurfaceView, SemanticNodeId)> {
        let terminal = self.typeinfo_surface_terminal(base, context)?;
        match self.graph().node_data(terminal).as_deref() {
            Some(SemanticNodeData::Object(view)) => Some((view.clone(), terminal)),
            _ => None,
        }
    }

    /// The terminal `Object` node of `base`'s complete one-level surface.
    fn typeinfo_surface_terminal(
        &self,
        base: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        verter_debug_assert_eq!(
            context.mode,
            crate::semantic_query::ProjectionMode::Shallow,
            "resolve_typeinfo_surface_view synthesises a one-level surface; mode must be Shallow"
        );
        let terminal_read =
            self.execute_read(crate::semantic_query::SemanticQueryKey::ProjectPath {
                base,
                path: verter_type_expr::OwnedSlice::from(
                    Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice(),
                ),
                context,
            });
        // A2 signal-split: fold a genuinely-incomplete terminal projection.
        crate::request_context::observe_component_meta_read_suppress(&terminal_read);
        // A partial terminal read never hands out a `SurfaceView`: its
        // total `closed()` witness would claim completeness the read does
        // not have (open-spread evidence, budget, cycles). Callers degrade
        // on `None` instead of consuming a completeness-claiming Object.
        if terminal_read.result_is_partial() {
            return None;
        }
        let terminal = match terminal_read.value {
            crate::semantic_query::QueryResult::Value(node) => node,
            crate::semantic_query::QueryResult::Recursive(node) => node,
            crate::semantic_query::QueryResult::Error(_) => return None,
        };
        matches!(
            self.graph().node_data(terminal).as_deref(),
            Some(SemanticNodeData::Object(_))
        )
        .then_some(terminal)
    }

    /// Name-only projection of [`Self::key_literals_from_keyspace_node`]
    /// for consumers that select or exclude members by NAME (`Pick` /
    /// `Omit` key-set readers). Duplicate names collapse: the numeric
    /// key `1` and the string key `"1"` address the SAME property, so a
    /// `1 | "1"` keyspace yields ONE name.
    ///
    /// `pub(crate)` (not `pub(super)`) so the `Pick` / `Omit` dispatch reducers
    /// (`project_semantic_dispatch::build`) reach this SINGLE shared keyspace
    /// enumerator rather than forking a second `TypeExpr`-domain key walker.
    pub(crate) fn key_names_from_keyspace_node(
        &self,
        node: SemanticNodeId,
    ) -> Option<Vec<PropertyKey>> {
        let keys = self.key_literals_from_keyspace_node(node)?;
        let mut names: Vec<PropertyKey> = Vec::with_capacity(keys.len());
        let mut seen = FxHashSet::default();
        for key in keys {
            if seen.insert(key.key.clone()) {
                names.push(key.key);
            }
        }
        Some(names)
    }

    /// Shared key-domain enumeration — the SOLE keyspace enumerator for
    /// `Pick` / `Omit` key sets and mapped-type key spaces. Yields one
    /// [`KeyDomainKey`] PER KEYSPACE LITERAL, preserving the literal's
    /// KIND alongside the canonical published name: K-dependent mapped
    /// values and `as` remaps substitute the ORIGINAL literal (pinned
    /// tsgo, probe12: `{ [K in 1]: K }` = `{ 1: 1 }`, never `{ 1: "1" }`).
    ///
    /// Duplicate names are NOT collapsed here: the numeric key `1` and
    /// the string key `"1"` produce the same property NAME but distinct
    /// substitution literals, and mapped consumers UNION the per-K
    /// values of same-name productions (probe12: `{ [K in 1 | "1"]: K }`
    /// = `{ 1: 1 | "1" }`). Only exact `(name, literal)` duplicates
    /// dedup (`1 | 1` is just `1`). Name-set consumers go through
    /// [`Self::key_names_from_keyspace_node`], which collapses names.
    pub(super) fn key_literals_from_keyspace_node(
        &self,
        node: SemanticNodeId,
    ) -> Option<Vec<KeyDomainKey>> {
        let domain = self.key_space_domain(node)?;
        domain.broad.is_empty().then(|| domain.keys.clone())
    }

    /// Index distribution also retains broad key kinds. Named members alone
    /// cannot prove that a dictionary's key domain is finite or empty.
    pub(super) fn index_keys_from_keyspace_node(
        &self,
        node: SemanticNodeId,
    ) -> Option<Vec<IndexKey>> {
        if matches!(
            self.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::Any | PrimitiveKind::Unknown
            ))
        ) {
            return None;
        }
        let domain = self.key_space_domain(node)?;
        let mut keys = domain
            .keys
            .iter()
            .map(|key| {
                self.key_domain_binder(key)
                    .map(|node| self.normalized_index_key_node(node))
            })
            .collect::<Option<Vec<_>>>()?;
        // Fixed kind order; canonical reduction owns the published union order.
        for kind in [
            PrimitiveKind::String,
            PrimitiveKind::Number,
            PrimitiveKind::Symbol,
        ] {
            if domain.broad.contains(&kind) {
                keys.push(IndexKey::Computed(
                    self.graph().intern_node(SemanticNodeData::Primitive(kind)),
                ));
            }
        }
        Some(keys)
    }

    /// The mapped binder for a nominal or broad access, only when the shared
    /// key-domain proof admits the entire requested domain. Literal selection
    /// and remap preimages remain the path walker's existing owners.
    pub(super) fn mapped_nonliteral_index_argument(
        &self,
        key_space: SemanticNodeId,
        index: &IndexKey,
    ) -> Option<SemanticNodeId> {
        let node = match index {
            IndexKey::UniqueSymbol(identity) => self.unique_symbol_key_type(identity),
            IndexKey::Computed(node) => *node,
            IndexKey::String(_) | IndexKey::Number(_) => return None,
        };
        if matches!(
            self.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::Any | PrimitiveKind::Unknown
            ))
        ) {
            return None;
        }
        // A closed template pattern equal to, or assignable to, a closed
        // pattern constraint admits the entire index without enumerating its
        // infinite literal domain (`a${number}` below `a${string}`). Open
        // interpolants retain the deferred access, just like any other
        // unbound generic domain.
        let pattern_index = if matches!(
            self.graph().node_data(node).as_deref(),
            Some(
                SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::BareRef(_)
                    | SemanticNodeData::ImportType(_)
            )
        ) {
            self.resolve_signature_source_carrier(
                node,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
        } else {
            node
        };
        if matches!(
            self.graph().node_data(pattern_index).as_deref(),
            Some(SemanticNodeData::TemplateLiteral { .. })
        ) && !super::raise::builtin_lowering_argument_is_open(self, pattern_index)
        {
            let pattern_constraint = self.resolve_signature_source_carrier(
                key_space,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            );
            if !matches!(
                self.graph().node_data(pattern_constraint).as_deref(),
                Some(SemanticNodeData::TemplateLiteral { .. })
            ) {
                return None;
            }
            let mut evidence = super::canonical_algebra::CanonicalEvidence::default();
            let mut budget = super::canonical_algebra::COMPARE_WORK_BUDGET;
            let equal = super::canonical_algebra::compare_structural(
                self.graph(),
                pattern_index,
                pattern_constraint,
                &mut evidence,
                &mut budget,
            ) == super::canonical_algebra::StructuralIdentity::Equal;
            self.deposit_canonical_evidence(evidence);
            if equal
                || self.template_pattern_accepts(pattern_index, pattern_constraint) == Some(true)
            {
                return Some(pattern_index);
            }
        }
        let requested = self.key_space_domain(node)?;
        if requested.broad.is_empty()
            && !matches!(
                requested.keys.as_slice(),
                [KeyDomainKey {
                    key: PropertyKey::UniqueSymbol(_),
                    ..
                }]
            )
        {
            return None;
        }
        let constraint = self.key_space_domain(key_space)?;
        if !requested.broad.is_subset(&constraint.broad)
            || !requested.keys.iter().all(|key| constraint.admits(key))
        {
            return None;
        }
        let mut arguments = requested
            .keys
            .iter()
            .map(|key| self.key_domain_binder(key))
            .collect::<Option<Vec<_>>>()?;
        for kind in [
            PrimitiveKind::String,
            PrimitiveKind::Number,
            PrimitiveKind::Symbol,
        ] {
            if requested.broad.contains(&kind) {
                arguments.push(self.graph().intern_node(SemanticNodeData::Primitive(kind)));
            }
        }
        Some(self.intern_normalized_union_or_intersection(&arguments, true))
    }

    /// Immutable key syntax has one reusable domain. Declaration references
    /// and symbolic patterns stay unknown here and use the fact-backed owner.
    pub(super) fn known_key_space_domain(
        &self,
        root: SemanticNodeId,
    ) -> Option<Arc<KeySpaceDomain>> {
        let (_guard, trip) = self.enter_connected_demand(false);
        if let Some(reasons) = trip {
            self.fold_local_partial_completeness(reasons);
            return None;
        }
        if let Some(cached) = self.graph().structural_domain_get(root) {
            return cached;
        }
        let mut done: rustc_hash::FxHashMap<SemanticNodeId, Option<Arc<KeySpaceDomain>>> =
            Default::default();
        let mut active = FxHashSet::default();
        let mut work = vec![(root, false)];
        while let Some((node, finish)) = work.pop() {
            if done.contains_key(&node) {
                continue;
            }
            if let Some(cached) = self.graph().structural_domain_get(node) {
                done.insert(node, cached);
                continue;
            }
            let data = self.graph().node_data(node)?;
            if !finish {
                if !active.insert(node) {
                    done.insert(node, None);
                    continue;
                }
                if let Err(reasons) = self.charge_connected_work() {
                    self.fold_local_partial_completeness(reasons);
                    return None;
                }
                work.push((node, true));
                if let composite
                @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) =
                    data.as_ref()
                {
                    work.extend(
                        composite
                            .composite_members()
                            .expect("key composition")
                            .iter()
                            .rev()
                            .map(|&child| (child, false)),
                    );
                }
                continue;
            }
            active.remove(&node);
            let domain = match data.as_ref() {
                SemanticNodeData::Literal(LiteralValue::String(name)) => {
                    Some(KeySpaceDomain::finite(vec![KeyDomainKey {
                        key: PropertyKey::string_literal(name.as_str()),
                        literal: Some(LiteralValue::String(name.clone())),
                    }]))
                }
                SemanticNodeData::Literal(LiteralValue::Number(number)) => {
                    Some(KeySpaceDomain::finite(vec![KeyDomainKey {
                        key: PropertyKey::from_js_number(*number),
                        literal: Some(LiteralValue::Number(*number)),
                    }]))
                }
                SemanticNodeData::TypeOfNominal(_) => {
                    Some(KeySpaceDomain::finite(vec![KeyDomainKey {
                        key: PropertyKey::UniqueSymbol(data.typeof_nominal_identity()?.clone()),
                        literal: None,
                    }]))
                }
                SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                    Some(KeySpaceDomain::default())
                }
                SemanticNodeData::Primitive(
                    kind @ (PrimitiveKind::String | PrimitiveKind::Number | PrimitiveKind::Symbol),
                ) => Some(KeySpaceDomain {
                    broad: FxHashSet::from_iter([*kind]),
                    ..Default::default()
                }),
                // An object surface names keys only as a `keyof` base; an
                // object arm of a key type (`string & { __brand: 'U' }`)
                // is a brand, not a key set.
                SemanticNodeData::KeyOf { base } => {
                    match self.graph().node_data(*base).as_deref() {
                        Some(SemanticNodeData::Object(surface)) => {
                            let mut keys = Vec::new();
                            let mut broad = FxHashSet::default();
                            let mut closed = !(surface.closed().has_index_signature()
                                && surface.index_signatures.is_empty());
                            for member in surface.positive_members() {
                                if let Err(reasons) = self.charge_connected_work() {
                                    self.fold_local_partial_completeness(reasons);
                                    return None;
                                }
                                if !member.visibility.is_public() {
                                    if surface.closed().has_index_signature() {
                                        closed = false;
                                    }
                                    continue;
                                }
                                if let Some(key) = member.key.cloned_known() {
                                    keys.push(key);
                                } else {
                                    closed = false;
                                }
                            }
                            for signature in surface.index_signatures.iter() {
                                match self.graph().node_data(signature.key_type).as_deref() {
                                    Some(SemanticNodeData::Primitive(PrimitiveKind::String)) => {
                                        broad
                                            .extend([PrimitiveKind::String, PrimitiveKind::Number]);
                                    }
                                    Some(SemanticNodeData::Primitive(
                                        kind @ (PrimitiveKind::Number | PrimitiveKind::Symbol),
                                    )) => {
                                        broad.insert(*kind);
                                    }
                                    _ => closed = false,
                                }
                            }
                            closed.then(|| {
                                let mut domain =
                                    KeySpaceDomain::finite(KeyDomainKey::from_keys(keys));
                                domain.broad = broad;
                                domain
                            })
                        }
                        _ => None,
                    }
                }
                composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                    let arms = composite.composite_members().expect("key composition");
                    let domains = arms
                        .iter()
                        .map(|arm| done.get(arm).cloned().flatten())
                        .collect::<Option<Vec<_>>>();
                    domains.map(|domains| {
                        let mut iter = domains.into_iter();
                        let mut result = iter.next().map(|d| (*d).clone()).unwrap_or_default();
                        for child in iter {
                            result = if matches!(composite, SemanticNodeData::Union(_)) {
                                result.union((*child).clone())
                            } else {
                                result.intersection((*child).clone())
                            };
                        }
                        result
                    })
                }
                _ => None,
            }
            .map(Arc::new);
            self.graph().structural_domain_publish(node, domain.clone());
            done.insert(node, domain);
        }
        done.remove(&root).flatten()
    }

    pub(super) fn key_space_domain(&self, node: SemanticNodeId) -> Option<Arc<KeySpaceDomain>> {
        if let Some(domain) = self.known_key_space_domain(node) {
            return Some(domain);
        }
        // Resolve one source view and enumerate its indexed headers. This is
        // a domain demand, so it never publishes a wide keyof value union.
        if let Some(SemanticNodeData::KeyOf { base }) = self.graph().node_data(node).as_deref() {
            let object = match self.normalize_node_for_structural_fact_demand(
                *base,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            ) {
                super::evaluate::StructuralFactDemandOutcome::Complete(node) => node,
                super::evaluate::StructuralFactDemandOutcome::Recovered { reasons, .. }
                | super::evaluate::StructuralFactDemandOutcome::Partial(reasons) => {
                    self.fold_local_partial_completeness(reasons);
                    return None;
                }
            };
            if matches!(
                self.graph().node_data(object).as_deref(),
                Some(SemanticNodeData::Object(_))
            ) {
                // The resolved view's own `keyof` node carries the reusable
                // domain memo.
                let keyof = self
                    .graph()
                    .intern_node(SemanticNodeData::KeyOf { base: object });
                return self.known_key_space_domain(keyof);
            }
        }
        // Key-set reads stay in structural transit. The explicit stack also
        // carries alias followups, so authored nesting cannot grow the stack.
        let (_connected_guard, trip) = self.enter_connected_demand(false);
        if let Some(reasons) = trip {
            self.fold_local_partial_completeness(reasons);
            return None;
        }
        let mut frames = vec![KeySpaceFrame::Enter(node)];
        let mut values: Vec<KeySpaceDomain> = Vec::new();
        let mut active = FxHashSet::default();
        while let Some(frame) = frames.pop() {
            match frame {
                KeySpaceFrame::Enter(node) => {
                    if !active.insert(node) {
                        self.fold_local_partial_completeness(
                            crate::semantic_query::PartialReasonSet::SAME_PATH_RECURSION,
                        );
                        return None;
                    }
                    if let Err(reasons) = self.charge_connected_work() {
                        self.fold_local_partial_completeness(reasons);
                        return None;
                    }
                    let resolved = self
                        .evaluate_deferred_semantic_node_with_context(
                            node,
                            crate::semantic_query::ProjectionReductionContext::structural_transit(),
                        )
                        .into_active_query_build_node(self);
                    if resolved != node {
                        frames.push(KeySpaceFrame::Leave(node));
                        frames.push(KeySpaceFrame::Enter(resolved));
                        continue;
                    }
                    let data = self.graph().node_data(node)?;
                    match data.as_ref() {
                        composite @ (SemanticNodeData::Union(_)
                        | SemanticNodeData::Intersection(_)) => {
                            let members =
                                composite.composite_members().expect("composite key domain");
                            frames.push(KeySpaceFrame::Combine {
                                node,
                                arms: members.len(),
                                union: matches!(composite, SemanticNodeData::Union(_)),
                            });
                            frames.extend(members.iter().rev().copied().map(KeySpaceFrame::Enter));
                        }
                        SemanticNodeData::Primitive(
                            kind @ (PrimitiveKind::String
                            | PrimitiveKind::Number
                            | PrimitiveKind::Symbol),
                        ) => {
                            values.push(KeySpaceDomain {
                                broad: FxHashSet::from_iter([*kind]),
                                ..KeySpaceDomain::default()
                            });
                            active.remove(&node);
                        }
                        SemanticNodeData::Primitive(
                            PrimitiveKind::Any | PrimitiveKind::Unknown,
                        ) => {
                            values.push(KeySpaceDomain {
                                broad: FxHashSet::from_iter([
                                    PrimitiveKind::String,
                                    PrimitiveKind::Number,
                                    PrimitiveKind::Symbol,
                                ]),
                                ..KeySpaceDomain::default()
                            });
                            active.remove(&node);
                        }
                        _ => match self.key_space_leaf(node)? {
                            KeySpaceLeaf::Finite(keys) => {
                                values.push(KeySpaceDomain::finite(keys));
                                active.remove(&node);
                            }
                            KeySpaceLeaf::Next(next) => {
                                frames.push(KeySpaceFrame::Leave(node));
                                frames.push(KeySpaceFrame::Enter(next));
                            }
                        },
                    }
                }
                KeySpaceFrame::Leave(node) => {
                    active.remove(&node);
                }
                KeySpaceFrame::Combine { node, arms, union } => {
                    let start = values.len().checked_sub(arms)?;
                    let mut children = values.drain(start..);
                    let mut domain = children.next().unwrap_or_default();
                    for child in children {
                        domain = if union {
                            domain.union(child)
                        } else {
                            domain.intersection(child)
                        };
                    }
                    values.push(domain);
                    active.remove(&node);
                }
            }
        }
        values.pop().map(Arc::new)
    }

    fn key_space_leaf(&self, resolved: SemanticNodeId) -> Option<KeySpaceLeaf> {
        // Resolve only the key-set leaf. Further heads and composites return
        // to the worklist; member values are never a key-domain demand.
        let data = self.graph().node_data(resolved)?;
        match data.as_ref() {
            SemanticNodeData::Literal(LiteralValue::String(name)) => {
                Some(KeySpaceLeaf::Finite(vec![KeyDomainKey {
                    key: PropertyKey::string_literal(name.as_str()),
                    literal: Some(LiteralValue::String(name.clone())),
                }]))
            }
            // Numeric-literal keys are legal keyspace members; they publish
            // as the canonical JS numeric string (pinned tsgo, probe10:
            // `Pick<any, 1>` = `{ 1: any }` ≡ `{ "1": any }`,
            // `Pick<any, 1.5>` = `{ "1.5": any }`, and a numeric key picks
            // the source member declared under that canonical name) while
            // the substitution literal keeps the NUMERIC kind.
            // Boolean / bigint literals are NOT valid property keys
            // (tsgo TS2344) — they stay non-enumerable via the catch-all.
            SemanticNodeData::Literal(LiteralValue::Number(number)) => {
                Some(KeySpaceLeaf::Finite(vec![KeyDomainKey {
                    key: PropertyKey::from_js_number(*number),
                    literal: Some(LiteralValue::Number(*number)),
                }]))
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                Some(KeySpaceLeaf::Finite(Vec::new()))
            }
            SemanticNodeData::TypeOfNominal(_) => Some(KeySpaceLeaf::Finite(vec![KeyDomainKey {
                key: PropertyKey::UniqueSymbol(data.typeof_nominal_identity()?.clone()),
                literal: None,
            }])),
            SemanticNodeData::TypeOf(_) => {
                let nominal = self.evaluate_deferred_semantic_node(resolved);
                (nominal != resolved).then_some(KeySpaceLeaf::Next(nominal))
            }
            // The canonical keyof producer includes index-signature domains
            // beside typed named keys. Demand only keys, never member values.
            SemanticNodeData::KeyOf { base } => {
                let subject = self.resolve_signature_source_carrier(
                    *base,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                );
                let read = self.execute_read(crate::semantic_query::SemanticQueryKey::KeyOf {
                    base: subject,
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Shallow,
                    ),
                });
                // A2 signal-split: fold a genuinely-incomplete keyof resolve.
                crate::request_context::observe_component_meta_read_suppress(&read);
                match read.value {
                    crate::semantic_query::QueryResult::Value(reduced) if reduced != resolved => {
                        Some(KeySpaceLeaf::Next(reduced))
                    }
                    _ => None,
                }
            }
            // A `BareRef` / `ImportType` carrier as the key space (`{ [K in
            // BareRef(Keys)]: V }` where `type Keys = 'a' | 'b'`). The
            // deferred-shell evaluator deliberately leaves these carriers
            // symbolic, but key-domain enumeration IS the macro-shape
            // enumeration path where the carrier head MUST resolve to its
            // declared key set. Resolve the head through the ONE shared
            // dispatch (`resolve_carrier_subject_node` → the head resolver)
            // under the value-expansion-free STRUCTURAL-TRANSIT context (NOT
            // `Published(Expanded)`): key-domain enumeration needs only the
            // resolved key set / literal union, so the carrier head resolves
            // under `Shallow` + `StructuralTransit` (member value surfaces are
            // NOT materialised). Recurse on the resolved key space; a carrier
            // that does not resolve (miss / recursive ref) stays unenumerated
            // (`None` — the deferred mapped shell owns it).
            SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_) => {
                let head_resolved = self.resolve_carrier_subject_node(
                    resolved,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                );
                if head_resolved == resolved {
                    return None;
                }
                Some(KeySpaceLeaf::Next(head_resolved))
            }
            // Unresolved alias carrier on the key-domain enumeration path. The
            // deferred-shell evaluator deliberately leaves `DeclRef` /
            // `InstantiationRef` carriers symbolic (so intermediate
            // indexed-access hops keep `Foo['k']` symbolic), but key-domain
            // enumeration IS the macro-shape enumeration path where the alias
            // MUST resolve to its declared key set — e.g. an `Omit<T, Keys>`
            // whose `Keys` is a cross-file `type Keys = 'a' | 'b'` alias (a
            // barrel-reexported keys alias). Resolve the carrier to its body
            // through the shared dispatch (`Instantiate`) and recurse. This is
            // path-precise (the demanded key set), not a breadth walk.
            SemanticNodeData::DeclRef { identity } => {
                // Key-domain enumeration needs the alias's declared key SET,
                // not its expanded member VALUES — instantiate under SHALLOW
                // (NOT `Expanded`): a keyspace alias (`type Keys = 'a' | 'b'`)
                // reduces to its literal union and a fixed-key body to its
                // one-level surface (names recovered by the recursive
                // enumeration), with member values left shallow. Avoids the
                // eager full-surface value expansion.
                let read = self.execute_read(crate::semantic_query::SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        self.type_slot_for(
                            identity.canonical_id.clone(),
                            identity.owner,
                            identity.decl_name.clone(),
                        ),
                        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        self.instantiate_context_for(
                            &identity.canonical_id,
                            crate::semantic_query::ProjectionReductionContext::published(
                                crate::semantic_query::ProjectionMode::Shallow,
                            ),
                        ),
                    ),
                ));
                // A2 signal-split: fold a genuinely-incomplete carrier resolve.
                crate::request_context::observe_component_meta_read_suppress(&read);
                let instantiated = match read.value {
                    crate::semantic_query::QueryResult::Value(id) => id,
                    _ => {
                        return self
                            .key_names_from_base_node(resolved)
                            .map(KeyDomainKey::from_keys)
                            .map(KeySpaceLeaf::Finite);
                    }
                };
                if instantiated == resolved {
                    return self
                        .key_names_from_base_node(resolved)
                        .map(KeyDomainKey::from_keys)
                        .map(KeySpaceLeaf::Finite);
                }
                Some(KeySpaceLeaf::Next(instantiated))
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                // Same value-expansion-free key-domain enumeration as the
                // `DeclRef` arm: instantiate the carrier under SHALLOW (NOT
                // `Expanded`) to recover its key SET without materialising
                // member value surfaces.
                let read = self.execute_read(crate::semantic_query::SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        self.type_slot_for(
                            base.canonical_id.clone(),
                            base.owner,
                            base.decl_name.clone(),
                        ),
                        Arc::clone(args.as_arc()),
                        self.instantiate_context_for(
                            &base.canonical_id,
                            crate::semantic_query::ProjectionReductionContext::published(
                                crate::semantic_query::ProjectionMode::Shallow,
                            ),
                        ),
                    ),
                ));
                // A2 signal-split: fold a genuinely-incomplete carrier resolve.
                crate::request_context::observe_component_meta_read_suppress(&read);
                let instantiated = match read.value {
                    crate::semantic_query::QueryResult::Value(id) => id,
                    _ => {
                        return self
                            .key_names_from_base_node(resolved)
                            .map(KeyDomainKey::from_keys)
                            .map(KeySpaceLeaf::Finite);
                    }
                };
                if instantiated == resolved {
                    return self
                        .key_names_from_base_node(resolved)
                        .map(KeyDomainKey::from_keys)
                        .map(KeySpaceLeaf::Finite);
                }
                Some(KeySpaceLeaf::Next(instantiated))
            }
            // An object arm of a key type (`string & { __brand: 'U' }`) is a
            // brand, not a key set; its member names are `keyof` evidence
            // only, which the `KeyOf` arm owns.
            SemanticNodeData::Object(_)
            | SemanticNodeData::ObjectSpreadProgram(_)
            | SemanticNodeData::MergedDecl { .. } => None,
            _ => self
                .key_names_from_base_node(resolved)
                .map(KeyDomainKey::from_keys)
                .map(KeySpaceLeaf::Finite),
        }
    }

    /// Chain X closure (Q1-X) —
    /// **non-emitting** key-domain membership predicate for path
    /// admission.
    ///
    /// Returns `Some(true)` iff `needle` is structurally proven to be a
    /// member of the keyspace `node`. Returns `Some(false)` iff `needle`
    /// is structurally proven NOT to be a member. Returns `None` when
    /// membership cannot be proven without falling back to whole-keyspace
    /// enumeration — callers MUST then either fall through to the
    /// primitive-keyspace tier or accept the unresolved carrier (NOT
    /// enumerate).
    ///
    /// ## Why
    ///
    /// The full enumeration-based path admission would call
    /// [`Self::key_names_from_keyspace_node`] to test "does this
    /// Mapped's key space contain `needle`?". That helper internally
    /// calls [`Self::evaluate_deferred_semantic_node`] on the
    /// keyspace node AND [`Self::key_names_from_base_node`] on a
    /// `KeyOf { base }` arm — both of which trigger `build_key_of` /
    /// `build_mapped_type` per-key emissions through
    /// `intern_keyspace_names` and the publication loop. That path
    /// emits the **entire** keyspace just to test membership of ONE
    /// literal segment, which is wasteful for the path-walker's
    /// admission predicate.
    ///
    /// This predicate replaces the enumeration-based admission with a
    /// structural walk that NEVER calls `evaluate_deferred_semantic_node`
    /// on a deferred shell, NEVER calls `key_names_from_base_node`, and
    /// NEVER routes through `Instantiate` / cold-build helpers. Every
    /// case it can decide is decided by walking
    /// already-resolved [`SemanticNodeData`] — `Object` → check members
    /// directly, `Literal` → compare, `Union` → recurse arms, `Never` →
    /// refute, `KeyOf { base }` → recurse into the base's structural
    /// surface ONLY when the base is itself an already-resolved Object
    /// / Intersection / Literal / Union / Never (NOT a `DeclRef`,
    /// `InstantiationRef`, `Opaque`, or other shape that would force a
    /// resolve).
    ///
    /// ## Soundness contract
    ///
    /// - `Some(true)` is a **structural proof of admission**. The
    ///   keyspace genuinely contains `needle`.
    /// - `Some(false)` is a **structural proof of refutation**. The
    ///   keyspace genuinely does NOT contain `needle`.
    /// - `None` is **inconclusive without emission**. The caller MUST
    ///   NOT treat this as refutation; it must fall through to the
    ///   primitive-keyspace tier or accept the carrier as unresolved.
    pub(super) fn keyspace_admits_literal_non_emitting(
        &self,
        node: SemanticNodeId,
        needle: &PropertyKey,
    ) -> Option<bool> {
        if let Some(domain) = self.known_key_space_domain(node) {
            return domain.membership(needle);
        }
        let data = self.graph().node_data(node)?;
        match data.as_ref() {
            // A single literal: admits iff it matches.
            SemanticNodeData::Literal(LiteralValue::String(name)) => {
                Some(&PropertyKey::string_literal(name.as_str()) == needle)
            }
            // Same canonical JS numeric string the key-domain enumeration
            // publishes — the two key-membership surfaces must agree.
            SemanticNodeData::Literal(LiteralValue::Number(n)) => {
                Some(&PropertyKey::from_js_number(*n) == needle)
            }
            // Never admits nothing.
            SemanticNodeData::Primitive(PrimitiveKind::Never) => Some(false),
            // A template literal pattern admits the names it matches
            // (`data-x` is among `data-${string}`).
            SemanticNodeData::TemplateLiteral { .. } => {
                drop(data);
                let name = match needle {
                    PropertyKey::String(name) => name.to_string(),
                    PropertyKey::Number(number) => number.to_string(),
                    PropertyKey::UniqueSymbol(_) => return Some(false),
                };
                let literal = self
                    .graph()
                    .intern_node(SemanticNodeData::Literal(LiteralValue::String(name)));
                self.template_pattern_accepts(literal, node)
            }
            // Union arms: admit iff ANY arm admits; refute iff ALL arms
            // refute. A single `None` arm makes the whole union
            // inconclusive.
            SemanticNodeData::Union(arms) => {
                let arms = arms.members_arc();
                drop(data);
                let mut any_admits = false;
                let mut all_refuted = true;
                for arm in arms.iter() {
                    match self.keyspace_admits_literal_non_emitting(*arm, needle) {
                        Some(true) => {
                            any_admits = true;
                            break;
                        }
                        Some(false) => {
                            // this arm refutes; keep checking
                        }
                        None => {
                            all_refuted = false;
                        }
                    }
                }
                if any_admits {
                    Some(true)
                } else if all_refuted {
                    Some(false)
                } else {
                    None
                }
            }
            // `keyof T`: structurally check T's surface ONLY if it is
            // already an enumerable shape. NEVER call
            // `evaluate_deferred_semantic_node` here — that is the
            // emission rail this predicate exists to avoid. If `base`
            // is a `DeclRef` / `InstantiationRef` / `Opaque` / any
            // deferred shape, return `None` (caller falls through).
            SemanticNodeData::KeyOf { base } => {
                let base_id = *base;
                drop(data);
                self.base_member_admission_non_emitting(base_id, needle)
            }
            // Anything else (Object, DeclRef, InstantiationRef,
            // primitives other than Never, Mapped, Conditional, …) is
            // not a keyspace shape this predicate decides — return
            // `None`. The caller falls through to the primitive-keyspace
            // tier or accepts the carrier as unresolved.
            _ => None,
        }
    }

    /// Companion to [`Self::keyspace_admits_literal_non_emitting`] for
    /// the `KeyOf { base }` arm: is `needle` a known structural member
    /// of `base`'s already-resolved surface, without enumerating the
    /// full member set?
    ///
    /// `Object` and `Intersection`-of-Objects are decidable cheaply
    /// here. `DeclRef`, `InstantiationRef`, `Opaque`, and other shapes
    /// that would force a resolve return `None` so the caller falls
    /// through. The intent is to admit the common simple cases (a
    /// `keyof T` whose `T` is a literal Object surface that the walker
    /// already lowered) without paying the emission cost of a
    /// keyspace-wide enumeration.
    ///
    /// `pub(super)` so the dispatch test module can assert the
    /// public-keyspace + fact-fast-path admission contract directly
    /// (a non-public member of an `Object` base is refuted; a present
    /// member of a cross-file `DeclRef` base is INCONCLUSIVE because the
    /// `MemberPresence` fact carries no visibility).
    pub fn base_member_admission_non_emitting(
        &self,
        base: SemanticNodeId,
        needle: &PropertyKey,
    ) -> Option<bool> {
        let data = self.graph().node_data(base)?;
        match data.as_ref() {
            SemanticNodeData::ObjectSpreadProgram(_) => {
                drop(data);
                let formula = match self.project_object_spread_for_consumer(
                    base,
                    crate::semantic_query::ObjectProjectionSelector::Key(needle.clone()),
                    crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Navigate,
                    ),
                ) {
                    crate::semantic_query::QueryResult::Value(formula) => formula,
                    crate::semantic_query::QueryResult::Recursive(_)
                    | crate::semantic_query::QueryResult::Error(_) => return None,
                };
                let mut present = true;
                for alternative in self.object_spread_cursor(&formula) {
                    let alternative = alternative.ok()?;
                    let closed = alternative.closed()?;
                    // The formula was projected under `Key(needle)`, so
                    // the lookup is inside the selector's declared set —
                    // a `None` (ungated) verdict is unreachable, and any
                    // absence-proof here is selector-local sound.
                    match closed.lookup(needle) {
                        Some(crate::semantic_query::ClosedKeyLookup::Present(fact))
                            if matches!(
                                fact.facets(),
                                crate::semantic_query::ProjectionEvidence::Proven(facets)
                                    if facets.visibility().is_public()
                            ) => {}
                        Some(crate::semantic_query::ClosedKeyLookup::Present(_))
                        | Some(crate::semantic_query::ClosedKeyLookup::AbsentProven)
                        | None => {
                            present = false;
                        }
                    }
                }
                Some(present)
            }
            // Public-keyspace admission: this predicate backs `keyof` / mapped /
            // indexed-access membership over `base`'s already-resolved surface.
            // A protected/private class member is NOT part of `keyof`, so it is
            // not an admissible key — admit a name only when it matches a PUBLIC
            // member. The full member set stays recorded for `native_props`.
            SemanticNodeData::Object(view) => match view.project_known_key(needle) {
                crate::semantic_query::SurfaceKeyProjection::Exact(member) => {
                    Some(member.visibility.is_public())
                }
                crate::semantic_query::SurfaceKeyProjection::AbsentProven => Some(false),
            },
            // Consult the parse-fact `MemberPresence` substrate for
            // `DeclRef` / `InstantiationRef` bases.
            //
            // Without this arm the `DeclRef` case falls through to
            // `None`, which (because Tier 3's
            // `primitive_keyspace_admits_segment` is `false` for
            // non-primitives) drives `key_admitted == Some(false)`.
            // The walker then falls through to the whole-surface
            // MappedType dispatch under `Published(Expanded)`,
            // `build_mapped_type` enumerates the entire keyspace,
            // and a per-key `ProjectMember` edge is emitted for
            // EVERY library member — the dominant residual emitter
            // for `extends Library` / generic-substituted carriers.
            //
            // The fix routes admission through
            // [`crate::file_artifact_store::FileFacts`]'s parse-domain
            // `FactKey::MemberPresence(exporter, name, Type)` fact.
            // The fact is content-addressed against the file's
            // observed content version, so the answer matches the
            // node's interned `DeclIdentity.whole_hash` exactly.
            //
            // Admission follows the inconclusive-and-resolve rule
            // (`MemberPresence` carries presence/key only, NO
            // visibility — see [`Self::member_presence_fact_admission`]):
            //
            // - PRESENT — the parse fact records a member named `needle`,
            //   but cannot prove it is PUBLIC. The predicate returns
            //   `None` (INCONCLUSIVE) so the caller falls through to full
            //   resolution, which carries visibility and applies the
            //   public gate at the resolved-surface chokepoints. A
            //   non-public member can therefore never be admitted on the
            //   strength of a bare presence fact.
            // - ABSENT — the artifact for `(canonical, whole_hash)`
            //   exists and lacks a `MemberPresence` for `needle`. The
            //   member is provably absent regardless of visibility; the
            //   predicate returns `Some(false)` (refute) so the walker's
            //   `can_narrow == false` path stops at an `opaque_miss`
            //   instead of triggering the leak.
            // - UNRECOVERABLE — the artifact is not recoverable for the
            //   observed whole_hash (evicted, schema mismatch,
            //   tombstoned). The predicate returns `None` and the caller
            //   falls through to the existing tiers.
            //
            // The fact lookup is non-emitting by construction — it
            // reads from `FileFacts`'s registry, never dispatches an
            // `Instantiate` / `Mapped` / `KeyOf` query and never calls
            // `build_mapped_type`'s publication loop.
            SemanticNodeData::DeclRef { identity } => self.member_presence_fact_admission(
                identity.canonical_id.as_ref(),
                identity.whole_hash,
                identity.decl_name.as_ref(),
                needle,
            ),
            SemanticNodeData::InstantiationRef { base, .. } => {
                // For a generic instantiation `Lib<…>`, the `keyof`
                // surface is determined by `Lib`'s member set — the
                // type arguments do not add or remove members at the
                // surface level. Query the base declaration's parse
                // facts directly; the same content-addressed
                // `MemberPresence` lookup is sound.
                self.member_presence_fact_admission(
                    base.canonical_id.as_ref(),
                    base.whole_hash,
                    base.decl_name.as_ref(),
                    needle,
                )
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = arms.members_arc();
                drop(data);
                let mut any_admits = false;
                let mut any_inconclusive = false;
                for arm in arms.iter() {
                    match self.base_member_admission_non_emitting(*arm, needle) {
                        Some(true) => {
                            any_admits = true;
                            break;
                        }
                        Some(false) => {
                            // this arm doesn't have the member; keep checking
                        }
                        None => {
                            any_inconclusive = true;
                        }
                    }
                }
                if any_admits {
                    Some(true)
                } else if any_inconclusive {
                    None
                } else {
                    Some(false)
                }
            }
            SemanticNodeData::Union(arms) => {
                // `keyof (A | B)` is the INTERSECTION of A's and B's
                // keysets. The needle admits iff EVERY arm admits.
                let arms = arms.members_arc();
                drop(data);
                let mut all_admit = true;
                let mut any_inconclusive = false;
                for arm in arms.iter() {
                    match self.base_member_admission_non_emitting(*arm, needle) {
                        Some(true) => {
                            // this arm admits; keep checking
                        }
                        Some(false) => {
                            all_admit = false;
                            break;
                        }
                        None => {
                            any_inconclusive = true;
                        }
                    }
                }
                if !all_admit {
                    Some(false)
                } else if any_inconclusive {
                    None
                } else {
                    Some(true)
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                // A merged interface's keyset is the UNION of every
                // contributor's members — the needle admits iff ANY contributor
                // admits (same accumulation as the intersection arm).
                let contributors = Arc::clone(contributors.as_arc());
                drop(data);
                let mut any_admits = false;
                let mut any_inconclusive = false;
                for contributor in contributors.iter() {
                    match self.base_member_admission_non_emitting(*contributor, needle) {
                        Some(true) => {
                            any_admits = true;
                            break;
                        }
                        Some(false) => {}
                        None => {
                            any_inconclusive = true;
                        }
                    }
                }
                if any_admits {
                    Some(true)
                } else if any_inconclusive {
                    None
                } else {
                    Some(false)
                }
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                // `keyof never` has every-key-vacuously membership;
                // soundly returning None here lets the caller decide
                // (the existing primitive tier handles the never case
                // by returning false-or-fallthrough).
                None
            }
            // Anything else (DeclRef, InstantiationRef, Opaque, Mapped,
            // Conditional, IndexedAccess, primitives other than Never,
            // …) is not decidable without a resolve. Return None so the
            // caller falls through.
            _ => None,
        }
    }

    /// Path-precise admission for Mapped narrowing when the mapper's
    /// `key_space` is a *non-enumerable* primitive (e.g. `string`,
    /// `number`) — the case `key_names_from_keyspace_node` returns
    /// `None` for.
    ///
    /// `Record<string, V>['foo']` and `{ [K in string]: V }['foo']`
    /// have `key_space = Primitive(String)`: the key domain is the
    /// entire `string` universe, but the enumerator cannot list its
    /// inhabitants. Narrowing here is sound because *every* string
    /// literal is admitted by the `string` key domain — substituting
    /// `K = "foo"` into the value expression evaluates to the same
    /// type the coarse Mapped path would assign to that key.
    ///
    /// Returns `true` when the (key_space, segment-domain) pair admits
    /// the literal:
    ///   - `Primitive(String)` admits a string-domain segment
    ///     (`Member`, `Index::String`, or a `TypeNode` normalised to a
    ///     string literal).
    ///   - `Primitive(Number)` admits a number-domain segment
    ///     (`Index::Number`, or a `TypeNode` normalised to a number
    ///     literal).
    ///   - `Primitive(Any)` / `Primitive(Unknown)` admit any literal —
    ///     fully permissive key domain.
    ///
    /// Returns `false` for any other key_space shape (the caller falls
    /// back to the coarse whole-surface Mapped path).
    ///
    /// `segment_is_string_domain` is `true` when the segment originated
    /// from a `Member(name)` / `Index::String` / `Index::TypeNode`
    /// normalised to a string literal; `false` when it originated from
    /// `Index::Number` / a `TypeNode` normalised to a number literal.
    /// (Pure ambiguity — the `TypeNode` still unresolved — is filtered
    /// upstream: `literal_name` would be `None` and the caller never
    /// invokes this helper.)
    pub(super) fn primitive_keyspace_admits_segment(
        &self,
        key_space: SemanticNodeId,
        segment_is_string_domain: bool,
    ) -> bool {
        let resolved = self.evaluate_deferred_semantic_node(key_space);
        let Some(data) = self.graph().node_data(resolved) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(PrimitiveKind::String) => segment_is_string_domain,
            SemanticNodeData::Primitive(PrimitiveKind::Number) => !segment_is_string_domain,
            SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown) => true,
            // Union of primitives (e.g. `string | number`) admits if any
            // arm admits the segment domain. Reuses the same recursive
            // check, mirroring `key_names_from_keyspace_node`'s union
            // handling but for primitive admission.
            SemanticNodeData::Union(members) => members.iter().any(|member| {
                self.primitive_keyspace_admits_segment(*member, segment_is_string_domain)
            }),
            _ => false,
        }
    }

    /// Chain X closure (Q1-X) — non-emitting member-presence admission backed
    /// by the parse-fact `FactKey::MemberPresence` substrate.
    ///
    /// Looks up the file artifact for `(canonical, observed_hash)` and
    /// queries the artifact's parse-fact registry for
    /// `MemberPresence { exporter: type_name, name: needle, space:
    /// Type }`. The lookup is constant-time and never dispatches an
    /// `Instantiate` / `Mapped` / `KeyOf` query.
    ///
    /// Admission follows the inconclusive-and-resolve rule:
    /// `MemberPresence` records presence/key only and carries NO
    /// visibility, so a bare presence fact can never *admit* a member
    /// into the public-only keyspace this predicate feeds. The mapping
    /// is therefore present→inconclusive, absent→refute (this path
    /// NEVER returns `Some(true)`):
    ///
    /// - `None` (PRESENT) — the parse fact records the member's
    ///   presence on the type's declaration body at the observed
    ///   content version, but cannot prove it is PUBLIC. Return `None`
    ///   (INCONCLUSIVE) so the caller falls through to full resolution,
    ///   which carries visibility and applies the public gate at the
    ///   resolved-surface chokepoints. A non-public member can never be
    ///   admitted on the strength of a presence fact.
    /// - `Some(false)` (ABSENT) — the artifact exists for the observed
    ///   content version, but the fact registry has no
    ///   `MemberPresence(type_name, needle, Type)` entry. The member
    ///   is provably absent regardless of visibility. Refute
    ///   structurally.
    /// - `None` (UNRECOVERABLE) — the file artifact for `(canonical,
    ///   observed_hash)` is not recoverable (evicted, schema
    ///   mismatch, content-hash drift). Fall through to the caller's
    ///   existing tiers so the unrecoverable case falls back to the
    ///   evaluator-backed enumeration path.
    ///
    /// The fact-registry's `MemberPresence` keys are emitted by the
    /// shallow-analysis fact emitter
    /// (`fact_emission::emit_type_symbols` at `fact_emission.rs:233`
    /// for type symbols, `:335` for enum members, `:364` for value-
    /// space object shapes) — every declared member of a type that
    /// the shallow walker observed has a corresponding presence fact
    /// in the file's `FileArtifacts.facts` registry. The fact is
    /// content-addressed so the observed `whole_hash` keys the same
    /// artifact the `DeclIdentity` was interned against.
    fn member_presence_fact_admission(
        &self,
        canonical: &str,
        observed_hash: HashValue,
        type_name: &str,
        needle: &PropertyKey,
    ) -> Option<bool> {
        // Empty identity (a synthesised carrier with no real
        // declaration) cannot resolve to a parse fact — fall through.
        if canonical.is_empty() || type_name.is_empty() {
            return None;
        }
        // Normalise the analysis canonical (matches the lookup used by
        // the signature builder at
        // `fact_signature_helpers::parse_fact_ref_for_observed_current_content`).
        let presence_key = FactKey::MemberPresence {
            exporter: InternedName::from(type_name),
            name: needle.clone(),
            space: SymbolSpace::Type,
        };
        // Visibility-aware admission (inconclusive-and-
        // resolve, NOT a `MemberPresence` schema change). `MemberPresence`
        // records presence/key only — it carries NO visibility. So:
        //
        // - PRESENT (`lookup(..).is_some()`): the member exists, but the fact
        //   cannot prove it is PUBLIC. Admitting it would leak a potentially
        //   protected/private member into the public keyspace (the `keyof` /
        //   mapped / indexed-access derivation this predicate feeds is
        //   public-only). Return `None` (INCONCLUSIVE) so the caller falls
        //   through to full resolution, which carries visibility and applies the
        //   public gate at the resolved-surface chokepoints
        //   (`base_member_admission_non_emitting`'s Object arm, `build_key_of`,
        //   the mapped/Pick/Omit derivations). Public members are admitted by
        //   that resolution (and its result is cached), so correctness is
        //   restored without a fact-schema change.
        // - ABSENT (`is_none()`): the member is provably absent regardless of
        //   visibility — refute structurally (`Some(false)`), unchanged.
        if self
            .ctx
            .member_presence_for_observed_content(canonical, observed_hash, presence_key)?
        {
            None
        } else {
            Some(false)
        }
    }
}
