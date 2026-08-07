//! Movement detection at the two ADMISSION-OWNING seams (`MU-1`).
//!
//! A compaction domain's terminal aggregate asserts "the domain held as
//! of generation `N`". If the domain advances between the moment a
//! scope's basis is installed and the moment it admits, that assertion
//! is false for whichever half of the observations sits on the other
//! side of the mutation — a stale serve over a whole domain at once.
//!
//! There are TWO seams, and they are not variations of one another:
//!
//! 1. the SIGNATURE-CONSUMING boundary (`install_fact_tracer`), which
//!    finalises and reports a typed outcome;
//! 2. the CACHEABILITY PROBE, which reads its verdict MID-SCOPE and can
//!    authorise a write from inside the closure — so an exit-only check
//!    runs after the write it was meant to gate.
//!
//! Both are exercised here, in both directions: a scope that spans a
//! real mutation refuses, and a scope that does not still admits.
//!
//! ## Where the basis comes from
//!
//! From the production chokepoint, not from the fixture. Each scope is
//! opened with a real request-bound source, so the basis these
//! assertions rest on is the one every production tracer gets. A scope
//! opened with an UNBOUND source is asserted to stay admissible across
//! the same mutation, which pins the short-circuit that keeps the check
//! free for a scope that compacts nothing.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use crate::fact_signature_helpers::{
    install_fact_tracer, with_cacheability_scope, FactTracerBasisSource,
};
use crate::resolved_import_facts::{ResolvedImportFacts, ResolvedImportFactsKey};
use crate::resolver_core::with_bare_host_ctx_for_test;
use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

fn host_with_a_file() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/proj/a.ts".to_string()),
            input_id: "/proj/a.ts".to_string(),
            source: Arc::from("export const a = 1\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
    host
}

/// Assert the enclosing scope was seeded with a basis that names at
/// least one domain.
///
/// Without it every movement assertion below is vacuous: an unseeded
/// scope short-circuits the re-check and admits unconditionally, so a
/// fixture that lost its binding would go green while testing nothing.
fn assert_scope_has_a_basis(host: &VerterHost) {
    assert!(
        host.current_fact_tracer()
            .expect("a tracer scope must be installed")
            .has_aggregate_basis(),
        "fixture invariant: the tracer chokepoint must have installed a basis naming at least \
         one domain, or movement detection short-circuits and every assertion below is vacuous"
    );
}

/// Advance the WORKSPACE-SHAPE domain.
///
/// The domain a seeded scope can actually MINT before a view population
/// reaches it: `ProjectGeneration` is a whole-host scalar no overlay
/// shadows, so its aggregate is base-scoped unconditionally. Movement
/// here is therefore movement the scope's own witness could misreport.
fn advance_workspace_shape(host: &VerterHost) {
    let before = host.project_type_store().project_generation();
    host.project_type_store().bump_project_generation();
    assert!(
        host.project_type_store().project_generation() > before,
        "fixture invariant: the project generation must genuinely advance, or no domain moved"
    );
}

/// Advance the SEMANTIC-IMPORTS domain by admitting a candidate.
fn advance_semantic_imports(host: &VerterHost) {
    let content_hash = host
        .current_or_read_whole_hash("/proj/a.ts")
        .expect("owner content hash");
    let env = host.host_view_env_hashes_for("/proj/a.ts");
    let key = ResolvedImportFactsKey {
        canonical: Arc::from("/proj/a.ts"),
        content_hash,
        parse_env_hash: env.parse_env_hash,
        resolve_env_hash: env.resolve_env_hash,
        resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    };
    let witness: Vec<FactVersionRef> = host
        .resolved_import_facts_witness_for(key.canonical.as_ref(), key.content_hash)
        .expect("the production witness must be rootable");
    assert!(
        host.project_type_store().resolved_import_facts().admit(
            key,
            Arc::new(ResolvedImportFacts::new()),
            witness
        ),
        "fixture invariant: the admission must succeed, or no domain moved"
    );
}

/// **Seam 1 — the signature-consuming boundary.** A scope whose basis
/// names a domain that advances mid-scope finalises as
/// `MutationUnstable`, a DISTINCT typed outcome that is never `Overflow`.
///
/// Mutation recipe, EXECUTED: delete the `note_basis_recheck(host, &mut
/// read_set);` call from `install_fact_tracer`. This test fails while
/// the no-mutation control below stays green.
#[test]
fn a_scope_spanning_a_domain_advance_finalises_as_mutation_unstable() {
    let host = host_with_a_file();

    let (_, finalise) = with_bare_host_ctx_for_test(&host, |ctx| {
        install_fact_tracer(&FactTracerBasisSource::from_ctx(ctx), || {
            assert_scope_has_a_basis(&host);
            advance_workspace_shape(&host);
        })
    });

    assert!(
        matches!(finalise, FactReadSetFinalise::MutationUnstable),
        "a domain this scope compacts against advanced between its basis being installed and \
         this finalisation, so the terminal aggregate would claim the domain held as of a \
         generation these observations do not come from. Got {finalise:?}"
    );
    assert!(
        !matches!(finalise, FactReadSetFinalise::Overflow),
        "and it must NEVER be reported as overflow: instability is a STABILITY failure, and \
         degrading it into a cardinality one refuses the attempt under exactly the size rail \
         this substrate exists to remove"
    );
}

