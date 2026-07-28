//! The fresh excess-property prepass — the union excess algorithm the
//! relation authority runs ONCE per frame, before ordinary union-arm
//! distribution, when the key's gate holds:
//!
//! ```text
//! source_freshness == Fresh && policy.excess_property_check
//!     && is_excess_check_target(target)
//! ```
//!
//! Algorithm (TS 5.4.5 `hasExcessProperties`):
//!
//! 1. broad-target skips: an empty-object-like target (`{}`, the `object`
//!    nonprimitive, unions/intersections thereof) or a global-`Object`
//!    supertype target skips excess checking entirely;
//! 2. a union target reduces: matching-discriminant narrowing first
//!    (property identities + ordinary relation on discriminant value types —
//!    may yield one arm or a subunion, never branch-local excess checks),
//!    else primitive filtering when the union contains the nonprimitive;
//! 3. candidates are EXACTLY the source members whose identity-borne origin
//!    is `FreshOwn` (direct properties, shorthand, methods, accessors);
//!    `SpreadTainted` / `NonLiteral` members are never candidates;
//! 4. a candidate name must be KNOWN in some remaining arm — a declared
//!    property or an index signature applicable to that exact key (the
//!    structured key type, never a blanket has-index bit) — else the frame
//!    rejects;
//! 5. for a UNION target, the candidate's value must relate to the union of
//!    the reduced arms' property/index value types (`undefined` for arms
//!    without the name): `NotAssignable` rejects, `Unknown` propagates
//!    `Unknown` (never collapsed), `Assignable` continues;
//! 6. passing the prepass proves nothing by itself — ordinary structural
//!    relation continues against the ORIGINAL target in the same frame, so
//!    union alternatives never rerun branch-local excess checking.
//!
//! **Engine capability without a production caller.** No non-test code path
//! constructs a [`RelateMemoKey`] with `excess_property_check: true` — the
//! only non-test references are the `RelationPolicy` field, the reducer's
//! gate, and oracle identity plumbing defaulting it to `false`. The prepass
//! becomes user-visible only when a checking surface issues a `Fresh` +
//! excess ask for an object-literal EXPRESSION source against its expected
//! type: a value-declaration initializer against its annotation, a call
//! argument against its parameter type, a return expression against the
//! declared return type, or a macro object argument against its declared
//! surface. The behavior is proven by the direct relation-level suite
//! (`fresh_excess_property_checking` in
//! `project_semantic_dispatch_invariants_tests.rs`), not by an end-to-end
//! fixture.
//!
//! **Present semantic gaps in this prepass.**
//!
//! - `arm_property_or_index_value`: when an arm remains an UNRESOLVABLE
//!   carrier, or reaches the non-object `_` fallthrough, the site can
//!   currently mint a fabricated `undefined` into the expected union. That
//!   can let a nested-union value check `Pass` against a widened expectation
//!   instead of remaining undecided. Resolution exhaustion already
//!   propagates `Undecided` and contributes no `undefined`; the residual
//!   carriers and non-object fallthrough must do the same. Only a genuinely
//!   missing property should contribute `undefined`.
//! - `is_global_object_reference`: global-`Object` recognition matches a
//!   `DeclRef` with the builtin/ambient canonical identity, but an ambient
//!   `Object` arriving as `Opaque(DeclPlaceholder)` is unwrapped before
//!   identification and is therefore not recognized. Excess checking can
//!   then run against the global object surface and reject extras. The fix is
//!   to match the same identity on the placeholder before unwrapping it.
//! - `is_empty_object_like_type` and `is_excess_check_target` resolve `Alias`
//!   (and the target predicate resolves `DeclRef`/`InstantiationRef`) but do
//!   not resolve named carriers in every position: union arms are not
//!   resolved before the emptiness skip, and
//!   `Opaque(DeclPlaceholder)` is missed. A union containing a named
//!   empty-object arm (`type NamedEmpty = {}` or `interface IEmpty {}`)
//!   alongside `{ a: number }` can therefore miss the emptiness skip and
//!   falsely reject an extra property that TypeScript accepts. The fix is to
//!   resolve carriers in both classifiers, with exhaustion remaining
//!   undecided.

