//! Discriminating guards + algebra unit tests for the U2 demand lattice
//! (`docs/arch/u2-query-value-domain-design.md` Deliverable #3 — the
//! `ProjectionDemand × EvalPolicy` lattice algebra, §3.1–§3.7).
//!
//! Every test here is DISCRIMINATING: it FAILS against a deliberately
//! broken lattice op and PASSES against the spec-correct one. The three
//! named guards are load-bearing R6 registry targets:
//!
//! - `query_modes_are_presets_over_projection_demand_eval_policy` — each
//!   `ProjectionMode` resolves (via `From<ProjectionMode> for Demand`) to
//!   EXACTLY its §3.7 `(ProjectionDemand, EvalPolicy)` point. Breaking any
//!   preset field mapping fails this.
//! - `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode` —
//!   `Demand::skeleton()` IS `generic_open=TypeParamShells` +
//!   `carrier_stop=StopAtCarrier`, and is INCOMPARABLE to `Expanded`
//!   (different regime ⇒ neither dominates). Making Skeleton use `Bound`
//!   would make it comparable and fail this.
//! - `cache_key_axes_are_minimal_and_normalized` — `apply_mask` is
//!   idempotent, masks `display_needs` to ⊥ unconditionally, and a mask
//!   that drops a load-bearing axis changes the normalized demand.

use std::sync::Arc;
// The NEW lattice vocabulary publishes through the `demand` module …
use verter_session::semantic_query::demand::{
    apply_mask, backfill_points, cached_satisfies, demand_at_hop, relevant_demand_axes,
    AliasPreservation, AxisMask, CarrierStopPolicy, Demand, DemandAxis, DisplayFacet, DisplayNeeds,
    EvalPolicy, GenericOpenPolicy, MaterializedPoint, MaterializedSet, MemberBodyDemand, MergeRole,
    NormalizationDepth, OperatorReduction, ProjectionDemand, ProjectionPath, ProvenanceNeed,
    SurfaceFacet, SurfaceFacetSet, SurfaceRole,
};
// … while the pre-existing `PathSegment`/`ProjectionMode` stay on their owner
// path (§3.6: the demand module does not re-export them).
use verter_session::semantic_query::{PathSegment, ProjectionMode};

fn seg(name: &str) -> PathSegment {
    PathSegment::Member(name.into())
}

fn path(names: &[&str]) -> ProjectionPath {
    ProjectionPath::from_segments(names.iter().map(|n| seg(n)))
}

// ----------------------------------------------------------------------
// NAMED GUARD 1 — query modes are presets over (ProjectionDemand, EvalPolicy)
// ----------------------------------------------------------------------

