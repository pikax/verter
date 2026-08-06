//! Validation contract for a compaction domain's TERMINAL AGGREGATE on
//! the session's store views.
//!
//! An aggregate is the strongest claim in the fact rail: it stands in for
//! every precise fact a scope observed in one domain. Accepting one
//! wrongly is not a coarse answer, it is a stale serve over a whole
//! domain at once. So each arm is asserted in BOTH directions — the
//! aggregate this view genuinely vouches for is accepted, and every
//! near-miss is refused:
//!
//! * a different generation (the stamp gate),
//! * a different view population at an IDENTICAL generation (the
//!   population gate — the one a numeric comparison alone would miss),
//! * a domain whose stamp this view does not capture at all.
//!
//! The population gate is the load-bearing one. A session overlay
//! re-roots whole hashes and parse facts while leaving the workspace
//! content generation untouched, so an overlay-derived content aggregate
//! and a base one can carry the SAME number while describing different
//! worlds.

use std::sync::Arc;

use verter_workspace::{
    AggregatePopulation, AggregateStamp, CompactionDomain, CompletionOverlayState,
    DomainGenerationFact, SessionOverlayFingerprint, ViewPopulation,
};

use crate::resolver_core::{FactVersionRef, StoreView};
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn host_with_a_file() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/a.ts".to_string(),
            source: Arc::from("export const a = 1\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
    host
}

fn freshly_built_view(host: &VerterHost) -> crate::resolver_store::HostStoreView {
    host.bump_store_view_epoch();
    host.resolver_store_view_read().into_owned_view()
}

fn aggregate(
    domain: CompactionDomain,
    population: AggregatePopulation,
    generation: u64,
) -> FactVersionRef {
    FactVersionRef::DomainGeneration(DomainGenerationFact {
        domain,
        population,
        stamp: AggregateStamp::Generation(generation),
    })
}

fn base_view() -> AggregatePopulation {
    AggregatePopulation::View(ViewPopulation::Base)
}

fn overlay_view() -> AggregatePopulation {
    AggregatePopulation::View(ViewPopulation::SessionOverlay(
        SessionOverlayFingerprint::new(0x0BAD_CAFE).expect("non-zero"),
    ))
}

/// The CONTENT domain's aggregate validates at exactly the generation
/// the view captured, and at no other.
///
/// Mutation recipe: in `HostStoreView::validates_domain_aggregate`, drop
/// the `aggregate.stamp == AggregateStamp::Generation(captured)`
/// conjunct from `matches_view_counter`. The stale-generation assertion
/// fails.
#[test]
fn a_content_aggregate_validates_only_at_the_views_captured_generation() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);
    let captured = view.content_generation;

    assert!(
        view.validates(&aggregate(CompactionDomain::Content, base_view(), captured)),
        "the view must vouch for the content generation it actually captured"
    );
    assert!(
        !view.validates(&aggregate(
            CompactionDomain::Content,
            base_view(),
            captured + 1
        )),
        "a content aggregate from a LATER generation describes content this view never saw"
    );
    assert!(
        !view.validates(&aggregate(
            CompactionDomain::Content,
            base_view(),
            captured.wrapping_sub(1)
        )),
        "and an EARLIER one describes content that has since moved"
    );
}

/// The population gate, stated where a numeric comparison cannot see it:
/// an overlay-population aggregate at the view's OWN generation is still
/// refused by a base view.
///
/// This is the assertion that separates "the numbers match" from "this
/// witness is mine". A session overlay re-roots whole hashes and parse
/// facts without touching the workspace content generation, so the two
/// aggregates below are numerically identical and semantically different.
///
/// Mutation recipe: drop the `population == self.view_population()`
/// conjunct from `matches_view_counter`. This test fails while every
/// stamp-axis assertion above stays green.
#[test]
fn an_overlay_population_aggregate_is_refused_by_a_base_view() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);
    let captured = view.content_generation;

    assert_eq!(
        view.view_population(),
        ViewPopulation::Base,
        "fixture invariant: a view built with no session overlay is the BASE view"
    );
    assert!(
        view.validates(&aggregate(CompactionDomain::Content, base_view(), captured)),
        "control: the base-population aggregate at this generation IS valid"
    );
    assert!(
        !view.validates(&aggregate(
            CompactionDomain::Content,
            overlay_view(),
            captured
        )),
        "an overlay-derived content aggregate must NOT satisfy a base read at the same \
         generation — the overlay re-roots whole hashes and parse facts while leaving the \
         content generation untouched, so the matching number describes a different world"
    );
}

/// The SOURCE-ENV domain, same two gates. Its stamp comes from a
/// separate counter precisely because the paths that move
/// `parse_env_hash` do not bump the content generation.
///
/// Mutation recipe: change the `CompactionDomain::SourceEnv` arm of
/// `validates_domain_aggregate` to read `Some(self.content_generation)`
/// instead of `self.source_env_generation`. The two counters are
/// independent, so the accept assertion fails.
#[test]
fn a_source_env_aggregate_validates_only_at_the_views_captured_source_env_generation() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);
    let captured = view
        .source_env_generation
        .expect("a standalone host's workspace exposes a source-env producer");

    assert!(
        view.validates(&aggregate(
            CompactionDomain::SourceEnv,
            base_view(),
            captured
        )),
        "the view must vouch for the source-env generation it captured"
    );
    assert!(
        !view.validates(&aggregate(
            CompactionDomain::SourceEnv,
            base_view(),
            captured + 1
        )),
        "a stale source-env aggregate must be refused"
    );
    assert!(
        !view.validates(&aggregate(
            CompactionDomain::SourceEnv,
            overlay_view(),
            captured
        )),
        "and the population gate applies here too"
    );
}