use std::sync::Arc;

use verter_type_expr::ExcessPropertyOrigin;

use super::relation::InferPosition;
use super::relation_predicates::index_signature_applies_to_property;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ClosedSurfaceView, InferBinding, PrimitiveKind, RelateMemoKey, RelationResult,
    SemanticNodeData, SemanticNodeId, SurfaceMember,
};

/// Three-valued prepass outcome. `Pass` continues into the ordinary
/// structural relation; the other two decide the frame.
pub(super) enum ExcessPrepassOutcome {
    /// No candidate violated the reduced target — continue relating.
    Pass,
    /// A fresh candidate is unknown in every remaining arm, or its value
    /// fails the reduced-arm union — the frame is `NotAssignable`.
    Reject,
    /// A required judgement (arm classification / value check) is
    /// undecidable — the frame stays `Unknown`, never collapsed.
    Undecided,
}

/// One resolved excess-target position: reference carriers resolve through
/// the shared dispatch BEFORE classification (TS's resolve-then-check
/// order); the global-`Object` interface is recognized on the RESOLVED
/// declaration identity.
enum ResolvedExcessNode {
    /// A concrete (or structurally classifiable) node.
    Node(SemanticNodeId),
    /// The GLOBAL `Object` interface (builtin / ambient-lib identity) —
    /// the broad-target skip.
    GlobalObject,
    /// The reference cannot be resolved — classification is impossible.
    Unresolvable,
    /// A resolution budget was exhausted — "gave up" is fail-closed and
    /// NEVER indistinguishable from "no excess": the prepass surfaces it
    /// as `Undecided`, not a silent Pass.
    Exhausted,
}

/// Tri-state discriminant slot of one arm (see
/// [`ProjectSemanticDispatch::arm_declared_property_value`]).
enum ArmDiscriminant {
    /// The arm declares the property with this value type.
    Value(SemanticNodeId),
    /// The arm decidedly has NO such declared property.
    Absent,
    /// The arm cannot be classified — membership is undecidable.
    Undecidable,
}