#[test]
fn query_modes_are_presets_over_projection_demand_eval_policy() {
    // INDEPENDENT ORACLE: every expected `Demand` below is a hand-written
    // struct literal — it NEVER calls `Demand::{identity,navigate,shallow,
    // expanded,skeleton}()`. `From<ProjectionMode>` is implemented via those
    // same constructors, so comparing the `From` output to a constructor would
    // make any shared-constructor field bug pass; the explicit literals make a
    // wrong field in ANY constructor fail the full-equality assertion below.

    // Identity (§3.7 row 1): empty path, no member/body, no sigs;
    // Bound/Plain/Standalone, Keep, None(norm), Leave(op), Continue.
    let expected_identity = Demand {
        projection: ProjectionDemand {
            path: ProjectionPath::empty(),
            facets: SurfaceFacetSet::empty(),
            member_demand: MemberBodyDemand::SetOnly,
            call_signatures: false,
            construct_signatures: false,
            index_signatures: false,
            display_needs: DisplayNeeds::empty(),
        },
        policy: EvalPolicy {
            alias_preservation: AliasPreservation::Keep,
            normalization_depth: NormalizationDepth::None,
            generic_open: GenericOpenPolicy::Bound,
            operator_reduction: OperatorReduction::Leave,
            carrier_stop: CarrierStopPolicy::Continue,
            surface_role: SurfaceRole::Plain,
            provenance: ProvenanceNeed::Drop,
            merge_role: MergeRole::Standalone,
        },
    };
    assert_eq!(Demand::from(ProjectionMode::Identity), expected_identity);

    // Navigate (§3.7 row 2): facets={Members}, SetOnly; Keep,
    // NavigateOnly(norm), op=NavigateOnly, Continue.
    let expected_navigate = Demand {
        projection: ProjectionDemand {
            path: ProjectionPath::empty(),
            facets: SurfaceFacetSet::single(SurfaceFacet::Members),
            member_demand: MemberBodyDemand::SetOnly,
            call_signatures: false,
            construct_signatures: false,
            index_signatures: false,
            display_needs: DisplayNeeds::empty(),
        },
        policy: EvalPolicy {
            alias_preservation: AliasPreservation::Keep,
            normalization_depth: NormalizationDepth::NavigateOnly,
            generic_open: GenericOpenPolicy::Bound,
            operator_reduction: OperatorReduction::NavigateOnly,
            carrier_stop: CarrierStopPolicy::Continue,
            surface_role: SurfaceRole::Plain,
            provenance: ProvenanceNeed::Drop,
            merge_role: MergeRole::Standalone,
        },
    };
    assert_eq!(Demand::from(ProjectionMode::Navigate), expected_navigate);

    // Shallow (§3.7 row 3): empty path, facets={Members}, SetOnly; Keep,
    // None(norm), op=Leave, Continue.
    let expected_shallow = Demand {
        projection: ProjectionDemand {
            path: ProjectionPath::empty(),
            facets: SurfaceFacetSet::single(SurfaceFacet::Members),
            member_demand: MemberBodyDemand::SetOnly,
            call_signatures: false,
            construct_signatures: false,
            index_signatures: false,
            display_needs: DisplayNeeds::empty(),
        },
        policy: EvalPolicy {
            alias_preservation: AliasPreservation::Keep,
            normalization_depth: NormalizationDepth::None,
            generic_open: GenericOpenPolicy::Bound,
            operator_reduction: OperatorReduction::Leave,
            carrier_stop: CarrierStopPolicy::Continue,
            surface_role: SurfaceRole::Plain,
            provenance: ProvenanceNeed::Drop,
            merge_role: MergeRole::Standalone,
        },
    };
    assert_eq!(Demand::from(ProjectionMode::Shallow), expected_shallow);

    // Expanded (§3.7 row 4): facets⊇{Members}, SetPlusBody; Inline,
    // Terminal(norm), op=Reduce, Continue.
    let expected_expanded = Demand {
        projection: ProjectionDemand {
            path: ProjectionPath::empty(),
            facets: SurfaceFacetSet::single(SurfaceFacet::Members),
            member_demand: MemberBodyDemand::SetPlusBody,
            call_signatures: false,
            construct_signatures: false,
            index_signatures: false,
            display_needs: DisplayNeeds::empty(),
        },
        policy: EvalPolicy {
            alias_preservation: AliasPreservation::Inline,
            normalization_depth: NormalizationDepth::Terminal,
            generic_open: GenericOpenPolicy::Bound,
            operator_reduction: OperatorReduction::Reduce,
            carrier_stop: CarrierStopPolicy::Continue,
            surface_role: SurfaceRole::Plain,
            provenance: ProvenanceNeed::Drop,
            merge_role: MergeRole::Standalone,
        },
    };
    assert_eq!(Demand::from(ProjectionMode::Expanded), expected_expanded);

    // Skeleton (§3.7 row 5): SetOnly; TypeParamShells/Plain/Standalone,
    // Keep, op=Leave, carrier_stop=StopAtCarrier.
    let expected_skeleton = Demand {
        projection: ProjectionDemand {
            path: ProjectionPath::empty(),
            facets: SurfaceFacetSet::empty(),
            member_demand: MemberBodyDemand::SetOnly,
            call_signatures: false,
            construct_signatures: false,
            index_signatures: false,
            display_needs: DisplayNeeds::empty(),
        },
        policy: EvalPolicy {
            alias_preservation: AliasPreservation::Keep,
            normalization_depth: NormalizationDepth::None,
            generic_open: GenericOpenPolicy::TypeParamShells,
            operator_reduction: OperatorReduction::Leave,
            carrier_stop: CarrierStopPolicy::StopAtCarrier,
            surface_role: SurfaceRole::Plain,
            provenance: ProvenanceNeed::Drop,
            merge_role: MergeRole::Standalone,
        },
    };
    assert_eq!(Demand::from(ProjectionMode::Skeleton), expected_skeleton);

    // Distinctness: presets are genuinely different points (not all the
    // same constant) — a constant-returning From impl would fail here.
    assert_ne!(expected_identity, expected_navigate);
    assert_ne!(expected_navigate, expected_shallow);
    assert_ne!(expected_shallow, expected_expanded);
    assert_ne!(expected_expanded, expected_skeleton);
}

// ----------------------------------------------------------------------
// NAMED GUARD 2 — Skeleton is TypeParamShells + StopAtCarrier, not a mode
// ----------------------------------------------------------------------

#[test]
fn skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode() {
    // Skeleton is structurally a hand-built point — no special casing.
    let skeleton = Demand::skeleton();
    let hand_built = Demand {
        projection: ProjectionDemand {
            path: ProjectionPath::empty(),
            facets: SurfaceFacetSet::empty(),
            member_demand: MemberBodyDemand::SetOnly,
            call_signatures: false,
            construct_signatures: false,
            index_signatures: false,
            display_needs: DisplayNeeds::empty(),
        },
        policy: skeleton.policy.clone(),
    };
    // The regime-distinguishing fields are exactly TypeParamShells + the
    // carrier stop; everything else is the navigate/shallow-shaped ⊥.
    assert_eq!(
        skeleton.policy.generic_open,
        GenericOpenPolicy::TypeParamShells
    );
    assert_eq!(
        skeleton.policy.carrier_stop,
        CarrierStopPolicy::StopAtCarrier
    );
    assert_eq!(skeleton.projection, hand_built.projection);

    // INCOMPARABLE to Expanded: different regime (TypeParamShells vs Bound)
    // ⇒ neither dominates the other. If Skeleton wrongly used Bound, it
    // would share Expanded's regime and become comparable → this fails.
    let expanded = Demand::expanded(ProjectionPath::empty());
    assert_ne!(skeleton.regime(), expanded.regime());
    assert!(!skeleton.dominates(&expanded));
    assert!(!expanded.dominates(&skeleton));
    // Cross-regime meet/join are undefined.
    assert!(Demand::meet(&skeleton, &expanded).is_none());
    assert!(Demand::join(&skeleton, &expanded).is_none());
}

// ----------------------------------------------------------------------
// NAMED GUARD 3 — cache key axes are minimal and normalized
// ----------------------------------------------------------------------