/// The control: the SAME basis, the SAME scope, NO mutation. Without it
/// the assertion above is satisfied by a check that refuses
/// unconditionally.
#[test]
fn a_scope_with_a_basis_and_no_domain_advance_still_admits() {
    let host = host_with_a_file();

    let (_, finalise) = with_bare_host_ctx_for_test(&host, |ctx| {
        install_fact_tracer(&FactTracerBasisSource::from_ctx(ctx), || {
            assert_scope_has_a_basis(&host);
            // Observe something so the signature is non-empty, but move no
            // domain.
            crate::resolver_core::resolver_context::observe_fan_out(
                FactVersionRef::FileWholeHash {
                    canonical_id: "/proj/a.ts".to_string(),
                    hash: [1_u8; 16],
                },
            );
        })
    });

    assert!(
        matches!(finalise, FactReadSetFinalise::Ok(_)),
        "no domain moved, so the scope must admit normally — a movement check that refuses a \
         quiescent scope would disable compaction entirely. Got {finalise:?}"
    );
}

/// A scope with NO basis is unaffected by the same mutation.
///
/// This is the short-circuit that keeps movement detection free until a
/// basis reaches a tracer: a scope that compacts nothing mints no
/// aggregate, so no generation movement can corrupt it. It is asserted
/// rather than assumed because it is what makes the production
/// behaviour today byte-identical to before.
#[test]
fn a_scope_with_no_basis_is_unaffected_by_a_domain_advance() {
    let host = host_with_a_file();

    let (_, finalise) = install_fact_tracer(&FactTracerBasisSource::unbound(&host), || {
        crate::resolver_core::resolver_context::observe_fan_out(FactVersionRef::FileWholeHash {
            canonical_id: "/proj/a.ts".to_string(),
            hash: [1_u8; 16],
        });
        advance_semantic_imports(&host);
    });

    assert!(
        matches!(finalise, FactReadSetFinalise::Ok(_)),
        "a scope that compacts nothing mints no aggregate, so a domain advancing under it cannot \
         make any witness wrong and must not refuse it. Got {finalise:?}"
    );
}

/// **Seam 2 — the cacheability probe**, read MID-SCOPE.
///
/// The probe is an admission boundary in its own right: it can authorise
/// a write from inside the scope's closure, so its verdict must include
/// a FRESH movement check rather than inheriting one taken on exit. The
/// assertion is taken from inside the closure, at the point a producer
/// would consult it before writing.
///
/// Mutation recipe, EXECUTED: delete the
/// `note_basis_recheck_on_cell(self.host, self.cell);` call from
/// `CacheabilityProbe::non_cacheable`. The IN-SCOPE assertion fails
/// while the post-scope verdict stays correct — which is exactly the
/// "exit-only check runs too late" failure this seam exists to prevent,
/// and is why an exit-only test would not have caught it.
#[test]
fn the_cacheability_probe_reports_instability_from_inside_the_scope() {
    let host = host_with_a_file();

    let (probe_verdict_in_scope, verdict_on_exit) = with_bare_host_ctx_for_test(&host, |ctx| {
        with_cacheability_scope(&FactTracerBasisSource::from_ctx(ctx), |probe| {
            assert_scope_has_a_basis(&host);
            assert!(
                !probe.non_cacheable(),
                "control: before the mutation the probe must report the scope CACHEABLE, or the \
             assertion below could be satisfied by a probe that always refuses"
            );
            advance_workspace_shape(&host);
            probe.non_cacheable()
        })
    });

    assert!(
        probe_verdict_in_scope,
        "the probe must observe the domain advance AT THE POINT IT IS ASKED — a producer \
         consults it here, before writing, so a verdict computed only on scope exit would run \
         after the write it was meant to gate"
    );
    assert!(
        verdict_on_exit,
        "and the exit verdict agrees: instability is sticky, so a scope cannot become stable \
         again"
    );
}