/// Tri-state known-name verdict for one arm (see
/// [`ProjectSemanticDispatch::arm_knows_property`]).
enum ArmKnows {
    Yes,
    No,
    /// The arm cannot be classified (unresolved carrier / open surface
    /// marker without structured infos) — known-ness is undecidable.
    Undecidable,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Run the fresh excess-property prepass for `key`'s frame. The caller
    /// has already checked the KEY half of the gate (`Fresh` +
    /// `excess_property_check` on an `Assignable` ask).
    pub(super) fn excess_property_prepass(
        &self,
        key: &RelateMemoKey,
        bindings: &mut Vec<InferBinding>,
    ) -> ExcessPrepassOutcome {
        let graph = self.graph();
        // Candidates live only on a direct Object-surface source (a fresh
        // literal IS a surface; every other shape has no FreshOwn members).
        let source_view = match graph.node_data(key.source).as_deref() {
            Some(SemanticNodeData::Object(view)) => view.clone(),
            _ => return ExcessPrepassOutcome::Pass,
        };
        let Some(source_closed) = source_view.closed() else {
            return ExcessPrepassOutcome::Undecided;
        };
        let candidates: Vec<SurfaceMember> = source_closed
            .complete_members()
            .iter()
            .filter(|m| m.excess_origin == ExcessPropertyOrigin::FreshOwn)
            .cloned()
            .collect();
        if candidates.is_empty() {
            return ExcessPrepassOutcome::Pass;
        }

        // Resolve the target BEFORE classification (TS's resolve-then-check
        // order): a named reference is the common target shape. The
        // global-`Object` skip fires on the resolved declaration identity;
        // an unresolvable reference cannot be classified — the prepass
        // passes and the ordinary relation stays honest about the carrier.
        let target = match self.resolve_excess_node(key.target) {
            ResolvedExcessNode::Node(node) => node,
            ResolvedExcessNode::GlobalObject | ResolvedExcessNode::Unresolvable => {
                return ExcessPrepassOutcome::Pass;
            }
            // A resolution budget ran out: fail closed — the ordinary
            // relation (which resolves without this cap) could accept on
            // width, and a silent Pass would make "gave up" read as
            // "no excess".
            ResolvedExcessNode::Exhausted => return ExcessPrepassOutcome::Undecided,
        };

        // Typed target-classification gate + broad-target skips. Both
        // predicates are tri-state: an exhausted classification is
        // undecided, never a silent skip.
        match self.is_excess_check_target(target, 0) {
            Some(false) => return ExcessPrepassOutcome::Pass,
            None => return ExcessPrepassOutcome::Undecided,
            Some(true) => {}
        }
        match self.is_empty_object_like_type(target, 0) {
            Some(true) => return ExcessPrepassOutcome::Pass,
            None => return ExcessPrepassOutcome::Undecided,
            Some(false) => {}
        }

        // Union-target reduction: resolve each arm (a global-`Object` arm
        // skips the whole check — the union is an Object supertype; an
        // unresolvable arm stays a carrier, which the known-name check
        // treats as undecidable), then discriminant narrowing, else
        // primitive filtering. A non-union target checks known-ness only
        // (its value compatibility is the ordinary relation's job). A
        // reduction CONTAMINATED by an undecidable discriminant keeps every
        // affected arm and forbids the accept side (sound rejects only).
        let mut reduction_contaminated = false;
        let (check_arms, target_was_union): (Vec<SemanticNodeId>, bool) =
            match graph.node_data(target).as_deref() {
                Some(SemanticNodeData::Union(arms)) => {
                    let raw_arms: Vec<SemanticNodeId> = arms.to_vec();
                    let mut resolved_arms: Vec<SemanticNodeId> = Vec::with_capacity(raw_arms.len());
                    for arm in raw_arms {
                        match self.resolve_excess_node(arm) {
                            ResolvedExcessNode::Node(node) => resolved_arms.push(node),
                            ResolvedExcessNode::GlobalObject => {
                                return ExcessPrepassOutcome::Pass;
                            }
                            // Keep the carrier: known-ness over it is
                            // undecidable, never silently absent.
                            ResolvedExcessNode::Unresolvable => resolved_arms.push(arm),
                            // Fail closed on an exhausted arm resolution.
                            ResolvedExcessNode::Exhausted => {
                                return ExcessPrepassOutcome::Undecided;
                            }
                        }
                    }
                    let reduced = self
                        .find_matching_discriminant_arms(
                            source_closed,
                            &resolved_arms,
                            &mut reduction_contaminated,
                        )
                        .unwrap_or_else(|| {
                            filter_primitives_if_contains_nonprimitive(self, &resolved_arms)
                        });
                    (reduced, true)
                }
                _ => (vec![target], false),
            };

        for candidate in &candidates {
            // Known-name check over the remaining arms: any arm that knows
            // the name admits it; all-No rejects; a No/Undecidable mix
            // cannot decide the rejection — propagate Unknown.
            let mut any_undecidable = false;
            let mut known = false;
            for arm in &check_arms {
                match self.arm_knows_property(*arm, candidate.name.as_ref(), 0) {
                    ArmKnows::Yes => {
                        known = true;
                        break;
                    }
                    ArmKnows::No => {}
                    ArmKnows::Undecidable => any_undecidable = true,
                }
            }
            if !known {
                if any_undecidable {
                    return ExcessPrepassOutcome::Undecided;
                }
                return ExcessPrepassOutcome::Reject;
            }

            // Union-target value check against the reduced arms' property /
            // index value union (`undefined` for arms without the name).
            if target_was_union {
                let mut expected: Vec<SemanticNodeId> = Vec::with_capacity(check_arms.len());
                for arm in &check_arms {
                    let Some(value) =
                        self.arm_property_or_index_value(*arm, candidate.name.as_ref())
                    else {
                        return ExcessPrepassOutcome::Undecided;
                    };
                    expected.push(value);
                }
                expected.dedup();
                let expected_node = match expected.as_slice() {
                    [single] => *single,
                    _ => graph.intern_node(SemanticNodeData::Union(Arc::from(
                        expected.into_boxed_slice(),
                    ))),
                };
                match self.relate_member(
                    candidate.value,
                    expected_node,
                    bindings,
                    InferPosition::Covariant,
                ) {
                    // Sound under contamination too: the kept arm set is a
                    // SUPERSET of the true reduction, so failing its wider
                    // expected union implies failing the true one.
                    RelationResult::NotAssignable => return ExcessPrepassOutcome::Reject,
                    RelationResult::Unknown => return ExcessPrepassOutcome::Undecided,
                    RelationResult::Assignable { .. } => {}
                }
            }
        }
        // Rejections above are sound against the kept-arm superset; an
        // ACCEPT that depended on a contaminated reduction is not — the
        // undecidable discriminant could legitimately have dropped the arm
        // that made a candidate known / value-compatible.
        if reduction_contaminated {
            return ExcessPrepassOutcome::Undecided;
        }
        ExcessPrepassOutcome::Pass
    }