#[test]
fn cache_key_axes_are_minimal_and_normalized() {
    // A demand that exercises many non-⊥ axes, including display_needs.
    let rich = Demand {
        projection: ProjectionDemand {
            path: path(&["a", "b"]),
            facets: SurfaceFacetSet::single(SurfaceFacet::Members),
            member_demand: MemberBodyDemand::SetPlusBody,
            call_signatures: true,
            construct_signatures: false,
            index_signatures: true,
            display_needs: DisplayNeeds::single(DisplayFacet::ExpandAliases),
        },
        policy: Demand::expanded(path(&["a", "b"])).policy.clone(),
    };
    let full = AxisMask::full();

    // (a) idempotent normalization.
    let once = apply_mask(&rich, &full);
    let twice = apply_mask(&once, &full);
    assert_eq!(once, twice, "apply_mask must be idempotent");

    // (b) display_needs ALWAYS normalizes to ⊥ (§14 invariant): two
    // demands differing ONLY in display_needs collapse to the same key.
    let mut other = rich.clone();
    other.projection.display_needs = DisplayNeeds::single(DisplayFacet::QualifyNames);
    assert_ne!(
        rich.projection.display_needs, other.projection.display_needs,
        "fixture precondition: the two demands really do differ in display_needs"
    );
    assert_eq!(
        apply_mask(&rich, &full),
        apply_mask(&other, &full),
        "display_needs must be masked out under any mask, even the full mask"
    );
    assert_eq!(
        apply_mask(&rich, &full).projection.display_needs,
        DisplayNeeds::empty()
    );

    // (c) a mask that drops a load-bearing axis CHANGES the normalized
    // demand (proves the mask is not a no-op). Drop NormalizationDepth.
    let without_norm = AxisMask::full().without(DemandAxis::NormalizationDepth);
    let dropped = apply_mask(&rich, &without_norm);
    assert_ne!(
        apply_mask(&rich, &full),
        dropped,
        "dropping a load-bearing axis must change the normalized demand"
    );
    assert_eq!(
        dropped.policy.normalization_depth,
        NormalizationDepth::None,
        "the dropped axis must be reset to its ⊥"
    );

    // relevant_demand_axes builds a mask from declared axes; display_needs
    // can never be declared (it is not a DemandAxis) so it is structurally
    // impossible to keep it.
    let declared = relevant_demand_axes(&[DemandAxis::Path, DemandAxis::MemberBody]);
    assert!(declared.contains(DemandAxis::Path));
    assert!(declared.contains(DemandAxis::MemberBody));
    assert!(!declared.contains(DemandAxis::NormalizationDepth));
}

// ----------------------------------------------------------------------
// ALGEBRA UNIT TESTS (§3.2 / §3.3 / §3.4 / §3.5)
// ----------------------------------------------------------------------

#[test]
fn regime_fields_induce_incomparability() {
    // Distinct generic_open ⇒ incomparable.
    let bound = Demand::expanded(ProjectionPath::empty());
    let shells = Demand::skeleton();
    assert!(!bound.dominates(&shells));
    assert!(!shells.dominates(&bound));

    // Distinct surface_role ⇒ incomparable even when every other field is
    // identical / dominated.
    let mut prop = Demand::expanded(path(&["a"]));
    prop.policy.surface_role = SurfaceRole::Prop;
    let mut emit = Demand::expanded(path(&["a"]));
    emit.policy.surface_role = SurfaceRole::Emit;
    assert_ne!(prop.regime(), emit.regime());
    assert!(!prop.dominates(&emit));
    assert!(!emit.dominates(&prop));

    // Distinct merge_role ⇒ incomparable.
    let mut standalone = Demand::shallow();
    standalone.policy.merge_role = MergeRole::Standalone;
    let mut heritage = Demand::shallow();
    heritage.policy.merge_role = MergeRole::Heritage;
    assert!(!standalone.dominates(&heritage));
    assert!(!heritage.dominates(&standalone));
}

#[test]
fn meet_is_total_within_regime_and_none_across_regimes() {
    // Within one regime: meet computes the componentwise glb.
    let a = Demand {
        projection: ProjectionDemand {
            path: path(&["c", "full", "bar"]),
            facets: SurfaceFacetSet::single(SurfaceFacet::Members),
            member_demand: MemberBodyDemand::SetPlusBody,
            call_signatures: true,
            construct_signatures: true,
            index_signatures: false,
            display_needs: DisplayNeeds::empty(),
        },
        policy: Demand::expanded(ProjectionPath::empty()).policy.clone(),
    };
    let b = Demand {
        projection: ProjectionDemand {
            path: path(&["c", "other"]),
            facets: SurfaceFacetSet::empty(),
            member_demand: MemberBodyDemand::SetOnly,
            call_signatures: false,
            construct_signatures: true,
            index_signatures: true,
            display_needs: DisplayNeeds::empty(),
        },
        // same regime as `a` (Bound/Plain/Standalone), but lower eval rungs
        policy: Demand::shallow().policy.clone(),
    };
    let m = Demand::meet(&a, &b).expect("same regime ⇒ meet exists");
    // path → longest common prefix = ["c"]
    assert_eq!(m.projection.path, path(&["c"]));
    // facets → ∩ = ∅
    assert_eq!(m.projection.facets, SurfaceFacetSet::empty());
    // member_demand → min = SetOnly
    assert_eq!(m.projection.member_demand, MemberBodyDemand::SetOnly);
    // sig bools → ∧
    assert!(!m.projection.call_signatures);
    assert!(m.projection.construct_signatures);
    assert!(!m.projection.index_signatures);
    // alias_preservation → min(Inline, Keep) = Keep
    assert_eq!(m.policy.alias_preservation, AliasPreservation::Keep);
    // normalization_depth → min(Terminal, None) = None
    assert_eq!(m.policy.normalization_depth, NormalizationDepth::None);
    // operator_reduction → min(Reduce, Leave) = Leave
    assert_eq!(m.policy.operator_reduction, OperatorReduction::Leave);

    // Across regimes: None.
    assert!(Demand::meet(&a, &Demand::skeleton()).is_none());
}