/// The probe's control: a basis, a probe, no mutation — still cacheable.
#[test]
fn the_cacheability_probe_stays_cacheable_without_a_domain_advance() {
    let host = host_with_a_file();

    let (_, non_cacheable) = with_bare_host_ctx_for_test(&host, |ctx| {
        with_cacheability_scope(&FactTracerBasisSource::from_ctx(ctx), |probe| {
            assert_scope_has_a_basis(&host);
            assert!(
                !probe.non_cacheable(),
                "a quiescent scope with a basis must stay cacheable when read mid-scope"
            );
        })
    });

    assert!(
        !non_cacheable,
        "and on exit too — a movement check that refuses a quiescent scope would disable every \
         cacheability probe in the host"
    );
}

/// **The corrected RouteSurface clock, at the seam it exists for.**
///
/// The augmentation index materialises INSIDE active fact tracers on the
/// same thread — a measured 96.9% of cold installs do. Under a clock
/// that counted index churn as a semantic advance, every such scope
/// would destabilise itself and every enclosing scope on the tracer
/// stack (measured: one inner mutation marks ~2.6 scopes), refusing
/// admission for exactly the work compaction exists to make reusable.
///
/// So this asserts the direction that is easy to lose: a scope that
/// merely WARMS the index stays admissible.
///
/// Mutation recipe, EXECUTED: in
/// `FileArtifactStore::publish_augmenter_set`, change `is_some_and` to
/// `is_none_or` (the `artifact_generation` rule). This test fails —
/// which is the whole reason the route-surface clock is separate from
/// `artifact_generation`.
#[test]
fn warming_the_augmentation_index_inside_a_scope_does_not_destabilise_it() {
    use crate::file_artifact_store::{
        AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind,
    };

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    for (path, source) in [
        ("/types.ts", "export interface Base { a: string }\n"),
        (
            "/aug.ts",
            "import type { Base } from './types'\n\
             declare module './types' { interface Base { b: number } }\n",
        ),
    ] {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(path.to_string()),
                input_id: path.to_string(),
                source: Arc::from(source),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert must succeed");
    }
    for path in ["/types.ts", "/aug.ts"] {
        host.ensure_indexed_ready(path)
            .unwrap_or_else(|| panic!("fixture invariant: {path} must index"));
    }

    let env = host.host_view_env_hashes();
    let key = AugmentationTargetKey {
        project_identity: host.host_view_project_identity(),
        resolve_env_hash: env.resolve_env_hash,
        lib_env_hash: env.lib_env_hash,
        population: AugmentationPopulation::Base,
        target: AugmentationTargetKind::ResolvedRelativeCanonical(Arc::from("/types.ts")),
    };

    let (set, finalise) = with_bare_host_ctx_for_test(&host, |ctx| {
        install_fact_tracer(&FactTracerBasisSource::from_ctx(ctx), || {
            assert_scope_has_a_basis(&host);
            host.project_type_store()
                .indexed()
                .ensure_augmentation_index_populated(
                    &key,
                    |augmenter, specifier| match host
                        .resolve_type_dependency_canonical(augmenter, specifier)
                    {
                        verter_workspace::ResolutionPublication::Admitted(admitted) => {
                            admitted.into_result().map(Arc::from)
                        }
                        _ => None,
                    },
                    None,
                )
        })
    });

    assert!(
        !set.entries.is_empty(),
        "fixture invariant: the index row must genuinely materialise a contributor, or no \
         augmentation-index mutation happened inside the scope at all"
    );
    assert!(
        matches!(finalise, FactReadSetFinalise::Ok(_)),
        "warming an index row from an unchanged artifact corpus is a cache population, so the \
         enclosing scope must still admit. Got {finalise:?}"
    );
}

