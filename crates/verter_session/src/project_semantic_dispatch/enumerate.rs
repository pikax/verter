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
    HashValue, LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId,
};
use verter_semantic::facts::registry::{FactKey, InternedName, SymbolSpace};

/// One enumerated key-domain member: the canonical published property
/// NAME plus the literal VALUE substituted for the mapper binder `K`.
///
/// `name` is the JS property-name string (`js_number_to_string` for
/// numeric literals); `literal` preserves the source literal KIND so
/// K-dependent mapped values and `as` remaps substitute `1`, never
/// `"1"` (pinned tsgo, probe12: `{ [K in 1]: K }` = `{ 1: 1 }`).
#[derive(Clone)]
pub(super) struct KeyDomainKey {
    pub(super) name: Arc<str>,
    pub(super) literal: LiteralValue,
}

impl KeyDomainKey {
    /// Lift a name-only enumeration (surface member names, `keyof`
    /// results) into key-domain entries. Member names are strings, so
    /// every lifted entry carries the STRING substitution kind.
    pub(super) fn from_names(names: Vec<Arc<str>>) -> Vec<KeyDomainKey> {
        names
            .into_iter()
            .map(|name| KeyDomainKey {
                literal: LiteralValue::String(name.as_ref().to_string()),
                name,
            })
            .collect()
    }
}

/// Worklist frame for the iterative `key_names_from_base_node`
/// driver. `Expand` advances one node; `Combine*` reduces the top N
/// prior results (one per arm) into the compound's key enumeration.
enum KeyNamesFrame {
    Expand(SemanticNodeId),
    CombineIntersection { arm_count: usize },
    CombineUnion { arm_count: usize },
}

impl<'a> ProjectSemanticDispatch<'a> {
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
    pub(crate) fn key_names_from_base_node(&self, base: SemanticNodeId) -> Option<Vec<Arc<str>>> {
        let mut work: Vec<KeyNamesFrame> = Vec::new();
        let mut results: Vec<Option<Vec<Arc<str>>>> = Vec::new();
        work.push(KeyNamesFrame::Expand(base));

        while let Some(frame) = work.pop() {
            match frame {
                KeyNamesFrame::Expand(id) => {
                    self.key_names_step(id, &mut work, &mut results);
                }
                KeyNamesFrame::CombineIntersection { arm_count } => {
                    // Accumulate enumerable arms; ignore unresolvable ones.
                    // Only return None when EVERY arm is unresolvable.
                    let start = results.len().saturating_sub(arm_count);
                    let arm_results: Vec<_> = results.drain(start..).collect();
                    let mut names: Vec<Arc<str>> = Vec::new();
                    let mut seen: FxHashSet<Arc<str>> = FxHashSet::default();
                    let mut any_enumerable = false;
                    for arm_names in arm_results.into_iter().flatten() {
                        any_enumerable = true;
                        for name in arm_names {
                            if seen.insert(Arc::clone(&name)) {
                                names.push(name);
                            }
                        }
                    }
                    results.push(if any_enumerable { Some(names) } else { None });
                }
                KeyNamesFrame::CombineUnion { arm_count } => {
                    // Keyof (A | B) = common keys across ALL arms (intersection
                    // of enumerated sets). Unresolvable arm → whole union None.
                    let start = results.len().saturating_sub(arm_count);
                    let arm_results: Vec<_> = results.drain(start..).collect();
                    let mut common: Option<FxHashSet<Arc<str>>> = None;
                    let mut unresolvable = false;
                    for arm in arm_results {
                        match arm {
                            Some(arm_names) => {
                                let arm_set: FxHashSet<Arc<str>> = arm_names.into_iter().collect();
                                common = Some(match common {
                                    Some(current) => current
                                        .intersection(&arm_set)
                                        .cloned()
                                        .collect::<FxHashSet<_>>(),
                                    None => arm_set,
                                });
                            }
                            None => {
                                unresolvable = true;
                                // Cannot early-break — must drain remaining
                                // results from the stack to keep `results`
                                // aligned for the next combine.
                            }
                        }
                    }
                    let combined = if unresolvable {
                        None
                    } else {
                        let mut names: Vec<Arc<str>> =
                            common.unwrap_or_default().into_iter().collect();
                        names.sort_unstable_by(|left, right| left.as_ref().cmp(right.as_ref()));
                        Some(names)
                    };
                    results.push(combined);
                }
            }
        }

        results.pop().unwrap_or(None)
    }