    /// Resolve one target/arm position: follow `Alias`, resolve
    /// `DeclRef` / `InstantiationRef` through the shared dispatch
    /// ([`Self::unwrap_identity_carrier_for_relation`] — the same
    /// `execute(Instantiate)` path the ordinary relation uses), and
    /// recognize the global `Object` interface on the reference's
    /// declaration identity (a userland `Object` declaration is a merge
    /// partner of the global in TS, so the skip applies to it too). A bare
    /// unresolved `Object` reference is recognized pre-resolution.
    fn resolve_excess_node(&self, node: SemanticNodeId) -> ResolvedExcessNode {
        let graph = self.graph();
        let mut current = node;
        let mut carrier_hops: u8 = 0;
        let mut alias_hops: u32 = 0;
        // bounded-loop: alias chains are acyclic by interning order (an
        // `Alias` can only reference an earlier node), so following them is
        // finite; the 4096 alias cap and the 8 carrier-unwrap cap are
        // defensive budgets whose exhaustion is EXHAUSTED (fail-closed),
        // never a silent skip.
        loop {
            if self.is_global_object_reference(current) {
                return ResolvedExcessNode::GlobalObject;
            }
            match graph.node_data(current).as_deref() {
                Some(SemanticNodeData::Alias(inner)) => {
                    alias_hops += 1;
                    if alias_hops > 4096 {
                        return ResolvedExcessNode::Exhausted;
                    }
                    current = *inner;
                }
                Some(
                    SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                        ..
                    }),
                ) => {
                    carrier_hops += 1;
                    if carrier_hops > 8 {
                        return ResolvedExcessNode::Exhausted;
                    }
                    match self.unwrap_identity_carrier_for_relation(current) {
                        super::relation::IdentityCarrierUnwrap::Concrete(resolved) => {
                            if resolved == current {
                                return ResolvedExcessNode::Node(current);
                            }
                            current = resolved;
                        }
                        super::relation::IdentityCarrierUnwrap::Unresolvable => {
                            return ResolvedExcessNode::Unresolvable;
                        }
                    }
                }
                Some(_) => return ResolvedExcessNode::Node(current),
                None => return ResolvedExcessNode::Unresolvable,
            }
        }
    }

    /// TS `isExcessPropertyCheckTarget`: an object carrier, the `object`
    /// nonprimitive, a union with SOME excess target arm, or an intersection
    /// of ALL excess target arms. Structural over the typed graph — never a
    /// spelling check. Tri-state: `Some(bool)` is a decided classification,
    /// `None` an EXHAUSTED one (nesting / resolution budget) — the caller
    /// fails closed, never a silent skip. Unresolved carriers classify
    /// `Some(false)` (the ordinary relation stays honest about the carrier).
    fn is_excess_check_target(&self, node: SemanticNodeId, depth: usize) -> Option<bool> {
        if depth > 8 {
            return None;
        }
        let graph = self.graph();
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Object(_)) => Some(true),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Object)) => Some(true),
            Some(SemanticNodeData::Alias(inner)) => self.is_excess_check_target(*inner, depth + 1),
            Some(SemanticNodeData::DeclRef { .. } | SemanticNodeData::InstantiationRef { .. }) => {
                match self.resolve_excess_node(node) {
                    ResolvedExcessNode::Node(resolved) if resolved != node => {
                        self.is_excess_check_target(resolved, depth + 1)
                    }
                    // The global Object interface is an object target (its
                    // SKIP is decided separately by the caller's resolution).
                    ResolvedExcessNode::GlobalObject => Some(true),
                    ResolvedExcessNode::Exhausted => None,
                    _ => Some(false),
                }
            }
            Some(SemanticNodeData::Union(arms)) => {
                let arms = arms.clone();
                let mut exhausted = false;
                for arm in arms.iter() {
                    match self.is_excess_check_target(*arm, depth + 1) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => exhausted = true,
                    }
                }
                if exhausted {
                    None
                } else {
                    Some(false)
                }
            }
            Some(SemanticNodeData::Intersection(arms)) => {
                let arms = arms.clone();
                let mut exhausted = false;
                for arm in arms.iter() {
                    match self.is_excess_check_target(*arm, depth + 1) {
                        Some(false) => return Some(false),
                        Some(true) => {}
                        None => exhausted = true,
                    }
                }
                if exhausted {
                    None
                } else {
                    Some(true)
                }
            }
            _ => Some(false),
        }
    }

    /// TS `isEmptyObjectType` over the graph: the empty resolved surface,
    /// the `object` nonprimitive, a union with SOME empty arm, an
    /// intersection of ALL empty arms. Tri-state like
    /// [`Self::is_excess_check_target`]: `None` = exhausted, fail closed.
    fn is_empty_object_like_type(&self, node: SemanticNodeId, depth: usize) -> Option<bool> {
        if depth > 8 {
            return None;
        }
        let graph = self.graph();
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Object(view)) => view.closed().map(|closed| closed.is_empty()),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Object)) => Some(true),
            Some(SemanticNodeData::Alias(inner)) => {
                self.is_empty_object_like_type(*inner, depth + 1)
            }
            Some(SemanticNodeData::Union(arms)) => {
                let arms = arms.clone();
                let mut exhausted = false;
                for arm in arms.iter() {
                    match self.is_empty_object_like_type(*arm, depth + 1) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => exhausted = true,
                    }
                }
                if exhausted {
                    None
                } else {
                    Some(false)
                }
            }
            Some(SemanticNodeData::Intersection(arms)) => {
                let arms = arms.clone();
                let mut exhausted = false;
                for arm in arms.iter() {
                    match self.is_empty_object_like_type(*arm, depth + 1) {
                        Some(false) => return Some(false),
                        Some(true) => {}
                        None => exhausted = true,
                    }
                }
                if exhausted {
                    None
                } else {
                    Some(true)
                }
            }
            _ => Some(false),
        }
    }

    /// The TS `isTypeSubsetOf(globalObjectType, target)` half of the skip:
    /// a reference whose declaration identity IS the GLOBAL `Object`
    /// interface — the builtin sentinel namespace or an ambient lib
    /// canonical. The NAME alone never grants the skip: a module-local
    /// `type Object = { … }` is an ordinary named target and stays
    /// excess-checked (TS parity — only the true global/ambient identity is
    /// the Object supertype). Consulted per position during
    /// [`Self::resolve_excess_node`]; a union containing the reference
    /// skips through its arm resolution.
    fn is_global_object_reference(&self, node: SemanticNodeId) -> bool {
        let graph = self.graph();
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => {
                identity.decl_name.as_ref() == "Object"
                    && (identity.canonical_id.as_ref() == "__builtin__"
                        || identity.canonical_id.starts_with("ambient:/"))
            }
            _ => false,
        }
    }

    /// TS `findMatchingDiscriminantType` + `discriminateTypeByDiscriminableItems`
    /// over the union arms: source members whose value is a UNIT type and
    /// whose name is a union discriminant (literal-typed, non-uniform across
    /// declaring arms) progressively narrow the arm set through ordinary
    /// relation on the discriminant value types. Returns `None` when no
    /// discriminator narrows anything (the caller falls back to primitive
    /// filtering).
    ///
    /// Three-valued discriminant matching: an `Assignable` sub-relation
    /// matches the arm; a DECIDED `NotAssignable` marks it droppable (it
    /// drops only when some arm matched); an `Unknown` sub-relation — or an
    /// arm whose discriminant slot is itself undecidable — KEEPS the arm
    /// unconditionally and marks the reduction CONTAMINATED via
    /// `contaminated`, so the caller never publishes an accept that
    /// depended on the undecided membership.
    fn find_matching_discriminant_arms(
        &self,
        source: ClosedSurfaceView<'_>,
        arms: &[SemanticNodeId],
        contaminated: &mut bool,
    ) -> Option<Vec<SemanticNodeId>> {
        let graph = self.graph();
        // Primitive arms start excluded (TS pre-seeds them `False`), but a
        // reduction that would drop EVERYTHING reverts to the full target.
        let mut include: Vec<bool> = arms
            .iter()
            .map(|arm| {
                !matches!(
                    graph.node_data(*arm).as_deref(),
                    Some(SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_))
                )
            })
            .collect();

        let discriminators: Vec<&SurfaceMember> = source
            .complete_members()
            .iter()
            .filter(|m| self.is_unit_type(m.value) && self.is_union_discriminant(arms, &m.name))
            .collect();
        if discriminators.is_empty() {
            return None;
        }

        let mut bindings: Vec<InferBinding> = Vec::new();
        for discriminator in discriminators {
            let mut matched = false;
            // `Maybe` marks arms whose discriminant DECIDEDLY did not match
            // this discriminator; they drop only when SOME arm matched. An
            // undecidable slot or an `Unknown` sub-relation never marks the
            // arm — it stays included and contaminates the reduction.
            let mut maybe: Vec<bool> = vec![false; arms.len()];
            for (index, arm) in arms.iter().enumerate() {
                if !include[index] {
                    continue;
                }
                match self.arm_declared_property_value(*arm, discriminator.name.as_ref()) {
                    ArmDiscriminant::Value(value) => match self.relate_member(
                        discriminator.value,
                        value,
                        &mut bindings,
                        InferPosition::Covariant,
                    ) {
                        RelationResult::Assignable { .. } => matched = true,
                        RelationResult::NotAssignable => maybe[index] = true,
                        RelationResult::Unknown => *contaminated = true,
                    },
                    // A DECIDED absence: the arm has no such declared
                    // property — droppable when some arm matched.
                    ArmDiscriminant::Absent => maybe[index] = true,
                    // Undecidable slot: keep the arm, contaminate.
                    ArmDiscriminant::Undecidable => *contaminated = true,
                }
            }
            if matched {
                for (index, is_maybe) in maybe.iter().enumerate() {
                    if *is_maybe {
                        include[index] = false;
                    }
                }
            }
        }

        let filtered: Vec<SemanticNodeId> = arms
            .iter()
            .zip(include.iter())
            .filter_map(|(arm, keep)| keep.then_some(*arm))
            .collect();
        if filtered.is_empty() || filtered.len() == arms.len() {
            return None;
        }
        Some(filtered)
    }

    /// Whether `name` is a DISCRIMINANT of the union: at least one declaring
    /// arm carries a literal-typed value for it and the declared values are
    /// not uniform across the declaring arms.
    fn is_union_discriminant(&self, arms: &[SemanticNodeId], name: &str) -> bool {
        let mut declared: Vec<SemanticNodeId> = Vec::new();
        for arm in arms {
            if let ArmDiscriminant::Value(value) = self.arm_declared_property_value(*arm, name) {
                declared.push(value);
            }
        }
        if declared.is_empty() {
            return false;
        }
        let any_literal = declared.iter().any(|value| self.is_unit_type(*value));
        let non_uniform = declared.iter().any(|value| *value != declared[0]);
        any_literal && (non_uniform || declared.len() < arms.len())
    }

    /// A unit (single-value) type: a literal, `null`, or `undefined`.
    fn is_unit_type(&self, node: SemanticNodeId) -> bool {
        matches!(
            self.graph().node_data(node).as_deref(),
            Some(
                SemanticNodeData::Literal(_)
                    | SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined)
            )
        )
    }

    /// The DECLARED property value of `name` on one arm's object surface.
    /// `Absent` is a DECIDED no (a classifiable arm without the property);
    /// an arm that cannot be classified (unresolved carrier / non-surface
    /// shape whose members are unknowable) is `Undecidable` — never a
    /// silent no. Index signatures do not contribute here — discriminants
    /// are declared properties.
    fn arm_declared_property_value(&self, arm: SemanticNodeId, name: &str) -> ArmDiscriminant {
        let arm = match self.resolve_excess_node(arm) {
            ResolvedExcessNode::Node(node) => node,
            ResolvedExcessNode::GlobalObject
            | ResolvedExcessNode::Unresolvable
            | ResolvedExcessNode::Exhausted => {
                return ArmDiscriminant::Undecidable;
            }
        };
        match self.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                let Some(closed) = view.closed() else {
                    return ArmDiscriminant::Undecidable;
                };
                match closed
                    .complete_members()
                    .iter()
                    .find(|member| member.name.as_ref() == name)
                {
                    Some(member) => ArmDiscriminant::Value(member.value),
                    None => ArmDiscriminant::Absent,
                }
            }
            Some(SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_)) => {
                ArmDiscriminant::Absent
            }
            _ => ArmDiscriminant::Undecidable,
        }
    }

    /// TS `isKnownProperty`: a declared property with the name, or an index
    /// signature APPLICABLE to that exact key (the structured key type —
    /// never the blanket `has_index_signature` bit); unions/intersections
    /// recurse. An open-surface marker without structured infos, or an
    /// unresolved carrier, is undecidable.
    fn arm_knows_property(&self, arm: SemanticNodeId, name: &str, depth: usize) -> ArmKnows {
        if depth > 8 {
            return ArmKnows::Undecidable;
        }
        // Resolve reference carriers before classification (a named arm is
        // the common case); an unresolvable arm is undecidable, and a
        // global-`Object` arm cannot decide a userland name.
        let arm = match self.resolve_excess_node(arm) {
            ResolvedExcessNode::Node(node) => node,
            ResolvedExcessNode::GlobalObject
            | ResolvedExcessNode::Unresolvable
            | ResolvedExcessNode::Exhausted => {
                return ArmKnows::Undecidable;
            }
        };
        let graph = self.graph();
        match graph.node_data(arm).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                let Some(closed) = view.closed() else {
                    return ArmKnows::Undecidable;
                };
                if closed
                    .complete_members()
                    .iter()
                    .any(|m| m.name.as_ref() == name)
                {
                    return ArmKnows::Yes;
                }
                for info in view.index_signatures.iter() {
                    if index_signature_applies_to_property(graph, info.key_type, name) {
                        return ArmKnows::Yes;
                    }
                }
                // A blanket open-surface marker without structured key infos
                // cannot decide applicability.
                if view.has_known_index_signature() && view.index_signatures.is_empty() {
                    return ArmKnows::Undecidable;
                }
                ArmKnows::No
            }
            Some(SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms)) => {
                let arms = arms.clone();
                let mut any_undecidable = false;
                for inner in arms.iter() {
                    match self.arm_knows_property(*inner, name, depth + 1) {
                        ArmKnows::Yes => return ArmKnows::Yes,
                        ArmKnows::No => {}
                        ArmKnows::Undecidable => any_undecidable = true,
                    }
                }
                if any_undecidable {
                    ArmKnows::Undecidable
                } else {
                    ArmKnows::No
                }
            }
            Some(SemanticNodeData::Alias(inner)) => {
                self.arm_knows_property(*inner, name, depth + 1)
            }
            // A primitive arm knows nothing (TS pre-excludes primitives).
            Some(SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_)) => ArmKnows::No,
            // Anything else (unresolved carriers, deferred shells) cannot be
            // classified — undecidable, never a fabricated rejection.
            _ => ArmKnows::Undecidable,
        }
    }

    /// TS `getTypeOfPropertyInTypes` per arm: the declared property type,
    /// else the applicable index-signature value type, else `undefined`.
    /// Resolution exhaustion returns `None` so the enclosing value check
    /// remains undecided.
    fn arm_property_or_index_value(
        &self,
        arm: SemanticNodeId,
        name: &str,
    ) -> Option<SemanticNodeId> {
        // Resolve reference carriers before looking up a value slot.
        // Exhaustion is not evidence of absence and therefore cannot enter
        // the ordinary `undefined` fallback below.
        let arm = match self.resolve_excess_node(arm) {
            ResolvedExcessNode::Node(node) => node,
            ResolvedExcessNode::GlobalObject | ResolvedExcessNode::Unresolvable => arm,
            ResolvedExcessNode::Exhausted => return None,
        };
        let graph = self.graph();
        match graph.node_data(arm).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                let closed = view.closed()?;
                if let Some(member) = closed
                    .complete_members()
                    .iter()
                    .find(|member| member.name.as_ref() == name)
                {
                    return Some(member.value);
                }
                for info in view.index_signatures.iter() {
                    if index_signature_applies_to_property(graph, info.key_type, name) {
                        return Some(info.value_type);
                    }
                }
                if view.has_known_index_signature() && view.index_signatures.is_empty() {
                    return None;
                }
                Some(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined)))
            }
            Some(SemanticNodeData::Union(arms)) => {
                let arms = arms.clone();
                let mut values: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for inner in arms.iter() {
                    values.push(self.arm_property_or_index_value(*inner, name)?);
                }
                values.dedup();
                Some(match values.as_slice() {
                    [single] => *single,
                    _ => graph.intern_node(SemanticNodeData::Union(Arc::from(
                        values.into_boxed_slice(),
                    ))),
                })
            }
            Some(SemanticNodeData::Intersection(arms)) => {
                // First declaring part wins (bounded approximation of the
                // merged intersection property).
                let arms = arms.clone();
                for inner in arms.iter() {
                    let value = self.arm_property_or_index_value(*inner, name)?;
                    if !matches!(
                        graph.node_data(value).as_deref(),
                        Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined))
                    ) {
                        return Some(value);
                    }
                }
                Some(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined)))
            }
            Some(SemanticNodeData::Alias(inner)) => self.arm_property_or_index_value(*inner, name),
            _ => Some(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined))),
        }
    }

    #[cfg(test)]
    pub(crate) fn open_surface_excess_facts_for_tests(
        &self,
        arm: SemanticNodeId,
        name: &str,
    ) -> (Option<bool>, bool, bool) {
        (
            self.is_empty_object_like_type(arm, 0),
            matches!(self.arm_knows_property(arm, name, 0), ArmKnows::Undecidable),
            matches!(
                self.arm_declared_property_value(arm, name),
                ArmDiscriminant::Undecidable
            ),
        )
    }
}

/// TS `filterPrimitivesIfContainsNonPrimitive`: when the union contains the
/// `object` nonprimitive, primitive arms drop (unless that empties the
/// union).
fn filter_primitives_if_contains_nonprimitive(
    dispatch: &ProjectSemanticDispatch<'_>,
    arms: &[SemanticNodeId],
) -> Vec<SemanticNodeId> {
    let graph = dispatch.graph();
    let contains_nonprimitive = arms.iter().any(|arm| {
        matches!(
            graph.node_data(*arm).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Object))
        )
    });
    if !contains_nonprimitive {
        return arms.to_vec();
    }
    let filtered: Vec<SemanticNodeId> = arms
        .iter()
        .filter(|arm| {
            !matches!(
                graph.node_data(**arm).as_deref(),
                Some(SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_))
            )
        })
        .copied()
        .collect();
    if filtered.is_empty() {
        arms.to_vec()
    } else {
        filtered
    }
}