/// **The re-check is `O(1)`, not one store-view read per admission
/// boundary.**
///
/// Movement detection runs at EVERY admission boundary and again on
/// scope exit — a path hotter than installation. Before a basis existed
/// both ends short-circuited on "this scope names no domain", so the
/// composer behind them was never reached and its cost was invisible.
/// Seeding a scope arms them, so the composition must be atomic loads
/// over a captured seed rather than a rebuilt view.
///
/// The oracle is the host's own `store_view_from_host_reads` counter —
/// the same rail the batch O(1)-read invariants measure. The window
/// opens AFTER the request context is bound, so the one read that binds
/// it is not counted against the scope.
///
/// Mutation recipe, EXECUTED: add
/// `let _ = self.host.resolver_store_view_read();` to
/// `FactTracerBasisSource::live_basis`. This test fails; the movement
/// fixtures above stay green, which is exactly why they could not have
/// caught it.
#[test]
fn a_seeded_scope_reads_no_store_view_at_install_or_at_any_admission_boundary() {
    const ADMISSION_BOUNDARIES: usize = 8;
    let host = host_with_a_file();

    let (reads, boundaries_seen) = with_bare_host_ctx_for_test(&host, |ctx| {
        let source = FactTracerBasisSource::from_ctx(ctx);
        let before = host
            .provenance()
            .store_view_from_host_reads
            .load(std::sync::atomic::Ordering::Relaxed);
        let (boundaries_seen, _) = with_cacheability_scope(&source, |probe| {
            // Without a basis every one of these short-circuits and the
            // assertion below is vacuous, so prove the scope is seeded.
            assert_scope_has_a_basis(&host);
            let mut seen = 0;
            for _ in 0..ADMISSION_BOUNDARIES {
                assert!(
                    !probe.non_cacheable(),
                    "control: no domain moved, so every boundary must report cacheable"
                );
                seen += 1;
            }
            seen
        });
        let after = host
            .provenance()
            .store_view_from_host_reads
            .load(std::sync::atomic::Ordering::Relaxed);
        (after - before, boundaries_seen)
    });

    assert_eq!(
        boundaries_seen, ADMISSION_BOUNDARIES,
        "fixture invariant: every admission boundary must have been consulted"
    );
    assert_eq!(
        reads, 0,
        "installing a basis and re-checking it {ADMISSION_BOUNDARIES} times must cost ZERO \
         store-view reads — the seed is captured once from a view the caller already holds and \
         the live half is atomic loads. Observed {reads}."
    );
}

/// The same window for an UNBOUND scope: it must cost zero reads too.
///
/// Pairs with the test above so "zero" cannot be satisfied by a scope
/// that simply never reaches the composer: the bound scope proves the
/// composer RUNS and is free, this one proves the short-circuit did not
/// silently become the only reason.
#[test]
fn an_unbound_scope_reads_no_store_view_either() {
    let host = host_with_a_file();
    let before = host
        .provenance()
        .store_view_from_host_reads
        .load(std::sync::atomic::Ordering::Relaxed);
    let (_, non_cacheable) =
        with_cacheability_scope(&FactTracerBasisSource::unbound(&host), |probe| {
            assert!(
                !host
                    .current_fact_tracer()
                    .expect("a tracer scope must be installed")
                    .has_aggregate_basis(),
                "an unbound source must seed no basis at all"
            );
            probe.non_cacheable()
        });
    let after = host
        .provenance()
        .store_view_from_host_reads
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(!non_cacheable);
    assert_eq!(
        after - before,
        0,
        "an unbound scope must read no store view"
    );
}

/// **Movement in a domain outside the scope's explicit participation
/// set does not destabilise it.**
///
/// A bound request population arms Content first. SemanticImports remains
/// precise and its stamp is absent from this basis, so no witness the
/// scope produces can claim that domain held and its generation moving
/// cannot make the witness wrong.
///
/// This matters far beyond tidiness: `SemanticImports` advances on every
/// resolved-import admission, which is to say inside essentially every
/// cold compute. Examining a stamp the scope cannot mint from would
/// refuse those computes' admission for a claim they never made.
///
/// The PAIR with
/// `a_scope_spanning_a_domain_advance_finalises_as_mutation_unstable`
/// above is what makes this discriminating: the same seam, the same
/// scope, one MINTABLE advance refuses and one UNMINTABLE advance does
/// not. A movement check that never refused would fail that test; one
/// that refused unconditionally fails this one.
///
/// Mutation recipe: change `HostStoreView::aggregate_basis_seed` from
/// `ViewAggregateDomains::CONTENT` to `ViewAggregateDomains::ALL`. This
/// test fails while the mintable Content movement control stays green.
#[test]
fn movement_outside_the_scopes_domain_participation_leaves_it_admissible() {
    use verter_workspace::CompactionDomain;

    let host = host_with_a_file();

    let (_, finalise) = with_bare_host_ctx_for_test(&host, |ctx| {
        let source = FactTracerBasisSource::from_ctx(ctx);
        let basis = source.live_basis();
        assert!(
            basis.semantic_imports.is_none(),
            "fixture invariant: SemanticImports is not in this scope's explicit domain \
             participation set"
        );
        assert!(
            !basis.can_mint(CompactionDomain::SemanticImports),
            "fixture invariant: and it must not be able to mint that domain, or the advance \
             below is legitimately destabilising"
        );
        install_fact_tracer(&source, || {
            assert_scope_has_a_basis(&host);
            advance_semantic_imports(&host);
        })
    });

    assert!(
        matches!(finalise, FactReadSetFinalise::Ok(_)),
        "a domain this scope cannot mint an aggregate for moved; the scope produced no claim \
         about it, so it must still admit. Got {finalise:?}"
    );
}