/// A view whose workspace exposes NO source-env producer refuses every
/// source-env aggregate rather than accepting one against a fabricated
/// constant.
///
/// `WorkspaceAccess::source_env_generation` defaults to `None`, and
/// `None` must mean "cannot vouch", not "generation zero" — a stamp that
/// never advances is a witness nothing can ever invalidate.
///
/// Mutation recipe: change the `SourceEnv` arm to
/// `matches_view_counter(Some(self.source_env_generation.unwrap_or(0)))`.
/// The generation-zero assertion below then passes validation and the
/// test fails.
#[test]
fn a_view_with_no_source_env_producer_refuses_every_source_env_aggregate() {
    let view = crate::resolver_store::HostStoreView::default();
    assert!(
        view.source_env_generation.is_none(),
        "fixture invariant: the default view has no source-env producer"
    );

    for generation in [0_u64, 1, 7] {
        assert!(
            !view.validates(&aggregate(
                CompactionDomain::SourceEnv,
                base_view(),
                generation
            )),
            "a view with no source-env producer must refuse a source-env aggregate at every \
             generation, including {generation} — accepting one would be a witness no producer \
             can ever advance"
        );
    }
}

/// `WorkspaceShape` is a whole-host scalar, so its aggregate is GLOBAL —
/// minted and validated under `View(Base)` regardless of overlays. That
/// is what lets a base scope and a session scope share one
/// workspace-shape witness instead of each minting a private copy.
///
/// Mutation recipe: change the `WorkspaceShape` arm to compare against
/// `self.view_population()` instead of the literal `ViewPopulation::Base`.
/// On a base view the two coincide, so this test still passes — which is
/// exactly why the base-population assertion below is paired with the
/// stale-generation one; see the module note. The discriminating change
/// for this arm is routing its stamp elsewhere (e.g. to
/// `self.content_generation`), which fails the accept assertion.
#[test]
fn a_workspace_shape_aggregate_validates_at_the_captured_project_generation() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);
    let captured = view.snapshot.roots.project_env_root.project_generation;

    assert!(
        view.validates(&aggregate(
            CompactionDomain::WorkspaceShape,
            base_view(),
            captured
        )),
        "the view must vouch for the project generation it captured"
    );
    assert!(
        !view.validates(&aggregate(
            CompactionDomain::WorkspaceShape,
            base_view(),
            captured + 1
        )),
        "a stale project generation must be refused"
    );
    assert!(
        !view.validates(&aggregate(
            CompactionDomain::WorkspaceShape,
            overlay_view(),
            captured
        )),
        "a whole-host scalar's aggregate is global; one labelled with an overlay population is \
         malformed and must be refused rather than silently accepted"
    );
}

/// The two COMPOSITE-stamp domains have no captured stamp on this view
/// yet, so they fail closed.
///
/// This is a deliberate refusal, not an oversight: refusing costs a
/// recompute, while accepting a witness the view cannot actually check
/// is a stale serve. When their stamps land, this test is the thing that
/// must be updated — and it will fail loudly rather than let a
/// half-wired domain start accepting.
///
/// Mutation recipe, EXECUTED: route either composite arm through
/// `matches_view_counter(...)`. The matching assertion fails.
#[test]
fn composite_stamp_domains_refuse_a_bare_counter_stamp() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);

    for domain in [
        CompactionDomain::SemanticImports,
        CompactionDomain::RouteSurface,
    ] {
        for generation in [0_u64, 1, 7] {
            assert!(
                !view.validates(&aggregate(domain, base_view(), generation)),
                "{domain:?} is a COMPOSITE domain: a bare `Generation` stamp names one clock and \
                 nothing about the key its store is addressed by, so accepting it at generation \
                 {generation} would let a compacted witness survive the change that re-keys \
                 every entry it stands for"
            );
        }
    }
}

/// The `RouteSurface` composite the view vouches for is accepted, and
/// every single-component perturbation of it is refused.
///
/// Mutation recipe, EXECUTED: replace the whole-stamp equality in the
/// `RouteSurface` arm with a field-wise comparison that omits one
/// component. The matching perturbation assertion fails.
#[test]
fn every_component_of_the_route_surface_composite_is_load_bearing() {
    use verter_workspace::RouteSurfaceStamp;

    let host = host_with_a_file();
    let view = freshly_built_view(&host);

    let captured = view
        .route_surface_stamp()
        .expect("a view over a live host captures every component of the composite");
    let AggregateStamp::RouteSurface(base) = captured else {
        panic!("the route-surface stamp must be the composite variant, got {captured:?}");
    };

    assert!(
        view.validates(&FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::RouteSurface,
            population: base_view(),
            stamp: captured,
        })),
        "control: the view must vouch for the composite it itself composed"
    );

    let perturbations: [(&str, RouteSurfaceStamp); 4] = [
        (
            "route_surface",
            RouteSurfaceStamp {
                route_surface: base.route_surface.wrapping_add(2),
                ..base
            },
        ),
        (
            "content",
            RouteSurfaceStamp {
                content: base.content.wrapping_add(1),
                ..base
            },
        ),
        (
            "source_env",
            RouteSurfaceStamp {
                source_env: base.source_env.wrapping_add(1),
                ..base
            },
        ),
        (
            "workspace_shape",
            RouteSurfaceStamp {
                workspace_shape: base.workspace_shape.wrapping_add(1),
                ..base
            },
        ),
    ];

    for (component, stamp) in perturbations {
        assert!(
            !view.validates(&FactVersionRef::DomainGeneration(DomainGenerationFact {
                domain: CompactionDomain::RouteSurface,
                population: base_view(),
                stamp: AggregateStamp::RouteSurface(stamp),
            })),
            "the `{component}` component moved, so the augmentation index this witness stands \
             for is addressed differently than the one it read — the view must refuse it"
        );
    }

    assert!(
        !view.validates(&FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::RouteSurface,
            population: overlay_view(),
            stamp: captured,
        })),
        "and the population gate applies: an overlay-derived route-surface composite must not \
         satisfy a base read, because the augmentation index is population-scoped"
    );
}

