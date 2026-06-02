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
    HashValue, LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
};
use verter_semantic::facts::registry::{FactKey, InternedName, SymbolSpace};

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
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                results.push(Some(Vec::new()));
            }
            // C16: DeclPlaceholder — expand via Instantiate before
            // enumerating keys. The placeholder's `whole_hash` is
            // payload-only diagnostic context; the `Instantiate` key
            // is content-free (R6) and `build_instantiate` re-sources
            // the live content hash from `ensure_indexed_ready` at
            // value-build time.
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash: _,
            }) => {
                let base = crate::semantic_query::DeclKey {
                    canonical_id: Arc::clone(canonical_id),
                    decl_name: Arc::clone(name),
                };
                drop(data);
                match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
                    base,
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    // Key-name enumeration consumes the body's structural
                    // shape (Object members, Union arms, etc.) — Expanded
                    // is required so the next Expand frame can read keys
                    // off the unwrapped surface, not a lazy Ref shell.
                    // Codex-hybrid spec: key
                    // enumeration is a legitimate publication-grade
                    // demand (the keyspace is the explicit consumer
                    // surface), so the context stays `Published +
                    // Expanded`.
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
                }) {
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
        debug_assert_eq!(
            context.mode,
            crate::semantic_query::ProjectionMode::Shallow,
            "resolve_typeinfo_surface_view synthesises a one-level surface; mode must be Shallow"
        );
        let terminal = match self.execute(crate::semantic_query::SemanticQueryKey::ProjectPath {
            base,
            path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
            context,
        }) {
            crate::semantic_query::QueryResult::Value(node)
            | crate::semantic_query::QueryResult::Recursive(node) => node,
            crate::semantic_query::QueryResult::Error(_) => return None,
        };
        match self.graph().node_data(terminal).as_deref() {
            Some(SemanticNodeData::Object(view)) => Some(view.clone()),
            _ => None,
        }
    }

    pub(super) fn key_names_from_keyspace_node(
        &self,
        node: SemanticNodeId,
    ) -> Option<Vec<Arc<str>>> {
        let resolved = self.evaluate_deferred_semantic_node(node);
        let data = self.graph().node_data(resolved)?;
        match data.as_ref() {
            SemanticNodeData::Literal(LiteralValue::String(name)) => {
                Some(vec![Arc::from(name.as_str())])
            }
            SemanticNodeData::Union(members) => {
                let mut names = Vec::new();
                let mut seen = FxHashSet::default();
                for member in members.iter() {
                    for name in self.key_names_from_keyspace_node(*member)? {
                        if seen.insert(Arc::clone(&name)) {
                            names.push(name);
                        }
                    }
                }
                Some(names)
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => Some(Vec::new()),
            SemanticNodeData::KeyOf { base } => self.key_names_from_base_node(*base),
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
                let instantiated =
                    match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
                        base: identity.to_decl_key(),
                        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            crate::semantic_query::ProjectionMode::Expanded,
                        ),
                    }) {
                        crate::semantic_query::QueryResult::Value(id) => id,
                        _ => return self.key_names_from_base_node(resolved),
                    };
                if instantiated == resolved {
                    return self.key_names_from_base_node(resolved);
                }
                self.key_names_from_keyspace_node(instantiated)
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let instantiated =
                    match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
                        base: base.to_decl_key(),
                        args: Arc::clone(args),
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            crate::semantic_query::ProjectionMode::Expanded,
                        ),
                    }) {
                        crate::semantic_query::QueryResult::Value(id) => id,
                        _ => return self.key_names_from_base_node(resolved),
                    };
                if instantiated == resolved {
                    return self.key_names_from_base_node(resolved);
                }
                self.key_names_from_keyspace_node(instantiated)
            }
            _ => self.key_names_from_base_node(resolved),
        }
    }

    /// Chain X closure (codex Q1-X) —
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
            SemanticNodeData::Literal(LiteralValue::Number(n)) => {
                let s = if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                    (*n as i64).to_string()
                } else {
                    n.to_string()
                };
                Some(s.as_str() == needle)
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
    /// `MemberPresence` fact carries no visibility — binding ruling §4).
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
            // - `Some(true)`  — the parse fact says the declared type
            //   has a member named `needle`. The walker admits the
            //   segment structurally; the narrowing path runs and
            //   never falls through to the whole-surface dispatch.
            // - `Some(false)` — the artifact for `(canonical,
            //   whole_hash)` exists and lacks a `MemberPresence` for
            //   `needle`. The walker refutes the segment; the
            //   admission predicate returns `Some(false)` so the
            //   walker's `can_narrow == false` path stops at an
            //   `opaque_miss` instead of triggering the leak.
            // - `None`        — the artifact is not recoverable for
            //   the observed whole_hash (evicted, schema mismatch,
            //   tombstoned). The predicate returns `None` and the
            //   caller falls through to the existing tiers.
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

    /// Chain X closure (codex 6th-consult
    /// Q1-X BINDING) — non-emitting member-presence admission backed
    /// by the parse-fact `FactKey::MemberPresence` substrate.
    ///
    /// Looks up the file artifact for `(canonical, observed_hash)` and
    /// queries the artifact's parse-fact registry for
    /// `MemberPresence { exporter: type_name, name: needle, space:
    /// Type }`. The lookup is constant-time and never dispatches an
    /// `Instantiate` / `Mapped` / `KeyOf` query.
    ///
    /// - `Some(true)`  — the parse fact records the member's presence
    ///   on the type's declaration body at the observed content
    ///   version. Admit structurally.
    /// - `Some(false)` — the artifact exists for the observed content
    ///   version, but the fact registry has no
    ///   `MemberPresence(type_name, needle, Type)` entry. The member
    ///   is provably absent. Refute structurally.
    /// - `None`        — the file artifact for `(canonical,
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
        // Visibility-aware admission (binding ruling §4: inconclusive-and-
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
        // Visibility-aware admission (binding ruling §4: inconclusive-and-
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
        // Visibility-aware admission (binding ruling §4: inconclusive-and-
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
        // Visibility-aware admission (binding ruling §4: inconclusive-and-
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