#[test]
fn meet_is_the_greatest_lower_bound_within_a_regime() {
    // Worked example: two comparable-ish points; meet must be a lower
    // bound of both AND dominate any other common lower bound.
    let a = Demand::expanded(path(&["c", "full", "bar"]));
    let b = Demand::expanded(path(&["c", "full"]));
    let m = Demand::meet(&a, &b).expect("same regime");
    // m is a lower bound of both: a ⊒ m and b ⊒ m.
    assert!(a.dominates(&m));
    assert!(b.dominates(&m));
    // m = meet = the longest common prefix point = ["c","full"] expanded
    // (b is itself the glb here because b ⊑ a on the path).
    assert_eq!(m.projection.path, path(&["c", "full"]));
    // A strictly-smaller common lower bound (shorter path) is dominated by m.
    let smaller = Demand::expanded(path(&["c"]));
    assert!(a.dominates(&smaller));
    assert!(b.dominates(&smaller));
    assert!(
        m.dominates(&smaller),
        "meet must dominate any common lower bound"
    );
}

#[test]
fn join_is_partial_on_path_prefix_and_regime() {
    // Prefix-comparable paths ⇒ Some(longer).
    let shorter = Demand::expanded(path(&["c"]));
    let longer = Demand::expanded(path(&["c", "full"]));
    let j = Demand::join(&shorter, &longer).expect("prefix-comparable ⇒ join exists");
    assert_eq!(j.projection.path, path(&["c", "full"]));

    // Divergent paths ⇒ None (no least upper bound).
    let left = Demand::expanded(path(&["c", "x"]));
    let right = Demand::expanded(path(&["c", "y"]));
    assert!(Demand::join(&left, &right).is_none());

    // Across regimes ⇒ None.
    assert!(Demand::join(&shorter, &Demand::skeleton()).is_none());

    // join uses componentwise dual (union / max) on the non-path fields.
    let mut a = Demand::shallow();
    a.projection.facets = SurfaceFacetSet::single(SurfaceFacet::Members);
    a.policy.provenance = ProvenanceNeed::Retain;
    let mut b = Demand::shallow();
    b.projection.facets = SurfaceFacetSet::single(SurfaceFacet::Call);
    b.policy.provenance = ProvenanceNeed::Drop;
    let j2 = Demand::join(&a, &b).expect("same regime, equal (empty) paths");
    assert!(j2.projection.facets.contains(SurfaceFacet::Members));
    assert!(j2.projection.facets.contains(SurfaceFacet::Call));
    assert_eq!(j2.policy.provenance, ProvenanceNeed::Retain); // max
}