/// **An isolated route-surface world change refuses a previously
/// captured composite.**
///
/// A project-shape bump isolates cleanly (measured: it moves the shape
/// component and nothing else), so the refusal is attributable.
///
/// Mutation recipe, EXECUTED: hard-code `workspace_shape: 0` in
/// `HostStoreView::route_surface_stamp`. This test fails while the
/// perturbation test above stays green.
#[test]
fn an_isolated_shape_movement_refuses_a_previously_captured_route_surface_composite() {
    use verter_workspace::RouteSurfaceStamp;

    let host = host_with_a_file();
    let components = |host: &VerterHost| -> RouteSurfaceStamp {
        match freshly_built_view(host)
            .route_surface_stamp()
            .expect("the view captures every component")
        {
            AggregateStamp::RouteSurface(stamp) => stamp,
            other => panic!("expected the composite, got {other:?}"),
        }
    };

    let before = components(&host);
    let aggregate = FactVersionRef::DomainGeneration(DomainGenerationFact {
        domain: CompactionDomain::RouteSurface,
        population: base_view(),
        stamp: AggregateStamp::RouteSurface(before),
    });
    assert!(
        freshly_built_view(&host).validates(&aggregate),
        "control: the view vouches for the composite it composed"
    );

    host.project_type_store().bump_project_generation();

    let after = components(&host);
    assert_eq!(
        (after.route_surface, after.content, after.source_env),
        (before.route_surface, before.content, before.source_env),
        "ISOLATION: a project-shape bump must move nothing but the shape component, or the \
         refusal below is attributable to a sibling"
    );
    assert_ne!(
        after.workspace_shape, before.workspace_shape,
        "fixture invariant: the bump must move the shape component"
    );
    assert!(
        !freshly_built_view(&host).validates(&aggregate),
        "the project graph moved, which re-composes the `AugmentationTargetKey` the index is \
         addressed by, so a witness compacted under the old shape must be refused"
    );
}

/// A `SemanticImports` aggregate carrying a BARE counter is refused, even
/// when that counter is exactly the store's live membership generation.
///
/// This is the assertion that makes the domain's stamp a COMPOSITE rather
/// than a counter with extra fields. The store answers per KEY, and every
/// key dimension — `content_hash`, `parse_env_hash`, `resolve_env_hash` —
/// lives in another domain. A witness pinning only the membership counter
/// keeps validating across a content edit or an env republication that
/// re-keys every slot it stands in for, because no admission happened.
///
/// Mutation recipe: in `validates_domain_aggregate`, route the
/// `SemanticImports` arm through
/// `matches_view_counter(self.semantic_imports_generation)`. This test
/// fails while the composite acceptance test below stays green.
#[test]
fn a_bare_counter_stamp_is_refused_for_the_semantic_imports_composite() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);
    let live = host
        .project_type_store()
        .resolved_import_facts()
        .stable_generation()
        .expect("a quiescent store reports a stable membership generation");

    assert!(
        !view.validates(&aggregate(
            CompactionDomain::SemanticImports,
            base_view(),
            live
        )),
        "a bare `Generation` stamp names the store's membership and NOTHING about the key \
         dimensions the store answers on, so accepting it would let a compacted semantic-import \
         witness survive the content edit or env republication that re-keys every slot it stands \
         for"
    );
}

/// The composite the view actually vouches for is accepted, and every
/// single-component perturbation of it is refused.
///
/// Each component answers for one distinct way a recorded semantic-import
/// fact stops being current, so a component that can be perturbed without
/// changing the verdict is a component that is not doing anything.
///
/// Mutation recipe, EXECUTED: replace the whole-stamp equality in
/// `validates_domain_aggregate`'s `SemanticImports` arm with a
/// field-wise comparison that OMITS one component — e.g. destructure
/// both `AggregateStamp::SemanticImports` values and conjoin
/// `semantic_imports`, `source_env`, `resolution` and `workspace_shape`
/// while dropping `content`. This test fails; every sibling stays green.
///
/// Note what this recipe does NOT cover, and why the behavioural sibling
/// below exists: hard-coding a component INSIDE
/// `semantic_imports_stamp` (`content: 0`) does not redden this test,
/// because the validator would then compose the same constant on both
/// sides and the perturbation would still differ. Proving a component is
/// sourced from a LIVE producer needs a real movement, not a synthetic
/// perturbation.
#[test]
fn every_component_of_the_semantic_imports_composite_is_load_bearing() {
    use verter_workspace::{AggregateStamp, SemanticImportsStamp};

    let host = host_with_a_file();
    let view = freshly_built_view(&host);

    let captured = view
        .semantic_imports_stamp()
        .expect("a view over a live host captures every component of the composite");
    let AggregateStamp::SemanticImports(base) = captured else {
        panic!("the semantic-imports stamp must be the composite variant, got {captured:?}");
    };

    let accepted = FactVersionRef::DomainGeneration(DomainGenerationFact {
        domain: CompactionDomain::SemanticImports,
        population: base_view(),
        stamp: captured,
    });
    assert!(
        view.validates(&accepted),
        "control: the view must vouch for the composite it itself composed, or every refusal \
         below is about an unrelated mismatch"
    );

    let perturbations: [(&str, SemanticImportsStamp); 4] = [
        (
            "membership",
            SemanticImportsStamp {
                semantic_imports: base.semantic_imports.wrapping_add(2),
                ..base
            },
        ),
        (
            "content",
            SemanticImportsStamp {
                content: base.content.wrapping_add(1),
                ..base
            },
        ),
        (
            "source_env",
            SemanticImportsStamp {
                source_env: base.source_env.wrapping_add(1),
                ..base
            },
        ),
        (
            "workspace_shape",
            SemanticImportsStamp {
                workspace_shape: base.workspace_shape.wrapping_add(1),
                ..base
            },
        ),
    ];

    for (component, stamp) in perturbations {
        assert!(
            !view.validates(&FactVersionRef::DomainGeneration(DomainGenerationFact {
                domain: CompactionDomain::SemanticImports,
                population: base_view(),
                stamp: AggregateStamp::SemanticImports(stamp),
            })),
            "the `{component}` component moved, so the store now answers a different slot than \
             the one the compacted witness read — the view must refuse it"
        );
    }
}