    /// Expand one node worth of key-name enumeration. Pushes either a
    /// direct result (`Some(names)` / `None`) onto `results`, or child
    /// expansions + a combine frame onto `work`.
    fn key_names_step(
        &self,
        base: SemanticNodeId,
        work: &mut Vec<KeyNamesFrame>,
        results: &mut Vec<Option<Vec<Arc<str>>>>,
    ) {
        let resolved = self.evaluate_deferred_semantic_node(base);
        let data = match self.graph().node_data(resolved) {
            Some(d) => d,
            None => {
                results.push(None);
                return;
            }
        };
        match data.as_ref() {
            SemanticNodeData::Object(surface) => {
                // `keyof ClassType` yields only public keys (TS semantics):
                // private/protected members are not part of the keyspace, so
                // mapped types (`{ [K in keyof T]: V }`, `Partial<T>`) and
                // `Pick`/`Omit` over a class never carry them. This is the
                // key-name-enumeration chokepoint; native_props reads the
                // surface directly and is unaffected.
                let names = surface
                    .members
                    .iter()
                    .filter(|member| member.visibility.is_public())
                    .map(|member| Arc::clone(&member.name))
                    .collect();
                results.push(Some(names));
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let n = arms.len();
                if n == 0 {
                    results.push(Some(Vec::new()));
                    return;
                }
                work.push(KeyNamesFrame::CombineIntersection { arm_count: n });
                for arm in arms.iter().rev() {
                    work.push(KeyNamesFrame::Expand(*arm));
                }
            }
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let n = arms.len();
                if n == 0 {
                    results.push(Some(Vec::new()));
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
                let contributors = Arc::clone(contributors);
                drop(data);
                let n = contributors.len();
                if n == 0 {
                    results.push(Some(Vec::new()));
                    return;
                }
                work.push(KeyNamesFrame::CombineIntersection { arm_count: n });
                for contributor in contributors.iter().rev() {
                    work.push(KeyNamesFrame::Expand(*contributor));
                }
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                results.push(Some(Vec::new()));
            }
            // DeclPlaceholder — expand via Instantiate before
            // enumerating keys. The placeholder's `whole_hash` is
            // payload-only diagnostic context; the `Instantiate` key
            // is content-free (R6) and `build_instantiate` re-sources
            // the live content hash from `ensure_indexed_ready_serve` at
            // value-build time.
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash: _,
            }) => {
                let base = self.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
                let owner_canonical = Arc::clone(canonical_id);
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
                        results.push(None);
                    }
                }
            }
            // Unresolvable shapes fall through to None — catch-all
            // matches deferred shells, primitives other than Never,
            // Literals, TypeParams, etc.
            _ => {
                results.push(None);
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
        debug_assert_eq!(
            context.mode,
            crate::semantic_query::ProjectionMode::Shallow,
            "resolve_typeinfo_surface_view synthesises a one-level surface; mode must be Shallow"
        );
        let terminal_read =
            self.execute_read(crate::semantic_query::SemanticQueryKey::ProjectPath {
                base,
                path: Arc::from(
                    Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice(),
                ),
                context,
            });
        // A2 signal-split: fold a genuinely-incomplete terminal projection.
        crate::request_context::observe_component_meta_read_suppress(&terminal_read);
        let terminal = match terminal_read.value {
            crate::semantic_query::QueryResult::Value(node) => node,
            crate::semantic_query::QueryResult::Recursive(node) => node,
            crate::semantic_query::QueryResult::Error(_) => return None,
        };
        match self.graph().node_data(terminal).as_deref() {
            Some(SemanticNodeData::Object(view)) => Some((view.clone(), terminal)),
            _ => None,
        }
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
    ) -> Option<Vec<Arc<str>>> {
        let keys = self.key_literals_from_keyspace_node(node)?;
        let mut names: Vec<Arc<str>> = Vec::with_capacity(keys.len());
        let mut seen = FxHashSet::default();
        for key in keys {
            if seen.insert(Arc::clone(&key.name)) {
                names.push(key.name);
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
        // Key-domain enumeration needs the keyspace's literal KEY SET, never its
        // member VALUES. Evaluate the deferred keyspace shell under the
        // value-expansion-free STRUCTURAL-TRANSIT context (NOT the default
        // `Published(Expanded)`): every operator re-dispatch along the
        // evaluation (`KeyOf` / `MappedType` / decl-placeholder `Instantiate`,
        // and a reducible conditional / mapped keyspace form) reduces to its key
        // surface WITHOUT reifying per-member value edges — so a reducible
        // keyspace node cannot enter expanded publication before the
        // per-variant arms below (the `Published(Shallow)` / `structural_transit`
        // carrier-head arms). The literal-union / name arms still recover the
        // exact key set.
        let resolved = self
            .evaluate_deferred_semantic_node_with_context(
                node,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
            .into_active_query_build_node(self);
        let data = self.graph().node_data(resolved)?;
        match data.as_ref() {
            SemanticNodeData::Literal(LiteralValue::String(name)) => Some(vec![KeyDomainKey {
                name: Arc::from(name.as_str()),
                literal: LiteralValue::String(name.clone()),
            }]),
            // Numeric-literal keys are legal keyspace members; they publish
            // as the canonical JS numeric string (pinned tsgo, probe10:
            // `Pick<any, 1>` = `{ 1: any }` ≡ `{ "1": any }`,
            // `Pick<any, 1.5>` = `{ "1.5": any }`, and a numeric key picks
            // the source member declared under that canonical name) while
            // the substitution literal keeps the NUMERIC kind.
            // Boolean / bigint literals are NOT valid property keys
            // (tsgo TS2344) — they stay non-enumerable via the catch-all.
            SemanticNodeData::Literal(LiteralValue::Number(number)) => Some(vec![KeyDomainKey {
                name: Arc::from(super::build::js_number_to_string(*number).as_str()),
                literal: LiteralValue::Number(*number),
            }]),
            SemanticNodeData::Union(members) => {
                let mut keys: Vec<KeyDomainKey> = Vec::new();
                for member in members.iter() {
                    for key in self.key_literals_from_keyspace_node(*member)? {
                        let duplicate = keys
                            .iter()
                            .any(|k| k.name == key.name && k.literal == key.literal);
                        if !duplicate {
                            keys.push(key);
                        }
                    }
                }
                Some(keys)
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => Some(Vec::new()),
            // `keyof` routes through the member-name enumerator: surface
            // member names are strings, so every enumerated key carries
            // the STRING substitution kind. (Known model gap, recorded:
            // tsgo keeps `keyof { 1: string }` numeric-kinded — the
            // surface-member model stores names only, so the declared
            // kind does not survive `keyof`.)
            //
            // When the member-name enumerator cannot resolve the base (the
            // base is a `BareRef` / `ImportType` carrier head, or a generic
            // `InstantiationRef` the name enumerator does not unwrap), route
            // the `keyof` through the shared `SemanticQueryKey::KeyOf`
            // producer (which normalises the base carrier at entry and
            // reduces `keyof InstantiationRef<…>` to its literal key union)
            // and recurse on the produced union — `keyof BareRef(Foo)<T>` /
            // `keyof Foo<T>` over a fixed-key body then enumerates its keys.
            SemanticNodeData::KeyOf { base } => {
                if let Some(names) = self.key_names_from_base_node(*base) {
                    return Some(KeyDomainKey::from_names(names));
                }
                // Key-domain enumeration needs the literal KEY UNION of `keyof
                // base`, NOT the member VALUES of `base`. Resolve the `keyof`
                // under the publication SHALLOW context (NOT `Expanded`): the
                // `KeyOf` producer normalises the base carrier and reduces to
                // the literal key union without materialising member value
                // surfaces — the value-expansion-free key-domain enumeration
                // semantics (no Table.vue-storm-adjacent eager value expansion).
                let read = self.execute_read(crate::semantic_query::SemanticQueryKey::KeyOf {
                    base: *base,
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Shallow,
                    ),
                });
                // A2 signal-split: fold a genuinely-incomplete keyof resolve.
                crate::request_context::observe_component_meta_read_suppress(&read);
                match read.value {
                    crate::semantic_query::QueryResult::Value(reduced) if reduced != resolved => {
                        self.key_literals_from_keyspace_node(reduced)
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
                self.key_literals_from_keyspace_node(head_resolved)
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
                            Arc::clone(&identity.canonical_id),
                            Arc::clone(&identity.decl_name),
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
                            .map(KeyDomainKey::from_names);
                    }
                };
                if instantiated == resolved {
                    return self
                        .key_names_from_base_node(resolved)
                        .map(KeyDomainKey::from_names);
                }
                self.key_literals_from_keyspace_node(instantiated)
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                // Same value-expansion-free key-domain enumeration as the
                // `DeclRef` arm: instantiate the carrier under SHALLOW (NOT
                // `Expanded`) to recover its key SET without materialising
                // member value surfaces.
                let read = self.execute_read(crate::semantic_query::SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        self.type_slot_for(
                            Arc::clone(&base.canonical_id),
                            Arc::clone(&base.decl_name),
                        ),
                        Arc::clone(args),
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
                            .map(KeyDomainKey::from_names);
                    }
                };
                if instantiated == resolved {
                    return self
                        .key_names_from_base_node(resolved)
                        .map(KeyDomainKey::from_names);
                }
                self.key_literals_from_keyspace_node(instantiated)
            }
            _ => self
                .key_names_from_base_node(resolved)
                .map(KeyDomainKey::from_names),
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
        needle: &str,
    ) -> Option<bool> {
        let data = self.graph().node_data(node)?;
        match data.as_ref() {
            // A single literal: admits iff it matches.
            SemanticNodeData::Literal(LiteralValue::String(name)) => Some(name.as_str() == needle),
            // Same canonical JS numeric string the key-domain enumeration
            // publishes — the two key-membership surfaces must agree.
            SemanticNodeData::Literal(LiteralValue::Number(n)) => {
                Some(super::build::js_number_to_string(*n) == needle)
            }
            // Never admits nothing.
            SemanticNodeData::Primitive(PrimitiveKind::Never) => Some(false),
            // Union arms: admit iff ANY arm admits; refute iff ALL arms
            // refute. A single `None` arm makes the whole union
            // inconclusive.
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
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
    pub(super) fn base_member_admission_non_emitting(
        &self,
        base: SemanticNodeId,
        needle: &str,
    ) -> Option<bool> {
        let data = self.graph().node_data(base)?;
        match data.as_ref() {
            // Public-keyspace admission: this predicate backs `keyof` / mapped /
            // indexed-access membership over `base`'s already-resolved surface.
            // A protected/private class member is NOT part of `keyof`, so it is
            // not an admissible key — admit a name only when it matches a PUBLIC
            // member. The full member set stays recorded for `native_props`.
            SemanticNodeData::Object(view) => Some(
                view.members
                    .iter()
                    .any(|m| m.visibility.is_public() && m.name.as_ref() == needle),
            ),
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
                let arms = Arc::clone(arms);
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
                let arms = Arc::clone(arms);
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
                let contributors = Arc::clone(contributors);
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
        needle: &str,
    ) -> Option<bool> {
        // Empty identity (a synthesised carrier with no real
        // declaration) cannot resolve to a parse fact — fall through.
        if canonical.is_empty() || type_name.is_empty() {
            return None;
        }
        // Normalise the analysis canonical (matches the lookup used by
        // the signature builder at
        // `fact_signature_helpers::parse_fact_ref_for_observed_current_content`).
        let analysis_canonical = self.ctx.normalized_analysis_canonical(canonical);
        let artifacts = self
            .ctx
            .project_type_store()
            .indexed()
            .get_artifacts_for_content(analysis_canonical.as_ref(), observed_hash)?;
        let presence_key = FactKey::MemberPresence {
            exporter: InternedName::from(type_name),
            name: InternedName::from(needle),
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
        if artifacts.facts.lookup(&presence_key).is_some() {
            None
        } else {
            Some(false)
        }
    }
}