#[test]
fn meet_and_join_pin_every_non_regime_axis() {
    // Two SAME-regime operands (Bound/Plain/Standalone) that DIFFER on EVERY
    // non-regime axis — so a `meet`/`join` bug on ANY single field (not just
    // the few the other tests spot-check) is independently caught here. Paths
    // are prefix-comparable, so the join exists.
    let a = Demand {
        projection: ProjectionDemand {
            path: path(&["c"]),
            facets: SurfaceFacetSet::single(SurfaceFacet::Members),
            member_demand: MemberBodyDemand::SetOnly,
            call_signatures: false,
            construct_signatures: false,
            index_signatures: true,
            display_needs: DisplayNeeds::single(DisplayFacet::ExpandAliases),
        },
        policy: EvalPolicy {
            alias_preservation: AliasPreservation::Keep,
            normalization_depth: NormalizationDepth::None,
            generic_open: GenericOpenPolicy::Bound,
            operator_reduction: OperatorReduction::Leave,
            carrier_stop: CarrierStopPolicy::StopAtCarrier,
            surface_role: SurfaceRole::Plain,
            provenance: ProvenanceNeed::Drop,
            merge_role: MergeRole::Standalone,
        },
    };
    let b = Demand {
        projection: ProjectionDemand {
            path: path(&["c", "x"]),
            facets: SurfaceFacetSet::single(SurfaceFacet::Call),
            member_demand: MemberBodyDemand::SetPlusBody,
            call_signatures: true,
            construct_signatures: true,
            index_signatures: false,
            display_needs: DisplayNeeds::single(DisplayFacet::QualifyNames),
        },
        policy: EvalPolicy {
            alias_preservation: AliasPreservation::Inline,
            normalization_depth: NormalizationDepth::Deep,
            generic_open: GenericOpenPolicy::Bound,
            operator_reduction: OperatorReduction::Reduce,
            carrier_stop: CarrierStopPolicy::Continue,
            surface_role: SurfaceRole::Plain,
            provenance: ProvenanceNeed::Retain,
            merge_role: MergeRole::Standalone,
        },
    };
    assert_eq!(a.regime(), b.regime(), "operands must share a regime");

    // meet = componentwise glb: min on the total chains, ∩ on the bitsets/bools,
    // longest-common-prefix on the path.
    let m = Demand::meet(&a, &b).expect("same regime ⇒ meet exists");
    assert_eq!(
        m.projection.path,
        path(&["c"]),
        "path → longest common prefix"
    );
    assert_eq!(m.projection.facets, SurfaceFacetSet::empty(), "facets → ∩");
    assert_eq!(
        m.projection.member_demand,
        MemberBodyDemand::SetOnly,
        "member_demand → min"
    );
    assert!(!m.projection.call_signatures, "call_signatures → ∧");
    assert!(
        !m.projection.construct_signatures,
        "construct_signatures → ∧"
    );
    assert!(!m.projection.index_signatures, "index_signatures → ∧");
    assert_eq!(
        m.projection.display_needs,
        DisplayNeeds::empty(),
        "display_needs → ∩"
    );
    assert_eq!(
        m.policy.alias_preservation,
        AliasPreservation::Keep,
        "alias_preservation → min"
    );
    assert_eq!(
        m.policy.normalization_depth,
        NormalizationDepth::None,
        "normalization_depth → min"
    );
    assert_eq!(
        m.policy.operator_reduction,
        OperatorReduction::Leave,
        "operator_reduction → min"
    );
    assert_eq!(
        m.policy.carrier_stop,
        CarrierStopPolicy::StopAtCarrier,
        "carrier_stop → min"
    );
    assert_eq!(
        m.policy.provenance,
        ProvenanceNeed::Drop,
        "provenance → min"
    );

    // join = componentwise lub: max on the total chains, ∪ on the bitsets/bools,
    // prefix-join (the longer path) on the path.
    let j = Demand::join(&a, &b).expect("prefix-comparable paths, same regime ⇒ join exists");
    assert_eq!(
        j.projection.path,
        path(&["c", "x"]),
        "path → prefix join (longer)"
    );
    let facets_union = SurfaceFacetSet::single(SurfaceFacet::Members)
        .union(SurfaceFacetSet::single(SurfaceFacet::Call));
    assert_eq!(j.projection.facets, facets_union, "facets → ∪");
    assert_eq!(
        j.projection.member_demand,
        MemberBodyDemand::SetPlusBody,
        "member_demand → max"
    );
    assert!(j.projection.call_signatures, "call_signatures → ∨");
    assert!(
        j.projection.construct_signatures,
        "construct_signatures → ∨"
    );
    assert!(j.projection.index_signatures, "index_signatures → ∨");
    let display_union = DisplayNeeds::single(DisplayFacet::ExpandAliases)
        .union(DisplayNeeds::single(DisplayFacet::QualifyNames));
    assert_eq!(
        j.projection.display_needs, display_union,
        "display_needs → ∪"
    );
    assert_eq!(
        j.policy.alias_preservation,
        AliasPreservation::Inline,
        "alias_preservation → max"
    );
    assert_eq!(
        j.policy.normalization_depth,
        NormalizationDepth::Deep,
        "normalization_depth → max"
    );
    assert_eq!(
        j.policy.operator_reduction,
        OperatorReduction::Reduce,
        "operator_reduction → max"
    );
    assert_eq!(
        j.policy.carrier_stop,
        CarrierStopPolicy::Continue,
        "carrier_stop → max"
    );
    assert_eq!(
        j.policy.provenance,
        ProvenanceNeed::Retain,
        "provenance → max"
    );
}

#[test]
fn meet_join_dominates_incomparable_on_surface_role_and_merge_role_axes() {
    // The regime tuple is (generic_open, surface_role, merge_role). The OTHER
    // cross-regime assertions flip ONLY the `generic_open` axis (via
    // `Demand::skeleton()`), so a regime guard that silently dropped
    // `surface_role` OR `merge_role` from the comparable-iff test would still
    // pass them. This pins the remaining two regime axes INDEPENDENTLY: a base
    // preset with a single regime field overridden must be incomparable on
    // `meet`, `join`, AND `dominates` (both directions).
    //
    // The operands always share an EMPTY path (so `prefix_join` exists and
    // `meet`'s longest-common-prefix is trivially defined) — the ONLY reason
    // these ops return `None`/`false` is the regime mismatch. If the guard
    // dropped the differing axis, the regimes would compare equal, `meet`/`join`
    // would return `Some`, and `dominates` would hold ⇒ these asserts FAIL.
    let base = Demand::shallow(); // Bound / Plain / Standalone, empty path
    assert_eq!(base.policy.generic_open, GenericOpenPolicy::Bound);
    assert_eq!(base.policy.surface_role, SurfaceRole::Plain);
    assert_eq!(base.policy.merge_role, MergeRole::Standalone);

    // --- surface_role axis: differ ONLY here (generic_open + merge_role equal) ---
    let a = base.clone(); // Plain
    let mut b = base.clone();
    b.policy.surface_role = SurfaceRole::Prop;
    assert_eq!(
        a.policy.generic_open, b.policy.generic_open,
        "generic_open held equal"
    );
    assert_eq!(
        a.policy.merge_role, b.policy.merge_role,
        "merge_role held equal"
    );
    assert_ne!(
        a.regime(),
        b.regime(),
        "differ ONLY by surface_role ⇒ different regime"
    );
    assert!(
        Demand::meet(&a, &b).is_none(),
        "surface_role mismatch ⇒ meet None (paths equal, so ONLY the regime forces it)"
    );
    assert!(
        Demand::join(&a, &b).is_none(),
        "surface_role mismatch ⇒ join None (paths equal, so ONLY the regime forces it)"
    );
    assert!(!a.dominates(&b), "surface_role mismatch ⇒ a ⋡ b");
    assert!(!b.dominates(&a), "surface_role mismatch ⇒ b ⋡ a");

    // --- merge_role axis: differ ONLY here (generic_open + surface_role equal) ---
    let c = base.clone(); // Standalone
    let mut d = base.clone();
    d.policy.merge_role = MergeRole::Heritage;
    assert_eq!(
        c.policy.generic_open, d.policy.generic_open,
        "generic_open held equal"
    );
    assert_eq!(
        c.policy.surface_role, d.policy.surface_role,
        "surface_role held equal"
    );
    assert_ne!(
        c.regime(),
        d.regime(),
        "differ ONLY by merge_role ⇒ different regime"
    );
    assert!(
        Demand::meet(&c, &d).is_none(),
        "merge_role mismatch ⇒ meet None (paths equal, so ONLY the regime forces it)"
    );
    assert!(
        Demand::join(&c, &d).is_none(),
        "merge_role mismatch ⇒ join None (paths equal, so ONLY the regime forces it)"
    );
    assert!(!c.dominates(&d), "merge_role mismatch ⇒ c ⋡ d");
    assert!(!d.dominates(&c), "merge_role mismatch ⇒ d ⋡ c");
}