/// The population gate applies to the composite too: an overlay-labelled
/// composite at the view's own stamp is refused by a base view.
#[test]
fn an_overlay_population_semantic_imports_composite_is_refused_by_a_base_view() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);
    let captured = view
        .semantic_imports_stamp()
        .expect("the view captures the composite");

    assert!(
        !view.validates(&FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::SemanticImports,
            population: overlay_view(),
            stamp: captured,
        })),
        "a session overlay re-roots the per-canonical content the semantic-import store keys on, \
         so an overlay-derived composite must not satisfy a base read at a numerically identical \
         stamp"
    );
}

/// A resolution-domain aggregate carrying a VIEW population is
/// malformed. The captured resolution world owns that adjudication, and
/// it refuses the whole `View` arm; this asserts the session's dispatch
/// actually routes there rather than answering on its own.
///
/// Mutation recipe: in the `CompactionDomain::Resolution` arm of
/// `validates_domain_aggregate`, replace the delegation with `true`.
/// This test fails.
#[test]
fn a_resolution_aggregate_with_a_view_population_is_refused() {
    let host = host_with_a_file();
    let view = freshly_built_view(&host);

    for population in [base_view(), overlay_view()] {
        for generation in [0_u64, 1] {
            assert!(
                !view.validates(&aggregate(
                    CompactionDomain::Resolution,
                    population,
                    generation
                )),
                "the resolution domain's population lives in its own identity space; a \
                 view-population aggregate claiming it is malformed"
            );
        }
    }
}

/// An EMPTY completion overlay validates exactly as its parent view, so
/// it must reuse the parent's Content aggregate. Once the same overlay
/// shadows a fact, that parent aggregate must be refused at the identical
/// generation: the numeric stamp did not move, but the validating
/// population did.
///
/// The workspace-shape control stays accepted on both sides because a
/// per-canonical completion does not alter a whole-project scalar.
#[test]
fn a_request_store_view_refines_content_validation_only_after_it_shadows() {
    use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};

    let host = host_with_a_file();
    let base = freshly_built_view(&host);
    let content_generation = base.content_generation;
    let project_generation = base.snapshot.roots.project_env_root.project_generation;

    let content = aggregate(CompactionDomain::Content, base_view(), content_generation);
    let shape = aggregate(
        CompactionDomain::WorkspaceShape,
        base_view(),
        project_generation,
    );

    // Controls: the base view genuinely accepts BOTH. Without these the
    // refusal below could be satisfied by the aggregates simply being
    // invalid for an unrelated reason.
    assert!(
        base.validates(&content),
        "control: the base view accepts this content aggregate"
    );
    assert!(
        base.validates(&shape),
        "control: the base view accepts this workspace-shape aggregate"
    );

    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let request_view = RequestStoreView::new(&base, Arc::clone(&overlay));

    assert!(
        request_view.validates(&content),
        "an empty completion overlay changes no validation answer and must reuse the parent's \
         Content aggregate"
    );
    assert!(
        request_view.validates(&shape),
        "control: a whole-project scalar delegates through an empty completion overlay"
    );

    overlay.insert_whole_hash_for_tests("/overlay-only.ts", [0xA5; 16]);

    assert!(
        !request_view.validates(&content),
        "a shadowing completion overlay must refuse its parent's Content aggregate even though \
         the numeric generation is unchanged"
    );
    assert!(
        request_view.validates(&shape),
        "control: shadowing one canonical still cannot alter WorkspaceShape"
    );
}

/// The completion population changes only with effective shadowing:
/// equal replacement preserves the revision, a changed value advances
/// it, and a different overlay has a different identity even at the same
/// revision. A derived-only entry counts as shadowing because the
/// validator consults that map too.
#[test]
fn completion_overlay_state_tracks_effective_shadowing_identity() {
    use crate::resolver_core::{CanonicalCompletionOverlay, DerivedFactKind};

    let first = CanonicalCompletionOverlay::new();
    let second = CanonicalCompletionOverlay::new();
    assert_eq!(
        first.completion_state_for_tests(),
        CompletionOverlayState::Empty
    );

    first.insert_whole_hash_for_tests("/late.ts", [0x11; 16]);
    let first_state = first.completion_state_for_tests();
    assert!(matches!(
        first_state,
        CompletionOverlayState::Shadowing { .. }
    ));

    first.insert_whole_hash_for_tests("/late.ts", [0x11; 16]);
    assert_eq!(
        first.completion_state_for_tests(),
        first_state,
        "an equal replacement changes no validation answer and must preserve the population"
    );

    first.insert_whole_hash_for_tests("/late.ts", [0x22; 16]);
    let changed_state = first.completion_state_for_tests();
    assert_ne!(
        changed_state, first_state,
        "changing an effective shadow must advance the population revision"
    );

    second.insert_whole_hash_for_tests("/late.ts", [0x22; 16]);
    assert_ne!(
        second.completion_state_for_tests(),
        changed_state,
        "different request overlays must not alias merely because their entries match"
    );

    let derived_only = CanonicalCompletionOverlay::new();
    derived_only.insert_derived_hash_for_tests("/route.ts", DerivedFactKind::Route, [0x33; 16]);
    assert!(matches!(
        derived_only.completion_state_for_tests(),
        CompletionOverlayState::Shadowing { .. }
    ));
}

/// No population is readable while a completion writer holds the
/// bracket open, even when that writer ultimately reports a no-op.
#[test]
fn completion_overlay_state_is_unavailable_during_a_writer() {
    use crate::resolver_core::CanonicalCompletionOverlay;

    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let writer_overlay = Arc::clone(&overlay);
    let writer = std::thread::spawn(move || {
        writer_overlay.hold_revision_in_flight_for_tests(entered_tx, release_rx);
    });

    entered_rx.recv().expect("writer must enter its bracket");
    assert_eq!(
        overlay.completion_state_for_tests(),
        CompletionOverlayState::InFlight,
        "an odd bracket is not the parent population and not a readable revision"
    );
    release_tx.send(()).expect("writer must still be waiting");
    writer.join().expect("writer must finish");
    assert_eq!(
        overlay.completion_state_for_tests(),
        CompletionOverlayState::Empty,
        "a no-op writer restores the exact prior stable state"
    );
}

