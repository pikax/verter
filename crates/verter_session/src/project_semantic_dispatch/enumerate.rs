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
    ///   **resolvable** arm's members under TS derived-member precedence:
    ///   own-body members (`declared_in_macro_type_arg == true`) shadow
    ///   heritage members of the same name (a two-pass merge — own-body
    ///   first, then heritage fills unclaimed names — because the heritage
    ///   fold orders the base arm BEFORE the own-body arm yet the derived
    ///   member must win). Within each provenance class, first-writer-wins
    ///   preserves the genuine `A & B` author-intersection arm order. Call
    ///   signatures from every arm are concatenated. Returns `None` only
    ///   when no arm is resolvable.
    /// - `Alias(target)` → an identity / utility alias shell
    ///   (`NoInfer<T>`, and any other `build_instantiate` arm that
    ///   interns a pass-through [`SemanticNodeData::Alias`]). The alias
    ///   is structurally transparent — `NoInfer<Base>` IS `Base` — so the
    ///   reader follows `target` and reads its surface, propagating the
    ///   caller's `provenance` UNCHANGED. An identity alias of the macro-T
    ///   root therefore surfaces its own-body members with the same
    ///   `declared_in_macro_type_arg` the un-aliased root would (the
    ///   eager same-file rail propagates provenance through the identity
    ///   utilities it handles — `Partial` / `Required` in
    ///   `verter_semantic::analysis::macros::resolve_type_to_prop_fields`
    ///   — rather than downgrading; only the TRANSFORMATIVE utilities
    ///   (`Omit` / `Pick`) reshape the surface and downgrade).
    /// - `DeclPlaceholder` → `Instantiate` the bare declaration body
    ///   under `Published(Skeleton)` carrying the provenance, then read
    ///   the unwrapped surface (see [`Self::surface_view_from_decl_identity`]
    ///   for the Skeleton-not-Expanded coupling).
    /// - `DeclRef` → resolve through `ResolveDecl` ("aliases follow") to
    ///   the declaration's `DeclPlaceholder`, then recurse. This is the
    ///   cross-file heritage carrier arm: `extends Base` where `Base` is
    ///   imported lowers to a `DeclRef` in `Navigate` / `Skeleton`.
    /// - `InstantiationRef` → `Instantiate` under the reader's `Skeleton`
    ///   demand (carrying the captured generic args + the caller's
    ///   provenance), then recurse. The cross-file generic heritage carrier
    ///   arm: `extends Base<T>`.
    /// - `Union(arms)` → the TS-correct shallow surface of a union-typed
    ///   macro payload is its COMMON members: a key published only when it
    ///   is present in EVERY arm (mirroring `key_names_from_base_node`'s
    ///   `CombineUnion` key intersection), typed as the union of the
    ///   per-arm member value types. A disjoint union has no common keys →
    ///   empty surface (still correct); an overlapping union surfaces only
    ///   the shared members. Union-arm members are reached THROUGH the
    ///   union, not written at the macro-T root, so they recurse
    ///   STRUCTURALLY (the synthesized common member is `false`).
    /// - Everything else (primitives, deferred shells, literals, type
    ///   params, …) → `None`. They have no single member surface a macro
    ///   payload reads.
    ///
    /// The walk is depth-bounded by the declaration graph: it expands a
    /// placeholder at most one `Instantiate` deep and recurses only
    /// through Alias / Intersection / Union arms, never through member
    /// value bodies (a member's value type is projected lazily by the
    /// caller via
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
        // `DeclPlaceholder` arm below instantiates the macro-T root in
        // `Skeleton` (NOT `Expanded`) under the caller's provenance (so
        // `build_instantiate`'s per-arm body lowering bakes own-body Object
        // arms `true` and heritage `Ref` arms structural), and the
        // `Intersection` arm merges the already-lowered arms with STRUCTURAL
        // recursion — preserving each arm's baked provenance without
        // re-stamping carriers.
        //
        // CARRIER-COMPLETE: a CROSS-FILE heritage `Ref` lowers
        // lazily to a `DeclRef` / `InstantiationRef` carrier in
        // `Navigate` / `Skeleton`. The dedicated carrier arms below resolve
        // those (DeclRef → ResolveDecl → DeclPlaceholder → recurse;
        // InstantiationRef → Skeleton Instantiate → recurse) so the
        // inherited members surface. A same-file heritage `Ref` is already
        // a `DeclPlaceholder` after the root Skeleton-instantiate of an
        // inline interface body, so both same-file and cross-file heritage
        // converge on the `DeclPlaceholder` reader without an eager Expand.
        let resolved = base;
        let data = self.graph().node_data(resolved)?;
        match data.as_ref() {
            SemanticNodeData::Object(view) => Some(MacroSurfaceView {
                members: view.members.to_vec(),
                call_signatures: view.call_signatures.to_vec(),
            }),
            // Identity / utility alias shell (`NoInfer<T>` and any other
            // `build_instantiate` arm that interns a pass-through Alias).
            // Structurally transparent: follow `target` and read its
            // surface, propagating the caller's provenance UNCHANGED so an
            // identity alias of the macro-T root keeps its own-body
            // members' `declared_in_macro_type_arg`. Without this arm the
            // alias fell through to the catch-all `None`, which
            // `ImportedMacroSurface::resolve_surface_view` turns into an
            // EMPTY macro surface — `defineProps<NoInfer<Base>>()` and
            // `type Props = NoInfer<Base>` would lose every member.
            SemanticNodeData::Alias(target) => {
                let target = *target;
                drop(data);
                self.surface_view_from_base_node(target, provenance)
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                // Resolve each arm's surface (STRUCTURAL provenance: each
                // arm's own-body-vs-heritage provenance was already decided
                // when `build_instantiate` lowered the declaration body
                // per-arm — own-body Object arms baked `true`, reference /
                // heritage arms `false`. Re-applying the caller's macro
                // provenance would wrongly re-stamp heritage members `true`).
                let mut any_resolvable = false;
                let mut arm_views: Vec<MacroSurfaceView> = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    if let Some(arm_view) = self.surface_view_from_base_node(
                        *arm,
                        crate::semantic_query::SurfaceProvenanceContext::Structural,
                    ) {
                        any_resolvable = true;
                        arm_views.push(arm_view);
                    }
                }
                if !any_resolvable {
                    return None;
                }
                // Member precedence (TS derived-member shadowing). A
                // declaration's OWN body (`interface Props extends Base {
                // dup: string }`) folds to `Intersection([Base,
                // OwnObject])` — heritage arm FIRST, own-body arm LAST — but
                // TS semantics make the DERIVED member shadow the inherited
                // one (`Props['dup']` is `string`, not `number` and not
                // `string & number`). A naive arm-order first-writer-wins
                // would keep the heritage `dup` (wrong). So we merge in TWO
                // passes keyed on the baked provenance: own-body members
                // (`declared_in_macro_type_arg == true`) win first, then
                // heritage members fill names not already claimed. Within
                // each provenance class first-writer-wins preserves the
                // genuine `A & B` author-intersection arm order (all arms
                // structural-false → second pass keeps left-to-right order).
                let mut merged = MacroSurfaceView::default();
                let mut seen: FxHashSet<Arc<str>> = FxHashSet::default();
                for own_body_pass in [true, false] {
                    for arm_view in &arm_views {
                        for member in &arm_view.members {
                            if member.declared_in_macro_type_arg != own_body_pass {
                                continue;
                            }
                            if seen.insert(Arc::clone(&member.name)) {
                                merged.members.push(member.clone());
                            }
                        }
                    }
                }
                for arm_view in arm_views {
                    merged.call_signatures.extend(arm_view.call_signatures);
                }
                Some(merged)
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
                self.surface_view_from_decl_identity(identity, resolved, provenance)
            }
            // Lazy declaration-reference carrier (`Navigate` / `Skeleton`
            // lowering of a bare `TypeExpr::Ref`). A cross-file heritage
            // arm (`extends Base` where `Base` is imported) lowers to a
            // `DeclRef` in the carrier-preserving modes; the Intersection
            // recursion above reaches it here. Resolve it through
            // `ResolveDecl` (the same "aliases follow" unwrap the walker
            // performs) to the declaration's `DeclPlaceholder`, then read
            // that surface — WITHOUT eagerly expanding (the
            // `surface_view_from_decl_identity` helper instantiates in
            // `Skeleton`, never `Expanded`, so the slot-binding eagerness
            // guard `enrich_does_not_eagerly_instantiate_carrier` stays at
            // zero synthesis-attributable `Expanded` Instantiate calls).
            SemanticNodeData::DeclRef { identity } => {
                let scope = crate::semantic_query::ScopeId {
                    canonical_id: Arc::clone(&identity.canonical_id),
                    local_scope: None,
                };
                let name = Arc::clone(&identity.decl_name);
                drop(data);
                match self.execute(crate::semantic_query::SemanticQueryKey::ResolveDecl(
                    crate::semantic_query::ResolveDeclKey { scope, name },
                )) {
                    crate::semantic_query::QueryResult::Value(decl) if decl != resolved => {
                        self.surface_view_from_base_node(decl, provenance)
                    }
                    _ => None,
                }
            }
            // Lazy generic-application carrier (`Navigate` / `Skeleton`
            // lowering of a `TypeExpr::Ref` with type arguments). A
            // cross-file generic heritage arm (`extends Base<string>`)
            // lowers to an `InstantiationRef`. Instantiate it under the
            // surface reader's `Skeleton` demand carrying the caller's
            // provenance, then read the instantiated body's surface. The
            // `args` are the captured generic arguments, so the substituted
            // member types surface (generic substitution is semantic
            // meaning). Like the `DeclRef` arm this never asks for the
            // `Expanded` body mode.
            SemanticNodeData::InstantiationRef { base, args } => {
                let base = base.clone();
                let args = Arc::clone(args);
                drop(data);
                match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
                    base: base.to_decl_key(),
                    args,
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Skeleton,
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
            // Union-typed macro payload: the TS-correct shallow surface is
            // the COMMON members — a key present in EVERY arm, typed as the
            // union of the per-arm member value types. Mirrors
            // `key_names_from_base_node`'s `CombineUnion` key intersection,
            // but synthesizes full members (value/optional/readonly) so the
            // macro interpretation reads types, not just names. A disjoint
            // union → no common keys → empty surface (the documented
            // common-members-only contract); an overlapping union → the
            // shared members only. Union-arm members are reached THROUGH the
            // union (not written at the macro-T root), so each arm recurses
            // STRUCTURALLY and the synthesized common member is `false`.
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                self.union_common_member_surface(&arms)
            }
            // Primitives, deferred shells, literals, type params, … have no
            // single member surface a macro payload reads.
            _ => None,
        }
    }

    /// Instantiate a declaration identity in `Skeleton` mode and read its
    /// one-level surface (the `DeclPlaceholder` / `DeclRef` carrier path of
    /// [`Self::surface_view_from_base_node`]).
    ///
    /// **Skeleton, never Expanded (the eagerness-guard coupling).** The macro-surface
    /// reader drives every carrier unwrap in a shallow / carrier-preserving
    /// mode: an `extends Base` heritage arm folds into an
    /// `Intersection([Object{own}, DeclRef{Base}])` whose `DeclRef` arm is
    /// resolved by the `DeclRef` match arm above — itself routing back here
    /// — so the inherited members surface WITHOUT the reader ever asking
    /// `build_instantiate` for the `Expanded` body mode. This keeps the
    /// synthesis-attributable `Expanded` Instantiate count at zero (the
    /// slot-binding eagerness guard `enrich_does_not_eagerly_instantiate_carrier`).
    ///
    /// `Skeleton` (vs `Navigate`) preserves carrier shells for unbound
    /// generic helpers so a `Conditional` heritage body does not collapse
    /// to `never`; for the common heritage / intersection / object surface
    /// the two modes lower identically (both carrier-preserving on `Ref`).
    /// The caller's `provenance` flows onto the instantiate so the
    /// declaration's OWN-body members keep `declared_in_macro_type_arg`
    /// while heritage `Ref` arms decay to structural inside
    /// `lower_decl_body_with_provenance`.
    fn surface_view_from_decl_identity(
        &self,
        identity: crate::semantic_query::DeclIdentity,
        resolved: SemanticNodeId,
        provenance: crate::semantic_query::SurfaceProvenanceContext,
    ) -> Option<MacroSurfaceView> {
        match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
            base: identity.to_decl_key(),
            args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Skeleton,
            )
            .with_provenance(provenance),
        }) {
            crate::semantic_query::QueryResult::Value(instantiated) if instantiated != resolved => {
                self.surface_view_from_base_node(instantiated, provenance)
            }
            _ => None,
        }
    }

    /// Synthesize the common-member surface of a union's arms (the
    /// `SemanticNodeData::Union` case of [`Self::surface_view_from_base_node`]).
    ///
    /// A member is published iff it is present (by name) in EVERY
    /// **resolvable** arm; its value type is the union of the per-arm
    /// member value nodes. This is the TS-correct shallow reading of a
    /// union-typed payload: `(A | B)['k']` is well-typed only when `k`
    /// exists in both `A` and `B`, and its type is `A['k'] | B['k']`.
    ///
    /// - Disjoint union (no shared key) → empty `MacroSurfaceView` (NOT
    ///   `None`: the union resolved, it simply has no common members —
    ///   distinct from an unresolvable carrier).
    /// - Any arm with no readable surface (a deferred carrier, a
    ///   primitive, …) makes the whole result `None`: a union whose arm
    ///   cannot be read has an unknown common-member set, so the reader
    ///   must not publish a partial intersection.
    /// - Member flags: optional iff optional in ANY arm (the union value
    ///   admits the optional case); readonly iff readonly in ALL arms (a
    ///   union property is writable only when writable in every arm);
    ///   `is_method` / `declared_in_macro_type_arg` are `false` on the
    ///   synthesized member (it carries a union value, not a literal
    ///   method, and is reached through the union rather than the macro-T
    ///   own body).
    /// - Call signatures: a union has no single call surface, so the
    ///   synthesized view carries none.
    fn union_common_member_surface(&self, arms: &[SemanticNodeId]) -> Option<MacroSurfaceView> {
        if arms.is_empty() {
            return Some(MacroSurfaceView::default());
        }
        // Read each arm's surface structurally (union-arm members are not
        // the macro-T own body). One unreadable arm → unknown common set.
        let arm_views: Vec<MacroSurfaceView> = arms
            .iter()
            .map(|arm| {
                self.surface_view_from_base_node(
                    *arm,
                    crate::semantic_query::SurfaceProvenanceContext::Structural,
                )
            })
            .collect::<Option<Vec<_>>>()?;

        // Keys common to ALL arms, in the first arm's declaration order.
        let first = &arm_views[0];
        let mut members: Vec<SurfaceMember> = Vec::new();
        for first_member in &first.members {
            // Collect this member's value node from every arm; skip the
            // key entirely if any arm lacks it (not a common member).
            let mut per_arm_values: Vec<SemanticNodeId> = Vec::with_capacity(arm_views.len());
            let mut optional_in_any = false;
            let mut readonly_in_all = true;
            let mut present_in_all = true;
            for arm_view in &arm_views {
                match arm_view
                    .members
                    .iter()
                    .find(|m| m.name == first_member.name)
                {
                    Some(arm_member) => {
                        per_arm_values.push(arm_member.value);
                        optional_in_any |= arm_member.optional;
                        readonly_in_all &= arm_member.readonly;
                    }
                    None => {
                        present_in_all = false;
                        break;
                    }
                }
            }
            if !present_in_all {
                continue;
            }
            // Value type = union of the per-arm member values. A single
            // shared value node stays as-is (no singleton union wrapper).
            let value = if per_arm_values.len() == 1 {
                per_arm_values[0]
            } else {
                self.graph().intern_node(SemanticNodeData::Union(Arc::from(
                    per_arm_values.into_boxed_slice(),
                )))
            };
            members.push(SurfaceMember {
                name: Arc::clone(&first_member.name),
                value,
                optional: optional_in_any,
                readonly: readonly_in_all,
                is_method: false,
                declared_in_macro_type_arg: false,
                // A synthesized union common member is reached THROUGH the
                // union, not the macro-T own body or a heritage overlay —
                // `Authored` (it never shadows / is shadowed).
                merge_role: crate::semantic_query::MemberMergeRole::Authored,
                // Union common-member: present in every arm, no single source
                // declaration site — genuinely synthetic. No spans and no
                // single declaration file (a multi-origin fact).
                spans: verter_type_expr::MemberSpans::default(),
                declaration_origin: None,
            });
        }
        Some(MacroSurfaceView {
            members,
            call_signatures: Vec::new(),
        })
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