#[test]
fn cached_satisfies_is_materialized_point_not_demand_dominance() {
    // A deep terminal compute that walked c→full→bar (expanding only bar)
    // records Navigate hops for c and c.full plus the terminal bar point.
    // It NEVER recorded ["c"] expanded, so it must NOT satisfy a request
    // for A['c'] expanded — even though the deep terminal demand would
    // "dominate" ["c"] expanded under naive §3.2 componentwise dominance.
    let recorded = MaterializedSet(
        vec![
            MaterializedPoint::new(Demand::navigate(path(&["c"]))),
            MaterializedPoint::new(Demand::navigate(path(&["c", "full"]))),
            MaterializedPoint::new(Demand::expanded(path(&["c", "full", "bar"]))),
        ]
        .into(),
    );

    // MISS: ["c"] expanded was never materialised (only Navigate at ["c"]).
    let want_c_expanded = MaterializedPoint::new(Demand::expanded(path(&["c"])));
    assert!(
        !cached_satisfies(&recorded, &want_c_expanded),
        "deep terminal must NOT satisfy a shallow surface it never materialised"
    );

    // HIT: ["c"] Navigate IS recorded, so a Navigate request at ["c"] hits.
    let want_c_navigate = MaterializedPoint::new(Demand::navigate(path(&["c"])));
    assert!(cached_satisfies(&recorded, &want_c_navigate));

    // backfill_points returns the recorded points verbatim — not derived.
    let back = backfill_points(&recorded);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].path(), &path(&["c"]));
    assert_eq!(
        back[2].point(),
        &Demand::expanded(path(&["c", "full", "bar"]))
    );
}

#[test]
fn materialized_point_path_equals_demand_path_and_deep_terminal_misses_shallow() {
    // §3.4 invariant: a `MaterializedPoint`'s path IS its demand's projection
    // path (single source of truth) — the illegal `outer != inner` state is
    // unrepresentable because the only stored field is the demand and `path()`
    // is DERIVED from it. This test demonstrates the constructor-derived path
    // and that it prevents the false hit the redundant outer path once enabled.
    let deep = MaterializedPoint::new(Demand::expanded(path(&["c", "full", "bar"])));

    // The record's path is the demand's path — they cannot diverge.
    assert_eq!(
        deep.path(),
        &path(&["c", "full", "bar"]),
        "path() must be derived from the demand's projection path"
    );
    assert_eq!(
        deep.path(),
        &deep.point().projection.path,
        "path() and point().projection.path are the same datum"
    );

    let recorded = MaterializedSet(vec![deep].into());

    // MISS: the only record sits at the DEEP terminal path. A request for ["c"]
    // Expanded must not hit. Were an independently-settable outer path
    // reintroduced and trusted by `cached_satisfies` (outer ["c"] wrapping a
    // deep inner demand), the prefix-based `semantically_dominates` would forge
    // a hit here — so this assertion FAILS if that footgun returns. With the
    // derived path it is structurally a MISS.
    let want_c_expanded = MaterializedPoint::new(Demand::expanded(path(&["c"])));
    assert!(
        !cached_satisfies(&recorded, &want_c_expanded),
        "deep-terminal record must not satisfy a shallow ['c'] Expanded request"
    );

    // And the deep terminal DOES satisfy itself (exact path + same demand) —
    // proving the MISS above is path-discrimination, not a blanket reject.
    let want_deep = MaterializedPoint::new(Demand::expanded(path(&["c", "full", "bar"])));
    assert!(
        cached_satisfies(&recorded, &want_deep),
        "the deep terminal must satisfy an identical deep request"
    );
}