/// Whole-signature validation leases one completion population. The
/// fixture moves the overlay immediately before the second fact: both
/// facts validate individually in their respective worlds, but their
/// union never described one world and must be refused.
#[test]
fn request_signature_validation_refuses_a_population_straddle() {
    use crate::resolver_core::{CanonicalCompletionOverlay, FactVersionRef, RequestStoreView};

    let host = host_with_a_file();
    let base = freshly_built_view(&host);
    let content = aggregate(
        CompactionDomain::Content,
        base_view(),
        base.content_generation,
    );
    let late = FactVersionRef::FileWholeHash {
        canonical_id: "/late.ts".to_string(),
        hash: [0x77; 16],
    };
    let signature = vec![content, late];

    let quiet_overlay = Arc::new(CanonicalCompletionOverlay::new());
    let quiet = RequestStoreView::new(&base, quiet_overlay);
    assert_eq!(
        quiet.validate_fact_signature(&signature, &[]),
        Ok(()),
        "control: with no movement, the parent aggregate and optimistic untracked dependency \
         are both valid in one empty-overlay world"
    );

    let moving_overlay = Arc::new(CanonicalCompletionOverlay::new());
    let hook_overlay = Arc::clone(&moving_overlay);
    let moving = RequestStoreView::new(&base, Arc::clone(&moving_overlay))
        .with_validation_step_hook_for_tests(Arc::new(move |step| {
            if step == 1 {
                hook_overlay.insert_whole_hash_for_tests("/late.ts", [0x77; 16]);
            }
        }));

    assert!(
        moving.validate_fact_signature(&signature, &[]).is_err(),
        "a signature assembled across the empty and shadowing populations must be refused even \
         though each fact validates in the state where it was visited"
    );
    assert!(matches!(
        moving_overlay.completion_state_for_tests(),
        CompletionOverlayState::Shadowing { .. }
    ));
}

/// The mint-side seed carries the exact effective request population.
/// Content and source environment are stable at their producer boundaries.
/// Semantic imports and route surface remain precise because their membership
/// advances inside cold computes that populate them.
#[test]
fn request_basis_arms_the_stable_view_domains_in_the_exact_completion_population() {
    use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};
    use verter_workspace::AggregateGenerations;

    let host = host_with_a_file();
    let base = freshly_built_view(&host);
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let request = RequestStoreView::new(&base, Arc::clone(&overlay));

    let empty_basis = AggregateGenerations::from_seed(
        &request.aggregate_basis_seed(),
        &host.live_aggregate_counters(),
    );
    assert_eq!(
        empty_basis.view_population,
        Some(ViewPopulation::Base),
        "an empty completion overlay must mint in its durable parent population"
    );
    assert!(
        empty_basis.can_mint(CompactionDomain::Content)
            && empty_basis.can_mint(CompactionDomain::SourceEnv),
        "the stable view-derived domains must compact in a request-bound scope"
    );
    assert!(
        !empty_basis.can_mint(CompactionDomain::SemanticImports)
            && !empty_basis.can_mint(CompactionDomain::RouteSurface),
        "semantic-import and route-surface publication advance inside cold computes that \
         populate them, so arming either would make those computes supersede their own basis"
    );

    overlay.insert_whole_hash_for_tests("/late.ts", [0x44; 16]);
    let shadowing_basis = AggregateGenerations::from_seed(
        &request.aggregate_basis_seed(),
        &host.live_aggregate_counters(),
    );
    assert!(matches!(
        shadowing_basis.view_population,
        Some(ViewPopulation::RequestCompletion(_))
    ));
    assert_ne!(
        shadowing_basis.view_population, empty_basis.view_population,
        "the same overlay after effective shadowing must fork from its parent population"
    );
    assert!(shadowing_basis.can_mint(CompactionDomain::Content));
    assert!(shadowing_basis.can_mint(CompactionDomain::SourceEnv));
    assert!(!shadowing_basis.can_mint(CompactionDomain::SemanticImports));
    assert!(!shadowing_basis.can_mint(CompactionDomain::RouteSurface));

    let stale = RequestStoreView::new_cold_seed(&base, overlay, false);
    assert_eq!(
        AggregateGenerations::from_seed(
            &stale.aggregate_basis_seed(),
            &host.live_aggregate_counters(),
        ),
        AggregateGenerations::default(),
        "a known-stale request view still vouches for no domain or population"
    );
}

/// Rail A is reusable only when the whole signature belongs to domains the
/// request view can validate on one rail. Resolution delegates to the durable
/// world and Content validates in the exact completion population, but their
/// union is not one reusable request-view witness.
#[test]
fn request_signature_refuses_a_mixed_resolution_and_content_aggregate() {
    use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};
    use verter_workspace::ResolutionPopulation;

    let host = host_with_a_file();
    let base = freshly_built_view(&host);
    let content = aggregate(
        CompactionDomain::Content,
        base_view(),
        base.content_generation,
    );
    let resolution_world = host
        .workspace_read()
        .capture_resolution_world()
        .expect("a standalone host must publish a resolution world");
    let resolution = FactVersionRef::DomainGeneration(DomainGenerationFact {
        domain: CompactionDomain::Resolution,
        population: AggregatePopulation::Resolution(ResolutionPopulation::Base),
        stamp: resolution_world
            .resolution_stamp(ResolutionPopulation::Base)
            .expect("a base world must answer for its own population"),
    });
    let request = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));

    assert_eq!(
        request.validate_fact_signature(std::slice::from_ref(&resolution), &[]),
        Ok(()),
        "control: a Resolution-only aggregate delegates to the durable base world"
    );
    assert_eq!(
        request.validate_fact_signature(std::slice::from_ref(&content), &[]),
        Ok(()),
        "control: a Content-only aggregate validates in the exact empty-completion population"
    );
    assert!(
        request
            .validate_fact_signature(&[resolution, content], &[])
            .is_err(),
        "one view-derived aggregate makes the whole Resolution-bearing Rail-A signature \
         ineligible for reuse; per-domain validity must not be mistaken for per-signature reuse"
    );
}

