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
    SurfaceMember,
};
use verter_semantic::facts::registry::{FactKey, InternedName, SymbolSpace};

/// One-level surface view of an imported macro target's resolved root,
/// reduced to the two fields the macro-shape interpretation consumes:
/// the named members (with their TS member metadata) and the call
/// signatures.
///
/// This is the typed-IR equivalent of the eager OXC
/// `ResolvedElements` member surface. Both the `defineProps` /
/// `defineEmits` / `defineSlots` shape producers and the slot-binding
/// graph read members and call signatures off this view rather than
/// re-deriving them from `keyof`-level names.
///
/// Constructed by
/// [`ProjectSemanticDispatch::surface_view_from_base_node`], which owns
/// the declaration-placeholder unwrap and the `A & B` Intersection
/// accumulation (member union, call-signature concatenation) so the
/// bridge does not re-implement either.
#[derive(Debug, Clone, Default)]
pub(crate) struct MacroSurfaceView {
    /// Named members of the surface, in declaration order. Carries the
    /// full `SurfaceMember` metadata (`optional`, `is_method`,
    /// `declared_in_macro_type_arg`, the value node) so the lazy macro
    /// interpretation reconstructs `AnalyzedPropField` /
    /// `AnalyzedSlotField` records bit-equivalently to the eager rail.
    pub(crate) members: Vec<SurfaceMember>,
    /// Call signatures of the surface, in declaration order. Each id is
    /// a `Function`-shaped node whose first parameter is the event-name
    /// literal — the `defineEmits` call-signature event extractor walks
    /// these.
    pub(crate) call_signatures: Vec<SemanticNodeId>,
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
    /// `pub(crate)` (not `pub(super)`) so the `component_meta`
    /// typed-IR bridge
    /// ([`crate::resolver_core::ImportedMacroSurface::enumerate_member_names`])
    /// reuses this single shared `keyof`-level enumerator rather than
    /// forking a second member-name walker. The bridge passes the
    /// `ResolveDecl` root node it already holds; this enumerator owns
    /// the declaration-placeholder unwrap + Intersection/Union
    /// accumulation so the bridge does not re-implement either.
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
                let names = surface
                    .members
                    .iter()
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
            // enumerating keys.
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            }) => {
                let identity = crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                };
                drop(data);
                match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
                    base: identity,
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

    /// Resolve a base node to its one-level [`MacroSurfaceView`] —
    /// named members + call signatures — carrying the caller's
    /// surface provenance (codex BINDING design).
    ///
    /// This is the surface-level sibling of
    /// [`Self::key_names_from_base_node`]: where the key-name enumerator
    /// returns member NAMES only, this returns the full
    /// [`SurfaceMember`] records plus the surface's call signatures, so
    /// the lazy macro-shape interpretation can read member optionality /
    /// `declared_in_macro_type_arg` and extract `defineEmits`
    /// call-signature event names without a second walk.
    ///
    /// `provenance` threads the macro-type-argument own-body entry
    /// context into the DeclPlaceholder unwrap below: when
    /// `MacroTypeArgOwnBody` the unwrapped declaration's OWN-body members
    /// surface with `declared_in_macro_type_arg = true`; heritage /
    /// utility / member-value lowering stay `false` (the lowering edge
    /// downgrades them). `Structural` is the no-op default that the
    /// emits / slots readers pass — `declared_in_macro_type_arg` is a
    /// props-axis concern.
    ///
    /// Composition (mirrors the enumerator):
    ///
    /// - `Object(view)` → return the view's members + call signatures
    ///   directly. The members already carry the provenance bit stamped
    ///   at lowering time (the DeclPlaceholder unwrap below interns the
    ///   instantiated body under the provenance-bearing context).
    /// - `Intersection(arms)` → accumulate the union of every
    ///   **resolvable** arm's members (first-writer-wins on a duplicate
    ///   member name, matching TS intersection member precedence) and
    ///   concatenate every arm's call signatures. The provenance flows
    ///   into each arm so an own-body intersection literal arm keeps the
    ///   bit. Returns `None` only when no arm is resolvable.
    /// - `DeclPlaceholder` → `Instantiate` the bare declaration body
    ///   under `Published(Expanded)` carrying the provenance, then read
    ///   the unwrapped surface.
    /// - Everything else (primitives, deferred shells, unions, …) →
    ///   `None`. A `Union` carrier has no single member surface a macro
    ///   payload reads, so it collapses to `None` here — the eager rail
    ///   never produces a macro surface from a bare union either.
    ///
    /// The walk is depth-bounded by the declaration graph: it expands a
    /// placeholder at most one `Instantiate` deep and recurses only
    /// through Intersection arms, never through member value bodies (a
    /// member's value type is projected lazily by the caller via
    /// [`crate::resolver_core::ImportedMacroSurface::project_named_member`]).
    pub(crate) fn surface_view_from_base_node(
        &self,
        base: SemanticNodeId,
        provenance: crate::semantic_query::SurfaceProvenanceContext,
    ) -> Option<MacroSurfaceView> {
        // Read the raw node and dispatch per shape. We deliberately do
        // NOT pre-evaluate via `evaluate_deferred_semantic_node_with_context`
        // (codex BINDING design): the `Published(Expanded)` evaluator
        // EAGERLY MERGES an intersection body into a single Object,
        // re-resolving any carrier (`Ref`) arm under the top-level
        // provenance — which would re-stamp a heritage `extends Base` arm
        // `declared_in_macro_type_arg = true`. Instead, the
        // `DeclPlaceholder` arm below instantiates the macro-T root under
        // the caller's provenance (so `build_instantiate`'s per-arm body
        // lowering bakes own-body Object arms `true` and heritage `Ref`
        // arms structural), and the `Intersection` arm merges the
        // already-lowered arms with STRUCTURAL recursion — preserving each
        // arm's baked provenance without re-stamping carriers.
        let resolved = base;
        let data = self.graph().node_data(resolved)?;
        match data.as_ref() {
            SemanticNodeData::Object(view) => Some(MacroSurfaceView {
                members: view.members.to_vec(),
                call_signatures: view.call_signatures.to_vec(),
            }),
            SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let mut merged = MacroSurfaceView::default();
                let mut seen: FxHashSet<Arc<str>> = FxHashSet::default();
                let mut any_resolvable = false;
                for arm in arms.iter() {
                    // Recurse into already-resolved intersection arms with
                    // STRUCTURAL provenance: each arm's own-body-vs-heritage
                    // provenance was already decided when `build_instantiate`
                    // lowered the declaration body per-arm (own-body Object
                    // arms baked `true`, reference arms `false`). An
                    // already-resolved Object arm carries its baked bit
                    // verbatim; a still-deferred carrier arm (a heritage
                    // `Ref`) re-resolves STRUCTURALLY here — re-applying the
                    // caller's macro provenance would wrongly re-stamp the
                    // heritage members `true`.
                    if let Some(arm_view) = self.surface_view_from_base_node(
                        *arm,
                        crate::semantic_query::SurfaceProvenanceContext::Structural,
                    ) {
                        any_resolvable = true;
                        for member in arm_view.members {
                            // First-writer-wins: an earlier intersection
                            // arm's member shadows a later arm's member
                            // of the same name (TS member-precedence on
                            // `A & B`).
                            if seen.insert(Arc::clone(&member.name)) {
                                merged.members.push(member);
                            }
                        }
                        merged.call_signatures.extend(arm_view.call_signatures);
                    }
                }
                any_resolvable.then_some(merged)
            }
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            }) => {
                let identity = crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                };
                drop(data);
                match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
                    base: identity,
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    )
                    .with_provenance(provenance),
                }) {
                    crate::semantic_query::QueryResult::Value(instantiated)
                        if instantiated != resolved =>
                    {
                        self.surface_view_from_base_node(instantiated, provenance)
                    }
                    _ => None,
                }
            }
            // Primitives, deferred shells, unions, literals, type params,
            // … have no single member surface a macro payload reads.
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
    fn base_member_admission_non_emitting(
        &self,
        base: SemanticNodeId,
        needle: &str,
    ) -> Option<bool> {
        let data = self.graph().node_data(base)?;
        match data.as_ref() {
            SemanticNodeData::Object(view) => {
                Some(view.members.iter().any(|m| m.name.as_ref() == needle))
            }
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
        // `lookup` returns `Some(&Fact)` when the presence fact exists
        // — admit. `None` means the type's declared body has no member
        // named `needle` in its shallow inventory — refute.
        Some(artifacts.facts.lookup(&presence_key).is_some())
    }
}