#[test]
fn cached_satisfies_misses_on_every_regime_axis_at_same_path() {
    // The cached_satisfies counterpart of
    // `meet_join_dominates_incomparable_on_surface_role_and_merge_role_axes`:
    // the warm-hit MISS must be driven PURELY by the regime guard in
    // `semantically_dominates`, not by an incidental non-regime axis difference.
    //
    // The old skeleton-vs-navigate form was CONFOUNDED: `Demand::skeleton()`
    // and `Demand::navigate()` differ not only on the `generic_open` regime axis
    // but ALSO on `facets`, `operator_reduction`, `carrier_stop`, and
    // `normalization_depth`. So `cached_satisfies` would MISS even if the regime
    // equality check were DELETED from `semantically_dominates` — the non-regime
    // axes alone block domination. That form did not isolate the regime gate.
    //
    // Here the recorded and requested points are IDENTICAL on EVERY non-regime
    // axis (same path, facets, member_demand, all sig bools, alias_preservation,
    // normalization_depth, operator_reduction, carrier_stop, provenance,
    // display_needs) and differ on EXACTLY ONE regime field at a time. With all
    // non-regime axes equal, every componentwise `>=`/subset/prefix clause in
    // `semantically_dominates` holds, so the ONLY thing that can force a MISS is
    // the `regime() != regime()` guard. Mentally delete that guard and each
    // `== false` below flips to a (failing) HIT.
    let base = Demand::navigate(path(&["c"])); // Bound / Plain / Standalone
    assert_eq!(base.policy.generic_open, GenericOpenPolicy::Bound);
    assert_eq!(base.policy.surface_role, SurfaceRole::Plain);
    assert_eq!(base.policy.merge_role, MergeRole::Standalone);

    // Override exactly one regime field; everything else stays `base`.
    let mut generic_open_variant = base.clone();
    generic_open_variant.policy.generic_open = GenericOpenPolicy::TypeParamShells;

    let mut surface_role_variant = base.clone();
    surface_role_variant.policy.surface_role = SurfaceRole::Prop;

    let mut merge_role_variant = base.clone();
    merge_role_variant.policy.merge_role = MergeRole::Heritage;

    for (axis, variant) in [
        (
            "generic_open: Bound vs TypeParamShells",
            generic_open_variant,
        ),
        ("surface_role: Plain vs Prop", surface_role_variant),
        ("merge_role: Standalone vs Heritage", merge_role_variant),
    ] {
        // Precondition: the pair differs ONLY by its regime — every other axis
        // is `base`, so the demands are equal once their regimes are stripped.
        assert_ne!(
            variant.regime(),
            base.regime(),
            "{axis}: variant must sit in a different regime"
        );
        assert_eq!(
            variant.projection, base.projection,
            "{axis}: projection (all non-regime projection axes) held equal"
        );
        assert_eq!(
            variant.policy.alias_preservation, base.policy.alias_preservation,
            "{axis}: alias_preservation held equal"
        );
        assert_eq!(
            variant.policy.normalization_depth, base.policy.normalization_depth,
            "{axis}: normalization_depth held equal"
        );
        assert_eq!(
            variant.policy.operator_reduction, base.policy.operator_reduction,
            "{axis}: operator_reduction held equal"
        );
        assert_eq!(
            variant.policy.carrier_stop, base.policy.carrier_stop,
            "{axis}: carrier_stop held equal"
        );
        assert_eq!(
            variant.policy.provenance, base.policy.provenance,
            "{axis}: provenance held equal"
        );

        // The recorded point sits in the variant regime; the request is `base`.
        // path() matches (both ["c"]), and every non-regime clause of
        // `semantically_dominates` holds — so the MISS is PURELY the regime gate.
        let recorded = MaterializedSet(vec![MaterializedPoint::new(variant)].into());
        let requested = MaterializedPoint::new(base.clone());
        assert!(
            !cached_satisfies(&recorded, &requested),
            "{axis}: cross-regime record must MISS (regime gate is the only differing axis)"
        );
    }

    // POSITIVE CONTROL: identical recorded + requested (same regime, same
    // non-regime axes) ⇒ HIT. This proves the three MISSes above are
    // regime-driven, not a blanket reject of `base`-shaped requests.
    let same = MaterializedSet(vec![MaterializedPoint::new(base.clone())].into());
    let requested = MaterializedPoint::new(base.clone());
    assert!(
        cached_satisfies(&same, &requested),
        "identical record + request (same regime) must HIT"
    );
}

#[test]
fn cached_satisfies_ignores_display_only_display_needs() {
    // §14.1 invariant: `display_needs` is display-only and NEVER drives
    // resolution — two queries differing ONLY in `display_needs` MUST share the
    // cached typed value. So a warm hit (cached_satisfies) must IGNORE
    // display_needs: a recorded point with one display_needs satisfies a
    // request with a DIFFERENT display_needs at the same path + regime.
    let mut recorded_point = Demand::expanded(path(&["c"]));
    recorded_point.projection.display_needs = DisplayNeeds::single(DisplayFacet::ExpandAliases);
    let recorded = MaterializedSet(vec![MaterializedPoint::new(recorded_point.clone())].into());

    let mut want_point = Demand::expanded(path(&["c"]));
    want_point.projection.display_needs = DisplayNeeds::single(DisplayFacet::QualifyNames);
    let want = MaterializedPoint::new(want_point.clone());

    // fixture precondition: the two really differ ONLY in display_needs.
    assert_ne!(
        recorded_point.projection.display_needs, want_point.projection.display_needs,
        "fixture precondition: differing display_needs"
    );

    // SEMANTIC HIT: display_needs must not gate satisfaction. FAILS if
    // cached_satisfies used the full `dominates` (which includes display_needs):
    // {QualifyNames} ⊄ {ExpandAliases}, so the full order would MISS.
    assert!(
        cached_satisfies(&recorded, &want),
        "display-only display_needs must not cause a semantic warm-hit MISS (§14.1)"
    );

    // The FULL product order (`dominates`) still distinguishes them — the
    // display sub-lattice / meet / join keep display_needs (§3.2).
    assert!(
        !recorded_point.dominates(&want_point),
        "full product order DOES include display_needs (display sub-lattice)"
    );
    // …and semantically_dominates (the warm-hit order) does NOT.
    assert!(recorded_point.semantically_dominates(&want_point));
    assert!(want_point.semantically_dominates(&recorded_point));
}