#[test]
fn strict_self_root_witness_reuses_only_the_exact_completion_population() {
    use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};

    let host = host_with_a_file();
    let base = freshly_built_view(&host);
    let canonical = "/completion-only.ts";
    let hash = [0x51; 16];

    let shared_overlay = Arc::new(CanonicalCompletionOverlay::new());
    shared_overlay.insert_whole_hash_for_tests(canonical, hash);
    let winner = RequestStoreView::new(&base, Arc::clone(&shared_overlay));
    let witness = winner
        .mint_strict_self_root_world(&[(canonical, hash)])
        .expect("the winner strictly validates every completion root");
    let signature = [FactVersionRef::StrictSelfRootWorld(witness)];

    let same_population_follower = RequestStoreView::new(&base, shared_overlay);
    assert_eq!(
        same_population_follower.validate_fact_signature(&signature, &[]),
        Ok(()),
        "a follower sharing the exact overlay id and revision can reuse the winner",
    );

    let different_overlay = Arc::new(CanonicalCompletionOverlay::new());
    different_overlay.insert_whole_hash_for_tests(canonical, hash);
    let different_population_follower = RequestStoreView::new(&base, different_overlay);
    assert!(
        different_population_follower
            .validate_fact_signature(&signature, &[])
            .is_err(),
        "an equal root set in a different completion population must fork",
    );
    let other_witness = different_population_follower
        .mint_strict_self_root_world(&[(canonical, hash)])
        .expect("the other population can mint its own witness");
    assert_ne!(witness, other_witness, "overlay identity is collision-free");
}

/// **The PRODUCER half of the population gate**, on a view whose
/// population is DERIVED from a real session overlay rather than
/// hand-written.
///
/// Every other test in this module builds a base view and hands the
/// aggregate a synthetic overlay population, which exercises the
/// validator's comparison but never `view_population()`'s session
/// branch. That branch is the producer: it is what decides a real
/// overlay-bearing view is not the base view. Collapsing it to
/// `ViewPopulation::Base` leaves the whole crate suite green, because
/// nothing else derives a population from a real overlay view.
///
/// The two halves are asserted together on purpose — the gate's actual
/// contract is that producer and validator AGREE, and either one alone
/// is satisfiable by a constant.
///
/// Note the direction. The sibling test checks an overlay-labelled
/// aggregate against a BASE view; this checks a base-labelled aggregate
/// against an OVERLAY view, which is the dangerous direction: under the
/// plant a base-derived Content aggregate satisfies a session read, so
/// base-rooted content is served under a view whose overlay shadows
/// exactly the per-canonical facts that aggregate collapsed.
///
/// Mutation recipe, VERIFIED: in `HostStoreView::view_population`,
/// change `Some(fingerprint) => ViewPopulation::SessionOverlay(fingerprint)`
/// to `Some(_) => ViewPopulation::Base`. Both assertions below fail.
#[test]
fn a_real_session_overlay_view_derives_its_population_and_refuses_base_aggregates() {
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

    let host = host_with_a_file();
    let base = freshly_built_view(&host);
    let content_generation = base.content_generation;

    // A real overlay over the host's file — the fingerprint is derived
    // by the view, never chosen by this test.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(
        "/proj/a.ts".to_string(),
        Arc::from("export const a = 2\n") as Arc<str>,
    );
    let session = OverlaidView::new(Arc::clone(&host), overlays);
    let fingerprint = session.fingerprint();
    assert_ne!(
        fingerprint, 0,
        "fixture invariant: a view with overlays installed must report a non-zero \
         overlay-set fingerprint, or it is indistinguishable from the base view"
    );

    let overlay_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &session);

    // PRODUCER: the population is derived from the real overlay set.
    assert_eq!(
        overlay_view.view_population(),
        ViewPopulation::SessionOverlay(
            SessionOverlayFingerprint::new(fingerprint)
                .expect("a non-zero overlay-set fingerprint is a session identity")
        ),
        "a view carrying a real session overlay must derive a SessionOverlay population from \
         that overlay set — reporting Base here would make an overlay view indistinguishable \
         from the base view to every aggregate"
    );

    // VALIDATOR, in the dangerous direction: a BASE-population Content
    // aggregate at a generation the BASE view accepts must be refused by
    // the overlay view.
    let base_aggregate = aggregate(CompactionDomain::Content, base_view(), content_generation);
    assert!(
        base.validates(&base_aggregate),
        "control: the base view accepts this aggregate, so the refusal below is about the \
         POPULATION and not about a stale generation"
    );
    assert!(
        !overlay_view.validates(&base_aggregate),
        "a session-overlay view must refuse a BASE-population content aggregate even at its own \
         content generation: the overlay re-roots whole hashes and parse facts without moving \
         the workspace content generation, so accepting it serves base-rooted content under a \
         view whose overlay shadows exactly the facts the aggregate collapsed"
    );
}

/// **The fingerprint-EQUALITY axis of the population gate.**
///
/// Every other population assertion in this module pits a `Base`
/// population against an overlay one, or the reverse. Both pairs differ
/// in DISCRIMINANT, so a validator that compared only the discriminant —
/// base matches base, any overlay matches any overlay — would satisfy all
/// of them. What that would let through is two DIFFERENT sessions
/// cross-validating each other's compacted witnesses: session B serving
/// content rooted in session A's overlay set.
///
/// Closing it needs two distinct REAL overlay views, which is why it is
/// its own fixture rather than one more assertion on an existing one.
/// Both fingerprints are DERIVED by the views from genuinely different
/// overlay sets, never chosen here, so the test cannot pass by
/// construction.
///
/// Mutation recipe: in `HostStoreView::validates_domain_aggregate`,
/// weaken `population == self.view_population()` to a discriminant-only
/// comparison —
/// `std::mem::discriminant(&population) == std::mem::discriminant(&self.view_population())`.
/// This test fails; every other population assertion in the module stays
/// green, which is precisely why the axis needed its own fixture.
#[test]
fn two_distinct_session_overlays_do_not_cross_validate_each_others_aggregates() {
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

    let host = host_with_a_file();

    let overlay_view_for = |source: &str| {
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert("/proj/a.ts".to_string(), Arc::from(source) as Arc<str>);
        let session = OverlaidView::new(Arc::clone(&host), overlays);
        let fingerprint = session.fingerprint();
        let view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &session);
        (view, fingerprint)
    };

    let (view_a, fingerprint_a) = overlay_view_for("export const a = 2\n");
    let (view_b, fingerprint_b) = overlay_view_for("export const a = 3\n");

    assert_ne!(
        fingerprint_a, fingerprint_b,
        "fixture invariant: two different overlay SETS must derive different fingerprints, or \
         this test cannot distinguish the two sessions at all"
    );
    assert_ne!(
        view_a.view_population(),
        view_b.view_population(),
        "fixture invariant: the derived populations must differ, or the assertion below is \
         comparing a view with itself"
    );

    // An aggregate minted under session A, at a generation session B also
    // captured. Only the overlay-set identity separates them.
    let a_aggregate = aggregate(
        CompactionDomain::Content,
        AggregatePopulation::View(view_a.view_population()),
        view_a.content_generation,
    );
    assert!(
        view_a.validates(&a_aggregate),
        "control: session A vouches for its own aggregate, so the refusal below is about the \
         overlay SET and not about a stale generation"
    );
    assert_eq!(
        view_a.content_generation, view_b.content_generation,
        "fixture invariant: both sessions must sit at the same content generation, or the \
         refusal below could be explained by the stamp axis alone"
    );

    assert!(
        !view_b.validates(&a_aggregate),
        "session B must refuse a witness minted under session A's overlay set. The two sessions \
         shadow DIFFERENT per-canonical content while sharing every workspace-level generation, \
         so a discriminant-only population comparison would serve A's overlay-rooted content \
         under B's view"
    );
}

/// **The behavioural half of the composite**: a REAL movement in a
/// component's own producer refuses a previously-captured composite,
/// with the movement ISOLATED to that component.
///
/// The perturbation test above proves the validator compares the whole
/// stamp. It cannot prove a component is sourced from a live producer —
/// hard-coding one inside `semantic_imports_stamp` composes the same
/// constant on both sides and survives it. Only a genuine mutation
/// distinguishes "this field is compared" from "this field tracks
/// something", and only an ISOLATED mutation attributes the refusal to
/// the component under test rather than to a sibling that moved along
/// with it.
///
/// Each arm therefore asserts its own isolation before asserting the
/// refusal. Without that the arm passes under both halves of its own
/// plant: an edit, for instance, republishes the resolution world as
/// well as moving the content generation, so a content arm that did not
/// check would be satisfied by the resolution component and would stay
/// green with `content` hard-coded to a constant.
///
/// Mutation recipe, EXECUTED: hard-code the moved component in
/// `HostStoreView::semantic_imports_stamp` — `workspace_shape: 0` for
/// the shape arm, `semantic_imports: 0` for the membership arm. The
/// matching assertion fails while the perturbation test above stays
/// green, which is the pair showing the two tests cover different things.
#[test]
fn an_isolated_component_movement_refuses_a_previously_captured_composite() {
    use verter_workspace::SemanticImportsStamp;

    let host = host_with_a_file();

    let components = |host: &VerterHost| -> SemanticImportsStamp {
        let view = freshly_built_view(host);
        match view
            .semantic_imports_stamp()
            .expect("the view captures every component")
        {
            AggregateStamp::SemanticImports(stamp) => stamp,
            other => panic!("the semantic-imports stamp must be the composite, got {other:?}"),
        }
    };
    let composite = |stamp: SemanticImportsStamp| {
        FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::SemanticImports,
            population: base_view(),
            stamp: AggregateStamp::SemanticImports(stamp),
        })
    };

    // --- WORKSPACE SHAPE, isolated -----------------------------------
    let before = components(&host);
    assert!(
        freshly_built_view(&host).validates(&composite(before)),
        "control: the view vouches for the composite it composed"
    );

    host.project_type_store().bump_project_generation();

    let after_shape = components(&host);
    assert_eq!(
        (
            after_shape.semantic_imports,
            after_shape.content,
            after_shape.source_env,
            after_shape.resolution
        ),
        (
            before.semantic_imports,
            before.content,
            before.source_env,
            before.resolution
        ),
        "ISOLATION: a project-shape bump must move nothing but the shape component, or the \
         refusal below is attributable to a sibling and this arm proves nothing about \
         `workspace_shape`"
    );
    assert_ne!(
        after_shape.workspace_shape, before.workspace_shape,
        "fixture invariant: the bump must move the shape component"
    );
    assert!(
        !freshly_built_view(&host).validates(&composite(before)),
        "the project graph moved, which re-composes the per-canonical env bundle the \
         semantic-import producer keys on. A witness compacted under the old shape must be \
         refused even though neither its membership nor its content moved — which is exactly the \
         case a bare membership counter would serve"
    );

    // --- MEMBERSHIP, isolated ----------------------------------------
    let before_admit = components(&host);
    let key = {
        let content_hash = host
            .current_or_read_whole_hash("/proj/a.ts")
            .expect("owner content hash");
        let env = host.host_view_env_hashes_for("/proj/a.ts");
        crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: Arc::from("/proj/a.ts"),
            content_hash,
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        }
    };
    let witness = host
        .resolved_import_facts_witness_for(key.canonical.as_ref(), key.content_hash)
        .expect("the production witness must be rootable");
    assert!(
        host.project_type_store().resolved_import_facts().admit(
            key,
            Arc::new(crate::resolved_import_facts::ResolvedImportFacts::new()),
            witness,
        ),
        "fixture invariant: the admission must succeed"
    );

    let after_admit = components(&host);
    assert_eq!(
        (
            after_admit.content,
            after_admit.source_env,
            after_admit.resolution,
            after_admit.workspace_shape
        ),
        (
            before_admit.content,
            before_admit.source_env,
            before_admit.resolution,
            before_admit.workspace_shape
        ),
        "ISOLATION: an admission must move nothing but the membership component"
    );
    assert_ne!(
        after_admit.semantic_imports, before_admit.semantic_imports,
        "fixture invariant: the admission must move the membership component"
    );
    assert!(
        !freshly_built_view(&host).validates(&composite(before_admit)),
        "the store's membership moved, so a witness that compacted the domain before it now \
         stands for a slot population that has changed"
    );
}