#[test]
fn demand_at_hop_is_monotone_in_the_terminal() {
    // A CONCRETE multi-hop terminal so the per-hop PATH is observable. `n` must
    // equal the terminal path length (one hop per segment).
    let leaf = path(&["c", "full", "bar"]);
    let n = leaf.len(); // == 3

    // t1, t2 share the SAME path AND regime; t2 widens ONLY the leaf eval rungs.
    let mut t1 = Demand::shallow(); // lower eval rungs
    t1.projection.path = leaf.clone();
    let mut t2 = Demand::shallow();
    t2.projection.path = leaf.clone();
    t2.policy.normalization_depth = NormalizationDepth::Deep; // strictly broader
    t2.policy.operator_reduction = OperatorReduction::Reduce;
    t2.policy.alias_preservation = AliasPreservation::Inline;
    t2.projection.member_demand = MemberBodyDemand::SetPlusBody;
    assert_eq!(t1.regime(), t2.regime());
    assert!(
        t2.dominates(&t1),
        "t2 ⊒ t1 precondition (same path, broader leaf eval)"
    );
    assert!(!t1.dominates(&t2));

    // Intermediate hops carry the EXACT recorded prefix path — the prefix
    // walked so far — NOT the empty path. (FAILS against an empty-path impl.)
    let expected_prefixes = [path(&["c"]), path(&["c", "full"])];
    // `i` is a hop counter passed to `demand_at_hop(i, n, …)` and compared
    // against `n`, not merely an index into `expected_prefixes`, so the
    // range loop is the clear form here.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        let h1 = demand_at_hop(i, n, &t1);
        let h2 = demand_at_hop(i, n, &t2);
        assert!(
            h2.dominates(&h1),
            "demand_at_hop must be monotone in the terminal at hop {i}"
        );
        if i + 1 < n {
            // Path-precise: hop i carries the terminal prefix [0..=i].
            assert_eq!(
                h1.projection.path, expected_prefixes[i],
                "intermediate hop {i} must carry the walked prefix, not []"
            );
            assert_eq!(h2.projection.path, expected_prefixes[i]);
            // Intermediate hops are IDENTICAL across the two terminals
            // (path-precise + monotone): same prefix, same regime, and the
            // eval rungs come from the constant NAVIGATE_PRESET — NOT the
            // terminal's widened rungs.
            assert_eq!(
                h1, h2,
                "intermediate hop {i} must not widen with a broader terminal"
            );
            assert_eq!(
                h1.policy.operator_reduction,
                OperatorReduction::NavigateOnly,
                "intermediate hop runs Navigate"
            );
            assert_eq!(
                h1.policy.normalization_depth,
                NormalizationDepth::NavigateOnly
            );
            assert_eq!(h1.projection.member_demand, MemberBodyDemand::SetOnly);
        } else {
            // Terminal hop is exactly the terminal demand (path included).
            assert_eq!(h1, t1);
            assert_eq!(h2, t2);
        }
    }
}

#[test]
fn projection_path_prefix_and_join_semantics() {
    let cf = path(&["c", "full"]);
    let cfb = path(&["c", "full", "bar"]);
    let cx = path(&["c", "x"]);

    assert!(cf.is_prefix_of(&cfb));
    assert!(!cfb.is_prefix_of(&cf));
    assert!(ProjectionPath::empty().is_prefix_of(&cf));

    // longest_common_prefix is total.
    assert_eq!(cfb.longest_common_prefix(&cx), path(&["c"]));
    assert_eq!(cf.longest_common_prefix(&cfb), cf.clone());

    // prefix_join: Some(longer) iff prefix-comparable, else None.
    assert_eq!(cf.prefix_join(&cfb), Some(cfb.clone()));
    assert_eq!(cfb.prefix_join(&cf), Some(cfb.clone()));
    assert_eq!(cf.prefix_join(&cx), None);

    // structural equality is the normal form.
    assert_eq!(path(&["c", "full"]), path(&["c", "full"]));
    assert_ne!(path(&["c", "full"]), path(&["c"]));
}

#[test]
fn projection_path_is_the_arc_representation_round_trips_identity() {
    // `ProjectionPath` IS the `Arc<[PathSegment]>` representation (§3.1) — a
    // thin newtype, NOT a parallel datum. The conversions are O(1) Arc
    // clones/moves so `SemanticQueryKey::ProjectPath` shares this exact
    // representation with zero conversion tax.
    let segments: Arc<[PathSegment]> = vec![seg("c"), seg("full")].into();

    let pp = ProjectionPath::from(Arc::clone(&segments));
    // as_arc borrows the SAME allocation — no copy, no re-interning.
    assert!(
        Arc::ptr_eq(pp.as_arc(), &segments),
        "as_arc must expose the wrapped Arc verbatim"
    );

    // Arc -> ProjectionPath -> Arc is identity (same allocation back out).
    let back: Arc<[PathSegment]> = pp.into_arc();
    assert!(
        Arc::ptr_eq(&back, &segments),
        "into_arc must hand back the same Arc allocation"
    );

    // The `Into` direction agrees on contents too.
    let pp2: ProjectionPath = Arc::clone(&segments).into();
    let back2: Arc<[PathSegment]> = pp2.into();
    assert_eq!(&back2[..], &segments[..]);
}