/// A content edit refuses a previously-captured composite — recorded at
/// the strength of the evidence, which is WEAKER than the isolated arms
/// above.
///
/// An `upsert` moves the content generation AND republishes the
/// resolution world (measured: `content 2→3` and the session
/// `ResolutionWorldId 5→6` on the same edit). No public host API reaches
/// a content mutation that leaves the resolution world alone, so this
/// arm proves the composite refuses across an edit — it does NOT
/// attribute the refusal to the `content` component.
///
/// The consequence is stated rather than hidden: hard-coding
/// `content` inside `semantic_imports_stamp` leaves this test GREEN
/// (executed and confirmed). `content`'s participation rests on the
/// whole-stamp equality proven by
/// [`every_component_of_the_semantic_imports_composite_is_load_bearing`]
/// plus its derivation from `self.content_generation`, and NOT on a
/// behavioural isolation this host shape can express. Whoever gains a
/// resolution-free content mutation inherits the obligation to tighten
/// this arm.
#[test]
fn an_edit_refuses_a_previously_captured_composite() {
    let host = host_with_a_file();
    let before = freshly_built_view(&host)
        .semantic_imports_stamp()
        .expect("the view captures every component");
    let aggregate = FactVersionRef::DomainGeneration(DomainGenerationFact {
        domain: CompactionDomain::SemanticImports,
        population: base_view(),
        stamp: before,
    });
    assert!(
        freshly_built_view(&host).validates(&aggregate),
        "control: the pre-edit view vouches for its own composite"
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/a.ts".to_string(),
            source: Arc::from("export const a = 2\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("edit must succeed");

    assert!(
        !freshly_built_view(&host).validates(&aggregate),
        "an edit re-keys every semantic-import slot for the edited canonical, so a witness that \
         compacted the domain beforehand must be refused. The store's own membership did NOT \
         move — no admission happened — so a bare membership counter would have served this"
    );
}

#[test]
fn derived_state_membership_advances_the_strict_self_root_world() {
    let host = host_with_a_file();
    let before_view = host.resolver_store_view_read().into_owned_view();
    let before = before_view
        .strict_self_root_world_identity()
        .expect("the in-memory workspace exposes a strict-root authority");
    let witness = FactVersionRef::StrictSelfRootWorld(before);
    assert!(before_view.validates(&witness), "same-world control");

    let canonical = "/proj/authority-only.ts";
    assert!(!host.derived_raw_cache().contains_key(canonical));
    drop(host.derived_raw_entry_or_default(canonical.to_string()));
    assert!(
        host.derived_raw_cache().contains_key(canonical),
        "fixture must exercise a vacant derived-state insertion",
    );

    assert!(
        !before_view.validates(&witness),
        "the old view must stop validating immediately after membership changes",
    );
    let after_view = host.resolver_store_view_read().into_owned_view();
    let after = after_view
        .strict_self_root_world_identity()
        .expect("the manager must rebuild under the new authority");
    assert_ne!(before, after);
    assert!(after_view.validates(&FactVersionRef::StrictSelfRootWorld(after)));
}

#[test]
fn occupied_derived_state_lookup_preserves_the_strict_self_root_world() {
    let host = host_with_a_file();
    let canonical = "/proj/already-present.ts";
    drop(host.derived_raw_entry_or_default(canonical.to_string()));
    let view = freshly_built_view(&host);
    let before = view
        .strict_self_root_world_identity()
        .expect("the settled authority is witnessable");

    drop(host.derived_raw_entry_or_default(canonical.to_string()));

    assert_eq!(
        view.strict_self_root_world_identity(),
        Some(before),
        "looking up an occupied row changes neither membership nor its authority world",
    );
}

#[test]
fn distinct_workspace_authorities_never_alias_strict_self_root_worlds() {
    let first = host_with_a_file();
    let second = host_with_a_file();
    let first_world = freshly_built_view(&first)
        .strict_self_root_world_identity()
        .expect("the first in-memory authority is witnessable");
    let second_world = freshly_built_view(&second)
        .strict_self_root_world_identity()
        .expect("the second in-memory authority is witnessable");

    assert_ne!(
        first_world, second_world,
        "a candidate minted by one workspace must never validate after an authority swap",
    );
}

#[test]
fn strict_self_root_world_is_unavailable_inside_an_authority_transition() {
    let host = host_with_a_file();
    host.ws().begin_strict_self_root_transition();
    let in_flight = freshly_built_view(&host);
    assert!(
        in_flight.strict_self_root_world_identity().is_none(),
        "a view captured after a writer starts must not mint the intermediate world",
    );
    host.ws().end_strict_self_root_transition();

    assert!(
        in_flight.strict_self_root_world_identity().is_none(),
        "the intermediate view cannot alias the completed world",
    );
    assert!(
        freshly_built_view(&host)
            .strict_self_root_world_identity()
            .is_some(),
        "control: a fresh settled view can mint again",
    );
}

#[test]
fn uncovered_filesystem_presence_is_not_compacted_into_a_strict_world() {
    use verter_workspace::WorkspaceRead as _;

    let unique = format!(
        "verter-strict-root-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos(),
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).expect("create fixture directory");
    let path = dir.join("artifact-only.ts");
    std::fs::write(&path, "export const disk = 1;\n").expect("write fixture source");
    let canonical = path.to_string_lossy().replace('\\', "/");

    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    assert!(
        workspace.file_exists(&canonical),
        "fixture must be visible through the backend's raw filesystem fallback",
    );
    let host = VerterHost::new(HostConfig::default(), workspace);
    let view = freshly_built_view(&host);
    assert!(
        !view.strict_self_root_is_witnessable(&canonical),
        "an external disk-presence answer without a complete event bridge cannot be collapsed",
    );

    std::fs::remove_dir_all(&dir).expect("remove fixture directory");
}
